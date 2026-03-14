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

pub struct ProcessExitInLibDetector;

static PROCESS_EXIT_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(std::)?process::exit\s*\(").expect("invalid process-exit-in-lib regex")
});

/// Returns true if the path ends with `main.rs` (exit() is legitimate there).
fn is_main_file(path: &std::path::Path) -> bool {
    path.file_name()
        .and_then(|f| f.to_str())
        == Some("main.rs")
}

#[async_trait]
impl Detector for ProcessExitInLibDetector {
    fn name(&self) -> &str {
        "process-exit-in-lib"
    }

    async fn analyze(&self, ctx: &AnalysisContext) -> Result<Vec<Finding>> {
        if ctx.language != Language::Rust {
            return Ok(vec![]);
        }

        let mut findings = Vec::new();

        for (path, source) in &ctx.source_cache {
            if is_test_file(path) || is_main_file(path) {
                continue;
            }

            for (line_num, line) in source.lines().enumerate() {
                let trimmed = line.trim();

                if is_comment(trimmed, ctx.language) {
                    continue;
                }

                if PROCESS_EXIT_RE.is_match(trimmed) {
                    let line_1based = (line_num + 1) as u32;

                    findings.push(Finding {
                        id: Uuid::new_v4(),
                        detector: self.name().into(),
                        severity: Severity::Medium,
                        category: FindingCategory::LogicBug,
                        file: path.clone(),
                        line: Some(line_1based),
                        title: format!(
                            "process::exit() in library code at line {}",
                            line_1based
                        ),
                        description: format!(
                            "process::exit() called in {}:{} — this bypasses Drop handlers \
                             and makes the function untestable.",
                            path.display(),
                            line_1based
                        ),
                        evidence: vec![],
                        covered: false,
                        suggestion: "Return an error instead of calling `process::exit()` \
                                     — this bypasses cleanup and makes the function untestable"
                            .into(),
                        explanation: None,
                        fix: None,
                        cwe_ids: vec![705],
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
    async fn detects_std_process_exit_in_lib() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/lib.rs"),
            "std::process::exit(1);\n".into(),
        );
        let ctx = make_ctx(files, Language::Rust);
        let findings = ProcessExitInLibDetector.analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, Severity::Medium);
        assert_eq!(findings[0].category, FindingCategory::LogicBug);
        assert_eq!(findings[0].cwe_ids, vec![705]);
    }

    #[tokio::test]
    async fn detects_process_exit_in_cli() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/cli.rs"),
            "process::exit(1);\n".into(),
        );
        let ctx = make_ctx(files, Language::Rust);
        let findings = ProcessExitInLibDetector.analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 1);
    }

    #[tokio::test]
    async fn skips_main_rs() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/main.rs"),
            "std::process::exit(0);\n".into(),
        );
        let ctx = make_ctx(files, Language::Rust);
        let findings = ProcessExitInLibDetector.analyze(&ctx).await.unwrap();
        assert!(findings.is_empty());
    }

    #[tokio::test]
    async fn skips_test_files() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("tests/integration.rs"),
            "std::process::exit(1);\n".into(),
        );
        let ctx = make_ctx(files, Language::Rust);
        let findings = ProcessExitInLibDetector.analyze(&ctx).await.unwrap();
        assert!(findings.is_empty());
    }

    #[tokio::test]
    async fn skips_non_rust() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/lib.py"),
            "std::process::exit(1);\n".into(),
        );
        let ctx = make_ctx(files, Language::Python);
        let findings = ProcessExitInLibDetector.analyze(&ctx).await.unwrap();
        assert!(findings.is_empty());
    }

    #[test]
    fn does_not_use_cargo_subprocess() {
        assert!(!ProcessExitInLibDetector.uses_cargo_subprocess());
    }
}
