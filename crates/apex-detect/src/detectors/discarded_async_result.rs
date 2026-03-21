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

pub struct DiscardedAsyncResultDetector;

// Rust: `let _ = something.await` — discards the result of an async call
static RUST_DISCARD_AWAIT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"let\s+_\s*=.*\.await").unwrap());

// JavaScript: `void asyncFn()` — explicit promise discard
static JS_VOID_ASYNC: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\s*void\s+\w").unwrap());

#[async_trait]
impl Detector for DiscardedAsyncResultDetector {
    fn name(&self) -> &str {
        "discarded-async-result"
    }

    async fn analyze(&self, ctx: &AnalysisContext) -> Result<Vec<Finding>> {
        // Only supported for Rust and JavaScript
        match ctx.language {
            Language::Rust | Language::JavaScript => {}
            _ => return Ok(vec![]),
        }

        let mut findings = Vec::new();

        for (path, source) in &ctx.source_cache {
            for (line_num, line) in source.lines().enumerate() {
                let trimmed = line.trim();
                if trimmed.is_empty() || is_comment(trimmed, ctx.language) {
                    continue;
                }

                let matches = match ctx.language {
                    Language::Rust => RUST_DISCARD_AWAIT.is_match(trimmed),
                    Language::JavaScript => JS_VOID_ASYNC.is_match(line),
                    _ => false,
                };

                if matches {
                    let line_1based = (line_num + 1) as u32;
                    findings.push(Finding {
                        id: Uuid::new_v4(),
                        detector: self.name().into(),
                        severity: Severity::Medium,
                        category: FindingCategory::LogicBug,
                        file: path.clone(),
                        line: Some(line_1based),
                        title: format!(
                            "Discarded async result at line {}",
                            line_1based
                        ),
                        description: format!(
                            "Line {} in {} discards the result of an async operation. \
                             Errors from this operation will be silently lost.",
                            line_1based,
                            path.display()
                        ),
                        evidence: vec![],
                        covered: false,
                        suggestion:
                            "Handle the async result or explicitly log the error instead of discarding it"
                                .into(),
                        explanation: None,
                        fix: None,
                        cwe_ids: vec![],
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
    use std::collections::HashMap;
    use std::path::PathBuf;

    fn make_ctx(files: HashMap<PathBuf, String>, lang: Language) -> AnalysisContext {
        AnalysisContext {
            language: lang,
            source_cache: files,
            ..AnalysisContext::test_default()
        }
    }

    // ---- Rust ----

    #[tokio::test]
    async fn detects_discarded_await_rust() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/lib.rs"),
            "let _ = client.send(msg).await;\n".into(),
        );
        let ctx = make_ctx(files, Language::Rust);
        let findings = DiscardedAsyncResultDetector.analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, Severity::Medium);
        assert_eq!(findings[0].category, FindingCategory::LogicBug);
    }

    #[tokio::test]
    async fn no_finding_for_handled_await_rust() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/lib.rs"),
            "let result = client.send(msg).await?;\n".into(),
        );
        let ctx = make_ctx(files, Language::Rust);
        let findings = DiscardedAsyncResultDetector.analyze(&ctx).await.unwrap();
        assert!(findings.is_empty());
    }

    #[tokio::test]
    async fn no_finding_for_non_await_discard_rust() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/lib.rs"),
            "let _ = some_sync_fn();\n".into(),
        );
        let ctx = make_ctx(files, Language::Rust);
        let findings = DiscardedAsyncResultDetector.analyze(&ctx).await.unwrap();
        assert!(findings.is_empty());
    }

    // ---- JavaScript ----

    #[tokio::test]
    async fn detects_void_async_js() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/utils.js"),
            "    void sendMetrics();\n".into(),
        );
        let ctx = make_ctx(files, Language::JavaScript);
        let findings = DiscardedAsyncResultDetector.analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 1);
    }

    #[tokio::test]
    async fn detects_void_async_js_no_indent() {
        let mut files = HashMap::new();
        files.insert(PathBuf::from("src/utils.js"), "void fetchData();\n".into());
        let ctx = make_ctx(files, Language::JavaScript);
        let findings = DiscardedAsyncResultDetector.analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 1);
    }

    #[tokio::test]
    async fn no_finding_for_awaited_promise_js() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/utils.js"),
            "const result = await fetchData();\n".into(),
        );
        let ctx = make_ctx(files, Language::JavaScript);
        let findings = DiscardedAsyncResultDetector.analyze(&ctx).await.unwrap();
        assert!(findings.is_empty());
    }

    // ---- Unsupported language ----

    #[tokio::test]
    async fn skips_unsupported_language() {
        let mut files = HashMap::new();
        files.insert(PathBuf::from("src/utils.py"), "void something\n".into());
        let ctx = make_ctx(files, Language::Python);
        let findings = DiscardedAsyncResultDetector.analyze(&ctx).await.unwrap();
        assert!(findings.is_empty());
    }

    #[tokio::test]
    async fn skips_comments_rust() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/lib.rs"),
            "// let _ = client.send(msg).await;\n".into(),
        );
        let ctx = make_ctx(files, Language::Rust);
        let findings = DiscardedAsyncResultDetector.analyze(&ctx).await.unwrap();
        assert!(findings.is_empty());
    }

    #[test]
    fn does_not_use_cargo_subprocess() {
        assert!(!DiscardedAsyncResultDetector.uses_cargo_subprocess());
    }
}
