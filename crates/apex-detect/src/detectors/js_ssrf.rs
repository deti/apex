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

pub struct JsSsrfDetector;

/// Matches HTTP request calls where the first argument is NOT a string literal.
/// This catches: fetch(var), axios.get(var), got(var), got.get(var),
/// http.get(var), https.get(var), new URL(var).
static SSRF_PATTERN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r#"(?:fetch|axios\.\w+|got(?:\.\w+)?|https?\.(?:get|request)|new\s+URL)\s*\(\s*[^"'`\s)]"#,
    )
    .expect("invalid SSRF regex")
});

#[async_trait]
impl Detector for JsSsrfDetector {
    fn name(&self) -> &str {
        "js-ssrf"
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

            for (line_num, line) in source.lines().enumerate() {
                let trimmed = line.trim();

                if is_comment(trimmed, ctx.language) {
                    continue;
                }

                if SSRF_PATTERN.is_match(trimmed) {
                    let line_1based = (line_num + 1) as u32;

                    findings.push(Finding {
                        id: Uuid::new_v4(),
                        detector: self.name().into(),
                        severity: Severity::High,
                        category: FindingCategory::Injection,
                        file: path.clone(),
                        line: Some(line_1based),
                        title: format!(
                            "Potential SSRF: HTTP request with non-literal URL at line {}",
                            line_1based
                        ),
                        description: format!(
                            "HTTP request function called with a variable URL in {}:{}. \
                             If the URL is user-controlled, this enables Server-Side Request Forgery.",
                            path.display(),
                            line_1based
                        ),
                        evidence: super::util::reachability_evidence(ctx, path, line_1based),
                        covered: false,
                        suggestion: "Validate and allowlist URLs before making HTTP requests. \
                                     Use a URL allowlist or validate the scheme and host."
                            .into(),
                        explanation: None,
                        fix: None,
                        cwe_ids: vec![918],
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
    async fn detects_fetch_with_variable() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/api.js"),
            "const resp = await fetch(url);\n".into(),
        );
        let ctx = make_ctx(files, Language::JavaScript);
        let findings = JsSsrfDetector.analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, Severity::High);
        assert_eq!(findings[0].category, FindingCategory::Injection);
        assert_eq!(findings[0].cwe_ids, vec![918]);
    }

    #[tokio::test]
    async fn detects_axios_get_with_variable() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/client.js"),
            "const res = axios.get(endpoint);\n".into(),
        );
        let ctx = make_ctx(files, Language::JavaScript);
        let findings = JsSsrfDetector.analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 1);
    }

    #[tokio::test]
    async fn detects_axios_post_with_variable() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/client.js"),
            "const res = axios.post(endpoint, data);\n".into(),
        );
        let ctx = make_ctx(files, Language::JavaScript);
        let findings = JsSsrfDetector.analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 1);
    }

    #[tokio::test]
    async fn detects_got_with_variable() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/fetch.js"),
            "const body = await got(targetUrl);\n".into(),
        );
        let ctx = make_ctx(files, Language::JavaScript);
        let findings = JsSsrfDetector.analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 1);
    }

    #[tokio::test]
    async fn detects_http_get_with_variable() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/proxy.js"),
            "http.get(userUrl, callback);\n".into(),
        );
        let ctx = make_ctx(files, Language::JavaScript);
        let findings = JsSsrfDetector.analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 1);
    }

    #[tokio::test]
    async fn detects_new_url_with_variable() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/redirect.js"),
            "const parsed = new URL(userInput);\n".into(),
        );
        let ctx = make_ctx(files, Language::JavaScript);
        let findings = JsSsrfDetector.analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 1);
    }

    #[tokio::test]
    async fn skips_fetch_with_string_literal() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/api.js"),
            "const resp = await fetch(\"https://api.example.com\");\n".into(),
        );
        let ctx = make_ctx(files, Language::JavaScript);
        let findings = JsSsrfDetector.analyze(&ctx).await.unwrap();
        assert!(findings.is_empty());
    }

    #[tokio::test]
    async fn skips_fetch_with_template_literal() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/api.js"),
            "const resp = await fetch(`https://api.example.com/v1`);\n".into(),
        );
        let ctx = make_ctx(files, Language::JavaScript);
        let findings = JsSsrfDetector.analyze(&ctx).await.unwrap();
        assert!(findings.is_empty());
    }

    #[tokio::test]
    async fn skips_fetch_with_single_quote_literal() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/api.js"),
            "const resp = await fetch('https://api.example.com');\n".into(),
        );
        let ctx = make_ctx(files, Language::JavaScript);
        let findings = JsSsrfDetector.analyze(&ctx).await.unwrap();
        assert!(findings.is_empty());
    }

    #[tokio::test]
    async fn skips_non_javascript_files() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/api.py"),
            "resp = fetch(url)\n".into(),
        );
        let ctx = make_ctx(files, Language::Python);
        let findings = JsSsrfDetector.analyze(&ctx).await.unwrap();
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
        let findings = JsSsrfDetector.analyze(&ctx).await.unwrap();
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
        let findings = JsSsrfDetector.analyze(&ctx).await.unwrap();
        assert!(findings.is_empty());
    }

    #[test]
    fn does_not_use_cargo_subprocess() {
        assert!(!JsSsrfDetector.uses_cargo_subprocess());
    }
}
