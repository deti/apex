/// Rust coverage instrumentation via `cargo llvm-cov --json`.
///
/// Uses LLVM source-based coverage. Each "region entry" segment (has_count=true,
/// is_region_entry=true, is_gap=false) is treated as a coverable unit. Segments
/// with count=0 are reported as uncovered branches; count>0 as executed.
///
/// Requires `cargo-llvm-cov` (`cargo install cargo-llvm-cov`) and
/// `LLVM_COV` / `LLVM_PROFDATA` env vars pointing to LLVM tools when using
/// a non-rustup Rust (e.g. Homebrew).
use apex_core::{
    command::{CommandRunner, CommandSpec, RealCommandRunner},
    error::{ApexError, Result},
    traits::Instrumentor,
    types::{BranchId, InstrumentedTarget, Target},
};
use async_trait::async_trait;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tracing::{info, warn};

pub struct RustCovInstrumentor {
    branch_ids: Vec<BranchId>,
    runner: Arc<dyn CommandRunner>,
}

impl RustCovInstrumentor {
    pub fn new() -> Self {
        RustCovInstrumentor {
            branch_ids: Vec::new(),
            runner: Arc::new(RealCommandRunner),
        }
    }

    /// Create a new instrumentor with a custom command runner (for testing).
    pub fn with_runner(runner: Arc<dyn CommandRunner>) -> Self {
        RustCovInstrumentor {
            branch_ids: Vec::new(),
            runner,
        }
    }
}

impl Default for RustCovInstrumentor {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Instrumentor for RustCovInstrumentor {
    async fn instrument(&self, target: &Target) -> Result<InstrumentedTarget> {
        let root = &target.root;

        if !has_llvm_cov_with_runner(self.runner.as_ref()).await {
            warn!(
                "cargo-llvm-cov not found; install with: cargo install cargo-llvm-cov\n\
                 Also set LLVM_COV and LLVM_PROFDATA if using a non-rustup Rust.\n\
                 Returning empty branch set."
            );
            return Ok(empty_result(target));
        }

        let json_path = root.join(".apex_coverage.json");
        let json_path_str = json_path.to_string_lossy().into_owned();

        let spec = CommandSpec::new("cargo", root).args([
            "llvm-cov",
            "--json",
            "--output-path",
            &json_path_str,
        ]);
        let output = self
            .runner
            .run_command(&spec)
            .await
            .map_err(|e| ApexError::Instrumentation(format!("cargo llvm-cov: {e}")))?;

        if output.exit_code != 0 {
            let stderr = String::from_utf8_lossy(&output.stderr);
            warn!(stderr = %stderr, "cargo llvm-cov exited non-zero; returning empty branch set");
            return Ok(empty_result(target));
        }

        let json_bytes = std::fs::read(&json_path)
            .map_err(|e| ApexError::Instrumentation(format!("read coverage json: {e}")))?;

        let (branch_ids, executed_branch_ids, file_paths) = parse_llvm_json(&json_bytes, root)
            .map_err(|e| ApexError::Instrumentation(format!("parse coverage json: {e}")))?;

        info!(
            total = branch_ids.len(),
            executed = executed_branch_ids.len(),
            files = file_paths.len(),
            "rust coverage baseline"
        );

        Ok(InstrumentedTarget {
            target: target.clone(),
            branch_ids,
            executed_branch_ids,
            file_paths,
            work_dir: root.clone(),
        })
    }

    fn branch_ids(&self) -> &[BranchId] {
        &self.branch_ids
    }
}

// ---------------------------------------------------------------------------
// JSON parser -- LLVM source-based coverage format
// ---------------------------------------------------------------------------

/// Segment layout: [line, col, count, has_count, is_region_entry, is_gap_region]
/// We treat each code-region-entry (has_count && is_region_entry && !is_gap) as
/// one coverable unit, mapped to a BranchId(file_id, line, col, direction=0).
#[allow(clippy::type_complexity)]
pub fn parse_llvm_json(
    bytes: &[u8],
    root: &Path,
) -> std::result::Result<
    (Vec<BranchId>, Vec<BranchId>, HashMap<u64, PathBuf>),
    Box<dyn std::error::Error + Send + Sync>,
> {
    let v: serde_json::Value = serde_json::from_slice(bytes)?;

    let mut branch_ids: Vec<BranchId> = Vec::new();
    let mut executed_ids: Vec<BranchId> = Vec::new();
    let mut file_paths: HashMap<u64, PathBuf> = HashMap::new();

    let data = v["data"].as_array().ok_or("missing data array")?;
    for entry in data {
        let files = entry["files"].as_array().ok_or("missing files array")?;
        for file in files {
            let filename = file["filename"].as_str().ok_or("missing filename")?;

            // Skip files outside the target root (stdlib, deps, etc.)
            let abs = Path::new(filename);
            let rel = match abs.strip_prefix(root) {
                Ok(r) => r.to_path_buf(),
                Err(_) => continue,
            };

            // Skip test files — we only want production branch coverage.
            let rel_str = rel.to_string_lossy();
            if rel_str.starts_with("tests/")
                || rel_str.contains("/tests/")
                || rel_str.ends_with("_test.rs")
                || rel_str.ends_with("_tests.rs")
            {
                continue;
            }

            let fid = fnv1a(&rel.to_string_lossy());
            file_paths.entry(fid).or_insert_with(|| rel.clone());

            let segments = file["segments"].as_array().ok_or("missing segments")?;
            for seg in segments {
                let arr = seg.as_array().ok_or("segment not array")?;
                if arr.len() < 6 {
                    continue;
                }
                let line = arr[0].as_u64().unwrap_or(0) as u32;
                let col = arr[1].as_u64().unwrap_or(0) as u16;
                let count = arr[2].as_u64().unwrap_or(0);
                let has_count = arr[3].as_bool().unwrap_or(false);
                let is_entry = arr[4].as_bool().unwrap_or(false);
                let is_gap = arr[5].as_bool().unwrap_or(false);

                if !has_count || !is_entry || is_gap {
                    continue;
                }

                let bid = BranchId::new(fid, line, col, 0);
                branch_ids.push(bid.clone());
                if count > 0 {
                    executed_ids.push(bid);
                }
            }
        }
    }

    // Deduplicate.
    branch_ids.sort_by_key(|b| (b.file_id, b.line, b.col));
    branch_ids.dedup();
    executed_ids.sort_by_key(|b| (b.file_id, b.line, b.col));
    executed_ids.dedup();

    Ok((branch_ids, executed_ids, file_paths))
}

// ---------------------------------------------------------------------------
// Delta coverage for RustTestSandbox
// ---------------------------------------------------------------------------

/// Run `cargo llvm-cov --json --test <name>` and return executed BranchIds,
/// filtered to only those the oracle doesn't yet know about.
pub async fn run_coverage_for_test(
    test_name: &str,
    root: &Path,
) -> std::result::Result<Vec<BranchId>, Box<dyn std::error::Error + Send + Sync>> {
    run_coverage_for_test_with_runner(test_name, root, &RealCommandRunner).await
}

/// Same as `run_coverage_for_test` but accepts a custom runner.
pub async fn run_coverage_for_test_with_runner(
    test_name: &str,
    root: &Path,
    runner: &dyn CommandRunner,
) -> std::result::Result<Vec<BranchId>, Box<dyn std::error::Error + Send + Sync>> {
    let json_path = root.join(".apex_delta.json");
    let json_path_str = json_path.to_string_lossy().into_owned();

    let spec = CommandSpec::new("cargo", root).args([
        "llvm-cov",
        "--json",
        "--output-path",
        &json_path_str,
        "--test",
        test_name,
    ]);
    let output = runner
        .run_command(&spec)
        .await
        .map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send + Sync>)?;

    if output.exit_code != 0 {
        return Ok(Vec::new());
    }

    let bytes = std::fs::read(&json_path)?;
    let (_, executed, _) = parse_llvm_json(&bytes, root)?;
    Ok(executed)
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Check if `cargo-llvm-cov` is available (using the real command runner).
pub async fn has_llvm_cov() -> bool {
    has_llvm_cov_with_runner(&RealCommandRunner).await
}

/// Check if `cargo-llvm-cov` is available (using a custom runner).
pub async fn has_llvm_cov_with_runner(runner: &dyn CommandRunner) -> bool {
    let spec = CommandSpec::new("cargo", "/tmp").args(["llvm-cov", "--version"]);
    runner
        .run_command(&spec)
        .await
        .map(|o| o.exit_code == 0)
        .unwrap_or(false)
}

fn empty_result(target: &Target) -> InstrumentedTarget {
    InstrumentedTarget {
        target: target.clone(),
        branch_ids: Vec::new(),
        executed_branch_ids: Vec::new(),
        file_paths: HashMap::new(),
        work_dir: target.root.clone(),
    }
}

pub fn fnv1a(s: &str) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in s.bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

#[cfg(test)]
mod tests {
    use super::*;
    use apex_core::command::CommandOutput;

    /// A test-only CommandRunner that returns a configurable output.
    struct FakeRunner {
        exit_code: i32,
        stdout: Vec<u8>,
        stderr: Vec<u8>,
        fail: bool,
    }

    impl FakeRunner {
        fn success() -> Self {
            FakeRunner {
                exit_code: 0,
                stdout: Vec::new(),
                stderr: Vec::new(),
                fail: false,
            }
        }

        fn success_with_stdout(stdout: Vec<u8>) -> Self {
            FakeRunner {
                exit_code: 0,
                stdout,
                stderr: Vec::new(),
                fail: false,
            }
        }

        fn failure(exit_code: i32) -> Self {
            FakeRunner {
                exit_code,
                stdout: Vec::new(),
                stderr: b"error occurred".to_vec(),
                fail: false,
            }
        }

        fn spawn_error() -> Self {
            FakeRunner {
                exit_code: -1,
                stdout: Vec::new(),
                stderr: Vec::new(),
                fail: true,
            }
        }
    }

    #[async_trait]
    impl CommandRunner for FakeRunner {
        async fn run_command(
            &self,
            _spec: &CommandSpec,
        ) -> apex_core::error::Result<CommandOutput> {
            if self.fail {
                return Err(ApexError::Subprocess {
                    exit_code: -1,
                    stderr: "spawn failed".into(),
                });
            }
            Ok(CommandOutput {
                exit_code: self.exit_code,
                stdout: self.stdout.clone(),
                stderr: self.stderr.clone(),
            })
        }
    }

    /// Minimal LLVM source-based coverage JSON.
    fn sample_llvm_json(root: &str) -> String {
        format!(
            r#"{{
  "data": [
    {{
      "files": [
        {{
          "filename": "{root}/src/main.rs",
          "segments": [
            [5, 1, 10, true, true, false],
            [8, 5, 0, true, true, false],
            [12, 1, 3, true, true, false],
            [15, 1, 0, false, false, false],
            [20, 1, 1, true, false, false],
            [25, 1, 0, true, true, true]
          ]
        }},
        {{
          "filename": "{root}/src/lib.rs",
          "segments": [
            [3, 1, 5, true, true, false],
            [7, 1, 0, true, true, false]
          ]
        }},
        {{
          "filename": "/rustc/abc123/library/core/src/ops.rs",
          "segments": [
            [1, 1, 100, true, true, false]
          ]
        }}
      ]
    }}
  ]
}}"#
        )
    }

    #[test]
    fn test_parse_llvm_json_counts() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let json = sample_llvm_json(root.to_str().unwrap());

        let (all, exec, fps) = parse_llvm_json(json.as_bytes(), root).unwrap();

        assert_eq!(all.len(), 5);
        assert_eq!(exec.len(), 3);
        assert_eq!(fps.len(), 2);
    }

    #[test]
    fn test_parse_llvm_json_skips_external_files() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let json = sample_llvm_json(root.to_str().unwrap());

        let (_, _, fps) = parse_llvm_json(json.as_bytes(), root).unwrap();

        for (_, path) in &fps {
            let s = path.to_string_lossy();
            assert!(!s.contains("ops.rs"), "should skip external file: {s}");
        }
    }

    #[test]
    fn test_parse_llvm_json_deduplication() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let json = format!(
            r#"{{
  "data": [
    {{
      "files": [
        {{
          "filename": "{root}/src/dup.rs",
          "segments": [
            [1, 1, 5, true, true, false],
            [1, 1, 5, true, true, false]
          ]
        }}
      ]
    }}
  ]
}}"#,
            root = root.to_str().unwrap()
        );

        let (all, exec, _) = parse_llvm_json(json.as_bytes(), root).unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(exec.len(), 1);
    }

    #[test]
    fn test_parse_llvm_json_empty_data() {
        let json = r#"{"data": [{"files": []}]}"#;
        let (all, exec, fps) = parse_llvm_json(json.as_bytes(), Path::new("/nonexistent")).unwrap();
        assert_eq!(all.len(), 0);
        assert_eq!(exec.len(), 0);
        assert_eq!(fps.len(), 0);
    }

    #[test]
    fn test_parse_llvm_json_short_segment_skipped() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let json = format!(
            r#"{{
  "data": [{{
    "files": [{{
      "filename": "{root}/src/short.rs",
      "segments": [[1, 2, 3, true, true]]
    }}]
  }}]
}}"#,
            root = root.to_str().unwrap()
        );

        let (all, _, _) = parse_llvm_json(json.as_bytes(), root).unwrap();
        assert_eq!(all.len(), 0);
    }

    #[test]
    fn test_parse_llvm_json_invalid_json() {
        let result = parse_llvm_json(b"not json", Path::new("/tmp"));
        assert!(result.is_err());
    }

    #[test]
    fn test_fnv1a_consistency() {
        assert_eq!(fnv1a("src/main.rs"), fnv1a("src/main.rs"));
        assert_ne!(fnv1a("src/main.rs"), fnv1a("src/lib.rs"));
    }

    #[test]
    fn test_fnv1a_empty_string() {
        assert_eq!(fnv1a(""), 0xcbf2_9ce4_8422_2325);
    }

    #[test]
    fn test_empty_result() {
        let target = Target {
            root: PathBuf::from("/my/project"),
            language: apex_core::types::Language::Rust,
            test_command: Vec::new(),
        };
        let result = empty_result(&target);
        assert!(result.branch_ids.is_empty());
        assert!(result.executed_branch_ids.is_empty());
        assert!(result.file_paths.is_empty());
        assert_eq!(result.work_dir, PathBuf::from("/my/project"));
        assert_eq!(result.target.root, PathBuf::from("/my/project"));
    }

    #[test]
    fn test_new_instrumentor() {
        let inst = RustCovInstrumentor::new();
        assert!(inst.branch_ids.is_empty());
    }

    #[test]
    fn test_default_instrumentor() {
        let inst = RustCovInstrumentor::default();
        assert!(inst.branch_ids.is_empty());
    }

    #[test]
    fn test_branch_ids_accessor() {
        use apex_core::traits::Instrumentor;
        let inst = RustCovInstrumentor::new();
        assert_eq!(inst.branch_ids().len(), 0);
    }

    #[test]
    fn test_parse_llvm_json_missing_data_key() {
        let json = r#"{"not_data": []}"#;
        let result = parse_llvm_json(json.as_bytes(), Path::new("/tmp"));
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_llvm_json_missing_files_key() {
        let json = r#"{"data": [{"not_files": []}]}"#;
        let result = parse_llvm_json(json.as_bytes(), Path::new("/tmp"));
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_llvm_json_missing_filename() {
        let json = r#"{"data": [{"files": [{"no_filename": true, "segments": []}]}]}"#;
        let result = parse_llvm_json(json.as_bytes(), Path::new("/tmp"));
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_llvm_json_missing_segments() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let json = format!(
            r#"{{"data": [{{"files": [{{"filename": "{}/src/a.rs"}}]}}]}}"#,
            root.display()
        );
        let result = parse_llvm_json(json.as_bytes(), root);
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_llvm_json_all_zero_counts() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let json = format!(
            r#"{{
  "data": [{{
    "files": [{{
      "filename": "{}/src/zero.rs",
      "segments": [
        [1, 1, 0, true, true, false],
        [5, 1, 0, true, true, false]
      ]
    }}]
  }}]
}}"#,
            root.display()
        );
        let (all, exec, fps) = parse_llvm_json(json.as_bytes(), root).unwrap();
        assert_eq!(all.len(), 2);
        assert_eq!(exec.len(), 0);
        assert_eq!(fps.len(), 1);
    }

    #[test]
    fn test_parse_llvm_json_gap_region_skipped() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let json = format!(
            r#"{{
  "data": [{{
    "files": [{{
      "filename": "{}/src/gap.rs",
      "segments": [
        [1, 1, 5, true, true, true],
        [2, 1, 5, true, true, false]
      ]
    }}]
  }}]
}}"#,
            root.display()
        );
        let (all, exec, _) = parse_llvm_json(json.as_bytes(), root).unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(exec.len(), 1);
    }

    #[test]
    fn test_parse_llvm_json_not_region_entry_skipped() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let json = format!(
            r#"{{
  "data": [{{
    "files": [{{
      "filename": "{}/src/noentry.rs",
      "segments": [
        [1, 1, 5, true, false, false],
        [2, 1, 5, false, true, false]
      ]
    }}]
  }}]
}}"#,
            root.display()
        );
        let (all, _, _) = parse_llvm_json(json.as_bytes(), root).unwrap();
        assert_eq!(all.len(), 0);
    }

    #[test]
    fn test_parse_llvm_json_multiple_data_entries() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let json = format!(
            r#"{{
  "data": [
    {{
      "files": [{{
        "filename": "{root}/src/a.rs",
        "segments": [[1, 1, 1, true, true, false]]
      }}]
    }},
    {{
      "files": [{{
        "filename": "{root}/src/b.rs",
        "segments": [[2, 1, 0, true, true, false]]
      }}]
    }}
  ]
}}"#,
            root = root.display()
        );
        let (all, exec, fps) = parse_llvm_json(json.as_bytes(), root).unwrap();
        assert_eq!(all.len(), 2);
        assert_eq!(exec.len(), 1);
        assert_eq!(fps.len(), 2);
    }

    #[test]
    fn test_parse_llvm_json_segment_not_array() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let json = format!(
            r#"{{
  "data": [{{
    "files": [{{
      "filename": "{}/src/bad.rs",
      "segments": ["not_an_array"]
    }}]
  }}]
}}"#,
            root.display()
        );
        let result = parse_llvm_json(json.as_bytes(), root);
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_llvm_json_col_preserved() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let json = format!(
            r#"{{
  "data": [{{
    "files": [{{
      "filename": "{}/src/col.rs",
      "segments": [
        [10, 42, 1, true, true, false]
      ]
    }}]
  }}]
}}"#,
            root.display()
        );
        let (all, _, _) = parse_llvm_json(json.as_bytes(), root).unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].line, 10);
        assert_eq!(all[0].col, 42);
        assert_eq!(all[0].direction, 0);
    }

    // -----------------------------------------------------------------------
    // Mock-based instrument() tests
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn test_has_llvm_cov_returns_true() {
        let runner = FakeRunner::success();
        assert!(has_llvm_cov_with_runner(&runner).await);
    }

    #[tokio::test]
    async fn test_has_llvm_cov_returns_false_on_failure() {
        let runner = FakeRunner::failure(1);
        assert!(!has_llvm_cov_with_runner(&runner).await);
    }

    #[tokio::test]
    async fn test_has_llvm_cov_returns_false_on_error() {
        let runner = FakeRunner::spawn_error();
        assert!(!has_llvm_cov_with_runner(&runner).await);
    }

    #[tokio::test]
    async fn test_instrument_no_llvm_cov_returns_empty() {
        // Runner that fails for "llvm-cov --version" check
        let runner = Arc::new(FakeRunner::failure(127));
        let inst = RustCovInstrumentor::with_runner(runner);

        let target = Target {
            root: PathBuf::from("/tmp/fake-project"),
            language: apex_core::types::Language::Rust,
            test_command: Vec::new(),
        };

        let result = inst.instrument(&target).await.unwrap();
        assert!(result.branch_ids.is_empty());
        assert!(result.executed_branch_ids.is_empty());
    }

    #[tokio::test]
    async fn test_instrument_success_with_mock() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().to_path_buf();
        let llvm_json = sample_llvm_json(root.to_str().unwrap());

        // Write the coverage JSON file that instrument() will read
        std::fs::write(root.join(".apex_coverage.json"), &llvm_json).unwrap();

        // Runner that succeeds for both the version check and the coverage run
        struct SuccessRunner;
        #[async_trait]
        impl CommandRunner for SuccessRunner {
            async fn run_command(
                &self,
                _spec: &CommandSpec,
            ) -> apex_core::error::Result<CommandOutput> {
                Ok(CommandOutput::success(Vec::new()))
            }
        }

        let runner = Arc::new(SuccessRunner);
        let inst = RustCovInstrumentor::with_runner(runner);

        let target = Target {
            root: root.clone(),
            language: apex_core::types::Language::Rust,
            test_command: Vec::new(),
        };

        let result = inst.instrument(&target).await.unwrap();
        assert_eq!(result.branch_ids.len(), 5);
        assert_eq!(result.executed_branch_ids.len(), 3);
        assert_eq!(result.file_paths.len(), 2);
    }

    #[tokio::test]
    async fn test_instrument_nonzero_exit_returns_empty() {
        use std::sync::atomic::{AtomicU32, Ordering};

        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().to_path_buf();

        // Runner: version check succeeds, but coverage run fails
        struct VersionOkCovFailRunner {
            call_count: AtomicU32,
        }
        #[async_trait]
        impl CommandRunner for VersionOkCovFailRunner {
            async fn run_command(
                &self,
                spec: &CommandSpec,
            ) -> apex_core::error::Result<CommandOutput> {
                let n = self.call_count.fetch_add(1, Ordering::SeqCst);
                if n == 0 {
                    // First call: version check
                    Ok(CommandOutput::success(Vec::new()))
                } else {
                    // Second call: coverage run fails
                    Ok(CommandOutput::failure(1, b"compilation error".to_vec()))
                }
            }
        }

        let runner = Arc::new(VersionOkCovFailRunner {
            call_count: AtomicU32::new(0),
        });
        let inst = RustCovInstrumentor::with_runner(runner);

        let target = Target {
            root,
            language: apex_core::types::Language::Rust,
            test_command: Vec::new(),
        };

        let result = inst.instrument(&target).await.unwrap();
        // Non-zero exit returns empty set
        assert!(result.branch_ids.is_empty());
    }

    #[tokio::test]
    async fn test_run_coverage_for_test_with_mock() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let llvm_json = sample_llvm_json(root.to_str().unwrap());

        // Write the delta JSON that the function will read
        std::fs::write(root.join(".apex_delta.json"), &llvm_json).unwrap();

        let runner = FakeRunner::success();
        let result = run_coverage_for_test_with_runner("my_test", root, &runner)
            .await
            .unwrap();
        assert_eq!(result.len(), 3); // 3 executed segments
    }

    #[tokio::test]
    async fn test_run_coverage_for_test_failure() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();

        let runner = FakeRunner::failure(1);
        let result = run_coverage_for_test_with_runner("my_test", root, &runner)
            .await
            .unwrap();
        assert!(result.is_empty()); // non-zero exit returns empty
    }

    // -----------------------------------------------------------------------
    // Additional coverage tests
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn test_run_coverage_for_test_spawn_error() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();

        let runner = FakeRunner::spawn_error();
        let result = run_coverage_for_test_with_runner("my_test", root, &runner).await;
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_llvm_json_segment_with_missing_values() {
        // Segments with null/missing values for count/has_count/is_entry/is_gap
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let json = format!(
            r#"{{
  "data": [{{
    "files": [{{
      "filename": "{}/src/edge.rs",
      "segments": [
        [1, 1, null, null, null, null],
        [2, 1, 5, true, true, false]
      ]
    }}]
  }}]
}}"#,
            root.display()
        );
        let (all, exec, _) = parse_llvm_json(json.as_bytes(), root).unwrap();
        // First segment: has_count=false (unwrap_or(false)), so skipped
        // Second segment: valid
        assert_eq!(all.len(), 1);
        assert_eq!(exec.len(), 1);
    }

    #[test]
    fn test_parse_llvm_json_zero_line_col() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let json = format!(
            r#"{{
  "data": [{{
    "files": [{{
      "filename": "{}/src/z.rs",
      "segments": [
        [0, 0, 1, true, true, false]
      ]
    }}]
  }}]
}}"#,
            root.display()
        );
        let (all, exec, _) = parse_llvm_json(json.as_bytes(), root).unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].line, 0);
        assert_eq!(all[0].col, 0);
        assert_eq!(exec.len(), 1);
    }

    #[test]
    fn test_parse_llvm_json_empty_segments() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let json = format!(
            r#"{{
  "data": [{{
    "files": [{{
      "filename": "{}/src/empty_seg.rs",
      "segments": []
    }}]
  }}]
}}"#,
            root.display()
        );
        let (all, exec, fps) = parse_llvm_json(json.as_bytes(), root).unwrap();
        assert_eq!(all.len(), 0);
        assert_eq!(exec.len(), 0);
        // File should still be registered in file_paths
        assert_eq!(fps.len(), 1);
    }

    #[test]
    fn test_parse_llvm_json_multiple_files_same_data_entry() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let json = format!(
            r#"{{
  "data": [{{
    "files": [
      {{
        "filename": "{root}/src/one.rs",
        "segments": [[1, 1, 1, true, true, false]]
      }},
      {{
        "filename": "{root}/src/two.rs",
        "segments": [[2, 1, 0, true, true, false]]
      }},
      {{
        "filename": "{root}/src/three.rs",
        "segments": [[3, 1, 5, true, true, false]]
      }}
    ]
  }}]
}}"#,
            root = root.display()
        );
        let (all, exec, fps) = parse_llvm_json(json.as_bytes(), root).unwrap();
        assert_eq!(all.len(), 3);
        assert_eq!(exec.len(), 2); // count=1 and count=5
        assert_eq!(fps.len(), 3);
    }

    #[test]
    fn test_fnv1a_single_chars() {
        assert_ne!(fnv1a("a"), fnv1a("b"));
        assert_ne!(fnv1a("a"), fnv1a("A"));
        assert_eq!(fnv1a("a"), fnv1a("a"));
    }

    #[test]
    fn test_fnv1a_long_string() {
        let long = "a".repeat(1000);
        let h1 = fnv1a(&long);
        let h2 = fnv1a(&long);
        assert_eq!(h1, h2);
        assert_ne!(h1, fnv1a(""));
    }

    #[test]
    fn test_with_runner_constructor() {
        let runner = Arc::new(FakeRunner::success());
        let inst = RustCovInstrumentor::with_runner(runner);
        assert!(inst.branch_ids.is_empty());
    }

    #[tokio::test]
    async fn test_instrument_spawn_error() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().to_path_buf();

        // Write coverage JSON (in case it gets that far)
        let json = sample_llvm_json(root.to_str().unwrap());
        std::fs::write(root.join(".apex_coverage.json"), &json).unwrap();

        // Runner: version check succeeds, coverage run fails with spawn error
        use std::sync::atomic::{AtomicU32, Ordering};
        struct VersionOkSpawnFailRunner {
            call_count: AtomicU32,
        }
        #[async_trait]
        impl CommandRunner for VersionOkSpawnFailRunner {
            async fn run_command(
                &self,
                _spec: &CommandSpec,
            ) -> apex_core::error::Result<CommandOutput> {
                let n = self.call_count.fetch_add(1, Ordering::SeqCst);
                if n == 0 {
                    Ok(CommandOutput::success(Vec::new()))
                } else {
                    Err(ApexError::Subprocess {
                        exit_code: -1,
                        stderr: "spawn failed".into(),
                    })
                }
            }
        }

        let runner = Arc::new(VersionOkSpawnFailRunner {
            call_count: AtomicU32::new(0),
        });
        let inst = RustCovInstrumentor::with_runner(runner);

        let target = Target {
            root,
            language: apex_core::types::Language::Rust,
            test_command: Vec::new(),
        };

        let result = inst.instrument(&target).await;
        assert!(result.is_err());
    }

    #[test]
    fn test_empty_result_preserves_language() {
        let target = Target {
            root: PathBuf::from("/my/project"),
            language: apex_core::types::Language::Rust,
            test_command: vec!["cargo".into(), "test".into()],
        };
        let result = empty_result(&target);
        assert_eq!(result.target.language, apex_core::types::Language::Rust);
        assert_eq!(result.target.test_command, vec!["cargo", "test"]);
    }

    #[test]
    fn test_parse_llvm_json_file_paths_deduplicated() {
        // Two data entries referencing the same file
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let json = format!(
            r#"{{
  "data": [
    {{
      "files": [{{
        "filename": "{root}/src/same.rs",
        "segments": [[1, 1, 1, true, true, false]]
      }}]
    }},
    {{
      "files": [{{
        "filename": "{root}/src/same.rs",
        "segments": [[2, 1, 0, true, true, false]]
      }}]
    }}
  ]
}}"#,
            root = root.display()
        );
        let (all, _, fps) = parse_llvm_json(json.as_bytes(), root).unwrap();
        assert_eq!(all.len(), 2);
        // file_paths uses entry(), so same file_id maps to one entry
        assert_eq!(fps.len(), 1);
    }

    #[test]
    fn test_parse_llvm_json_skips_test_files() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let json = format!(
            r#"{{
  "data": [
    {{
      "files": [
        {{
          "filename": "{root}/src/lib.rs",
          "segments": [[1, 1, 5, true, true, false]]
        }},
        {{
          "filename": "{root}/tests/integration.rs",
          "segments": [[1, 1, 3, true, true, false]]
        }},
        {{
          "filename": "{root}/src/foo_test.rs",
          "segments": [[1, 1, 2, true, true, false]]
        }},
        {{
          "filename": "{root}/crates/bar/tests/unit.rs",
          "segments": [[1, 1, 1, true, true, false]]
        }},
        {{
          "filename": "{root}/src/helpers_tests.rs",
          "segments": [[1, 1, 1, true, true, false]]
        }}
      ]
    }}
  ]
}}"#,
            root = root.display()
        );
        let (all, executed, fps) = parse_llvm_json(json.as_bytes(), root).unwrap();
        // Only src/lib.rs should survive — all test files filtered out
        assert_eq!(fps.len(), 1);
        let path = fps.values().next().unwrap();
        assert_eq!(path, &PathBuf::from("src/lib.rs"));
        assert_eq!(all.len(), 1);
        assert_eq!(executed.len(), 1);
    }
}
