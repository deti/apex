//! Coverage-guided property-based testing — infer properties from code patterns.

use serde::{Deserialize, Serialize};

/// Inferred property categories from source code patterns.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum InferredProperty {
    /// f(f(x)) == f(x) — applying function twice gives same result.
    Idempotent { function: String },
    /// f(a, b) == f(b, a) — argument order doesn't matter.
    Commutative { function: String },
    /// f(x) monotonically increases/decreases with x.
    Monotonic { function: String, increasing: bool },
    /// Function never throws/panics for any valid input.
    NoException { function: String },
    /// len(f(x)) == len(x) — output preserves input length.
    LengthPreserving { function: String },
    /// f(encode(x)) == x — round-trip property.
    RoundTrip { encode: String, decode: String },
}

/// Pattern-based property inferrer (MVP: string-matching).
pub struct PropertyInferer;

/// Idempotent indicator prefixes.
const IDEMPOTENT_PREFIXES: &[&str] = &["sort", "normalize", "canonicalize", "deduplicate", "dedup"];

/// Commutative indicator prefixes.
const COMMUTATIVE_PREFIXES: &[&str] = &["add", "merge", "combine", "union", "sum"];

/// Length-preserving indicator prefixes.
const LENGTH_PRESERVING_PREFIXES: &[&str] = &["filter", "map", "transform"];

/// Round-trip pairs: (encode_prefix, decode_prefix).
const ROUNDTRIP_PAIRS: &[(&str, &str)] = &[
    ("encode", "decode"),
    ("serialize", "deserialize"),
    ("compress", "decompress"),
    ("encrypt", "decrypt"),
    ("to_json", "from_json"),
    ("to_string", "from_string"),
    ("to_bytes", "from_bytes"),
];

impl PropertyInferer {
    /// Infer properties from source code text.
    ///
    /// Looks for common patterns: encode/decode pairs, sort functions,
    /// serialization round-trips, etc.
    pub fn infer(source: &str) -> Vec<InferredProperty> {
        let mut props = Vec::new();
        let functions = Self::extract_function_names(source);

        // Check for round-trip pairs first (uses two functions).
        let mut roundtrip_fns = std::collections::HashSet::new();
        for &(enc_prefix, dec_prefix) in ROUNDTRIP_PAIRS {
            let enc_match = functions.iter().find(|f| f.starts_with(enc_prefix));
            let dec_match = functions.iter().find(|f| f.starts_with(dec_prefix));
            if let (Some(enc), Some(dec)) = (enc_match, dec_match) {
                roundtrip_fns.insert(enc.clone());
                roundtrip_fns.insert(dec.clone());
                props.push(InferredProperty::RoundTrip {
                    encode: enc.clone(),
                    decode: dec.clone(),
                });
            }
        }

        for func in &functions {
            // Idempotent check.
            if IDEMPOTENT_PREFIXES
                .iter()
                .any(|p| func.starts_with(p))
            {
                props.push(InferredProperty::Idempotent {
                    function: func.clone(),
                });
            }

            // Commutative check.
            if COMMUTATIVE_PREFIXES
                .iter()
                .any(|p| func.starts_with(p))
            {
                props.push(InferredProperty::Commutative {
                    function: func.clone(),
                });
            }

            // Length-preserving check.
            if LENGTH_PRESERVING_PREFIXES
                .iter()
                .any(|p| func.starts_with(p))
            {
                props.push(InferredProperty::LengthPreserving {
                    function: func.clone(),
                });
            }

            // NoException for any public function.
            if Self::is_public_function(source, func) {
                props.push(InferredProperty::NoException {
                    function: func.clone(),
                });
            }
        }

        props
    }

    /// Generate a hypothesis test for a given property.
    pub fn generate_hypothesis_test(prop: &InferredProperty, language: &str) -> String {
        match language {
            "python" | "py" => Self::generate_python_hypothesis(prop),
            _ => Self::generate_python_hypothesis(prop), // default to Python
        }
    }

    fn generate_python_hypothesis(prop: &InferredProperty) -> String {
        match prop {
            InferredProperty::Idempotent { function } => {
                format!(
                    "from hypothesis import given, strategies as st\n\n\
                     @given(x=st.text())\n\
                     def test_{function}_idempotent(x):\n    \
                         assert {function}({function}(x)) == {function}(x)\n"
                )
            }
            InferredProperty::Commutative { function } => {
                format!(
                    "from hypothesis import given, strategies as st\n\n\
                     @given(a=st.integers(), b=st.integers())\n\
                     def test_{function}_commutative(a, b):\n    \
                         assert {function}(a, b) == {function}(b, a)\n"
                )
            }
            InferredProperty::Monotonic {
                function,
                increasing,
            } => {
                let op = if *increasing { "<=" } else { ">=" };
                format!(
                    "from hypothesis import given, assume, strategies as st\n\n\
                     @given(a=st.integers(), b=st.integers())\n\
                     def test_{function}_monotonic(a, b):\n    \
                         assume(a <= b)\n    \
                         assert {function}(a) {op} {function}(b)\n"
                )
            }
            InferredProperty::NoException { function } => {
                format!(
                    "from hypothesis import given, strategies as st\n\n\
                     @given(x=st.text())\n\
                     def test_{function}_no_exception(x):\n    \
                         {function}(x)  # should not raise\n"
                )
            }
            InferredProperty::LengthPreserving { function } => {
                format!(
                    "from hypothesis import given, strategies as st\n\n\
                     @given(xs=st.lists(st.integers()))\n\
                     def test_{function}_length_preserving(xs):\n    \
                         assert len({function}(xs)) == len(xs)\n"
                )
            }
            InferredProperty::RoundTrip { encode, decode } => {
                format!(
                    "from hypothesis import given, strategies as st\n\n\
                     @given(x=st.text())\n\
                     def test_{encode}_{decode}_roundtrip(x):\n    \
                         assert {decode}({encode}(x)) == x\n"
                )
            }
        }
    }

    /// Extract function names from source code.
    /// Supports Python `def name(`, Rust `fn name(`, JS `function name(`.
    fn extract_function_names(source: &str) -> Vec<String> {
        let mut names = Vec::new();
        for line in source.lines() {
            let trimmed = line.trim();
            // Python: def func_name(
            if let Some(rest) = trimmed.strip_prefix("def ") {
                if let Some(name) = rest.split('(').next() {
                    let name = name.trim();
                    if !name.is_empty() {
                        names.push(name.to_string());
                    }
                }
            }
            // Rust: fn func_name( or pub fn func_name(
            else if let Some(pos) = trimmed.find("fn ") {
                let after_fn = &trimmed[pos + 3..];
                if let Some(name) = after_fn.split('(').next() {
                    let name = name.trim();
                    if !name.is_empty() && name.chars().all(|c| c.is_alphanumeric() || c == '_') {
                        names.push(name.to_string());
                    }
                }
            }
            // JS: function func_name(
            else if let Some(rest) = trimmed.strip_prefix("function ") {
                if let Some(name) = rest.split('(').next() {
                    let name = name.trim();
                    if !name.is_empty() {
                        names.push(name.to_string());
                    }
                }
            }
        }
        names
    }

    /// Check if a function is public in the source.
    fn is_public_function(source: &str, func_name: &str) -> bool {
        for line in source.lines() {
            let trimmed = line.trim();
            // Python: def at module level (no leading whitespace or with `def`)
            if trimmed.starts_with(&format!("def {func_name}(")) {
                return true;
            }
            // Rust: pub fn
            if trimmed.contains(&format!("pub fn {func_name}(")) {
                return true;
            }
            // JS: export function or function at top level
            if trimmed.starts_with(&format!("function {func_name}("))
                || trimmed.contains(&format!("export function {func_name}("))
            {
                return true;
            }
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn infer_idempotent_from_sort() {
        let source = "def sort_items(xs):\n    return sorted(xs)\n";
        let props = PropertyInferer::infer(source);
        assert!(props.contains(&InferredProperty::Idempotent {
            function: "sort_items".into(),
        }));
    }

    #[test]
    fn infer_roundtrip_from_encode_decode() {
        let source = "def encode(data):\n    pass\ndef decode(data):\n    pass\n";
        let props = PropertyInferer::infer(source);
        assert!(props.contains(&InferredProperty::RoundTrip {
            encode: "encode".into(),
            decode: "decode".into(),
        }));
    }

    #[test]
    fn infer_commutative_from_merge() {
        let source = "def merge(a, b):\n    return a + b\n";
        let props = PropertyInferer::infer(source);
        assert!(props.contains(&InferredProperty::Commutative {
            function: "merge".into(),
        }));
    }

    #[test]
    fn infer_no_exception_from_public_fn() {
        let source = "def process(data):\n    return data.strip()\n";
        let props = PropertyInferer::infer(source);
        assert!(props.contains(&InferredProperty::NoException {
            function: "process".into(),
        }));
    }

    #[test]
    fn infer_length_preserving_from_map() {
        let source = "def map_items(xs):\n    return [x * 2 for x in xs]\n";
        let props = PropertyInferer::infer(source);
        assert!(props.contains(&InferredProperty::LengthPreserving {
            function: "map_items".into(),
        }));
    }

    #[test]
    fn infer_empty_source_no_properties() {
        let props = PropertyInferer::infer("");
        assert!(props.is_empty());
    }

    #[test]
    fn infer_multiple_properties() {
        let source = "\
def sort_list(xs):
    return sorted(xs)
def encode(data):
    pass
def decode(data):
    pass
def merge(a, b):
    return a + b
";
        let props = PropertyInferer::infer(source);
        // Should have idempotent, roundtrip, commutative, and NoException entries
        assert!(props.len() >= 4);
        assert!(props.iter().any(|p| matches!(p, InferredProperty::Idempotent { .. })));
        assert!(props.iter().any(|p| matches!(p, InferredProperty::RoundTrip { .. })));
        assert!(props.iter().any(|p| matches!(p, InferredProperty::Commutative { .. })));
    }

    #[test]
    fn generate_idempotent_test() {
        let prop = InferredProperty::Idempotent {
            function: "sort".into(),
        };
        let test = PropertyInferer::generate_hypothesis_test(&prop, "python");
        assert!(test.contains("sort(sort(x)) == sort(x)"));
        assert!(test.contains("hypothesis"));
    }

    #[test]
    fn generate_roundtrip_test() {
        let prop = InferredProperty::RoundTrip {
            encode: "encode".into(),
            decode: "decode".into(),
        };
        let test = PropertyInferer::generate_hypothesis_test(&prop, "python");
        assert!(test.contains("decode(encode(x)) == x"));
    }

    #[test]
    fn generate_commutative_test() {
        let prop = InferredProperty::Commutative {
            function: "add".into(),
        };
        let test = PropertyInferer::generate_hypothesis_test(&prop, "python");
        assert!(test.contains("add(a, b) == add(b, a)"));
    }

    #[test]
    fn generate_no_exception_test() {
        let prop = InferredProperty::NoException {
            function: "process".into(),
        };
        let test = PropertyInferer::generate_hypothesis_test(&prop, "python");
        assert!(test.contains("process(x)"));
        assert!(!test.contains("assert"));
    }

    #[test]
    fn infer_normalize_is_idempotent() {
        let source = "def normalize(text):\n    return text.lower().strip()\n";
        let props = PropertyInferer::infer(source);
        assert!(props.contains(&InferredProperty::Idempotent {
            function: "normalize".into(),
        }));
    }
}
