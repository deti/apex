//! Python condition text → SMTLIB2 conversion.
//!
//! Handles simple comparison patterns produced by `apex_tracer.py`:
//!
//!   `x > 0`       → `(> x 0)`
//!   `count >= 5`  → `(>= count 5)`
//!   `n == 3`      → `(= n 3)`
//!   `x != 0`      → `(not (= x 0))`
//!   `y < 10`      → `(< y 10)`
//!   `z <= 7`      → `(<= z 7)`
//!
//! Attribute access like `self.value` is encoded as `self_DOT_value` to keep
//! SMTLIB2 identifiers valid.

// ---------------------------------------------------------------------------
// Condition → SMTLIB2
// ---------------------------------------------------------------------------

/// Convert a Python comparison condition to an SMTLIB2 expression string.
///
/// Returns `None` for compound or unsupported conditions.
pub fn condition_to_smtlib2(condition: &str) -> Option<String> {
    let s = condition.trim();

    // Try operators in length-descending order so `>=` is matched before `>`.
    const OPS: &[&str] = &[">=", "<=", "==", "!=", ">", "<"];

    for op in OPS {
        if let Some(idx) = s.find(op) {
            // Make sure the match is the operator and not part of an identifier.
            let before = &s[..idx];
            let after = &s[idx + op.len()..];

            let lhs = before.trim();
            let rhs = after.trim();

            if !is_identifier(lhs) {
                continue;
            }

            // Only integer literals supported for now.
            if let Ok(n) = rhs.parse::<i64>() {
                let smt_name = encode_identifier(lhs);
                let smt = match *op {
                    ">" => format!("(> {smt_name} {n})"),
                    ">=" => format!("(>= {smt_name} {n})"),
                    "<" => format!("(< {smt_name} {n})"),
                    "<=" => format!("(<= {smt_name} {n})"),
                    "==" => format!("(= {smt_name} {n})"),
                    "!=" => format!("(not (= {smt_name} {n}))"),
                    _ => {
                        debug_assert!(false, "unexpected op {op} in OPS list");
                        return None;
                    }
                };
                return Some(smt);
            }
        }
    }

    None
}

/// Encode a Python identifier for use in SMTLIB2 (dots → `_DOT_`).
fn encode_identifier(s: &str) -> String {
    s.replace('.', "_DOT_")
}

fn is_identifier(s: &str) -> bool {
    if s.is_empty() {
        return false;
    }
    let mut chars = s.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if !first.is_alphabetic() && first != '_' {
        return false;
    }
    chars.all(|c| c.is_alphanumeric() || c == '_' || c == '.')
}

// ---------------------------------------------------------------------------
// Variable extraction
// ---------------------------------------------------------------------------

/// SMTLIB2 keywords and operators that should not be treated as variables.
static KEYWORDS: &[&str] = &[
    "assert",
    "not",
    "and",
    "or",
    "xor",
    "iff",
    "=>",
    "ite",
    "true",
    "false",
    "let",
    "forall",
    "exists",
    "Int",
    "Bool",
    "Real",
    "declare-const",
    "check-sat",
    "get-model",
    "define-fun",
];

/// Extract all variable names (non-keyword identifiers) from an SMTLIB2 expression.
///
/// Only identifiers that start with a letter or `_` and aren't SMTLIB2 keywords
/// are returned. Used by the Z3 solver to declare `(declare-const …)` before
/// asserting constraints.
pub fn extract_variables(smtlib2: &str) -> Vec<String> {
    let mut vars: Vec<String> = Vec::new();
    let mut chars = smtlib2.char_indices().peekable();

    while let Some(&(_, c)) = chars.peek() {
        if c.is_ascii_alphabetic() || c == '_' {
            let start = match chars.peek() {
                Some(&(i, _)) => i,
                None => break,
            };
            let mut end = start;
            while let Some(&(i, ch)) = chars.peek() {
                if ch.is_ascii_alphanumeric() || ch == '_' || ch == '.' {
                    end = i + ch.len_utf8();
                    chars.next();
                } else {
                    break;
                }
            }
            let token = &smtlib2[start..end];
            if !KEYWORDS.contains(&token) && token.parse::<i64>().is_err() {
                let owned = token.to_string();
                if !vars.contains(&owned) {
                    vars.push(owned);
                }
            }
        } else {
            chars.next();
        }
    }

    vars
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_gt() {
        assert_eq!(condition_to_smtlib2("x > 0"), Some("(> x 0)".into()));
    }

    #[test]
    fn test_ge() {
        assert_eq!(
            condition_to_smtlib2("count >= 5"),
            Some("(>= count 5)".into())
        );
    }

    #[test]
    fn test_eq() {
        assert_eq!(condition_to_smtlib2("n == 3"), Some("(= n 3)".into()));
    }

    #[test]
    fn test_ne() {
        assert_eq!(condition_to_smtlib2("x != 0"), Some("(not (= x 0))".into()));
    }

    #[test]
    fn test_lt_negative() {
        assert_eq!(condition_to_smtlib2("y < -1"), Some("(< y -1)".into()));
    }

    #[test]
    fn test_unsupported() {
        assert_eq!(condition_to_smtlib2("x > 0 and y < 5"), None);
        assert_eq!(condition_to_smtlib2("len(lst) > 0"), None);
    }

    #[test]
    fn test_extract_vars() {
        let vars = extract_variables("(> x 0)");
        assert_eq!(vars, vec!["x"]);

        let vars = extract_variables("(not (= count 3))");
        assert_eq!(vars, vec!["count"]);

        let vars = extract_variables("(and (> x 0) (< y 10))");
        assert!(vars.contains(&"x".to_string()));
        assert!(vars.contains(&"y".to_string()));
    }

    #[test]
    fn test_dot_encoding() {
        assert_eq!(
            condition_to_smtlib2("self.value > 0"),
            Some("(> self_DOT_value 0)".into())
        );
    }

    #[test]
    fn test_le() {
        assert_eq!(condition_to_smtlib2("z <= 7"), Some("(<= z 7)".into()));
    }

    #[test]
    fn test_whitespace_trimming() {
        assert_eq!(condition_to_smtlib2("  x > 0  "), Some("(> x 0)".into()));
    }

    #[test]
    fn test_empty_condition() {
        assert_eq!(condition_to_smtlib2(""), None);
    }

    #[test]
    fn test_non_integer_rhs() {
        assert_eq!(condition_to_smtlib2("x > foo"), None);
    }

    #[test]
    fn test_is_identifier_empty() {
        assert!(!is_identifier(""));
    }

    #[test]
    fn test_is_identifier_starts_with_digit() {
        assert!(!is_identifier("3abc"));
    }

    #[test]
    fn test_is_identifier_valid() {
        assert!(is_identifier("_foo"));
        assert!(is_identifier("bar"));
        assert!(is_identifier("self.x"));
    }

    #[test]
    fn test_encode_identifier_no_dots() {
        assert_eq!(encode_identifier("foo"), "foo");
    }

    #[test]
    fn test_encode_identifier_multiple_dots() {
        assert_eq!(encode_identifier("a.b.c"), "a_DOT_b_DOT_c");
    }

    #[test]
    fn test_extract_vars_empty() {
        let vars = extract_variables("");
        assert!(vars.is_empty());
    }

    #[test]
    fn test_extract_vars_no_duplicates() {
        let vars = extract_variables("(and (> x 0) (< x 10))");
        assert_eq!(vars.iter().filter(|v| *v == "x").count(), 1);
    }

    #[test]
    fn test_extract_vars_skips_keywords() {
        let vars = extract_variables("(and true false)");
        assert!(vars.is_empty());
    }

    #[test]
    fn test_extract_vars_with_underscores() {
        let vars = extract_variables("(> _my_var 0)");
        assert_eq!(vars, vec!["_my_var"]);
    }

    #[test]
    fn test_is_identifier_with_special_chars() {
        assert!(!is_identifier("foo!bar"));
        assert!(!is_identifier("x y"));
        assert!(is_identifier("_"));
        assert!(is_identifier("a123"));
    }

    #[test]
    fn test_condition_only_operator_no_rhs() {
        assert_eq!(condition_to_smtlib2("x >"), None);
    }

    #[test]
    fn test_condition_only_operator_no_lhs() {
        assert_eq!(condition_to_smtlib2("> 5"), None);
    }

    #[test]
    fn test_extract_vars_numeric_tokens_skipped() {
        // Numeric tokens that start with a letter (like identifiers) won't parse as i64
        let vars = extract_variables("(> abc 123)");
        assert_eq!(vars, vec!["abc"]);
    }

    #[test]
    fn test_extract_vars_only_parens_and_numbers() {
        let vars = extract_variables("(123 456)");
        assert!(vars.is_empty());
    }

    #[test]
    fn test_extract_vars_with_dots() {
        let vars = extract_variables("(> self.x 0)");
        assert_eq!(vars, vec!["self.x"]);
    }

    #[test]
    fn test_condition_underscore_var() {
        assert_eq!(
            condition_to_smtlib2("_x > 5"),
            Some("(> _x 5)".into())
        );
    }

    #[test]
    fn test_condition_large_number() {
        assert_eq!(
            condition_to_smtlib2("n == 999999"),
            Some("(= n 999999)".into())
        );
    }

    #[test]
    fn test_condition_negative_number_all_ops() {
        assert_eq!(condition_to_smtlib2("x >= -10"), Some("(>= x -10)".into()));
        assert_eq!(condition_to_smtlib2("x <= -1"), Some("(<= x -1)".into()));
        assert_eq!(condition_to_smtlib2("x == -5"), Some("(= x -5)".into()));
        assert_eq!(condition_to_smtlib2("x != -3"), Some("(not (= x -3))".into()));
    }

    // -----------------------------------------------------------------------
    // proptest properties
    // -----------------------------------------------------------------------

    use proptest::prelude::*;

    proptest! {
        #[test]
        fn prop_condition_roundtrip_gt(
            var in "[a-z][a-z_]{0,8}",
            val in -1000i64..1000,
        ) {
            let condition = format!("{var} > {val}");
            let result = condition_to_smtlib2(&condition);
            prop_assert!(result.is_some(), "failed for: {condition}");
            let smt = result.unwrap();
            prop_assert!(smt.starts_with("(> "));
            prop_assert!(smt.contains(&val.to_string()));
        }

        #[test]
        fn prop_condition_roundtrip_all_ops(
            var in "[a-z][a-z_]{0,8}",
            val in -1000i64..1000,
            op_idx in 0usize..6,
        ) {
            let ops = [">=", "<=", "==", "!=", ">", "<"];
            let op = ops[op_idx];
            let condition = format!("{var} {op} {val}");
            let result = condition_to_smtlib2(&condition);
            prop_assert!(result.is_some(), "failed for: {condition}");
        }

        #[test]
        fn prop_extract_variables_finds_all_inserted_vars(
            var1 in "[a-z][a-z_]{2,8}",
            var2 in "[a-z][a-z_]{2,8}",
        ) {
            // Skip SMTLIB2 keywords — extract_variables correctly filters them
            if KEYWORDS.contains(&var1.as_str()) || KEYWORDS.contains(&var2.as_str()) {
                return Ok(());
            }
            let expr = format!("(and (> {} 0) (< {} 10))", var1, var2);
            let vars = extract_variables(&expr);
            prop_assert!(vars.contains(&var1), "missing var1={} in {:?}", var1, vars);
            if var1 != var2 {
                prop_assert!(vars.contains(&var2), "missing var2={} in {:?}", var2, vars);
            }
        }

        /// Fuzz-like: arbitrary strings should never panic condition_to_smtlib2.
        #[test]
        fn prop_condition_to_smtlib2_never_panics(s in "\\PC{0,64}") {
            // Should return Some or None, never panic
            let _ = condition_to_smtlib2(&s);
        }

        /// Fuzz-like: arbitrary strings should never panic extract_variables.
        #[test]
        fn prop_extract_variables_never_panics(s in "\\PC{0,128}") {
            // Should return a Vec, never panic
            let _ = extract_variables(&s);
        }

        #[test]
        fn prop_encode_identifier_dots(
            base in "[a-z]{1,4}",
            parts in proptest::collection::vec("[a-z]{1,4}", 0..4),
        ) {
            let ident = if parts.is_empty() {
                base.clone()
            } else {
                format!("{}.{}", base, parts.join("."))
            };
            let encoded = encode_identifier(&ident);
            // No dots should remain in the encoded form
            prop_assert!(!encoded.contains('.'), "dots remain in: {encoded}");
            // Encoding should be deterministic
            prop_assert_eq!(&encoded, &encode_identifier(&ident));
        }
    }

    // ------------------------------------------------------------------
    // Additional gap-filling tests
    // ------------------------------------------------------------------

    #[test]
    fn test_condition_lt() {
        assert_eq!(condition_to_smtlib2("y < 10"), Some("(< y 10)".into()));
    }

    #[test]
    fn test_is_identifier_with_digit_not_first() {
        // Digit after first char is valid
        assert!(is_identifier("a1b2c3"));
        assert!(is_identifier("_123"));
    }

    #[test]
    fn test_is_identifier_starts_with_hyphen() {
        assert!(!is_identifier("-bad"));
    }

    #[test]
    fn test_extract_vars_all_keywords_skipped() {
        for kw in KEYWORDS {
            let expr = format!("(= {} 0)", kw);
            let vars = extract_variables(&expr);
            assert!(
                !vars.contains(&kw.to_string()),
                "keyword '{}' should not appear in vars",
                kw
            );
        }
    }

    #[test]
    fn test_extract_vars_multiple_dots() {
        let vars = extract_variables("(> self.value.x 0)");
        assert!(vars.contains(&"self.value.x".to_string()));
    }

    #[test]
    fn test_condition_to_smtlib2_lhs_not_identifier_skips() {
        // LHS with a digit should fail is_identifier check and return None
        assert_eq!(condition_to_smtlib2("1invalid > 5"), None);
    }

    #[test]
    fn test_condition_to_smtlib2_rhs_variable_returns_none() {
        // RHS that's a var name (not integer) should return None
        assert_eq!(condition_to_smtlib2("x > y"), None);
    }

    #[test]
    fn test_extract_vars_numeric_false_positive_never_occurs() {
        // Verify tokens that parse as i64 are excluded
        let vars = extract_variables("(> x 42)");
        assert!(!vars.contains(&"42".to_string()));
        assert!(vars.contains(&"x".to_string()));
    }

    #[test]
    fn test_condition_ge_negative_large() {
        assert_eq!(condition_to_smtlib2("n >= -1000"), Some("(>= n -1000)".into()));
    }

    #[test]
    fn test_extract_vars_underscore_only() {
        let vars = extract_variables("(> _ 0)");
        assert!(vars.contains(&"_".to_string()));
    }
}
