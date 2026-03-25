use apex_core::error::Result;
use apex_core::types::Language;
use async_trait::async_trait;
use regex::Regex;
use std::collections::HashSet;
use std::sync::LazyLock;
use uuid::Uuid;

use super::util::{find_loop_scopes, in_any_scope, is_comment};
use crate::context::AnalysisContext;
use crate::finding::{Finding, FindingCategory, Severity};
use crate::Detector;

pub struct AlgorithmicComplexityDetector;

// ---------------------------------------------------------------------------
// Loop header extraction helpers
// ---------------------------------------------------------------------------

/// Extract the collection/variable name from a Python `for x in <expr>:` header.
/// Returns `None` if the line is not a for-loop header or the expression is
/// too complex to summarise in a single token.
fn py_for_collection(line: &str) -> Option<String> {
    // Match: `for VAR in EXPR:` — capture the first word of EXPR.
    static RE: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"^\s*for\s+\S+\s+in\s+(\w[\w.]*)").expect("py for collection regex")
    });
    RE.captures(line)
        .and_then(|c| c.get(1))
        .map(|m| m.as_str().to_string())
}

/// Extract the iterable variable from a JS `for (... of EXPR)` or `for (... in EXPR)` header.
fn js_for_collection(line: &str) -> Option<String> {
    static RE: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"\bfor\s*\([^)]*\b(?:of|in)\s+(\w[\w.]*)").expect("js for collection regex")
    });
    RE.captures(line)
        .and_then(|c| c.get(1))
        .map(|m| m.as_str().to_string())
}

/// Extract the iterable from a Rust `for x in &?EXPR` header.
fn rust_for_collection(line: &str) -> Option<String> {
    static RE: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"\bfor\s+\S+\s+in\s+&?(\w[\w.]*)").expect("rust for collection regex")
    });
    RE.captures(line)
        .and_then(|c| c.get(1))
        .map(|m| m.as_str().to_string())
}

// ---------------------------------------------------------------------------
// forEach nested-loop detection (JavaScript)
// ---------------------------------------------------------------------------

/// JS: detect `.forEach(` on the same line as an existing forEach scope.
/// Returns true if the line contains a `.forEach(` call and we are already
/// inside another forEach body (i.e., the same collection pattern repeated).
fn js_foreach_line(line: &str) -> bool {
    static RE: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"\.forEach\s*\(").expect("forEach regex"));
    RE.is_match(line)
}

// ---------------------------------------------------------------------------
// Recursion detection helpers
// ---------------------------------------------------------------------------

/// Python: detect `def NAME(` header — returns the function name.
fn py_fn_name(line: &str) -> Option<String> {
    static RE: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"^\s*def\s+(\w+)\s*\(").expect("py fn name regex"));
    RE.captures(line)
        .and_then(|c| c.get(1))
        .map(|m| m.as_str().to_string())
}

/// JS: detect `function NAME(` or arrow/method — returns the function name.
fn js_fn_name(line: &str) -> Option<String> {
    static RE: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"(?:function\s+(\w+)|(?:const|let|var)\s+(\w+)\s*=\s*(?:async\s+)?(?:function|\(.*\)\s*=>))")
            .expect("js fn name regex")
    });
    RE.captures(line).and_then(|c| {
        c.get(1)
            .or_else(|| c.get(2))
            .map(|m| m.as_str().to_string())
    })
}

/// Rust: detect `fn NAME(` — returns the function name.
fn rust_fn_name(line: &str) -> Option<String> {
    static RE: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"^\s*(?:pub\s+(?:\w+\s+)?)?(?:async\s+)?fn\s+(\w+)\s*[<(]")
            .expect("rust fn name regex")
    });
    RE.captures(line)
        .and_then(|c| c.get(1))
        .map(|m| m.as_str().to_string())
}

// ---------------------------------------------------------------------------
// Memoization suppression indicators
// ---------------------------------------------------------------------------

static PY_MEMO_INDICATORS: &[&str] = &[
    "@lru_cache",
    "@functools.cache",
    "@functools.lru_cache",
    "cache[",
    "memo[",
    "_memo",
    "_cache",
    "memoize",
];

static JS_MEMO_INDICATORS: &[&str] = &[
    "cache[", "memo[", "memoize", "_cache", "_memo", "Map.get(", ".has(",
];

static RUST_MEMO_INDICATORS: &[&str] = &[
    "HashMap",
    "BTreeMap",
    "cache.get(",
    "cache.insert(",
    "memo.get(",
    "memo.insert(",
];

fn has_memoization(source: &str, indicators: &[&str]) -> bool {
    indicators.iter().any(|ind| source.contains(ind))
}

// ---------------------------------------------------------------------------
// Quadratic list-building patterns
// ---------------------------------------------------------------------------

/// Python: `result = result + [` or `result += [` inside a loop.
static PY_LIST_CONCAT: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?:\w+\s*=\s*\w+\s*\+\s*\[|\w+\s*\+=\s*\[)").expect("py list concat regex")
});

/// JS: `.concat(` call (array concat is O(n²) in a loop).
static JS_ARRAY_CONCAT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\.concat\s*\(").expect("js array concat regex"));

// ---------------------------------------------------------------------------
// Core analysis
// ---------------------------------------------------------------------------

fn analyze_source(path: &std::path::Path, source: &str, lang: Language) -> Vec<Finding> {
    let mut findings = Vec::new();

    findings.extend(detect_nested_loops(path, source, lang));
    findings.extend(detect_unmemoized_recursion(path, source, lang));
    findings.extend(detect_quadratic_list_build(path, source, lang));

    findings
}

// ---------------------------------------------------------------------------
// Pattern 1: nested loops over the same collection
// ---------------------------------------------------------------------------

fn detect_nested_loops(path: &std::path::Path, source: &str, lang: Language) -> Vec<Finding> {
    let lines: Vec<&str> = source.lines().collect();
    let outer_scopes = find_loop_scopes(source, lang);

    if outer_scopes.len() < 2 {
        // Need at least 2 loop scopes for nesting to be possible.
        // (One outer scope will contain inner scope lines.)
        // We still proceed — a single outer scope may enclose inner loops.
    }

    // Collect outer loop header lines and their collection names.
    // Strategy: for each loop header line, note what collection it iterates.
    // Then check if any line *inside* that loop scope is another loop header
    // over the same collection.

    let extract_collection: fn(&str) -> Option<String> = match lang {
        Language::Python => py_for_collection,
        Language::JavaScript => js_for_collection,
        Language::Rust => rust_for_collection,
        _ => return Vec::new(),
    };

    let mut findings = Vec::new();
    let mut reported_lines: HashSet<usize> = HashSet::new();

    for (outer_line_idx, outer_line) in lines.iter().enumerate() {
        let trimmed = outer_line.trim();
        if trimmed.is_empty() || is_comment(trimmed, lang) {
            continue;
        }

        // Determine if this line is a loop header outside of any tracked scope
        // (i.e., it is itself an outer loop header).
        let outer_collection = match extract_collection(outer_line) {
            Some(c) if !c.is_empty() => c,
            _ => {
                // Also handle JS forEach as an outer loop.
                if lang == Language::JavaScript && js_foreach_line(outer_line) {
                    // We can't easily extract the collection for forEach here;
                    // mark with a sentinel and handle below.
                    "__foreach__".to_string()
                } else {
                    continue;
                }
            }
        };

        // Find the scope(s) that start at or after this outer loop header.
        // The outer scope body is the one whose start_line > outer_line_idx.
        let outer_scope = outer_scopes
            .iter()
            .find(|s| s.start_line > outer_line_idx && s.start_line <= outer_line_idx + 3);

        let outer_scope = match outer_scope {
            Some(s) => s,
            None => continue,
        };

        // Search inside this outer scope for an inner loop over the same collection.
        for inner_line_idx in outer_scope.start_line..=outer_scope.end_line {
            if inner_line_idx >= lines.len() {
                break;
            }
            let inner_line = lines[inner_line_idx];
            let inner_trimmed = inner_line.trim();
            if inner_trimmed.is_empty() || is_comment(inner_trimmed, lang) {
                continue;
            }

            let is_nested_loop = if outer_collection == "__foreach__" {
                // JS forEach: flag if there's another forEach inside
                lang == Language::JavaScript && js_foreach_line(inner_line)
            } else {
                match extract_collection(inner_line) {
                    Some(inner_col) => inner_col == outer_collection,
                    None => false,
                }
            };

            if is_nested_loop && !reported_lines.contains(&outer_line_idx) {
                reported_lines.insert(outer_line_idx);
                let line_1based = (outer_line_idx + 1) as u32;
                findings.push(Finding {
                    id: Uuid::new_v4(),
                    detector: "algorithmic-complexity".into(),
                    severity: Severity::Medium,
                    category: FindingCategory::PerformanceRisk,
                    file: path.to_path_buf(),
                    line: Some(line_1based),
                    title: "Nested loop over same collection — O(n\u{00B2}) complexity".into(),
                    description: format!(
                        "Outer loop at line {} iterates over the same collection as an inner \
                         loop at line {}. This results in O(n\u{00B2}) iterations and can cause \
                         severe performance degradation for large inputs.",
                        line_1based,
                        inner_line_idx + 1
                    ),
                    evidence: vec![],
                    covered: false,
                    suggestion: match lang {
                        Language::Python => "Consider using a HashSet/dict for O(1) lookup \
                             instead of a nested loop. If checking membership, replace \
                             the inner loop with `if item in set(outer_collection)`."
                            .into(),
                        Language::JavaScript => "Consider using a Set or Map for O(1) lookup. \
                             Replace the inner loop with `outerSet.has(item)` after \
                             building `const outerSet = new Set(arr)`."
                            .into(),
                        Language::Rust => "Consider using a HashSet for O(1) lookup. \
                             Build `let set: HashSet<_> = items.iter().collect()` \
                             before the outer loop and use `set.contains(&item)`."
                            .into(),
                        _ => "Consider replacing the inner loop with a hash-based O(1) lookup."
                            .into(),
                    },
                    explanation: None,
                    fix: None,
                    cwe_ids: vec![400],
                    noisy: false,
                    base_severity: None,
                    coverage_confidence: None,
                });
                break; // One finding per outer loop
            }
        }
    }

    findings
}

// ---------------------------------------------------------------------------
// Pattern 2: recursive functions without memoization
// ---------------------------------------------------------------------------

fn detect_unmemoized_recursion(
    path: &std::path::Path,
    source: &str,
    lang: Language,
) -> Vec<Finding> {
    let lines: Vec<&str> = source.lines().collect();
    let mut findings = Vec::new();

    let extract_fn_name: fn(&str) -> Option<String> = match lang {
        Language::Python => py_fn_name,
        Language::JavaScript => js_fn_name,
        Language::Rust => rust_fn_name,
        _ => return Vec::new(),
    };

    let memo_indicators: &[&str] = match lang {
        Language::Python => PY_MEMO_INDICATORS,
        Language::JavaScript => JS_MEMO_INDICATORS,
        Language::Rust => RUST_MEMO_INDICATORS,
        _ => return Vec::new(),
    };

    // For each function definition, find its body extent and check for
    // self-calls without memoization.
    let n = lines.len();
    let mut i = 0usize;

    while i < n {
        let line = lines[i];
        let trimmed = line.trim();

        if trimmed.is_empty() || is_comment(trimmed, lang) {
            i += 1;
            continue;
        }

        if let Some(fn_name) = extract_fn_name(line) {
            // Find the body extent for this function.
            let (body_start, body_end) = find_fn_body(lines.as_slice(), i, lang);

            if body_start > body_end || body_end >= n {
                i += 1;
                continue;
            }

            let body = lines[body_start..=body_end].join("\n");

            // Check if function calls itself.
            let self_call_pattern = format!("{}(", fn_name);
            if !body.contains(&self_call_pattern) {
                i += 1;
                continue;
            }

            // Check for memoization in the surrounding context:
            // look at the few lines before the definition (decorators) and the body.
            let context_start = i.saturating_sub(3);
            let context = lines[context_start..=body_end].join("\n");
            if has_memoization(&context, memo_indicators) {
                i += 1;
                continue;
            }

            let line_1based = (i + 1) as u32;
            findings.push(Finding {
                id: Uuid::new_v4(),
                detector: "algorithmic-complexity".into(),
                severity: Severity::High,
                category: FindingCategory::PerformanceRisk,
                file: path.to_path_buf(),
                line: Some(line_1based),
                title: format!(
                    "Recursive function `{}` without memoization — potential O(2^n) complexity",
                    fn_name
                ),
                description: format!(
                    "Function `{}` defined at line {} calls itself without memoization. \
                     Naive recursion on overlapping sub-problems (e.g., Fibonacci, subset \
                     enumeration) results in exponential time complexity O(2^n).",
                    fn_name, line_1based
                ),
                evidence: vec![],
                covered: false,
                suggestion: match lang {
                    Language::Python => format!(
                        "Add `@functools.lru_cache(maxsize=None)` above `def {}(...)` \
                         to cache previously computed results and reduce complexity to O(n).",
                        fn_name
                    ),
                    Language::JavaScript => format!(
                        "Introduce a memoization map: `const _cache = new Map()` before `{}`, \
                         then check `if (_cache.has(key)) return _cache.get(key)` at the start \
                         of the function body.",
                        fn_name
                    ),
                    Language::Rust => format!(
                        "Introduce a `HashMap` cache parameter or use a wrapper that caches \
                         results: `let mut cache: HashMap<_, _> = HashMap::new()` and pass \
                         `&mut cache` to `{}` on each recursive call.",
                        fn_name
                    ),
                    _ => "Add memoization to cache previously computed results.".into(),
                },
                explanation: None,
                fix: None,
                cwe_ids: vec![400],
                noisy: false,
                base_severity: None,
                coverage_confidence: None,
            });

            // Skip past this function's body to avoid re-analyzing inner defs.
            i = body_end + 1;
            continue;
        }

        i += 1;
    }

    findings
}

/// Find the line range [body_start, body_end] (0-based, inclusive) for the
/// function whose header is on `header_line`.
fn find_fn_body(lines: &[&str], header_line: usize, lang: Language) -> (usize, usize) {
    let n = lines.len();

    if matches!(lang, Language::Python) {
        // Indent-tracked: body starts at header_line+1 (first indented line)
        // and ends when indentation returns to header level.
        let header_indent = leading_spaces(lines[header_line]);
        let mut body_start = header_line + 1;
        while body_start < n && lines[body_start].trim().is_empty() {
            body_start += 1;
        }
        if body_start >= n {
            return (header_line + 1, header_line);
        }
        let mut body_end = body_start;
        let mut j = body_start + 1;
        while j < n {
            let l = lines[j];
            let lt = l.trim();
            if !lt.is_empty() {
                if leading_spaces(l) <= header_indent {
                    break;
                }
                body_end = j;
            }
            j += 1;
        }
        (body_start, body_end)
    } else {
        // Brace-tracked: find the opening `{` then the matching `}`.
        let mut depth: i32 = 0;
        let mut open_line: Option<usize> = None;
        let mut close_line: Option<usize> = None;
        let mut in_string: Option<char> = None;
        let mut prev_backslash = false;

        'outer: for offset in 0..(n - header_line) {
            let lnum = header_line + offset;
            for ch in lines[lnum].chars() {
                if let Some(q) = in_string {
                    if ch == q && !prev_backslash {
                        in_string = None;
                    }
                    prev_backslash = ch == '\\' && !prev_backslash;
                    continue;
                }
                prev_backslash = false;
                match ch {
                    '"' | '\'' | '`' => in_string = Some(ch),
                    '{' => {
                        if open_line.is_none() {
                            open_line = Some(lnum);
                        }
                        depth += 1;
                    }
                    '}' => {
                        depth -= 1;
                        if depth <= 0 && open_line.is_some() {
                            close_line = Some(lnum);
                            break 'outer;
                        }
                    }
                    _ => {}
                }
            }
        }

        match (open_line, close_line) {
            (Some(open), Some(close)) if open < close => (open + 1, close.saturating_sub(1)),
            (Some(open), Some(close)) => (open, close),
            _ => (header_line + 1, header_line),
        }
    }
}

fn leading_spaces(line: &str) -> usize {
    let mut count = 0usize;
    for ch in line.chars() {
        match ch {
            ' ' => count += 1,
            '\t' => count += 4,
            _ => break,
        }
    }
    count
}

// ---------------------------------------------------------------------------
// Pattern 3: quadratic list/array building in a loop
// ---------------------------------------------------------------------------

fn detect_quadratic_list_build(
    path: &std::path::Path,
    source: &str,
    lang: Language,
) -> Vec<Finding> {
    let lines: Vec<&str> = source.lines().collect();
    let loop_scopes = find_loop_scopes(source, lang);

    if loop_scopes.is_empty() {
        return Vec::new();
    }

    let mut findings = Vec::new();

    for (line_idx, line) in lines.iter().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() || is_comment(trimmed, lang) {
            continue;
        }

        if !in_any_scope(&loop_scopes, line_idx) {
            continue;
        }

        let flagged = match lang {
            Language::Python => PY_LIST_CONCAT.is_match(line),
            Language::JavaScript => {
                JS_ARRAY_CONCAT.is_match(line)
                    // Suppress `.push(` — that's O(1) amortized, not O(n²)
                    && !line.contains(".push(")
            }
            _ => false,
        };

        if flagged {
            let line_1based = (line_idx + 1) as u32;
            findings.push(Finding {
                id: Uuid::new_v4(),
                detector: "algorithmic-complexity".into(),
                severity: Severity::Medium,
                category: FindingCategory::PerformanceRisk,
                file: path.to_path_buf(),
                line: Some(line_1based),
                title: "Quadratic list/array building inside loop — O(n\u{00B2}) complexity".into(),
                description: format!(
                    "List/array concatenation at line {} is inside a loop. \
                     Each concatenation allocates a new array of growing size, \
                     resulting in O(n\u{00B2}) total work and memory allocations.",
                    line_1based
                ),
                evidence: vec![],
                covered: false,
                suggestion: match lang {
                    Language::Python => "Use `list.append(item)` inside the loop and convert \
                         to a list when done, or use a list comprehension outside the loop. \
                         Avoid `result = result + [item]` or `result += [item]` patterns."
                        .into(),
                    Language::JavaScript => "Use `arr.push(item)` for O(1) amortized appends \
                         instead of `arr.concat(item)`. Build a final array with `Array.from()` \
                         or spread syntax if needed."
                        .into(),
                    _ => "Use an O(1) append operation instead of creating a new collection \
                         on each iteration."
                        .into(),
                },
                explanation: None,
                fix: None,
                cwe_ids: vec![400],
                noisy: false,
                base_severity: None,
                coverage_confidence: None,
            });
        }
    }

    findings
}

// ---------------------------------------------------------------------------
// Detector trait implementation
// ---------------------------------------------------------------------------

#[async_trait]
impl Detector for AlgorithmicComplexityDetector {
    fn name(&self) -> &str {
        "algorithmic-complexity"
    }

    async fn analyze(&self, ctx: &AnalysisContext) -> Result<Vec<Finding>> {
        let mut findings = Vec::new();

        for (path, source) in &ctx.source_cache {
            let lang = match path.extension().and_then(|e| e.to_str()) {
                Some("rs") => Language::Rust,
                Some("py") => Language::Python,
                Some("js") | Some("jsx") => Language::JavaScript,
                Some("ts") | Some("tsx") => Language::JavaScript,
                _ => continue,
            };
            findings.extend(analyze_source(path, source, lang));
        }

        Ok(findings)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn detect_python(source: &str) -> Vec<Finding> {
        analyze_source(&PathBuf::from("src/app.py"), source, Language::Python)
    }

    fn detect_js(source: &str) -> Vec<Finding> {
        analyze_source(&PathBuf::from("src/app.js"), source, Language::JavaScript)
    }

    fn detect_rust(source: &str) -> Vec<Finding> {
        analyze_source(&PathBuf::from("src/lib.rs"), source, Language::Rust)
    }

    // -----------------------------------------------------------------------
    // Test 1: Nested loops over the same collection (Python) — should find
    // -----------------------------------------------------------------------

    #[test]
    fn detects_nested_loops_same_collection_python() {
        let src = "\
def find_duplicates(items):
    for x in items:
        for y in items:
            if x != y and x == y:
                print(x)
";
        let findings = detect_python(src);
        let nested: Vec<_> = findings
            .iter()
            .filter(|f| f.title.contains("Nested loop"))
            .collect();
        assert!(
            !nested.is_empty(),
            "Expected a nested-loop finding, got: {:?}",
            findings.iter().map(|f| &f.title).collect::<Vec<_>>()
        );
        assert_eq!(nested[0].severity, Severity::Medium);
        assert_eq!(nested[0].cwe_ids, vec![400]);
        assert_eq!(nested[0].category, FindingCategory::PerformanceRisk);
    }

    // -----------------------------------------------------------------------
    // Test 2: Nested loops over DIFFERENT collections — no finding
    // -----------------------------------------------------------------------

    #[test]
    fn no_finding_nested_loops_different_collections_python() {
        let src = "\
def combine(rows, cols):
    for r in rows:
        for c in cols:
            process(r, c)
";
        let findings = detect_python(src);
        let nested: Vec<_> = findings
            .iter()
            .filter(|f| f.title.contains("Nested loop"))
            .collect();
        assert!(
            nested.is_empty(),
            "Should not flag different collections, got: {:?}",
            nested.iter().map(|f| &f.title).collect::<Vec<_>>()
        );
    }

    // -----------------------------------------------------------------------
    // Test 3: Recursive function without memoization (Python) — should find
    // -----------------------------------------------------------------------

    #[test]
    fn detects_unmemoized_recursion_python() {
        let src = "\
def fib(n):
    if n <= 1:
        return n
    return fib(n - 1) + fib(n - 2)
";
        let findings = detect_python(src);
        let rec: Vec<_> = findings
            .iter()
            .filter(|f| f.title.contains("Recursive"))
            .collect();
        assert!(
            !rec.is_empty(),
            "Expected a recursion finding, got: {:?}",
            findings.iter().map(|f| &f.title).collect::<Vec<_>>()
        );
        assert_eq!(rec[0].severity, Severity::High);
        assert_eq!(rec[0].cwe_ids, vec![400]);
    }

    // -----------------------------------------------------------------------
    // Test 4: Recursive function WITH @lru_cache — no finding
    // -----------------------------------------------------------------------

    #[test]
    fn no_finding_memoized_recursion_lru_cache_python() {
        let src = "\
import functools

@functools.lru_cache(maxsize=None)
def fib(n):
    if n <= 1:
        return n
    return fib(n - 1) + fib(n - 2)
";
        let findings = detect_python(src);
        let rec: Vec<_> = findings
            .iter()
            .filter(|f| f.title.contains("Recursive"))
            .collect();
        assert!(
            rec.is_empty(),
            "Should not flag memoized recursion with @lru_cache, got: {:?}",
            rec.iter().map(|f| &f.title).collect::<Vec<_>>()
        );
    }

    // -----------------------------------------------------------------------
    // Test 5: Quadratic list building in a loop (Python) — should find
    // -----------------------------------------------------------------------

    #[test]
    fn detects_quadratic_list_build_python() {
        let src = "\
def build(items):
    result = []
    for item in items:
        result = result + [item]
    return result
";
        let findings = detect_python(src);
        let quad: Vec<_> = findings
            .iter()
            .filter(|f| f.title.contains("Quadratic"))
            .collect();
        assert!(
            !quad.is_empty(),
            "Expected a quadratic list-build finding, got: {:?}",
            findings.iter().map(|f| &f.title).collect::<Vec<_>>()
        );
        assert_eq!(quad[0].severity, Severity::Medium);
        assert_eq!(quad[0].cwe_ids, vec![400]);
    }

    // -----------------------------------------------------------------------
    // Test 6: Normal list.append() in a loop — no finding
    // -----------------------------------------------------------------------

    #[test]
    fn no_finding_list_append_in_loop_python() {
        let src = "\
def build(items):
    result = []
    for item in items:
        result.append(item)
    return result
";
        let findings = detect_python(src);
        let quad: Vec<_> = findings
            .iter()
            .filter(|f| f.title.contains("Quadratic"))
            .collect();
        assert!(
            quad.is_empty(),
            "Should not flag list.append(), got: {:?}",
            quad.iter().map(|f| &f.title).collect::<Vec<_>>()
        );
    }

    // -----------------------------------------------------------------------
    // Additional: JS nested loops same collection
    // -----------------------------------------------------------------------

    #[test]
    fn detects_nested_loops_same_collection_js() {
        let src = "\
function findPairs(arr) {
    for (const x of arr) {
        for (const y of arr) {
            if (x !== y) console.log(x, y);
        }
    }
}
";
        let findings = detect_js(src);
        let nested: Vec<_> = findings
            .iter()
            .filter(|f| f.title.contains("Nested loop"))
            .collect();
        assert!(
            !nested.is_empty(),
            "Expected nested-loop finding for JS, got: {:?}",
            findings.iter().map(|f| &f.title).collect::<Vec<_>>()
        );
    }

    // -----------------------------------------------------------------------
    // Additional: JS array concat in loop
    // -----------------------------------------------------------------------

    #[test]
    fn detects_array_concat_in_loop_js() {
        let src = "\
function build(items) {
    let result = [];
    for (const item of items) {
        result = result.concat(item);
    }
    return result;
}
";
        let findings = detect_js(src);
        let quad: Vec<_> = findings
            .iter()
            .filter(|f| f.title.contains("Quadratic"))
            .collect();
        assert!(
            !quad.is_empty(),
            "Expected quadratic array-build finding for JS, got: {:?}",
            findings.iter().map(|f| &f.title).collect::<Vec<_>>()
        );
    }

    // -----------------------------------------------------------------------
    // Additional: Rust nested loops same collection
    // -----------------------------------------------------------------------

    #[test]
    fn detects_nested_loops_same_collection_rust() {
        let src = "\
fn find_pairs(items: &[i32]) {
    for x in &items {
        for y in &items {
            if x != y { println!(\"{} {}\", x, y); }
        }
    }
}
";
        let findings = detect_rust(src);
        let nested: Vec<_> = findings
            .iter()
            .filter(|f| f.title.contains("Nested loop"))
            .collect();
        assert!(
            !nested.is_empty(),
            "Expected nested-loop finding for Rust, got: {:?}",
            findings.iter().map(|f| &f.title).collect::<Vec<_>>()
        );
    }

    // -----------------------------------------------------------------------
    // Additional: Rust recursive function without memoization
    // -----------------------------------------------------------------------

    #[test]
    fn detects_unmemoized_recursion_rust() {
        let src = "\
fn fib(n: u64) -> u64 {
    if n <= 1 {
        return n;
    }
    fib(n - 1) + fib(n - 2)
}
";
        let findings = detect_rust(src);
        let rec: Vec<_> = findings
            .iter()
            .filter(|f| f.title.contains("Recursive"))
            .collect();
        assert!(
            !rec.is_empty(),
            "Expected recursion finding for Rust, got: {:?}",
            findings.iter().map(|f| &f.title).collect::<Vec<_>>()
        );
        assert_eq!(rec[0].severity, Severity::High);
    }

    // -----------------------------------------------------------------------
    // Additional: Rust recursive function WITH HashMap cache — no finding
    // -----------------------------------------------------------------------

    #[test]
    fn no_finding_memoized_recursion_hashmap_rust() {
        let src = "\
fn fib(n: u64, cache: &mut HashMap<u64, u64>) -> u64 {
    if let Some(&v) = cache.get(&n) {
        return v;
    }
    if n <= 1 {
        cache.insert(n, n);
        return n;
    }
    let result = fib(n - 1, cache) + fib(n - 2, cache);
    cache.insert(n, result);
    result
}
";
        let findings = detect_rust(src);
        let rec: Vec<_> = findings
            .iter()
            .filter(|f| f.title.contains("Recursive"))
            .collect();
        assert!(
            rec.is_empty(),
            "Should not flag memoized recursion with HashMap cache, got: {:?}",
            rec.iter().map(|f| &f.title).collect::<Vec<_>>()
        );
    }

    // -----------------------------------------------------------------------
    // Additional: JS unmemoized recursion
    // -----------------------------------------------------------------------

    #[test]
    fn detects_unmemoized_recursion_js() {
        let src = "\
function fib(n) {
    if (n <= 1) return n;
    return fib(n - 1) + fib(n - 2);
}
";
        let findings = detect_js(src);
        let rec: Vec<_> = findings
            .iter()
            .filter(|f| f.title.contains("Recursive"))
            .collect();
        assert!(
            !rec.is_empty(),
            "Expected recursion finding for JS, got: {:?}",
            findings.iter().map(|f| &f.title).collect::<Vec<_>>()
        );
    }

    // -----------------------------------------------------------------------
    // Additional: noisy field is false
    // -----------------------------------------------------------------------

    #[test]
    fn findings_not_noisy() {
        let src = "\
def fib(n):
    if n <= 1:
        return n
    return fib(n - 1) + fib(n - 2)
";
        let findings = detect_python(src);
        assert!(findings.iter().all(|f| !f.noisy));
    }

    // -----------------------------------------------------------------------
    // Additional: detector name is correct
    // -----------------------------------------------------------------------

    #[test]
    fn detector_name() {
        assert_eq!(
            AlgorithmicComplexityDetector.name(),
            "algorithmic-complexity"
        );
    }
}
