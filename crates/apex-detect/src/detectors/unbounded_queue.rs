/// CWE-400/770 — Uncontrolled Resource Consumption: unbounded channel/queue creation.
///
/// An unbounded queue allows senders to enqueue work faster than consumers can
/// process it, leading to unbounded memory growth and eventual OOM.  Common
/// sources:
///
/// - Rust `tokio::sync::mpsc::unbounded_channel()` — no backpressure
/// - Rust `std::sync::mpsc::channel()` — unbounded std channel
/// - Python `queue.Queue()` without `maxsize` — defaults to 0 (unbounded)
///
/// Safe alternatives:
/// - `tokio::sync::mpsc::channel(N)` — bounded, blocks sender at capacity
/// - `queue.Queue(maxsize=N)` — bounded Python queue
///
/// Severity: Medium
/// Languages: Rust, Python
use apex_core::error::Result;
use apex_core::types::Language;
use async_trait::async_trait;
use regex::Regex;
use std::sync::LazyLock;
use uuid::Uuid;

use super::util::{in_test_block, is_comment, is_test_file};
use crate::context::AnalysisContext;
use crate::finding::{Finding, FindingCategory, Severity};
use crate::Detector;

pub struct UnboundedQueueDetector;

// ---- Rust patterns ----

/// `tokio::sync::mpsc::unbounded_channel()` — explicitly unbounded.
static TOKIO_UNBOUNDED: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"unbounded_channel\s*\(\s*\)").expect("unbounded_channel regex must compile")
});

/// `std::sync::mpsc::channel()` without a capacity argument — unbounded.
/// We match `mpsc::channel()` (no argument inside parens).
/// `mpsc::sync_channel(N)` is bounded, so we do NOT flag it.
static STD_MPSC_CHANNEL: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"mpsc::channel\s*\(\s*\)").expect("mpsc channel regex must compile")
});

// ---- Python patterns ----

/// `queue.Queue()` or `Queue()` without a maxsize argument.
/// We flag when the constructor call has empty parens (no maxsize).
static PY_QUEUE_UNBOUNDED: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\bQueue\s*\(\s*\)").expect("Queue regex must compile"));

/// Detect bounded Queue — `Queue(maxsize=...)` or `Queue(N)` — to suppress.
static PY_QUEUE_BOUNDED: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\bQueue\s*\(\s*(?:maxsize\s*=\s*)?\d+").expect("bounded Queue regex must compile")
});

#[async_trait]
impl Detector for UnboundedQueueDetector {
    fn name(&self) -> &str {
        "unbounded-queue"
    }

    async fn analyze(&self, ctx: &AnalysisContext) -> Result<Vec<Finding>> {
        match ctx.language {
            Language::Rust | Language::Python => {}
            _ => return Ok(vec![]),
        }

        let mut findings = Vec::new();

        for (path, source) in &ctx.source_cache {
            if is_test_file(path) {
                continue;
            }

            for (line_num, line) in source.lines().enumerate() {
                let trimmed = line.trim();
                if trimmed.is_empty() || is_comment(trimmed, ctx.language) {
                    continue;
                }

                if ctx.language == Language::Rust && in_test_block(source, line_num) {
                    continue;
                }

                let matched = match ctx.language {
                    Language::Rust => {
                        TOKIO_UNBOUNDED.is_match(trimmed) || STD_MPSC_CHANNEL.is_match(trimmed)
                    }
                    Language::Python => {
                        // Flag Queue() with no args; suppress Queue(N) or Queue(maxsize=N).
                        PY_QUEUE_UNBOUNDED.is_match(trimmed) && !PY_QUEUE_BOUNDED.is_match(trimmed)
                    }
                    _ => false,
                };

                if matched {
                    let line_1based = (line_num + 1) as u32;
                    let (title_detail, description_detail) = match ctx.language {
                        Language::Rust if TOKIO_UNBOUNDED.is_match(trimmed) => (
                            "tokio unbounded channel",
                            "tokio::sync::mpsc::unbounded_channel() has no backpressure. \
                             Prefer tokio::sync::mpsc::channel(N) to bound the queue.",
                        ),
                        Language::Rust => (
                            "std::sync::mpsc unbounded channel",
                            "std::sync::mpsc::channel() is unbounded. \
                             Use std::sync::mpsc::sync_channel(N) for a bounded alternative.",
                        ),
                        _ => (
                            "Python Queue without maxsize",
                            "queue.Queue() defaults to maxsize=0 (unbounded). \
                             Pass a maxsize argument to limit queue depth.",
                        ),
                    };

                    findings.push(Finding {
                        id: Uuid::new_v4(),
                        detector: self.name().into(),
                        severity: Severity::Medium,
                        category: FindingCategory::SecuritySmell,
                        file: path.clone(),
                        line: Some(line_1based),
                        title: format!(
                            "Unbounded queue ({}) at line {}",
                            title_detail, line_1based
                        ),
                        description: format!(
                            "Line {} in {} creates an unbounded queue. {}",
                            line_1based,
                            path.display(),
                            description_detail
                        ),
                        evidence: vec![],
                        covered: false,
                        suggestion: "Use a bounded channel or queue to apply backpressure and \
                                     prevent unbounded memory growth."
                            .into(),
                        explanation: None,
                        fix: None,
                        cwe_ids: vec![400, 770],
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
    use std::collections::HashMap;
    use std::path::PathBuf;

    fn rust_ctx(files: HashMap<PathBuf, String>) -> AnalysisContext {
        AnalysisContext {
            language: Language::Rust,
            source_cache: files,
            ..AnalysisContext::test_default()
        }
    }

    fn py_ctx(files: HashMap<PathBuf, String>) -> AnalysisContext {
        AnalysisContext {
            language: Language::Python,
            source_cache: files,
            ..AnalysisContext::test_default()
        }
    }

    // ---- Rust true positives ----

    #[tokio::test]
    async fn detects_tokio_unbounded_channel() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/server.rs"),
            "let (tx, rx) = tokio::sync::mpsc::unbounded_channel();\n".into(),
        );
        let ctx = rust_ctx(files);
        let findings = UnboundedQueueDetector.analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, Severity::Medium);
        assert!(findings[0].cwe_ids.contains(&400));
    }

    #[tokio::test]
    async fn detects_std_mpsc_channel_unbounded() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/worker.rs"),
            "let (sender, receiver) = std::sync::mpsc::channel();\n".into(),
        );
        let ctx = rust_ctx(files);
        let findings = UnboundedQueueDetector.analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 1);
    }

    // ---- Rust true negatives ----

    #[tokio::test]
    async fn no_finding_tokio_bounded_channel() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/server.rs"),
            "let (tx, rx) = tokio::sync::mpsc::channel(100);\n".into(),
        );
        let ctx = rust_ctx(files);
        let findings = UnboundedQueueDetector.analyze(&ctx).await.unwrap();
        assert!(findings.is_empty(), "bounded channel should not flag");
    }

    // ---- Python true positives ----

    #[tokio::test]
    async fn detects_python_queue_no_maxsize() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/worker.py"),
            "from queue import Queue\nwork_queue = Queue()\n".into(),
        );
        let ctx = py_ctx(files);
        let findings = UnboundedQueueDetector.analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].category, FindingCategory::SecuritySmell);
    }

    // ---- Python true negatives ----

    #[tokio::test]
    async fn no_finding_python_queue_with_maxsize() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/worker.py"),
            "q = Queue(maxsize=50)\n".into(),
        );
        let ctx = py_ctx(files);
        let findings = UnboundedQueueDetector.analyze(&ctx).await.unwrap();
        assert!(findings.is_empty(), "bounded Queue should not flag");
    }

    #[tokio::test]
    async fn no_finding_python_queue_positional_maxsize() {
        let mut files = HashMap::new();
        files.insert(PathBuf::from("src/worker.py"), "q = Queue(100)\n".into());
        let ctx = py_ctx(files);
        let findings = UnboundedQueueDetector.analyze(&ctx).await.unwrap();
        assert!(findings.is_empty(), "Queue(100) should not flag");
    }

    #[tokio::test]
    async fn no_finding_unsupported_language() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/main.go"),
            "ch := make(chan int)\n".into(),
        );
        let ctx = AnalysisContext {
            language: Language::Go,
            source_cache: files,
            ..AnalysisContext::test_default()
        };
        let findings = UnboundedQueueDetector.analyze(&ctx).await.unwrap();
        assert!(findings.is_empty());
    }

    #[test]
    fn does_not_use_cargo_subprocess() {
        assert!(!UnboundedQueueDetector.uses_cargo_subprocess());
    }
}
