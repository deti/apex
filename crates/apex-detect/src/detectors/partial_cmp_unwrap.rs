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

pub struct PartialCmpUnwrapDetector;

static PARTIAL_CMP_UNWRAP: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"partial_cmp\([^)]*\)\s*\.unwrap\(\)").expect("invalid partial_cmp_unwrap regex")
});

#[async_trait]
impl Detector for PartialCmpUnwrapDetector {
    fn name(&self) -> &str {
        "partial-cmp-unwrap"
    }

    async fn analyze(&self, ctx: &AnalysisContext) -> Result<Vec<Finding>> {
        let mut findings = Vec::new();

        // Only applies to Rust code
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

                if PARTIAL_CMP_UNWRAP.is_match(trimmed) {
                    let line_1based = (line_num + 1) as u32;

                    findings.push(Finding {
                        id: Uuid::new_v4(),
                        detector: self.name().into(),
                        severity: Severity::Medium,
                        category: FindingCategory::PanicPath,
                        file: path.clone(),
                        line: Some(line_1based),
                        title: format!(
                            "partial_cmp().unwrap() panics on NaN at line {}",
                            line_1based
                        ),
                        description: format!(
                            "Calling .partial_cmp().unwrap() in {}:{} will panic if either \
                             operand is NaN",
                            path.display(),
                            line_1based
                        ),
                        evidence: vec![],
                        covered: false,
                        suggestion:
                            "Use `.unwrap_or(std::cmp::Ordering::Equal)` or `.total_cmp()` \
                             for NaN safety"
                                .into(),
                        explanation: None,
                        fix: None,
                        cwe_ids: vec![754],
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
    async fn detects_sort_by_partial_cmp_unwrap() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/main.rs"),
            "scores.sort_by(|a, b| a.partial_cmp(b).unwrap())\n".into(),
        );
        let ctx = make_ctx(files, Language::Rust);
        let findings = PartialCmpUnwrapDetector.analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, Severity::Medium);
        assert_eq!(findings[0].category, FindingCategory::PanicPath);
        assert_eq!(findings[0].cwe_ids, vec![754]);
    }

    #[tokio::test]
    async fn detects_field_partial_cmp_unwrap() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/sort.rs"),
            "b.score.partial_cmp(&a.score).unwrap()\n".into(),
        );
        let ctx = make_ctx(files, Language::Rust);
        let findings = PartialCmpUnwrapDetector.analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 1);
    }

    #[tokio::test]
    async fn ignores_unwrap_or() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/main.rs"),
            "a.partial_cmp(b).unwrap_or(Ordering::Equal)\n".into(),
        );
        let ctx = make_ctx(files, Language::Rust);
        let findings = PartialCmpUnwrapDetector.analyze(&ctx).await.unwrap();
        assert!(findings.is_empty());
    }

    #[tokio::test]
    async fn ignores_total_cmp() {
        let mut files = HashMap::new();
        files.insert(PathBuf::from("src/main.rs"), "a.total_cmp(b)\n".into());
        let ctx = make_ctx(files, Language::Rust);
        let findings = PartialCmpUnwrapDetector.analyze(&ctx).await.unwrap();
        assert!(findings.is_empty());
    }

    #[tokio::test]
    async fn skips_non_rust_languages() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/main.py"),
            "scores.sort_by(|a, b| a.partial_cmp(b).unwrap())\n".into(),
        );
        let ctx = make_ctx(files, Language::Python);
        let findings = PartialCmpUnwrapDetector.analyze(&ctx).await.unwrap();
        assert!(findings.is_empty());
    }

    #[tokio::test]
    async fn skips_test_files() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("tests/sort_test.rs"),
            "scores.sort_by(|a, b| a.partial_cmp(b).unwrap())\n".into(),
        );
        let ctx = make_ctx(files, Language::Rust);
        let findings = PartialCmpUnwrapDetector.analyze(&ctx).await.unwrap();
        assert!(findings.is_empty());
    }

    #[tokio::test]
    async fn skips_comments() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/main.rs"),
            "// scores.sort_by(|a, b| a.partial_cmp(b).unwrap())\n".into(),
        );
        let ctx = make_ctx(files, Language::Rust);
        let findings = PartialCmpUnwrapDetector.analyze(&ctx).await.unwrap();
        assert!(findings.is_empty());
    }

    #[test]
    fn does_not_use_cargo_subprocess() {
        assert!(!PartialCmpUnwrapDetector.uses_cargo_subprocess());
    }
}
