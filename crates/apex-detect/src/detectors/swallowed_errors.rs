use apex_core::error::Result;
use apex_core::types::Language;
use async_trait::async_trait;
use regex::Regex;
use std::sync::LazyLock;
use uuid::Uuid;

use super::util::find_except_scopes;
use crate::context::AnalysisContext;
use crate::finding::{Finding, FindingCategory, Severity};
use crate::Detector;

pub struct SwallowedErrorsDetector;

/// JS `catch` opener
static JS_CATCH: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\bcatch\s*\(").expect("js catch regex"));

/// Go `if err != nil {` opener
static GO_ERR_NIL: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\bif\s+err\s*!=\s*nil").expect("go err nil regex"));

/// Body is "empty" if every non-empty line is pass, a comment, or whitespace.
fn python_body_is_empty(lines: &[&str], start: usize, end: usize) -> bool {
    for t in lines.iter().take(end + 1).skip(start).map(|l| l.trim()) {
        if t.is_empty() || t == "pass" || t.starts_with('#') {
            continue;
        }
        return false;
    }
    true
}

/// Scan lines starting from `opener_line` to find the `{` opening brace and
/// matching `}` close brace.  Returns `(open_line, close_line)` or `None` if
/// not found.  Then checks whether the interior is empty/comment-only.
///
/// This handles the case that `find_scopes` deliberately skips empty brace bodies.
fn brace_body_is_empty_from(lines: &[&str], opener_line: usize) -> Option<(usize, usize)> {
    // Skip any `}` that appear before the first `{` (they belong to enclosing blocks).
    let mut searching = true;
    let mut open_line: Option<usize> = None;
    let mut close_line: Option<usize> = None;
    let mut depth = 0i32;

    for (idx, line) in lines.iter().enumerate().skip(opener_line) {
        for ch in line.chars() {
            if searching {
                if ch == '{' {
                    open_line = Some(idx);
                    depth = 1;
                    searching = false;
                }
                // Ignore `}` while searching for the opening brace
            } else {
                match ch {
                    '{' => depth += 1,
                    '}' => {
                        depth -= 1;
                        if depth == 0 {
                            close_line = Some(idx);
                        }
                    }
                    _ => {}
                }
            }
        }
        if close_line.is_some() {
            break;
        }
    }

    let (open, close) = (open_line?, close_line?);

    // Body is lines strictly between open and close braces.
    // For same-line `{}` — always empty.
    // For consecutive lines — body lines are open+1 .. close-1.
    if open == close {
        // Single line: check interior between { and }
        if let Some(ob) = lines[open].find('{') {
            let after = &lines[open][ob + 1..];
            if let Some(cb) = after.find('}') {
                let interior = after[..cb].trim();
                if interior.is_empty() {
                    return Some((open, close));
                }
            }
        }
        return None;
    }

    // Multi-line body: check whether every line between open and close is empty/comment
    for t in lines.iter().take(close).skip(open + 1).map(|l| l.trim()) {
        if t.is_empty() || t.starts_with("//") || t.starts_with("/*") || t.starts_with('*') {
            continue;
        }
        return None; // non-empty body
    }

    Some((open, close))
}

fn analyze_source(path: &std::path::Path, source: &str, lang: Language) -> Vec<Finding> {
    let lines: Vec<&str> = source.lines().collect();
    let mut findings = Vec::new();

    match lang {
        Language::Python => {
            let scopes = find_except_scopes(source, lang);
            for scope in &scopes {
                if python_body_is_empty(&lines, scope.start_line, scope.end_line) {
                    let line_1based = (scope.start_line + 1) as u32;
                    findings.push(make_finding(path, line_1based, lang));
                }
            }
        }
        Language::JavaScript | Language::Java => {
            for (line_idx, line) in lines.iter().enumerate() {
                if !JS_CATCH.is_match(line) {
                    continue;
                }
                if brace_body_is_empty_from(&lines, line_idx).is_some() {
                    let line_1based = (line_idx + 1) as u32;
                    findings.push(make_finding(path, line_1based, lang));
                }
            }
        }
        Language::Go => {
            for (line_idx, line) in lines.iter().enumerate() {
                if !GO_ERR_NIL.is_match(line) {
                    continue;
                }
                if brace_body_is_empty_from(&lines, line_idx).is_some() {
                    let line_1based = (line_idx + 1) as u32;
                    findings.push(make_finding(path, line_1based, lang));
                }
            }
        }
        _ => {}
    }

    findings
}

fn make_finding(path: &std::path::Path, line_1based: u32, lang: Language) -> Finding {
    let example = match lang {
        Language::Python => "except: pass",
        Language::Go => "if err != nil {}",
        _ => "catch(e) {}",
    };
    Finding {
        id: Uuid::new_v4(),
        detector: "swallowed-errors".into(),
        severity: Severity::Medium,
        category: FindingCategory::SecuritySmell,
        file: path.to_path_buf(),
        line: Some(line_1based),
        title: "Swallowed error — empty error handler".into(),
        description: format!(
            "Error handler at line {} silently discards the exception (`{}`). \
             Swallowed errors hide bugs, mask security issues, and make debugging extremely difficult.",
            line_1based, example
        ),
        evidence: vec![],
        covered: false,
        suggestion: "Log the error, return it to the caller, or add an explicit comment \
                     explaining why it is safe to ignore."
            .into(),
        explanation: None,
        fix: None,
        cwe_ids: vec![390],
                    noisy: false,
    }
}

#[async_trait]
impl Detector for SwallowedErrorsDetector {
    fn name(&self) -> &str {
        "swallowed-errors"
    }

    async fn analyze(&self, ctx: &AnalysisContext) -> Result<Vec<Finding>> {
        let mut findings = Vec::new();

        for (path, source) in &ctx.source_cache {
            let lang = match path.extension().and_then(|e| e.to_str()) {
                Some("py") => Language::Python,
                Some("js") | Some("jsx") => Language::JavaScript,
                Some("ts") | Some("tsx") => Language::JavaScript,
                Some("java") => Language::Java,
                Some("go") => Language::Go,
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

    fn detect_python(source: &str) -> Vec<Finding> {
        analyze_source(&PathBuf::from("src/app.py"), source, Language::Python)
    }

    fn detect_js(source: &str) -> Vec<Finding> {
        analyze_source(&PathBuf::from("src/app.js"), source, Language::JavaScript)
    }

    fn detect_go(source: &str) -> Vec<Finding> {
        analyze_source(&PathBuf::from("src/main.go"), source, Language::Go)
    }

    // ---- positive: Python ----

    #[test]
    fn detects_except_pass() {
        let src = "\
try:
    risky()
except:
    pass
";
        let findings = detect_python(src);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, Severity::Medium);
        assert_eq!(findings[0].cwe_ids, vec![390]);
    }

    #[test]
    fn detects_except_specific_pass() {
        let src = "\
try:
    risky()
except ValueError:
    pass
";
        let findings = detect_python(src);
        assert_eq!(findings.len(), 1);
    }

    // ---- positive: JS ----

    #[test]
    fn detects_empty_catch_js() {
        let src = "\
try {
    risky();
} catch (e) {
}
";
        let findings = detect_js(src);
        assert_eq!(findings.len(), 1);
    }

    // ---- positive: Go ----

    #[test]
    fn detects_empty_err_check_go() {
        let src = "\
result, err := doSomething()
if err != nil {
}
use(result)
";
        let findings = detect_go(src);
        assert_eq!(findings.len(), 1);
    }

    // ---- negative: non-empty handlers ----

    #[test]
    fn no_finding_python_with_log() {
        let src = "\
try:
    risky()
except Exception as e:
    log.error(e)
    raise
";
        let findings = detect_python(src);
        assert_eq!(findings.len(), 0);
    }

    #[test]
    fn no_finding_js_with_body() {
        let src = "\
try {
    risky();
} catch (e) {
    console.error(e);
}
";
        let findings = detect_js(src);
        assert_eq!(findings.len(), 0);
    }

    // ---- negative: only comments in catch (still flagged) ----

    #[test]
    fn detects_catch_with_only_comment_js() {
        let src = "\
try {
    risky();
} catch (e) {
    // TODO: handle this
}
";
        let findings = detect_js(src);
        // Comments-only body is still a swallowed error
        assert_eq!(findings.len(), 1);
    }

    // ---- edge: multiple swallowed ----

    #[test]
    fn detects_multiple_swallowed() {
        let src = "\
try:
    a()
except TypeError:
    pass

try:
    b()
except ValueError:
    pass
";
        let findings = detect_python(src);
        assert_eq!(findings.len(), 2);
    }
}
