use apex_core::error::Result;
use async_trait::async_trait;
use regex::Regex;
use std::path::Path;
use std::sync::LazyLock;
use uuid::Uuid;

use super::util::{is_comment, is_test_file};
use crate::context::AnalysisContext;
use crate::finding::{Finding, FindingCategory, Severity};
use crate::Detector;

pub struct MissingTimeoutDetector;

/// Regex matching HTTP client calls that should have a timeout parameter.
///
/// Captures: requests.{get,post,put,delete,patch,head,request}(
///           httpx.{get,post,put,delete,patch,head}(
///           urlopen(
static HTTP_CALL_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?:requests\.(get|post|put|delete|patch|head|request)|httpx\.(get|post|put|delete|patch|head)|(?:urllib\.request\.)?urlopen)\s*\(",
    )
    .expect("HTTP call regex must compile")
});

fn has_timeout(line: &str) -> bool {
    line.contains("timeout=") || line.contains("timeout:")
}

fn analyze_source(path: &Path, source: &str) -> Vec<Finding> {
    let mut findings = Vec::new();

    for (line_idx, line) in source.lines().enumerate() {
        let trimmed = line.trim();

        if is_comment(trimmed, apex_core::types::Language::Python) {
            continue;
        }

        if let Some(m) = HTTP_CALL_RE.find(trimmed) {
            if !has_timeout(trimmed) {
                let call_text = m.as_str().trim_end_matches('(');
                let line_1based = (line_idx + 1) as u32;

                findings.push(Finding {
                    id: Uuid::new_v4(),
                    detector: "timeout".into(),
                    severity: Severity::Medium,
                    category: FindingCategory::SecuritySmell,
                    file: path.to_path_buf(),
                    line: Some(line_1based),
                    title: "HTTP request without timeout".into(),
                    description: format!(
                        "{call_text}() called without timeout= parameter. \
                         This can hang indefinitely if the server doesn't respond."
                    ),
                    evidence: vec![],
                    covered: false,
                    suggestion: format!(
                        "Add timeout= parameter, e.g., {call_text}(url, timeout=30)"
                    ),
                    explanation: None,
                    fix: None,
                    cwe_ids: vec![400],
                    noisy: false, base_severity: None, coverage_confidence: None,
                });
            }
        }
    }

    findings
}

#[async_trait]
impl Detector for MissingTimeoutDetector {
    fn name(&self) -> &str {
        "timeout"
    }

    async fn analyze(&self, ctx: &AnalysisContext) -> Result<Vec<Finding>> {
        let mut findings = Vec::new();

        for (path, source) in &ctx.source_cache {
            if path.extension().is_none_or(|ext| ext != "py") {
                continue;
            }
            if is_test_file(path) {
                continue;
            }

            findings.extend(analyze_source(path, source));
        }

        Ok(findings)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn detect(source: &str) -> Vec<Finding> {
        analyze_source(&PathBuf::from("src/app.py"), source)
    }

    #[test]
    fn detects_requests_get_without_timeout() {
        let findings = detect("requests.get('http://example.com')");
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, Severity::Medium);
        assert_eq!(findings[0].category, FindingCategory::SecuritySmell);
        assert!(findings[0].title.contains("without timeout"));
    }

    #[test]
    fn passes_requests_get_with_timeout() {
        let findings = detect("requests.get('http://example.com', timeout=30)");
        assert_eq!(findings.len(), 0);
    }

    #[test]
    fn detects_urlopen_without_timeout() {
        let findings = detect("urlopen('http://example.com')");
        assert_eq!(findings.len(), 1);
        assert!(findings[0].description.contains("urlopen"));
    }

    #[test]
    fn passes_urlopen_with_timeout() {
        let findings = detect("urlopen('http://example.com', timeout=10)");
        assert_eq!(findings.len(), 0);
    }

    #[test]
    fn detects_httpx_without_timeout() {
        let findings = detect("httpx.post('http://example.com', data=d)");
        assert_eq!(findings.len(), 1);
        assert!(findings[0].description.contains("httpx.post"));
    }

    #[test]
    fn ignores_non_http_calls() {
        let findings = detect("print('hello')");
        assert_eq!(findings.len(), 0);
    }

    #[test]
    fn detects_multiple_calls() {
        let source = "\
requests.get('http://a.com')
requests.post('http://b.com', timeout=5)
httpx.delete('http://c.com')
";
        let findings = detect(source);
        assert_eq!(findings.len(), 2);
    }

    #[test]
    fn detects_requests_post_without_timeout() {
        let findings = detect("requests.post('http://example.com', data=payload)");
        assert_eq!(findings.len(), 1);
    }

    #[test]
    fn detects_requests_request_without_timeout() {
        let findings = detect("requests.request('GET', 'http://example.com')");
        assert_eq!(findings.len(), 1);
    }

    #[test]
    fn passes_with_timeout_colon() {
        // Some patterns use timeout: in dict-style kwargs
        let findings = detect("httpx.get('url', timeout: 30)");
        assert_eq!(findings.len(), 0);
    }

    #[test]
    fn skips_comments() {
        let findings = detect("# requests.get('http://example.com')");
        assert_eq!(findings.len(), 0);
    }

    #[test]
    fn detects_urllib_request_urlopen() {
        let findings = detect("urllib.request.urlopen('http://example.com')");
        assert_eq!(findings.len(), 1);
    }
}
