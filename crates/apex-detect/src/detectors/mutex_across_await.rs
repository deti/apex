/// CWE-833 — Deadlock: `std::sync::Mutex` (or `parking_lot::Mutex`) lock guard
/// held across an `.await` point.
///
/// Holding a blocking mutex guard across an `.await` means the guard is alive
/// while the task is suspended, preventing any other task from acquiring the
/// lock.  This causes deadlocks when the executor tries to re-enter the same
/// mutex on a single-threaded runtime, and is a logic error on multi-threaded
/// runtimes.  The correct fix is to either drop the guard before `.await`, or
/// switch to `tokio::sync::Mutex` (which is async-aware).
///
/// Severity: High
/// Languages: Rust only
use apex_core::error::Result;
use apex_core::types::Language;
use async_trait::async_trait;
use regex::Regex;
use std::sync::LazyLock;
use uuid::Uuid;

use super::util::{find_async_fn_scopes, in_test_block, is_comment};
use crate::context::AnalysisContext;
use crate::finding::{Finding, FindingCategory, Severity};
use crate::Detector;

pub struct MutexAcrossAwaitDetector;

/// Matches `.lock()` or `.lock().unwrap()` — acquiring a blocking mutex guard.
/// Excludes `tokio::sync::Mutex` because its `.lock().await` is async-safe.
static LOCK_CALL: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\.lock\(\)").expect("lock regex must compile"));

/// Matches an `.await` expression.
static AWAIT_EXPR: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\.await").expect("await regex must compile"));

/// Matches `tokio::sync::Mutex` or `tokio::sync::RwLock` — these are async-safe.
static TOKIO_ASYNC_MUTEX: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"tokio::sync::(Mutex|RwLock)").expect("tokio mutex regex must compile")
});

/// Matches `drop(` — explicit guard drop before await is safe.
static DROP_CALL: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\bdrop\s*\(").expect("drop regex must compile"));

#[async_trait]
impl Detector for MutexAcrossAwaitDetector {
    fn name(&self) -> &str {
        "mutex-across-await"
    }

    async fn analyze(&self, ctx: &AnalysisContext) -> Result<Vec<Finding>> {
        if ctx.language != Language::Rust {
            return Ok(vec![]);
        }

        let mut findings = Vec::new();

        for (path, source) in &ctx.source_cache {
            let async_scopes = find_async_fn_scopes(source, Language::Rust);
            if async_scopes.is_empty() {
                continue;
            }

            let lines: Vec<&str> = source.lines().collect();

            // Strategy: within each async scope, find a .lock() line and then
            // check whether an .await appears before the scope closes (or before
            // a drop() on a line after the lock acquisition).
            for scope in &async_scopes {
                let scope_lines = &lines[scope.start_line..=scope.end_line];
                let mut lock_line_idx: Option<usize> = None;

                for (rel, line) in scope_lines.iter().enumerate() {
                    let abs_idx = scope.start_line + rel;
                    let trimmed = line.trim();

                    if trimmed.is_empty() || is_comment(trimmed, Language::Rust) {
                        continue;
                    }

                    // Skip lines inside #[cfg(test)]
                    if in_test_block(source, abs_idx) {
                        continue;
                    }

                    // If we see an explicit drop(), clear the pending lock.
                    if DROP_CALL.is_match(trimmed) {
                        lock_line_idx = None;
                    }

                    // If we haven't seen a lock yet, look for one.
                    if lock_line_idx.is_none() && LOCK_CALL.is_match(trimmed) {
                        // Skip tokio async mutexes — they're safe.
                        if TOKIO_ASYNC_MUTEX.is_match(trimmed) {
                            continue;
                        }
                        // A lock().await on the SAME line means we locked a tokio mutex idiom
                        // or immediately awaited — still potentially blocking, but the guard
                        // never outlives a single expression.  Only flag if .await is on a
                        // LATER line.
                        if AWAIT_EXPR.is_match(trimmed) {
                            // Same-line lock+await: guard dropped immediately after, fine.
                            continue;
                        }
                        lock_line_idx = Some(abs_idx);
                        continue;
                    }

                    // If we have a pending lock and we see .await — flag it.
                    if let Some(lock_abs) = lock_line_idx {
                        if AWAIT_EXPR.is_match(trimmed) {
                            let lock_line_1based = (lock_abs + 1) as u32;
                            let await_line_1based = (abs_idx + 1) as u32;
                            findings.push(Finding {
                                id: Uuid::new_v4(),
                                detector: self.name().into(),
                                severity: Severity::High,
                                category: FindingCategory::LogicBug,
                                file: path.clone(),
                                line: Some(lock_line_1based),
                                title: format!(
                                    "Blocking mutex guard held across .await at line {}",
                                    await_line_1based
                                ),
                                description: format!(
                                    "A std::sync::Mutex (or parking_lot::Mutex) guard acquired at \
                                     line {} in {} is still alive at the .await on line {}. \
                                     This can cause deadlocks or starvation. Drop the guard \
                                     before the .await, or use tokio::sync::Mutex instead.",
                                    lock_line_1based,
                                    path.display(),
                                    await_line_1based
                                ),
                                evidence: vec![],
                                covered: false,
                                suggestion: "Drop the guard before the .await point, or replace \
                                             std::sync::Mutex with tokio::sync::Mutex"
                                    .into(),
                                explanation: None,
                                fix: None,
                                cwe_ids: vec![833],
                    noisy: false,
                            });
                            // Report once per lock acquisition.
                            lock_line_idx = None;
                        }
                    }

                    // A closing brace is a nested block close; the guard would
                    // have been dropped when the inner block ended.
                    if trimmed.starts_with('}') {
                        lock_line_idx = None;
                    }
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

    fn make_ctx(files: HashMap<PathBuf, String>) -> AnalysisContext {
        AnalysisContext {
            language: Language::Rust,
            source_cache: files,
            ..AnalysisContext::test_default()
        }
    }

    // ---- True positives ----

    #[tokio::test]
    async fn detects_lock_then_await_in_async_fn() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/lib.rs"),
            r#"
async fn handler(state: Arc<Mutex<State>>) {
    let guard = state.lock().unwrap();
    do_something(&guard);
    some_future().await;
    drop(guard); // too late — guard still alive at .await
}
"#
            .into(),
        );
        let ctx = make_ctx(files);
        let findings = MutexAcrossAwaitDetector.analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, Severity::High);
        assert_eq!(findings[0].category, FindingCategory::LogicBug);
        assert!(findings[0].cwe_ids.contains(&833));
    }

    #[tokio::test]
    async fn detects_parking_lot_lock_across_await() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/server.rs"),
            r#"
async fn update(shared: Arc<parking_lot::Mutex<Config>>) {
    let _cfg = shared.lock();
    fetch_update().await;
}
"#
            .into(),
        );
        let ctx = make_ctx(files);
        let findings = MutexAcrossAwaitDetector.analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 1);
    }

    // ---- True negatives ----

    #[tokio::test]
    async fn no_finding_guard_dropped_before_await() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/lib.rs"),
            r#"
async fn handler(state: Arc<Mutex<State>>) {
    {
        let guard = state.lock().unwrap();
        do_something(&guard);
    } // guard dropped here
    some_future().await;
}
"#
            .into(),
        );
        let ctx = make_ctx(files);
        let findings = MutexAcrossAwaitDetector.analyze(&ctx).await.unwrap();
        assert!(
            findings.is_empty(),
            "guard dropped before await should not flag"
        );
    }

    #[tokio::test]
    async fn no_finding_tokio_sync_mutex() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/lib.rs"),
            // tokio::sync::Mutex — its lock() returns a future; the guard is async-safe
            r#"
async fn handler(state: Arc<tokio::sync::Mutex<State>>) {
    let guard = state.lock().await;
    do_something(&guard);
    other_future().await;
}
"#
            .into(),
        );
        let ctx = make_ctx(files);
        let findings = MutexAcrossAwaitDetector.analyze(&ctx).await.unwrap();
        assert!(findings.is_empty(), "tokio::sync::Mutex should not flag");
    }

    #[tokio::test]
    async fn no_finding_not_in_async_fn() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/lib.rs"),
            r#"
fn sync_fn(state: Arc<Mutex<State>>) {
    let guard = state.lock().unwrap();
    process(&guard);
}
"#
            .into(),
        );
        let ctx = make_ctx(files);
        let findings = MutexAcrossAwaitDetector.analyze(&ctx).await.unwrap();
        assert!(findings.is_empty(), "sync fn should not flag");
    }

    #[tokio::test]
    async fn no_finding_python_language() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/app.py"),
            "guard = mutex.lock()\nresult = await coro()\n".into(),
        );
        let ctx = AnalysisContext {
            language: Language::Python,
            source_cache: files,
            ..AnalysisContext::test_default()
        };
        let findings = MutexAcrossAwaitDetector.analyze(&ctx).await.unwrap();
        assert!(findings.is_empty());
    }

    #[test]
    fn does_not_use_cargo_subprocess() {
        assert!(!MutexAcrossAwaitDetector.uses_cargo_subprocess());
    }
}
