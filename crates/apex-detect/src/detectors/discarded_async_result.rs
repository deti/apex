use apex_core::error::Result;
use async_trait::async_trait;
use regex::Regex;
use std::sync::LazyLock;
use uuid::Uuid;

use super::util::{is_comment, is_test_file};
use crate::context::AnalysisContext;
use crate::finding::{Finding, FindingCategory, Severity};
use crate::Detector;
use apex_core::types::Language;

pub struct DiscardedAsyncResultDetector;

static DISCARDED_AWAIT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"let\s+_\s*=\s*.*\.await\s*;").unwrap());

#[async_trait]
impl Detector for DiscardedAsyncResultDetector {
    fn name(&self) -> &str {
        "discarded-async-result"
    }

    async fn analyze(&self, ctx: &AnalysisContext) -> Result<Vec<Finding>> {
        let mut findings = Vec::new();

        if ctx.language != Language::Rust {
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

                if DISCARDED_AWAIT.is_match(trimmed) {
                    let line_1based = (line_num + 1) as u32;

                    findings.push(Finding {
                        id: Uuid::new_v4(),
                        detector: self.name().into(),
                        severity: Severity::Medium,
                        category: FindingCategory::LogicBug,
                        file: path.clone(),
                        line: Some(line_1based),
                        title: format!(
                            "Discarded async Result at line {}",
                            line_1based
                        ),
                        description: format!(
                            "Async result silently discarded with `let _ =` in {}:{}",
                            path.display(),
                            line_1based
                        ),
                        evidence: vec![],
                        covered: false,
                        suggestion: "Log or propagate the error instead of discarding with `let _ =`".into(),
                        explanation: None,
                        fix: None,
                        cwe_ids: vec![252],
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

    #[tokio::test]
    async fn detects_discarded_await() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/main.rs"),
            "let _ = bar().await;\n".into(),
        );
        let ctx = make_ctx(files, Language::Rust);
        let findings = DiscardedAsyncResultDetector.analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, Severity::Medium);
        assert_eq!(findings[0].category, FindingCategory::LogicBug);
        assert_eq!(findings[0].cwe_ids, vec![252]);
    }

    #[tokio::test]
    async fn detects_discarded_method_await() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/lib.rs"),
            "let _ = strategy.observe(result).await;\n".into(),
        );
        let ctx = make_ctx(files, Language::Rust);
        let findings = DiscardedAsyncResultDetector.analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 1);
    }

    #[tokio::test]
    async fn ignores_assigned_await() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/main.rs"),
            "let result = bar().await;\n".into(),
        );
        let ctx = make_ctx(files, Language::Rust);
        let findings = DiscardedAsyncResultDetector.analyze(&ctx).await.unwrap();
        assert!(findings.is_empty());
    }

    #[tokio::test]
    async fn ignores_propagated_await() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/main.rs"),
            "bar().await?;\n".into(),
        );
        let ctx = make_ctx(files, Language::Rust);
        let findings = DiscardedAsyncResultDetector.analyze(&ctx).await.unwrap();
        assert!(findings.is_empty());
    }

    #[tokio::test]
    async fn ignores_non_rust_language() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/main.py"),
            "let _ = bar().await;\n".into(),
        );
        let ctx = make_ctx(files, Language::Python);
        let findings = DiscardedAsyncResultDetector.analyze(&ctx).await.unwrap();
        assert!(findings.is_empty());
    }

    #[tokio::test]
    async fn ignores_test_files() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("tests/test_async.rs"),
            "let _ = bar().await;\n".into(),
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
