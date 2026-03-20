use apex_core::error::Result;
use apex_core::types::Language;
use async_trait::async_trait;
use regex::Regex;
use std::sync::LazyLock;
use uuid::Uuid;

use super::util::{find_loop_scopes, in_any_scope, is_comment};
use crate::context::AnalysisContext;
use crate::finding::{Finding, FindingCategory, Severity};
use crate::Detector;

pub struct StringConcatInLoopDetector;

// ---------------------------------------------------------------------------
// Patterns
// ---------------------------------------------------------------------------

/// Rust: `s.push_str(` or `s += ` where the RHS is likely a string.
/// We detect `push_str(` and `+= ` but suppress numeric patterns like `+= 1`.
static RUST_PUSH_STR: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\.push_str\s*\(").expect("push_str regex"));

/// `+= ` assignment — could be numeric, we suppress if RHS is a number literal
static PLUS_ASSIGN: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\+=\s*").expect("+= regex"));

/// Numeric RHS: `+= 1`, `+= 0.5`, etc.
static NUMERIC_RHS: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\+=\s*\d").expect("numeric rhs regex"));

/// Python: `s += "..."` or `s = s + "..."` — string-specific.
/// We detect `+=` and suppress numeric.
static PY_PLUS_ASSIGN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\+=\s*").expect("py += regex"));

/// JS: `s += ` on a variable
static JS_PLUS_ASSIGN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\+=\s*").expect("js += regex"));

/// JS: `s = s +` pattern
static JS_STR_CONCAT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\w+\s*=\s*\w+\s*\+\s*").expect("js str concat regex"));

fn is_numeric_assign(line: &str) -> bool {
    NUMERIC_RHS.is_match(line)
}

fn analyze_source(path: &std::path::Path, source: &str, lang: Language) -> Vec<Finding> {
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
            Language::Rust => {
                let has_push_str = RUST_PUSH_STR.is_match(line);
                let has_plus_assign = PLUS_ASSIGN.is_match(line) && !is_numeric_assign(line);
                has_push_str || has_plus_assign
            }
            Language::Python => PY_PLUS_ASSIGN.is_match(line) && !is_numeric_assign(line),
            Language::JavaScript => {
                let has_plus_assign = JS_PLUS_ASSIGN.is_match(line) && !is_numeric_assign(line);
                // `s = s + x` pattern — only flag if not a number
                let has_concat = JS_STR_CONCAT.is_match(line) && !is_numeric_assign(line);
                has_plus_assign || has_concat
            }
            _ => false,
        };

        if flagged {
            let line_1based = (line_idx + 1) as u32;
            findings.push(Finding {
                id: Uuid::new_v4(),
                detector: "string-concat-in-loop".into(),
                severity: Severity::Low,
                category: FindingCategory::SecuritySmell,
                file: path.to_path_buf(),
                line: Some(line_1based),
                title: "String concatenation inside loop — O(n²) performance".into(),
                description: format!(
                    "String concatenation at line {} is inside a loop. \
                     Repeated allocation of new strings on each iteration is O(n²) \
                     and can cause excessive memory pressure for large inputs.",
                    line_1based
                ),
                evidence: vec![],
                covered: false,
                suggestion: match lang {
                    Language::Rust => "Collect into a `Vec<String>` then call `.join(\"\")`, \
                         or pre-allocate with `String::with_capacity(n)`."
                        .into(),
                    Language::Python => {
                        "Collect parts into a list then join: `''.join(parts)`".into()
                    }
                    _ => {
                        "Use an array and `.join('')` after the loop, or a template literal.".into()
                    }
                },
                explanation: None,
                fix: None,
                cwe_ids: vec![400],
            });
        }
    }

    findings
}

#[async_trait]
impl Detector for StringConcatInLoopDetector {
    fn name(&self) -> &str {
        "string-concat-in-loop"
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn detect_rust(source: &str) -> Vec<Finding> {
        analyze_source(&PathBuf::from("src/lib.rs"), source, Language::Rust)
    }

    fn detect_python(source: &str) -> Vec<Finding> {
        analyze_source(&PathBuf::from("src/app.py"), source, Language::Python)
    }

    fn detect_js(source: &str) -> Vec<Finding> {
        analyze_source(&PathBuf::from("src/app.js"), source, Language::JavaScript)
    }

    // ---- positive: Rust ----

    #[test]
    fn detects_push_str_in_loop_rust() {
        let src = "\
fn build(items: &[&str]) -> String {
    let mut s = String::new();
    for item in items {
        s.push_str(item);
    }
    s
}
";
        let findings = detect_rust(src);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, Severity::Low);
        assert_eq!(findings[0].cwe_ids, vec![400]);
    }

    #[test]
    fn detects_plus_assign_str_in_loop_rust() {
        let src = "\
fn build(items: &[&str]) -> String {
    let mut s = String::new();
    for item in items {
        s += item;
    }
    s
}
";
        let findings = detect_rust(src);
        assert_eq!(findings.len(), 1);
    }

    // ---- positive: Python ----

    #[test]
    fn detects_plus_assign_in_loop_python() {
        let src = "\
def build(items):
    result = ''
    for item in items:
        result += item
    return result
";
        let findings = detect_python(src);
        assert_eq!(findings.len(), 1);
    }

    // ---- positive: JS ----

    #[test]
    fn detects_plus_assign_in_loop_js() {
        let src = "\
function build(items) {
    let s = '';
    for (const item of items) {
        s += item;
    }
    return s;
}
";
        let findings = detect_js(src);
        assert_eq!(findings.len(), 1);
    }

    // ---- negative: numeric += not flagged ----

    #[test]
    fn no_finding_numeric_plus_assign_rust() {
        let src = "\
fn count(items: &[i32]) -> i32 {
    let mut total = 0;
    for item in items {
        total += 1;
    }
    total
}
";
        let findings = detect_rust(src);
        assert_eq!(findings.len(), 0);
    }

    #[test]
    fn no_finding_numeric_plus_assign_python() {
        let src = "\
def count(items):
    n = 0
    for item in items:
        n += 1
    return n
";
        let findings = detect_python(src);
        assert_eq!(findings.len(), 0);
    }

    // ---- negative: outside loop ----

    #[test]
    fn no_finding_push_str_outside_loop_rust() {
        let src = "\
fn build() -> String {
    let mut s = String::new();
    s.push_str(\"hello\");
    s
}
";
        let findings = detect_rust(src);
        assert_eq!(findings.len(), 0);
    }

    // ---- edge: while loop ----

    #[test]
    fn detects_in_while_loop_python() {
        let src = "\
def build():
    result = ''
    while cond():
        result += get_part()
    return result
";
        let findings = detect_python(src);
        assert_eq!(findings.len(), 1);
    }
}
