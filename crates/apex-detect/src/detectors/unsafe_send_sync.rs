use apex_core::error::Result;
use async_trait::async_trait;
use regex::Regex;
use std::sync::LazyLock;
use uuid::Uuid;

use super::util::is_test_file;
use crate::context::AnalysisContext;
use crate::finding::{Finding, FindingCategory, Severity};
use crate::Detector;

pub struct UnsafeSendSyncDetector;

static UNSAFE_SEND_SYNC_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"unsafe\s+impl\s+(Send|Sync)\s+for").expect("invalid unsafe send/sync regex")
});

/// Check whether any of the preceding `count` lines contain a `// SAFETY:` comment
/// (case-insensitive on the word "safety").
fn has_safety_comment(lines: &[&str], match_line: usize, count: usize) -> bool {
    let start = match_line.saturating_sub(count);
    for line in lines.iter().take(match_line).skip(start) {
        let trimmed = line.trim();
        if trimmed.starts_with("//") {
            let comment_body = trimmed.trim_start_matches('/').trim();
            if comment_body.to_ascii_lowercase().starts_with("safety:") {
                return true;
            }
        }
    }
    false
}

#[async_trait]
impl Detector for UnsafeSendSyncDetector {
    fn name(&self) -> &str {
        "unsafe-send-sync"
    }

    async fn analyze(&self, ctx: &AnalysisContext) -> Result<Vec<Finding>> {
        use apex_core::types::Language;

        if ctx.language != Language::Rust {
            return Ok(vec![]);
        }

        let mut findings = Vec::new();

        for (path, source) in &ctx.source_cache {
            if is_test_file(path) {
                continue;
            }

            let lines: Vec<&str> = source.lines().collect();

            for (line_idx, line) in lines.iter().enumerate() {
                if UNSAFE_SEND_SYNC_RE.is_match(line) && !has_safety_comment(&lines, line_idx, 3) {
                    let line_1based = (line_idx + 1) as u32;
                    findings.push(Finding {
                        id: Uuid::new_v4(),
                        detector: self.name().into(),
                        severity: Severity::High,
                        category: FindingCategory::UnsafeCode,
                        file: path.clone(),
                        line: Some(line_1based),
                        title: format!(
                            "Unsafe Send/Sync impl without safety comment at line {line_1based}"
                        ),
                        description: format!(
                            "Found `{}` in {}:{} without a preceding `// SAFETY:` comment",
                            line.trim(),
                            path.display(),
                            line_1based
                        ),
                        evidence: vec![],
                        covered: false,
                        suggestion:
                            "Add a `// SAFETY:` comment explaining why this Send/Sync impl is safe"
                                .into(),
                        explanation: None,
                        fix: None,
                        cwe_ids: vec![362],
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
    async fn detects_unsafe_send_without_safety_comment() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/lib.rs"),
            "unsafe impl Send for Foo {}\n".into(),
        );
        let ctx = make_ctx(files, Language::Rust);
        let findings = UnsafeSendSyncDetector.analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, Severity::High);
        assert_eq!(findings[0].category, FindingCategory::UnsafeCode);
        assert_eq!(findings[0].cwe_ids, vec![362]);
    }

    #[tokio::test]
    async fn detects_unsafe_sync_without_safety_comment() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/lib.rs"),
            "unsafe impl Sync for Bar {}\n".into(),
        );
        let ctx = make_ctx(files, Language::Rust);
        let findings = UnsafeSendSyncDetector.analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, Severity::High);
    }

    #[tokio::test]
    async fn skips_when_safety_comment_present_uppercase() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/lib.rs"),
            "// SAFETY: Foo is only used behind Arc<Mutex<_>>\nunsafe impl Send for Foo {}\n"
                .into(),
        );
        let ctx = make_ctx(files, Language::Rust);
        let findings = UnsafeSendSyncDetector.analyze(&ctx).await.unwrap();
        assert!(findings.is_empty());
    }

    #[tokio::test]
    async fn skips_when_safety_comment_present_titlecase() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/lib.rs"),
            "// Safety: single-threaded access guaranteed\nunsafe impl Sync for Bar {}\n".into(),
        );
        let ctx = make_ctx(files, Language::Rust);
        let findings = UnsafeSendSyncDetector.analyze(&ctx).await.unwrap();
        assert!(findings.is_empty());
    }

    #[tokio::test]
    async fn skips_non_rust_languages() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/lib.py"),
            "unsafe impl Send for Foo {}\n".into(),
        );
        let ctx = make_ctx(files, Language::Python);
        let findings = UnsafeSendSyncDetector.analyze(&ctx).await.unwrap();
        assert!(findings.is_empty());
    }

    #[tokio::test]
    async fn skips_test_files() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("tests/test_sync.rs"),
            "unsafe impl Send for Foo {}\n".into(),
        );
        let ctx = make_ctx(files, Language::Rust);
        let findings = UnsafeSendSyncDetector.analyze(&ctx).await.unwrap();
        assert!(findings.is_empty());
    }

    #[tokio::test]
    async fn safety_comment_within_3_lines() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/lib.rs"),
            "// SAFETY: protected by external lock\n\n\nunsafe impl Send for Baz {}\n".into(),
        );
        let ctx = make_ctx(files, Language::Rust);
        let findings = UnsafeSendSyncDetector.analyze(&ctx).await.unwrap();
        assert!(findings.is_empty());
    }

    #[tokio::test]
    async fn safety_comment_too_far_away() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/lib.rs"),
            "// SAFETY: protected by external lock\n\n\n\nunsafe impl Send for Baz {}\n".into(),
        );
        let ctx = make_ctx(files, Language::Rust);
        let findings = UnsafeSendSyncDetector.analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 1);
    }

    #[test]
    fn does_not_use_cargo_subprocess() {
        assert!(!UnsafeSendSyncDetector.uses_cargo_subprocess());
    }
}
