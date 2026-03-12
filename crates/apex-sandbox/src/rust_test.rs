/// Sandbox that runs a Rust test candidate in a cargo workspace and measures
/// coverage delta via `cargo llvm-cov`.
///
/// Flow:
///   1. Write candidate code to `<root>/tests/apex_probe_<uuid>.rs`.
///   2. Run `cargo llvm-cov --json --test apex_probe_<uuid>` (or plain `cargo test`).
///   3. Filter executed regions by oracle state to find newly covered units.
///   4. Delete the probe file.
///   5. Return ExecutionResult with new_branches delta.
use apex_core::{
    error::{ApexError, Result},
    traits::Sandbox,
    types::{BranchId, ExecutionResult, ExecutionStatus, InputSeed, Language, SnapshotId},
};
use apex_coverage::CoverageOracle;
use apex_instrument::rust_cov::{has_llvm_cov, run_coverage_for_test};
use async_trait::async_trait;
use std::{collections::HashMap, path::PathBuf, sync::Arc, time::Instant};
use tracing::{info, warn};
use uuid::Uuid;

pub struct RustTestSandbox {
    oracle: Arc<CoverageOracle>,
    target_root: PathBuf,
}

impl RustTestSandbox {
    pub fn new(
        oracle: Arc<CoverageOracle>,
        _file_paths: Arc<HashMap<u64, PathBuf>>,
        target_root: PathBuf,
    ) -> Self {
        RustTestSandbox {
            oracle,
            target_root,
        }
    }
}

#[async_trait]
impl Sandbox for RustTestSandbox {
    async fn run(&self, input: &InputSeed) -> Result<ExecutionResult> {
        let code = std::str::from_utf8(&input.data)
            .map_err(|e| ApexError::Sandbox(format!("non-UTF-8 test code: {e}")))?;

        let probe_name = format!("apex_probe_{}", Uuid::new_v4().simple());
        let probe_path = self
            .target_root
            .join("tests")
            .join(format!("{probe_name}.rs"));

        std::fs::create_dir_all(self.target_root.join("tests"))
            .map_err(|e| ApexError::Sandbox(format!("create tests dir: {e}")))?;

        std::fs::write(&probe_path, code)
            .map_err(|e| ApexError::Sandbox(format!("write probe: {e}")))?;

        let use_llvm = has_llvm_cov().await;
        let start = Instant::now();

        let output = if use_llvm {
            // Run with coverage instrumentation.
            tokio::process::Command::new("cargo")
                .args(["llvm-cov", "--test", &probe_name, "--no-report"])
                .current_dir(&self.target_root)
                .output()
                .await
                .map_err(|e| ApexError::Sandbox(format!("cargo llvm-cov: {e}")))?
        } else {
            tokio::process::Command::new("cargo")
                .args(["test", "--test", &probe_name])
                .current_dir(&self.target_root)
                .output()
                .await
                .map_err(|e| ApexError::Sandbox(format!("cargo test: {e}")))?
        };
        let duration_ms = start.elapsed().as_millis() as u64;

        let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
        let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
        let status = if output.status.success() {
            ExecutionStatus::Pass
        } else {
            ExecutionStatus::Fail
        };

        // Get coverage delta.
        let new_branches: Vec<BranchId> = if use_llvm && output.status.success() {
            match run_coverage_for_test(&probe_name, &self.target_root).await {
                Ok(executed) => {
                    let oracle = &self.oracle;
                    executed
                        .into_iter()
                        .filter(|b| {
                            matches!(
                                oracle.state_of(b),
                                Some(apex_core::types::BranchState::Uncovered)
                            )
                        })
                        .collect()
                }
                Err(e) => {
                    warn!(error = %e, "coverage delta failed");
                    Vec::new()
                }
            }
        } else {
            Vec::new()
        };

        // Clean up probe file.
        let _ = std::fs::remove_file(&probe_path);

        if !new_branches.is_empty() {
            info!(
                probe = %probe_name,
                new = new_branches.len(),
                "rust sandbox: new regions covered"
            );
        }

        Ok(ExecutionResult {
            seed_id: input.id,
            status,
            new_branches,
            trace: None,
            duration_ms,
            stdout,
            stderr,
        })
    }

    async fn snapshot(&self) -> Result<SnapshotId> {
        Err(ApexError::Sandbox(
            "RustTestSandbox does not support snapshots".into(),
        ))
    }

    async fn restore(&self, _id: SnapshotId) -> Result<()> {
        Err(ApexError::Sandbox(
            "RustTestSandbox does not support restore".into(),
        ))
    }

    fn language(&self) -> Language {
        Language::Rust
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_oracle() -> Arc<CoverageOracle> {
        Arc::new(CoverageOracle::new())
    }

    fn make_file_paths() -> Arc<HashMap<u64, PathBuf>> {
        Arc::new(HashMap::new())
    }

    #[test]
    fn new_stores_oracle_and_root() {
        let oracle = make_oracle();
        let root = PathBuf::from("/my/project");
        let sb = RustTestSandbox::new(oracle.clone(), make_file_paths(), root.clone());
        assert_eq!(sb.target_root, root);
        // oracle is the same Arc
        assert!(Arc::ptr_eq(&sb.oracle, &oracle));
    }

    #[test]
    fn language_returns_rust() {
        use apex_core::traits::Sandbox;
        let sb = RustTestSandbox::new(make_oracle(), make_file_paths(), PathBuf::from("/tmp"));
        assert_eq!(sb.language(), Language::Rust);
    }

    #[test]
    fn snapshot_returns_error() {
        let sb = RustTestSandbox::new(make_oracle(), make_file_paths(), PathBuf::from("/tmp"));
        let rt = tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap();
        let result = rt.block_on(async {
            use apex_core::traits::Sandbox;
            sb.snapshot().await
        });
        assert!(result.is_err());
    }

    #[test]
    fn restore_returns_error() {
        let sb = RustTestSandbox::new(make_oracle(), make_file_paths(), PathBuf::from("/tmp"));
        let rt = tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap();
        let result = rt.block_on(async {
            use apex_core::traits::Sandbox;
            sb.restore(SnapshotId::new()).await
        });
        assert!(result.is_err());
    }

    #[test]
    fn file_paths_param_is_ignored() {
        // _file_paths is unused in RustTestSandbox — just ensure construction works
        let mut fps = HashMap::new();
        fps.insert(42u64, PathBuf::from("src/main.rs"));
        let sb = RustTestSandbox::new(make_oracle(), Arc::new(fps), PathBuf::from("/root"));
        assert_eq!(sb.target_root, PathBuf::from("/root"));
    }

    // ------------------------------------------------------------------
    // Additional branch-coverage tests
    // ------------------------------------------------------------------

    /// `snapshot()` error message contains sandbox name.
    #[test]
    fn snapshot_error_message_contains_name() {
        let sb = RustTestSandbox::new(make_oracle(), make_file_paths(), PathBuf::from("/tmp"));
        let rt = tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap();
        let err = rt.block_on(async {
            use apex_core::traits::Sandbox;
            sb.snapshot().await
        });
        let msg = format!("{}", err.unwrap_err());
        assert!(msg.contains("RustTestSandbox"), "error: {msg}");
    }

    /// `restore()` error message contains sandbox name.
    #[test]
    fn restore_error_message_contains_name() {
        let sb = RustTestSandbox::new(make_oracle(), make_file_paths(), PathBuf::from("/tmp"));
        let rt = tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap();
        let err = rt.block_on(async {
            use apex_core::traits::Sandbox;
            sb.restore(SnapshotId::new()).await
        });
        let msg = format!("{}", err.unwrap_err());
        assert!(msg.contains("RustTestSandbox"), "error: {msg}");
    }

    /// `language()` is `Rust`.
    #[test]
    fn language_is_rust() {
        use apex_core::traits::Sandbox;
        let sb = RustTestSandbox::new(make_oracle(), make_file_paths(), PathBuf::from("/r"));
        assert_eq!(sb.language(), apex_core::types::Language::Rust);
    }

    /// Two sandboxes created with different roots differ in `target_root`.
    #[test]
    fn different_roots_are_different() {
        let sb1 = RustTestSandbox::new(make_oracle(), make_file_paths(), PathBuf::from("/a"));
        let sb2 = RustTestSandbox::new(make_oracle(), make_file_paths(), PathBuf::from("/b"));
        assert_ne!(sb1.target_root, sb2.target_root);
    }

    /// Oracle is shared via Arc — mutations on the original Arc are visible
    /// through the sandbox's copy.
    #[test]
    fn oracle_arc_is_shared() {
        let oracle = make_oracle();
        let sb = RustTestSandbox::new(oracle.clone(), make_file_paths(), PathBuf::from("/r"));
        let b = apex_core::types::BranchId::new(1, 1, 0, 0);
        oracle.register_branches([b]);
        assert_eq!(sb.oracle.total_count(), 1);
    }
}
