use apex_core::{
    error::{ApexError, Result},
    hash::fnv1a_hash,
    traits::Sandbox,
    types::{BranchId, ExecutionResult, ExecutionStatus, InputSeed, Language, SnapshotId},
};
use apex_coverage::CoverageOracle;
use async_trait::async_trait;
use serde::Deserialize;
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::Arc,
    time::Instant,
};
use tracing::{debug, warn};

/// Check if `uv` is available on PATH.
fn resolve_uv() -> Option<String> {
    std::process::Command::new("uv")
        .arg("--version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .ok()
        .filter(|s| s.success())
        .map(|_| "uv".to_string())
}

// ---------------------------------------------------------------------------
// Coverage JSON wire types (mirrors apex-instrument)
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct ApexCoverageJson {
    files: HashMap<String, FileData>,
}

#[derive(Deserialize)]
struct FileData {
    executed_branches: Vec<[i64; 2]>,
    #[allow(dead_code)]
    missing_branches: Vec<[i64; 2]>,
    #[allow(dead_code)]
    all_branches: Vec<[i64; 2]>,
}

// ---------------------------------------------------------------------------
// PythonTestSandbox
// ---------------------------------------------------------------------------

/// Runs a Python test candidate under `coverage.py`, then computes which
/// previously-uncovered branches were newly hit.
///
/// `InputSeed.data` must be UTF-8 Python source code.
#[allow(dead_code)]
pub struct PythonTestSandbox {
    oracle: Arc<CoverageOracle>,
    /// Maps file_id (FNV-1a of repo-relative path) → repo-relative PathBuf.
    file_paths: Arc<HashMap<u64, PathBuf>>,
    target_dir: PathBuf,
    timeout_ms: u64,
    /// Cached uv binary path — resolved once at construction time.
    uv_bin: Option<String>,
}

impl PythonTestSandbox {
    pub fn new(
        oracle: Arc<CoverageOracle>,
        file_paths: Arc<HashMap<u64, PathBuf>>,
        target_dir: PathBuf,
    ) -> Self {
        PythonTestSandbox {
            oracle,
            file_paths,
            target_dir,
            timeout_ms: 30_000,
            uv_bin: resolve_uv(),
        }
    }

    pub fn with_timeout(mut self, ms: u64) -> Self {
        self.timeout_ms = ms;
        self
    }

    /// Parse the coverage JSON file and return (file_id, line, direction) tuples
    /// for all branches that were executed.
    fn executed_branches_from_json(&self, json_path: &Path) -> Result<Vec<BranchId>> {
        let content = std::fs::read_to_string(json_path)
            .map_err(|e| ApexError::Sandbox(format!("read coverage json: {e}")))?;

        let data: ApexCoverageJson = serde_json::from_str(&content)
            .map_err(|e| ApexError::Sandbox(format!("parse coverage json: {e}")))?;

        let mut branches = Vec::new();
        for (abs_path, fdata) in &data.files {
            // Normalise to repo-root-relative path — must match how the
            // instrumentor computed file_id for the oracle.
            let rel = Path::new(abs_path)
                .strip_prefix(&self.target_dir)
                .unwrap_or(Path::new(abs_path));
            let file_id = fnv1a_hash(&rel.to_string_lossy());

            for pair in &fdata.executed_branches {
                let from_line = pair[0].unsigned_abs() as u32;
                let direction = if pair[1] < 0 { 1u8 } else { 0u8 };
                branches.push(BranchId::new(file_id, from_line, 0, direction));
            }
        }
        Ok(branches)
    }
}

#[async_trait]
impl Sandbox for PythonTestSandbox {
    async fn run(&self, input: &InputSeed) -> Result<ExecutionResult> {
        let start = Instant::now();

        // Write candidate test code to a temporary .py file.
        // NamedTempFile must stay alive until pytest finishes.
        let tmp_dir =
            tempfile::tempdir().map_err(|e| ApexError::Sandbox(format!("tempdir: {e}")))?;

        let test_file = tmp_dir.path().join("test_apex_candidate.py");
        let code = std::str::from_utf8(&input.data)
            .map_err(|e| ApexError::Sandbox(format!("candidate not valid UTF-8: {e}")))?;
        std::fs::write(&test_file, code)
            .map_err(|e| ApexError::Sandbox(format!("write candidate: {e}")))?;

        // Paths for coverage data (unique per run via tmp_dir UUID).
        let cov_data = tmp_dir.path().join("cov.data");
        let cov_json = tmp_dir.path().join("cov.json");

        // TODO(security): Replace direct tokio::process::Command with CommandRunner
        // to enable sandboxing, auditing, and test mocking. Requires adding a
        // `runner: Arc<dyn CommandRunner>` field and updating the Sandbox trait or
        // this struct's constructor.

        // Validate that either uv or python3 is available before spawning.
        if let Some(uv) = &self.uv_bin {
            let uv_check = tokio::process::Command::new(uv)
                .args(["run", "python3", "--version"])
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status()
                .await;
            if uv_check.map(|s| !s.success()).unwrap_or(true) {
                return Err(ApexError::Sandbox(
                    "uv run python3 failed — is python3 available via uv?".into(),
                ));
            }
        } else {
            let python_check = tokio::process::Command::new("python3")
                .arg("--version")
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status()
                .await;
            if python_check.map(|s| !s.success()).unwrap_or(true) {
                return Err(ApexError::Sandbox("python3 not found in PATH".into()));
            }
        }

        // Step 1: run pytest under coverage.py
        // Prefer `uv run python3` when uv is available.
        // Use spawn() + kill_on_drop(true) so the child is killed if the timeout
        // fires — prevents zombie processes leaking after a timeout.
        let run_output = if let Some(uv) = &self.uv_bin {
            let child = tokio::process::Command::new(uv)
                .args([
                    "run",
                    "python3",
                    "-m",
                    "coverage",
                    "run",
                    "--branch",
                    "--source=.",
                    &format!("--data-file={}", cov_data.display()),
                    "-m",
                    "pytest",
                    &test_file.to_string_lossy(),
                    "-x",
                    "-q",
                    "--tb=short",
                ])
                .current_dir(&self.target_dir)
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .kill_on_drop(true)
                .spawn()
                .map_err(|e| ApexError::Sandbox(format!("spawn pytest: {e}")))?;
            tokio::time::timeout(
                std::time::Duration::from_millis(self.timeout_ms),
                child.wait_with_output(),
            )
            .await
            .map(|r| r.map_err(|e| ApexError::Sandbox(format!("spawn pytest: {e}"))))
            .map_err(|_| ())
        } else {
            let child = tokio::process::Command::new("python3")
                .args([
                    "-m",
                    "coverage",
                    "run",
                    "--branch",
                    "--source=.",
                    &format!("--data-file={}", cov_data.display()),
                    "-m",
                    "pytest",
                    &test_file.to_string_lossy(),
                    "-x",
                    "-q",
                    "--tb=short",
                ])
                .current_dir(&self.target_dir)
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .kill_on_drop(true)
                .spawn()
                .map_err(|e| ApexError::Sandbox(format!("spawn pytest: {e}")))?;
            tokio::time::timeout(
                std::time::Duration::from_millis(self.timeout_ms),
                child.wait_with_output(),
            )
            .await
            .map(|r| r.map_err(|e| ApexError::Sandbox(format!("spawn pytest: {e}"))))
            .map_err(|_| ())
        };

        let duration_ms = start.elapsed().as_millis() as u64;

        let run_output = match run_output {
            Err(()) => {
                return Ok(ExecutionResult {
                    seed_id: input.id,
                    status: ExecutionStatus::Timeout,
                    new_branches: Vec::new(),
                    trace: None,
                    duration_ms,
                    stdout: String::new(),
                    stderr: String::new(),
                    input: None,
                });
            }
            Ok(Err(e)) => return Err(e),
            Ok(Ok(o)) => o,
        };

        let stdout = String::from_utf8_lossy(&run_output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&run_output.stderr).to_string();

        let status = match run_output.status.code() {
            Some(0) => ExecutionStatus::Pass,
            Some(1) => ExecutionStatus::Fail, // test failures
            Some(code) if code < 0 => ExecutionStatus::Crash,
            _ => ExecutionStatus::Fail,
        };

        // Step 2: export coverage to JSON (best-effort; may fail on syntax errors).
        // Bounded to 60 s to prevent a stalled coverage export from hanging the run.
        // Use spawn() + kill_on_drop(true) so the child is killed if the timeout
        // fires — prevents zombie processes leaking after a timeout.
        let json_ok = if let Some(uv) = &self.uv_bin {
            let child = tokio::process::Command::new(uv)
                .args([
                    "run",
                    "python3",
                    "-m",
                    "coverage",
                    "json",
                    &format!("--data-file={}", cov_data.display()),
                    "-o",
                    &cov_json.to_string_lossy(),
                ])
                .current_dir(&self.target_dir)
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .kill_on_drop(true)
                .spawn();
            match child {
                Err(e) => {
                    warn!(error = %e, "coverage json command failed to spawn");
                    false
                }
                Ok(child) => {
                    match tokio::time::timeout(
                        std::time::Duration::from_secs(60),
                        child.wait_with_output(),
                    )
                    .await
                    {
                        Ok(Ok(o)) => o.status.success(),
                        Ok(Err(e)) => {
                            warn!(error = %e, "coverage json command failed");
                            false
                        }
                        Err(_) => {
                            warn!("coverage json export timed out after 60 s — skipping");
                            false
                        }
                    }
                }
            }
        } else {
            let child = tokio::process::Command::new("python3")
                .args([
                    "-m",
                    "coverage",
                    "json",
                    &format!("--data-file={}", cov_data.display()),
                    "-o",
                    &cov_json.to_string_lossy(),
                ])
                .current_dir(&self.target_dir)
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .kill_on_drop(true)
                .spawn();
            match child {
                Err(e) => {
                    warn!(error = %e, "coverage json command failed to spawn");
                    false
                }
                Ok(child) => {
                    match tokio::time::timeout(
                        std::time::Duration::from_secs(60),
                        child.wait_with_output(),
                    )
                    .await
                    {
                        Ok(Ok(o)) => o.status.success(),
                        Ok(Err(e)) => {
                            warn!(error = %e, "coverage json command failed");
                            false
                        }
                        Err(_) => {
                            warn!("coverage json export timed out after 60 s — skipping");
                            false
                        }
                    }
                }
            }
        };

        // Step 3: compute coverage delta vs oracle
        let new_branches = if json_ok && cov_json.exists() {
            match self.executed_branches_from_json(&cov_json) {
                Ok(executed) => executed
                    .into_iter()
                    .filter(|b| {
                        matches!(
                            self.oracle.state_of(b),
                            Some(apex_core::types::BranchState::Uncovered)
                        )
                    })
                    .collect(),
                Err(e) => {
                    warn!(error = %e, "failed to parse candidate coverage JSON");
                    Vec::new()
                }
            }
        } else {
            debug!("coverage JSON not produced for this candidate");
            Vec::new()
        };

        Ok(ExecutionResult {
            seed_id: input.id,
            status,
            new_branches,
            trace: None,
            duration_ms,
            stdout,
            stderr,
            input: None,
        })
    }

    async fn snapshot(&self) -> Result<SnapshotId> {
        Err(ApexError::NotSupported(
            "PythonTestSandbox does not support snapshots".into(),
        ))
    }

    async fn restore(&self, _id: SnapshotId) -> Result<()> {
        Err(ApexError::NotSupported(
            "PythonTestSandbox does not support restore".into(),
        ))
    }

    fn language(&self) -> Language {
        Language::Python
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
    fn fnv1a_deterministic() {
        assert_eq!(fnv1a_hash("foo/bar.py"), fnv1a_hash("foo/bar.py"));
    }

    #[test]
    fn fnv1a_different_inputs_differ() {
        assert_ne!(fnv1a_hash("a.py"), fnv1a_hash("b.py"));
    }

    #[test]
    fn fnv1a_empty_string() {
        // Should not panic and should return the FNV offset basis.
        let h = fnv1a_hash("");
        assert_eq!(h, 0xcbf2_9ce4_8422_2325);
    }

    #[test]
    fn new_sets_default_timeout() {
        let sb = PythonTestSandbox::new(make_oracle(), make_file_paths(), PathBuf::from("/proj"));
        assert_eq!(sb.timeout_ms, 30_000);
        assert_eq!(sb.target_dir, PathBuf::from("/proj"));
    }

    #[test]
    fn new_caches_uv_bin_at_construction() {
        let sb = PythonTestSandbox::new(make_oracle(), make_file_paths(), PathBuf::from("/proj"));
        // uv_bin is either None (uv absent) or Some("uv") — never panics.
        match &sb.uv_bin {
            None => {}
            Some(s) => assert_eq!(s, "uv", "expected 'uv', got: {s}"),
        }
    }

    #[test]
    fn with_timeout_overrides() {
        let sb = PythonTestSandbox::new(make_oracle(), make_file_paths(), PathBuf::from("/proj"))
            .with_timeout(5_000);
        assert_eq!(sb.timeout_ms, 5_000);
    }

    #[test]
    fn language_returns_python() {
        use apex_core::traits::Sandbox;
        let sb = PythonTestSandbox::new(make_oracle(), make_file_paths(), PathBuf::from("/proj"));
        assert_eq!(sb.language(), Language::Python);
    }

    #[test]
    fn snapshot_not_supported() {
        let sb = PythonTestSandbox::new(make_oracle(), make_file_paths(), PathBuf::from("/proj"));
        let rt = tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap();
        let err = rt.block_on(async {
            use apex_core::traits::Sandbox;
            sb.snapshot().await
        });
        assert!(err.is_err());
    }

    #[test]
    fn restore_not_supported() {
        let sb = PythonTestSandbox::new(make_oracle(), make_file_paths(), PathBuf::from("/proj"));
        let rt = tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap();
        let err = rt.block_on(async {
            use apex_core::traits::Sandbox;
            sb.restore(SnapshotId::new()).await
        });
        assert!(err.is_err());
    }

    #[test]
    fn executed_branches_from_json_basic() {
        let tmp = tempfile::tempdir().unwrap();
        let target_dir = tmp.path().to_path_buf();
        let json_path = tmp.path().join("cov.json");

        let json = format!(
            r#"{{
  "files": {{
    "{}/src/app.py": {{
      "executed_branches": [[10, 12], [20, -1]],
      "missing_branches": [[30, 35]],
      "all_branches": [[10, 12], [20, -1], [30, 35]]
    }}
  }}
}}"#,
            target_dir.display()
        );
        std::fs::write(&json_path, &json).unwrap();

        let sb = PythonTestSandbox::new(make_oracle(), make_file_paths(), target_dir);
        let branches = sb.executed_branches_from_json(&json_path).unwrap();

        assert_eq!(branches.len(), 2);
        // [10, 12] → line 10, direction 0 (positive to_line)
        assert_eq!(branches[0].line, 10);
        assert_eq!(branches[0].direction, 0);
        // [20, -1] → line 20, direction 1 (negative to_line)
        assert_eq!(branches[1].line, 20);
        assert_eq!(branches[1].direction, 1);
    }

    #[test]
    fn executed_branches_from_json_strips_prefix() {
        let tmp = tempfile::tempdir().unwrap();
        let target_dir = tmp.path().to_path_buf();
        let json_path = tmp.path().join("cov.json");

        let json = format!(
            r#"{{
  "files": {{
    "{}/src/mod.py": {{
      "executed_branches": [[1, 2]],
      "missing_branches": [],
      "all_branches": [[1, 2]]
    }}
  }}
}}"#,
            target_dir.display()
        );
        std::fs::write(&json_path, &json).unwrap();

        let sb = PythonTestSandbox::new(make_oracle(), make_file_paths(), target_dir);
        let branches = sb.executed_branches_from_json(&json_path).unwrap();

        // file_id should be based on relative path "src/mod.py"
        let expected_fid = fnv1a_hash("src/mod.py");
        assert_eq!(branches[0].file_id, expected_fid);
    }

    #[test]
    fn executed_branches_from_json_no_prefix_match() {
        let tmp = tempfile::tempdir().unwrap();
        let json_path = tmp.path().join("cov.json");

        // File path does NOT share target_dir prefix
        let json = r#"{
  "files": {
    "/other/path/src/app.py": {
      "executed_branches": [[5, 10]],
      "missing_branches": [],
      "all_branches": [[5, 10]]
    }
  }
}"#;
        std::fs::write(&json_path, json).unwrap();

        let sb = PythonTestSandbox::new(
            make_oracle(),
            make_file_paths(),
            PathBuf::from("/nonexistent"),
        );
        let branches = sb.executed_branches_from_json(&json_path).unwrap();

        // Should still parse — uses full path as fallback
        assert_eq!(branches.len(), 1);
        let expected_fid = fnv1a_hash("/other/path/src/app.py");
        assert_eq!(branches[0].file_id, expected_fid);
    }

    #[test]
    fn executed_branches_from_json_empty_files() {
        let tmp = tempfile::tempdir().unwrap();
        let json_path = tmp.path().join("cov.json");
        std::fs::write(&json_path, r#"{"files": {}}"#).unwrap();

        let sb = PythonTestSandbox::new(make_oracle(), make_file_paths(), PathBuf::from("/x"));
        let branches = sb.executed_branches_from_json(&json_path).unwrap();
        assert!(branches.is_empty());
    }

    #[test]
    fn executed_branches_from_json_invalid_json() {
        let tmp = tempfile::tempdir().unwrap();
        let json_path = tmp.path().join("cov.json");
        std::fs::write(&json_path, "not json at all").unwrap();

        let sb = PythonTestSandbox::new(make_oracle(), make_file_paths(), PathBuf::from("/x"));
        let result = sb.executed_branches_from_json(&json_path);
        assert!(result.is_err());
    }

    #[test]
    fn executed_branches_from_json_missing_file() {
        let sb = PythonTestSandbox::new(make_oracle(), make_file_paths(), PathBuf::from("/x"));
        let result = sb.executed_branches_from_json(Path::new("/no/such/file.json"));
        assert!(result.is_err());
    }

    #[test]
    fn executed_branches_from_json_multiple_files() {
        let tmp = tempfile::tempdir().unwrap();
        let target_dir = tmp.path().to_path_buf();
        let json_path = tmp.path().join("cov.json");

        let json = format!(
            r#"{{
  "files": {{
    "{td}/a.py": {{
      "executed_branches": [[1, 2]],
      "missing_branches": [],
      "all_branches": [[1, 2]]
    }},
    "{td}/b.py": {{
      "executed_branches": [[3, 4], [5, -1]],
      "missing_branches": [],
      "all_branches": [[3, 4], [5, -1]]
    }}
  }}
}}"#,
            td = target_dir.display()
        );
        std::fs::write(&json_path, &json).unwrap();

        let sb = PythonTestSandbox::new(make_oracle(), make_file_paths(), target_dir);
        let branches = sb.executed_branches_from_json(&json_path).unwrap();

        assert_eq!(branches.len(), 3);
    }

    // ------------------------------------------------------------------
    // Additional branch-coverage tests
    // ------------------------------------------------------------------

    /// `fnv1a_hash` determinism and the empty-string offset-basis.
    #[test]
    fn fnv1a_hash_empty_returns_offset_basis() {
        assert_eq!(fnv1a_hash(""), 0xcbf2_9ce4_8422_2325);
    }

    /// `fnv1a_hash` produces distinct values for different inputs.
    #[test]
    fn fnv1a_hash_distinct_paths() {
        let paths = ["src/a.py", "src/b.py", "lib/util.py"];
        let hashes: std::collections::HashSet<u64> = paths.iter().map(|p| fnv1a_hash(p)).collect();
        assert_eq!(hashes.len(), paths.len());
    }

    /// `with_timeout` builder — already tested, but also verify it chains.
    #[test]
    fn with_timeout_chains() {
        let sb = PythonTestSandbox::new(make_oracle(), make_file_paths(), PathBuf::from("/p"))
            .with_timeout(1_000)
            .with_timeout(2_000); // second call wins
        assert_eq!(sb.timeout_ms, 2_000);
    }

    /// `executed_branches_from_json` with `pair[1] >= 0` → direction 0.
    /// `executed_branches_from_json` with `pair[1] < 0` → direction 1.
    /// Both arms of the ternary are exercised.
    #[test]
    fn executed_branches_direction_both_arms() {
        let tmp = tempfile::tempdir().unwrap();
        let json_path = tmp.path().join("d.json");

        let json = r#"{
  "files": {
    "/x/a.py": {
      "executed_branches": [[10, 0], [20, -5]],
      "missing_branches": [],
      "all_branches": []
    }
  }
}"#;
        std::fs::write(&json_path, json).unwrap();

        let sb = PythonTestSandbox::new(make_oracle(), make_file_paths(), PathBuf::from("/x"));
        let branches = sb.executed_branches_from_json(&json_path).unwrap();
        assert_eq!(branches.len(), 2);
        // [10, 0]  → pair[1] == 0 >= 0  → direction 0
        assert_eq!(branches[0].direction, 0);
        // [20, -5] → pair[1] < 0        → direction 1
        assert_eq!(branches[1].direction, 1);
    }

    /// `executed_branches_from_json` with `pair[1] > 0` (positive, not zero) → direction 0.
    #[test]
    fn executed_branches_positive_to_line_direction_zero() {
        let tmp = tempfile::tempdir().unwrap();
        let json_path = tmp.path().join("e.json");

        let json = r#"{
  "files": {
    "/x/b.py": {
      "executed_branches": [[5, 99]],
      "missing_branches": [],
      "all_branches": []
    }
  }
}"#;
        std::fs::write(&json_path, json).unwrap();

        let sb = PythonTestSandbox::new(make_oracle(), make_file_paths(), PathBuf::from("/x"));
        let branches = sb.executed_branches_from_json(&json_path).unwrap();
        assert_eq!(branches.len(), 1);
        // pair[1] = 99 >= 0 → direction = 0
        assert_eq!(branches[0].direction, 0);
        assert_eq!(branches[0].line, 5);
    }

    /// `executed_branches_from_json` with a negative `pair[0]`
    /// → `unsigned_abs()` produces a positive line number.
    #[test]
    fn executed_branches_negative_from_line_uses_unsigned_abs() {
        let tmp = tempfile::tempdir().unwrap();
        let json_path = tmp.path().join("f.json");

        let json = r#"{
  "files": {
    "/x/c.py": {
      "executed_branches": [[-7, 0]],
      "missing_branches": [],
      "all_branches": []
    }
  }
}"#;
        std::fs::write(&json_path, json).unwrap();

        let sb = PythonTestSandbox::new(make_oracle(), make_file_paths(), PathBuf::from("/x"));
        let branches = sb.executed_branches_from_json(&json_path).unwrap();
        assert_eq!(branches.len(), 1);
        // pair[0] = -7 → unsigned_abs() = 7 as u32
        assert_eq!(branches[0].line, 7);
    }

    /// `snapshot()` error message contains the sandbox name.
    #[tokio::test]
    async fn snapshot_error_message() {
        use apex_core::traits::Sandbox;
        let sb = PythonTestSandbox::new(make_oracle(), make_file_paths(), PathBuf::from("/p"));
        let err = sb.snapshot().await.unwrap_err();
        let msg = format!("{err}");
        assert!(msg.contains("PythonTestSandbox"), "error: {msg}");
    }

    /// `restore()` error message contains the sandbox name.
    #[tokio::test]
    async fn restore_error_message() {
        use apex_core::traits::Sandbox;
        let sb = PythonTestSandbox::new(make_oracle(), make_file_paths(), PathBuf::from("/p"));
        let err = sb.restore(SnapshotId::new()).await.unwrap_err();
        let msg = format!("{err}");
        assert!(msg.contains("PythonTestSandbox"), "error: {msg}");
    }

    /// `executed_branches_from_json` with an empty executed_branches list → no branches.
    #[test]
    fn executed_branches_from_json_empty_executed() {
        let tmp = tempfile::tempdir().unwrap();
        let json_path = tmp.path().join("g.json");

        let json = r#"{
  "files": {
    "/x/d.py": {
      "executed_branches": [],
      "missing_branches": [[1, 2]],
      "all_branches": [[1, 2]]
    }
  }
}"#;
        std::fs::write(&json_path, json).unwrap();

        let sb = PythonTestSandbox::new(make_oracle(), make_file_paths(), PathBuf::from("/x"));
        let branches = sb.executed_branches_from_json(&json_path).unwrap();
        assert!(branches.is_empty());
    }

    /// `strip_prefix` succeeds: relative path used for file_id hash.
    /// `strip_prefix` fails: absolute path used as fallback.
    #[test]
    fn executed_branches_strip_prefix_vs_fallback() {
        let tmp = tempfile::tempdir().unwrap();
        let target_dir = tmp.path().to_path_buf();
        let json_path = tmp.path().join("h.json");

        let abs_inside = format!("{}/src/mod.py", target_dir.display());
        let abs_outside = "/other/root/foo.py";

        let json = format!(
            r#"{{
  "files": {{
    "{abs_inside}": {{
      "executed_branches": [[1, 0]],
      "missing_branches": [],
      "all_branches": []
    }},
    "{abs_outside}": {{
      "executed_branches": [[2, 0]],
      "missing_branches": [],
      "all_branches": []
    }}
  }}
}}"#
        );
        std::fs::write(&json_path, &json).unwrap();

        let sb = PythonTestSandbox::new(make_oracle(), make_file_paths(), target_dir.clone());
        let branches = sb.executed_branches_from_json(&json_path).unwrap();
        assert_eq!(branches.len(), 2);

        // The branch from abs_inside should use the relative path "src/mod.py".
        let expected_inside = fnv1a_hash("src/mod.py");
        // The branch from abs_outside uses the full absolute path.
        let expected_outside = fnv1a_hash(abs_outside);

        let file_ids: std::collections::HashSet<u64> = branches.iter().map(|b| b.file_id).collect();
        assert!(file_ids.contains(&expected_inside), "expected inside fid");
        assert!(file_ids.contains(&expected_outside), "expected outside fid");
    }
}
