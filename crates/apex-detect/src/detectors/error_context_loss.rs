use apex_core::error::Result;
use apex_core::types::Language;
use async_trait::async_trait;
use regex::Regex;
use std::sync::LazyLock;
use uuid::Uuid;

use super::util::{find_except_scopes, in_any_scope, is_comment};

/// Scan `lines` for JS `catch (...)` openers and return their body line ranges.
/// Uses a forward-only brace scan that skips any `}` before the first `{`.
fn collect_catch_ranges(lines: &[&str]) -> Vec<(usize, usize)> {
    let mut ranges = Vec::new();
    let n = lines.len();

    for opener in 0..n {
        if !lines[opener].contains("catch") {
            continue;
        }
        // Forward scan: skip `}` until we find `{`, then track depth.
        let mut searching = true;
        let mut open_line: Option<usize> = None;
        let mut depth = 0i32;
        let mut close_line: Option<usize> = None;

        'outer: for (idx, line_chars) in lines.iter().enumerate().skip(opener) {
            for ch in line_chars.chars() {
                if searching {
                    if ch == '{' {
                        open_line = Some(idx);
                        depth = 1;
                        searching = false;
                    }
                } else {
                    match ch {
                        '{' => depth += 1,
                        '}' => {
                            depth -= 1;
                            if depth == 0 {
                                close_line = Some(idx);
                                break 'outer;
                            }
                        }
                        _ => {}
                    }
                }
            }
        }

        if let (Some(open), Some(close)) = (open_line, close_line) {
            if open == close {
                // single line — body is within the line
                ranges.push((open, close));
            } else {
                ranges.push((open + 1, close.saturating_sub(1)));
            }
        }
    }
    ranges
}
use crate::context::AnalysisContext;
use crate::finding::{Finding, FindingCategory, Severity};
use crate::Detector;

pub struct ErrorContextLossDetector;

// ---------------------------------------------------------------------------
// Patterns
// ---------------------------------------------------------------------------

/// Rust: `.map_err(|_|` — discards the original error
static RUST_MAP_ERR_DISCARD: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\.map_err\(\s*\|_\|").expect("rust map_err discard regex"));

/// Python: `raise X(` without `from` keyword in the same line,
/// while inside an except block.
static PY_RAISE_WITHOUT_FROM: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^\s*raise\s+\w[\w.]*\s*\(").expect("py raise without from regex")
});

/// JS: `throw new Error(` inside a catch block — wraps without preserving cause
static JS_THROW_NEW_ERROR: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\bthrow\s+new\s+\w*Error\s*\(").expect("js throw new error regex")
});

fn analyze_source(path: &std::path::Path, source: &str, lang: Language) -> Vec<Finding> {
    let lines: Vec<&str> = source.lines().collect();
    let mut findings = Vec::new();

    match lang {
        Language::Rust => {
            for (line_idx, line) in lines.iter().enumerate() {
                let trimmed = line.trim();
                if trimmed.is_empty() || is_comment(trimmed, lang) {
                    continue;
                }
                if RUST_MAP_ERR_DISCARD.is_match(line) {
                    let line_1based = (line_idx + 1) as u32;
                    findings.push(Finding {
                        id: Uuid::new_v4(),
                        detector: "error-context-loss".into(),
                        severity: Severity::Low,
                        category: FindingCategory::SecuritySmell,
                        file: path.to_path_buf(),
                        line: Some(line_1based),
                        title: "Error context discarded in map_err".into(),
                        description: format!(
                            "`.map_err(|_|` at line {} discards the original error, \
                             losing diagnostic context. Callers cannot distinguish error causes.",
                            line_1based
                        ),
                        evidence: vec![],
                        covered: false,
                        suggestion: "Capture the original error: `.map_err(|e| MyError::from(e))` \
                                     or `.map_err(|e| anyhow::anyhow!(\"context: {}\", e))`"
                            .into(),
                        explanation: None,
                        fix: None,
                        cwe_ids: vec![755],
                    });
                }
            }
        }
        Language::Python => {
            let except_scopes = find_except_scopes(source, lang);
            for (line_idx, line) in lines.iter().enumerate() {
                let trimmed = line.trim();
                if trimmed.is_empty() || is_comment(trimmed, lang) {
                    continue;
                }
                // Must be inside an except block
                if !in_any_scope(&except_scopes, line_idx) {
                    continue;
                }
                // raise X(...) without `from`
                if PY_RAISE_WITHOUT_FROM.is_match(line) && !line.contains(" from ") {
                    let line_1based = (line_idx + 1) as u32;
                    findings.push(Finding {
                        id: Uuid::new_v4(),
                        detector: "error-context-loss".into(),
                        severity: Severity::Low,
                        category: FindingCategory::SecuritySmell,
                        file: path.to_path_buf(),
                        line: Some(line_1based),
                        title: "Exception raised without chaining original cause".into(),
                        description: format!(
                            "`raise X(...)` at line {} inside an except block does not chain \
                             the original exception. Use `raise X(...) from e` to preserve \
                             the traceback for debugging.",
                            line_1based
                        ),
                        evidence: vec![],
                        covered: false,
                        suggestion:
                            "Use `raise NewException(...) from original_exc` to preserve context."
                                .into(),
                        explanation: None,
                        fix: None,
                        cwe_ids: vec![755],
                    });
                }
            }
        }
        Language::JavaScript => {
            // Collect catch block line ranges by scanning for catch openers, then
            // finding the matching { ... } body via a forward brace scan that
            // skips any preceding `}` on the same line.
            let catch_ranges = collect_catch_ranges(&lines);
            for (line_idx, line) in lines.iter().enumerate() {
                let trimmed = line.trim();
                if trimmed.is_empty() || is_comment(trimmed, lang) {
                    continue;
                }
                // Check if this line is inside any catch body
                if !catch_ranges
                    .iter()
                    .any(|&(s, e)| line_idx >= s && line_idx <= e)
                {
                    continue;
                }
                if JS_THROW_NEW_ERROR.is_match(line) {
                    // Suppressed if the line includes `{ cause:` (Error cause chaining ES2022)
                    if line.contains("cause:") || line.contains("cause :") {
                        continue;
                    }
                    let line_1based = (line_idx + 1) as u32;
                    findings.push(Finding {
                        id: Uuid::new_v4(),
                        detector: "error-context-loss".into(),
                        severity: Severity::Low,
                        category: FindingCategory::SecuritySmell,
                        file: path.to_path_buf(),
                        line: Some(line_1based),
                        title: "Error thrown in catch without wrapping original cause".into(),
                        description: format!(
                            "`throw new Error(...)` at line {} inside a catch block does not \
                             wrap the original error. The original stack trace is lost.",
                            line_1based
                        ),
                        evidence: vec![],
                        covered: false,
                        suggestion: "Wrap the original error: \
                                     `throw new Error('context', { cause: e })` (ES2022) \
                                     or use a custom error class that accepts a `cause` parameter."
                            .into(),
                        explanation: None,
                        fix: None,
                        cwe_ids: vec![755],
                    });
                }
            }
        }
        _ => {}
    }

    findings
}

#[async_trait]
impl Detector for ErrorContextLossDetector {
    fn name(&self) -> &str {
        "error-context-loss"
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
    fn detects_map_err_discard_rust() {
        let src = r#"fn load() -> Result<Data> {
    fs::read("file").map_err(|_| Error::Io)
}
"#;
        let findings = detect_rust(src);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, Severity::Low);
        assert_eq!(findings[0].cwe_ids, vec![755]);
    }

    // ---- positive: Python ----

    #[test]
    fn detects_raise_without_from_python() {
        let src = "\
try:
    parse()
except ValueError as e:
    raise RuntimeError('failed')
";
        let findings = detect_python(src);
        assert_eq!(findings.len(), 1);
        assert!(findings[0].title.contains("chaining"));
    }

    // ---- positive: JS ----

    #[test]
    fn detects_throw_without_cause_js() {
        let src = "\
try {
    parse();
} catch (e) {
    throw new Error('parse failed');
}
";
        let findings = detect_js(src);
        assert_eq!(findings.len(), 1);
    }

    // ---- negative: Rust with proper map_err ----

    #[test]
    fn no_finding_map_err_captures_error() {
        let src = r#"fn load() -> Result<Data> {
    fs::read("file").map_err(|e| Error::Io(e))
}
"#;
        let findings = detect_rust(src);
        assert_eq!(findings.len(), 0);
    }

    // ---- negative: Python with `from` ----

    #[test]
    fn no_finding_raise_from_python() {
        let src = "\
try:
    parse()
except ValueError as e:
    raise RuntimeError('failed') from e
";
        let findings = detect_python(src);
        assert_eq!(findings.len(), 0);
    }

    // ---- negative: JS with cause ----

    #[test]
    fn no_finding_js_with_cause() {
        let src = "\
try {
    parse();
} catch (e) {
    throw new Error('parse failed', { cause: e });
}
";
        let findings = detect_js(src);
        assert_eq!(findings.len(), 0);
    }

    // ---- edge: raise outside except not flagged ----

    #[test]
    fn raise_outside_except_not_flagged_python() {
        let src = "\
def handler():
    raise RuntimeError('not in except')
";
        let findings = detect_python(src);
        assert_eq!(findings.len(), 0);
    }
}
