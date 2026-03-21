use apex_core::error::Result;
use apex_core::types::Language;
use async_trait::async_trait;
use regex::Regex;
use std::sync::LazyLock;
use uuid::Uuid;

use super::util::is_comment;
use crate::context::AnalysisContext;
use crate::finding::{Finding, FindingCategory, Severity};
use crate::Detector;

pub struct BroadExceptionDetector;

// ---------------------------------------------------------------------------
// Broad exception patterns
// ---------------------------------------------------------------------------

/// Python: `except:` (bare) or `except Exception` or `except BaseException`
static PY_BROAD: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^\s*except\s*(?:Exception|BaseException)?\s*(?:as\s+\w+\s*)?:")
        .expect("py broad except regex")
});

/// Python bare `except:` (no exception type at all)
static PY_BARE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^\s*except\s*:").expect("py bare except regex"));

/// Java/Kotlin: `catch (Throwable` or `catch (Exception`
static JAVA_BROAD: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\bcatch\s*\(\s*(?:Throwable|Exception)\b").expect("java broad catch regex")
});

/// JS/TS: `catch (` — every catch is potentially broad; we flag `catch (e)` where
/// `e` is a plain identifier (no type narrowing).  We don't require type narrowing
/// here — we flag all JS catch blocks and let the suppression logic handle re-throws.
static JS_CATCH: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\bcatch\s*(?:\(|\{)").expect("js catch regex"));

/// Suppression: body contains bare `raise` (Python) or `throw` (JS/Java/Kotlin).
fn body_has_reraise(lines: &[&str], start: usize, end: usize, lang: Language) -> bool {
    for t in lines.iter().take(end + 1).skip(start).map(|l| l.trim()) {
        match lang {
            Language::Python => {
                if t == "raise" || t.starts_with("raise ") {
                    return true;
                }
            }
            _ => {
                if t == "throw;" || t.starts_with("throw ") || t.starts_with("throw;") {
                    return true;
                }
            }
        }
    }
    false
}

/// Scan from `opener_line` forward to find the first `{` that opens a block,
/// ignoring any `}` that appear before the opening `{` (they belong to enclosing blocks).
/// Returns `(body_start, body_end)` line indices (exclusive of the brace lines themselves).
fn scan_brace_body_after(lines: &[&str], opener_line: usize) -> Option<(usize, usize)> {
    let mut searching_for_open = true;
    let mut open_line: Option<usize> = None;
    let mut depth = 0i32;

    for (idx, line) in lines.iter().enumerate().skip(opener_line) {
        for ch in line.chars() {
            if searching_for_open {
                if ch == '{' {
                    open_line = Some(idx);
                    depth = 1;
                    searching_for_open = false;
                }
                // Skip `}` until we find the opening `{`
            } else {
                match ch {
                    '{' => depth += 1,
                    '}' => {
                        depth -= 1;
                        if depth == 0 {
                            let open = open_line?;
                            let close = idx;
                            if open == close {
                                return Some((open, close));
                            }
                            return Some((open + 1, close - 1));
                        }
                    }
                    _ => {}
                }
            }
        }
    }
    None
}

/// Check whether a brace body starting from `opener_line` contains a reraise.
fn brace_body_has_reraise(lines: &[&str], opener_line: usize, lang: Language) -> bool {
    match scan_brace_body_after(lines, opener_line) {
        Some((start, end)) => body_has_reraise(lines, start, end, lang),
        None => false,
    }
}

fn analyze_source(path: &std::path::Path, source: &str, lang: Language) -> Vec<Finding> {
    let lines: Vec<&str> = source.lines().collect();
    let mut findings = Vec::new();

    match lang {
        Language::Python => {
            for (line_idx, line) in lines.iter().enumerate() {
                let trimmed = line.trim();
                if trimmed.is_empty() || is_comment(trimmed, lang) {
                    continue;
                }
                let is_broad = PY_BARE.is_match(line) || PY_BROAD.is_match(line);

                if !is_broad {
                    continue;
                }

                // Check suppression: does the except body contain a bare raise?
                // For Python, the body is indented lines after the except clause.
                // We scan forward to find the indented body.
                let opener_indent = line.chars().take_while(|c| c.is_whitespace()).count();
                let body_start = line_idx + 1;
                let body_end = {
                    let mut end = body_start;
                    for (j, l) in lines.iter().enumerate().skip(body_start) {
                        let lt = l.trim();
                        if lt.is_empty() {
                            continue;
                        }
                        let indent = l.chars().take_while(|c| c.is_whitespace()).count();
                        if indent <= opener_indent {
                            break;
                        }
                        end = j;
                    }
                    end
                };

                let suppressed =
                    body_start <= body_end && body_has_reraise(&lines, body_start, body_end, lang);

                if !suppressed {
                    let line_1based = (line_idx + 1) as u32;
                    findings.push(make_finding(path, line_1based, lang, trimmed));
                }
            }
        }
        Language::Java => {
            for (line_idx, line) in lines.iter().enumerate() {
                if !JAVA_BROAD.is_match(line) {
                    continue;
                }
                let suppressed = brace_body_has_reraise(&lines, line_idx, lang);
                if !suppressed {
                    let line_1based = (line_idx + 1) as u32;
                    findings.push(make_finding(path, line_1based, lang, line.trim()));
                }
            }
        }
        Language::JavaScript => {
            // JS doesn't have typed catches (pre-TS 4.0) so we flag any catch
            // unless the body immediately re-throws.
            for (line_idx, line) in lines.iter().enumerate() {
                if !JS_CATCH.is_match(line) {
                    continue;
                }
                let suppressed = brace_body_has_reraise(&lines, line_idx, lang);
                if !suppressed {
                    let line_1based = (line_idx + 1) as u32;
                    findings.push(make_finding(path, line_1based, lang, line.trim()));
                }
            }
        }
        _ => {}
    }

    findings
}

fn make_finding(
    path: &std::path::Path,
    line_1based: u32,
    lang: Language,
    excerpt: &str,
) -> Finding {
    let example = match lang {
        Language::Python => "except Exception:",
        Language::Java => "catch (Exception e)",
        _ => "catch (e)",
    };
    Finding {
        id: Uuid::new_v4(),
        detector: "broad-exception".into(),
        severity: Severity::Medium,
        category: FindingCategory::SecuritySmell,
        file: path.to_path_buf(),
        line: Some(line_1based),
        title: "Overly broad exception catch".into(),
        description: format!(
            "Line {} catches a broad exception type (`{}`). Broad catches mask unexpected \
             errors, hide security issues, and prevent proper error propagation. \
             Found: `{}`",
            line_1based, example, excerpt
        ),
        evidence: vec![],
        covered: false,
        suggestion: "Catch the most specific exception type(s) you expect. \
                     If you must catch broadly, at minimum log the error and re-raise."
            .into(),
        explanation: None,
        fix: None,
        cwe_ids: vec![396],
                    noisy: false, base_severity: None, coverage_confidence: None,
    }
}

#[async_trait]
impl Detector for BroadExceptionDetector {
    fn name(&self) -> &str {
        "broad-exception"
    }

    async fn analyze(&self, ctx: &AnalysisContext) -> Result<Vec<Finding>> {
        let mut findings = Vec::new();

        for (path, source) in &ctx.source_cache {
            let lang = match path.extension().and_then(|e| e.to_str()) {
                Some("py") => Language::Python,
                Some("js") | Some("jsx") => Language::JavaScript,
                Some("ts") | Some("tsx") => Language::JavaScript,
                Some("java") | Some("kt") => Language::Java,
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

    fn detect_java(source: &str) -> Vec<Finding> {
        analyze_source(&PathBuf::from("src/Main.java"), source, Language::Java)
    }

    fn detect_js(source: &str) -> Vec<Finding> {
        analyze_source(&PathBuf::from("src/app.js"), source, Language::JavaScript)
    }

    // ---- positive: Python ----

    #[test]
    fn detects_bare_except() {
        let src = "\
try:
    risky()
except:
    log('err')
";
        let findings = detect_python(src);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, Severity::Medium);
        assert_eq!(findings[0].cwe_ids, vec![396]);
    }

    #[test]
    fn detects_except_exception() {
        let src = "\
try:
    risky()
except Exception as e:
    log(e)
";
        let findings = detect_python(src);
        assert_eq!(findings.len(), 1);
    }

    // ---- positive: Java ----

    #[test]
    fn detects_catch_exception_java() {
        let src = "\
try {
    risky();
} catch (Exception e) {
    log(e);
}
";
        let findings = detect_java(src);
        assert_eq!(findings.len(), 1);
    }

    #[test]
    fn detects_catch_throwable_java() {
        let src = "\
try {
    risky();
} catch (Throwable t) {
    log(t);
}
";
        let findings = detect_java(src);
        assert_eq!(findings.len(), 1);
    }

    // ---- negative: suppressed by raise/throw ----

    #[test]
    fn suppressed_by_reraise_python() {
        let src = "\
try:
    risky()
except Exception as e:
    log(e)
    raise
";
        let findings = detect_python(src);
        assert_eq!(findings.len(), 0);
    }

    #[test]
    fn suppressed_by_throw_java() {
        let src = "\
try {
    risky();
} catch (Exception e) {
    throw e;
}
";
        let findings = detect_java(src);
        assert_eq!(findings.len(), 0);
    }

    // ---- negative: specific exception ----

    #[test]
    fn no_finding_specific_exception_python() {
        let src = "\
try:
    risky()
except ValueError as e:
    handle(e)
";
        let findings = detect_python(src);
        assert_eq!(findings.len(), 0);
    }

    // ---- edge: JS catch ----

    #[test]
    fn detects_js_catch() {
        let src = "\
try {
    risky();
} catch (e) {
    console.log(e);
}
";
        let findings = detect_js(src);
        assert_eq!(findings.len(), 1);
    }

    #[test]
    fn suppressed_js_rethrow() {
        let src = "\
try {
    risky();
} catch (e) {
    throw e;
}
";
        let findings = detect_js(src);
        assert_eq!(findings.len(), 0);
    }
}
