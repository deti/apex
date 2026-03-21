use apex_core::error::Result;
use apex_core::types::Language;
use async_trait::async_trait;
use regex::Regex;
use std::sync::LazyLock;
use uuid::Uuid;

use super::util::{is_comment, is_test_file};
use crate::context::AnalysisContext;
use crate::finding::{Finding, FindingCategory, Severity};
use crate::Detector;

pub struct JsTimeoutDetector;

/// Matches HTTP request calls: fetch(, axios.get(, axios.post(, http.get(, https.get(
static HTTP_CALL: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?:fetch|axios\.\w+|https?\.(?:get|request|post))\s*\(")
        .expect("invalid HTTP call regex")
});

const TIMEOUT_INDICATORS: &[&str] = &["timeout", "signal", "AbortController"];

/// Check if any of the surrounding lines (within `radius`) contain timeout indicators.
fn has_timeout_nearby(lines: &[&str], center: usize, radius: usize) -> bool {
    let start = center.saturating_sub(radius);
    let end = (center + radius + 1).min(lines.len());
    for line in &lines[start..end] {
        if TIMEOUT_INDICATORS.iter().any(|ind| line.contains(ind)) {
            return true;
        }
    }
    false
}

#[async_trait]
impl Detector for JsTimeoutDetector {
    fn name(&self) -> &str {
        "js-timeout"
    }

    async fn analyze(&self, ctx: &AnalysisContext) -> Result<Vec<Finding>> {
        let mut findings = Vec::new();

        if ctx.language != Language::JavaScript {
            return Ok(findings);
        }

        for (path, source) in &ctx.source_cache {
            if is_test_file(path) {
                continue;
            }

            let all_lines: Vec<&str> = source.lines().collect();

            for (line_num, line) in all_lines.iter().enumerate() {
                let trimmed = line.trim();

                if is_comment(trimmed, ctx.language) {
                    continue;
                }

                if HTTP_CALL.is_match(trimmed) && !has_timeout_nearby(&all_lines, line_num, 3) {
                    let line_1based = (line_num + 1) as u32;

                    findings.push(Finding {
                        id: Uuid::new_v4(),
                        detector: self.name().into(),
                        severity: Severity::Low,
                        category: FindingCategory::SecuritySmell,
                        file: path.clone(),
                        line: Some(line_1based),
                        title: format!(
                            "HTTP request without timeout at line {}",
                            line_1based
                        ),
                        description: format!(
                            "HTTP request in {}:{} has no timeout or abort signal. \
                             This can lead to resource exhaustion if the server is unresponsive.",
                            path.display(),
                            line_1based
                        ),
                        evidence: vec![],
                        covered: false,
                        suggestion:
                            "Add a timeout option or AbortController signal to HTTP requests"
                                .into(),
                        explanation: None,
                        fix: None,
                        cwe_ids: vec![400],
                    noisy: false, base_severity: None, coverage_confidence: None,
                    });
                }
            }
        }

        Ok(findings)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::AnalysisContext;
    use apex_core::types::Language;
    use std::collections::HashMap;
    use std::path::PathBuf;

    fn make_ctx(files: HashMap<PathBuf, String>, lang: Language) -> AnalysisContext {
        AnalysisContext {
            language: lang,
            source_cache: files,
            ..AnalysisContext::test_default()
        }
    }

    #[tokio::test]
    async fn detects_fetch_without_timeout() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/api.js"),
            "const resp = await fetch(url);\n".into(),
        );
        let ctx = make_ctx(files, Language::JavaScript);
        let findings = JsTimeoutDetector.analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, Severity::Low);
        assert_eq!(findings[0].category, FindingCategory::SecuritySmell);
        assert_eq!(findings[0].cwe_ids, vec![400]);
    }

    #[tokio::test]
    async fn detects_axios_without_timeout() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/client.js"),
            "const res = axios.get(url);\n".into(),
        );
        let ctx = make_ctx(files, Language::JavaScript);
        let findings = JsTimeoutDetector.analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 1);
    }

    #[tokio::test]
    async fn skips_fetch_with_signal() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/api.js"),
            "const resp = await fetch(url, { signal: controller.signal });\n".into(),
        );
        let ctx = make_ctx(files, Language::JavaScript);
        let findings = JsTimeoutDetector.analyze(&ctx).await.unwrap();
        assert!(findings.is_empty());
    }

    #[tokio::test]
    async fn skips_axios_with_timeout() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/client.js"),
            "const res = axios.get(url, { timeout: 5000 });\n".into(),
        );
        let ctx = make_ctx(files, Language::JavaScript);
        let findings = JsTimeoutDetector.analyze(&ctx).await.unwrap();
        assert!(findings.is_empty());
    }

    #[tokio::test]
    async fn skips_fetch_with_abort_controller_nearby() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/api.js"),
            "const controller = new AbortController();\nconst resp = await fetch(url);\n".into(),
        );
        let ctx = make_ctx(files, Language::JavaScript);
        let findings = JsTimeoutDetector.analyze(&ctx).await.unwrap();
        assert!(findings.is_empty());
    }

    #[tokio::test]
    async fn skips_non_javascript() {
        let mut files = HashMap::new();
        files.insert(PathBuf::from("src/api.py"), "resp = fetch(url)\n".into());
        let ctx = make_ctx(files, Language::Python);
        let findings = JsTimeoutDetector.analyze(&ctx).await.unwrap();
        assert!(findings.is_empty());
    }

    #[tokio::test]
    async fn skips_test_files() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("test/api.test.js"),
            "const resp = await fetch(url);\n".into(),
        );
        let ctx = make_ctx(files, Language::JavaScript);
        let findings = JsTimeoutDetector.analyze(&ctx).await.unwrap();
        assert!(findings.is_empty());
    }

    #[tokio::test]
    async fn skips_comments() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/api.js"),
            "// const resp = await fetch(url);\n".into(),
        );
        let ctx = make_ctx(files, Language::JavaScript);
        let findings = JsTimeoutDetector.analyze(&ctx).await.unwrap();
        assert!(findings.is_empty());
    }

    #[tokio::test]
    async fn detects_http_get_without_timeout() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/proxy.js"),
            "http.get(url, callback);\n".into(),
        );
        let ctx = make_ctx(files, Language::JavaScript);
        let findings = JsTimeoutDetector.analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 1);
    }

    #[tokio::test]
    async fn skips_timeout_on_nearby_line() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/api.js"),
            "const opts = { timeout: 3000 };\nconst resp = await fetch(url, opts);\n".into(),
        );
        let ctx = make_ctx(files, Language::JavaScript);
        let findings = JsTimeoutDetector.analyze(&ctx).await.unwrap();
        assert!(findings.is_empty());
    }

    #[test]
    fn does_not_use_cargo_subprocess() {
        assert!(!JsTimeoutDetector.uses_cargo_subprocess());
    }

    #[test]
    fn has_timeout_nearby_finds_on_same_line() {
        let lines = vec!["fetch(url, { timeout: 5000 })"];
        assert!(has_timeout_nearby(&lines, 0, 3));
    }

    #[test]
    fn has_timeout_nearby_finds_within_radius() {
        let lines = vec![
            "const controller = new AbortController();",
            "// blank",
            "fetch(url);",
        ];
        assert!(has_timeout_nearby(&lines, 2, 3));
    }

    #[test]
    fn has_timeout_nearby_misses_outside_radius() {
        let lines = vec![
            "const controller = new AbortController();",
            "// 1",
            "// 2",
            "// 3",
            "// 4",
            "fetch(url);",
        ];
        assert!(!has_timeout_nearby(&lines, 5, 3));
    }
}
