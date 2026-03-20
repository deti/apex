/// CWE-775 — Missing Release of File Descriptor or Handle.
///
/// Python `open()` calls that are NOT guarded by a `with` statement will leave
/// the file descriptor open until the garbage collector finalises the object.
/// In CPython this is deterministic, but in PyPy or Jython finalisation may
/// never happen.  The idiomatic and portable pattern is `with open(...) as f:`.
///
/// Flagged patterns:
///   - `f = open(...)` (bare assignment, no context manager)
///   - Any line containing `open(` where the line does not start with `with `
///
/// Suppressed:
///   - `with open(` — correct usage
///   - Lines already inside a `with` block that re-use an open handle
///   - Test files
///
/// Severity: Medium
/// Languages: Python only
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

pub struct OpenWithoutWithDetector;

/// Matches any call to `open(` in Python source.
static OPEN_CALL: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\bopen\s*\(").expect("open regex must compile"));

/// Matches a line that starts (after stripping indent) with `with ` — the
/// context manager statement.
static WITH_STMT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^\s*with\s+").expect("with regex must compile"));

#[async_trait]
impl Detector for OpenWithoutWithDetector {
    fn name(&self) -> &str {
        "open-without-with"
    }

    async fn analyze(&self, ctx: &AnalysisContext) -> Result<Vec<Finding>> {
        if ctx.language != Language::Python {
            return Ok(vec![]);
        }

        let mut findings = Vec::new();

        for (path, source) in &ctx.source_cache {
            if is_test_file(path) {
                continue;
            }

            for (line_num, line) in source.lines().enumerate() {
                let trimmed = line.trim();
                if trimmed.is_empty() || is_comment(trimmed, Language::Python) {
                    continue;
                }

                // Only flag lines that contain open()
                if !OPEN_CALL.is_match(trimmed) {
                    continue;
                }

                // Suppress: the line itself starts with `with open(` — correct usage.
                if WITH_STMT.is_match(line) {
                    continue;
                }

                let line_1based = (line_num + 1) as u32;
                findings.push(Finding {
                    id: Uuid::new_v4(),
                    detector: self.name().into(),
                    severity: Severity::Medium,
                    category: FindingCategory::SecuritySmell,
                    file: path.clone(),
                    line: Some(line_1based),
                    title: format!("open() without context manager at line {}", line_1based),
                    description: format!(
                        "Line {} in {} calls open() outside a `with` statement. \
                         The file descriptor may not be released promptly, causing \
                         resource leaks that are especially problematic on non-CPython \
                         runtimes where finalisation is non-deterministic.",
                        line_1based,
                        path.display()
                    ),
                    evidence: vec![],
                    covered: false,
                    suggestion: "Use `with open(...) as f:` to ensure the file is closed \
                                 deterministically."
                        .into(),
                    explanation: None,
                    fix: None,
                    cwe_ids: vec![775],
                });
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

    fn make_ctx(files: HashMap<PathBuf, String>) -> AnalysisContext {
        AnalysisContext {
            language: Language::Python,
            source_cache: files,
            ..AnalysisContext::test_default()
        }
    }

    // ---- True positives ----

    #[tokio::test]
    async fn detects_bare_assignment_open() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/reader.py"),
            "f = open('data.txt', 'r')\ndata = f.read()\nf.close()\n".into(),
        );
        let ctx = make_ctx(files);
        let findings = OpenWithoutWithDetector.analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, Severity::Medium);
        assert_eq!(findings[0].category, FindingCategory::SecuritySmell);
        assert!(findings[0].cwe_ids.contains(&775));
    }

    #[tokio::test]
    async fn detects_open_in_function_body() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/writer.py"),
            "def write_result(path, data):\n    fh = open(path, 'w')\n    fh.write(data)\n".into(),
        );
        let ctx = make_ctx(files);
        let findings = OpenWithoutWithDetector.analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 1);
    }

    // ---- True negatives ----

    #[tokio::test]
    async fn no_finding_with_open() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/reader.py"),
            "with open('data.txt', 'r') as f:\n    data = f.read()\n".into(),
        );
        let ctx = make_ctx(files);
        let findings = OpenWithoutWithDetector.analyze(&ctx).await.unwrap();
        assert!(findings.is_empty(), "with open() should not flag");
    }

    #[tokio::test]
    async fn no_finding_in_test_file() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("tests/test_reader.py"),
            "f = open('fixture.txt')\n".into(),
        );
        let ctx = make_ctx(files);
        let findings = OpenWithoutWithDetector.analyze(&ctx).await.unwrap();
        assert!(findings.is_empty(), "test file should be skipped");
    }

    #[tokio::test]
    async fn no_finding_rust_language() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/lib.rs"),
            "let f = open(\"path\");\n".into(),
        );
        let ctx = AnalysisContext {
            language: Language::Rust,
            source_cache: files,
            ..AnalysisContext::test_default()
        };
        let findings = OpenWithoutWithDetector.analyze(&ctx).await.unwrap();
        assert!(findings.is_empty());
    }

    #[tokio::test]
    async fn no_finding_comment_line() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/app.py"),
            "# f = open('data.txt')  -- old approach\n".into(),
        );
        let ctx = make_ctx(files);
        let findings = OpenWithoutWithDetector.analyze(&ctx).await.unwrap();
        assert!(findings.is_empty());
    }

    #[tokio::test]
    async fn no_finding_nested_with_open() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/app.py"),
            "def process():\n    with open('file.txt') as f:\n        data = f.read()\n".into(),
        );
        let ctx = make_ctx(files);
        let findings = OpenWithoutWithDetector.analyze(&ctx).await.unwrap();
        assert!(findings.is_empty());
    }

    #[test]
    fn does_not_use_cargo_subprocess() {
        assert!(!OpenWithoutWithDetector.uses_cargo_subprocess());
    }
}
