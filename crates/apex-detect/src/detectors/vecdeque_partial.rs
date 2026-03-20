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

pub struct VecDequePartialDetector;

static AS_SLICES_DOT_ZERO: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\.as_slices\(\)\s*\.0").expect("invalid vecdeque-partial regex"));

#[async_trait]
impl Detector for VecDequePartialDetector {
    fn name(&self) -> &str {
        "vecdeque-partial"
    }

    async fn analyze(&self, ctx: &AnalysisContext) -> Result<Vec<Finding>> {
        if ctx.language != Language::Rust {
            return Ok(vec![]);
        }

        let mut findings = Vec::new();

        for (path, source) in &ctx.source_cache {
            if is_test_file(path) {
                continue;
            }

            for (line_num, line) in source.lines().enumerate() {
                let trimmed = line.trim();

                if is_comment(trimmed, ctx.language) {
                    continue;
                }

                if AS_SLICES_DOT_ZERO.is_match(trimmed) {
                    let line_1based = (line_num + 1) as u32;

                    findings.push(Finding {
                        id: Uuid::new_v4(),
                        detector: self.name().into(),
                        severity: Severity::High,
                        category: FindingCategory::LogicBug,
                        file: path.clone(),
                        line: Some(line_1based),
                        title: format!(
                            "VecDeque::as_slices().0 discards wrapped data at line {}",
                            line_1based
                        ),
                        description: format!(
                            "Only the first slice of VecDeque::as_slices() is used in {}:{}. \
                             Wrapped entries in the second slice are silently lost.",
                            path.display(),
                            line_1based
                        ),
                        evidence: vec![],
                        covered: false,
                        suggestion: "Use `.iter()` or destructure both slices: \
                                     `let (a, b) = ring.as_slices()`"
                            .into(),
                        explanation: None,
                        fix: None,
                        cwe_ids: vec![682],
                    noisy: false,
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
    async fn detects_as_slices_dot_zero() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/lib.rs"),
            "let data = ring.as_slices().0;\n".into(),
        );
        let ctx = make_ctx(files, Language::Rust);
        let findings = VecDequePartialDetector.analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, Severity::High);
        assert_eq!(findings[0].category, FindingCategory::LogicBug);
        assert_eq!(findings[0].cwe_ids, vec![682]);
    }

    #[tokio::test]
    async fn detects_as_slices_dot_zero_trailing_comma() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/buffer.rs"),
            "ring.as_slices().0,\n".into(),
        );
        let ctx = make_ctx(files, Language::Rust);
        let findings = VecDequePartialDetector.analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 1);
    }

    #[tokio::test]
    async fn ignores_proper_destructure() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/lib.rs"),
            "let (a, b) = ring.as_slices();\n".into(),
        );
        let ctx = make_ctx(files, Language::Rust);
        let findings = VecDequePartialDetector.analyze(&ctx).await.unwrap();
        assert!(findings.is_empty());
    }

    #[tokio::test]
    async fn ignores_iter_collect() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/lib.rs"),
            "ring.iter().collect()\n".into(),
        );
        let ctx = make_ctx(files, Language::Rust);
        let findings = VecDequePartialDetector.analyze(&ctx).await.unwrap();
        assert!(findings.is_empty());
    }

    #[tokio::test]
    async fn skips_test_files() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("tests/integration.rs"),
            "let data = ring.as_slices().0;\n".into(),
        );
        let ctx = make_ctx(files, Language::Rust);
        let findings = VecDequePartialDetector.analyze(&ctx).await.unwrap();
        assert!(findings.is_empty());
    }

    #[tokio::test]
    async fn skips_non_rust() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/lib.py"),
            "let data = ring.as_slices().0;\n".into(),
        );
        let ctx = make_ctx(files, Language::Python);
        let findings = VecDequePartialDetector.analyze(&ctx).await.unwrap();
        assert!(findings.is_empty());
    }

    #[test]
    fn does_not_use_cargo_subprocess() {
        assert!(!VecDequePartialDetector.uses_cargo_subprocess());
    }
}
