use apex_core::{
    error::{ApexError, Result},
    hash::fnv1a_hash,
    traits::Sandbox,
    types::{BranchId, ExecutionResult, ExecutionStatus, InputSeed, Language, SnapshotId},
};
use apex_coverage::CoverageOracle;
use async_trait::async_trait;
use std::{collections::HashMap, path::PathBuf, sync::Arc, time::Instant};
use tracing::{debug, warn};

// ---------------------------------------------------------------------------
// RubyTestSandbox
// ---------------------------------------------------------------------------

/// Runs a Ruby test candidate under SimpleCov, then computes which
/// previously-uncovered branches were newly hit.
///
/// `InputSeed.data` must be UTF-8 Ruby source code.
#[allow(dead_code)]
pub struct RubyTestSandbox {
    oracle: Arc<CoverageOracle>,
    /// Maps file_id (FNV-1a of repo-relative path) → repo-relative PathBuf.
    file_paths: Arc<HashMap<u64, PathBuf>>,
    target_dir: PathBuf,
    pub timeout_ms: u64,
}

impl RubyTestSandbox {
    pub fn new(
        oracle: Arc<CoverageOracle>,
        file_paths: Arc<HashMap<u64, PathBuf>>,
        target_dir: PathBuf,
    ) -> Self {
        RubyTestSandbox {
            oracle,
            file_paths,
            target_dir,
            timeout_ms: 30_000,
        }
    }

    pub fn with_timeout(mut self, ms: u64) -> Self {
        self.timeout_ms = ms;
        self
    }
}

// ---------------------------------------------------------------------------
// SimpleCov JSON parsing
// ---------------------------------------------------------------------------

/// Parse SimpleCov JSON (`.resultset.json` or `coverage.json`) into
/// `(all_branches, covered_branches)`.
///
/// SimpleCov format:
/// ```json
/// {
///   "<runner>": {
///     "coverage": {
///       "<file>": { "lines": [null, 1, 0, null, 2] }
///     }
///   }
/// }
/// ```
/// Each non-null entry in `lines` is an executable line. Entries with a
/// count > 0 are covered.
pub fn parse_simplecov_branches(json: &str) -> Result<(Vec<BranchId>, Vec<BranchId>)> {
    let parsed: serde_json::Value = serde_json::from_str(json)
        .map_err(|e| ApexError::Sandbox(format!("simplecov JSON: {e}")))?;

    let mut all = Vec::new();
    let mut covered = Vec::new();

    // SimpleCov format: { "<runner>": { "coverage": { "<file>": { "lines": [...] } } } }
    for (_runner, runner_data) in parsed.as_object().into_iter().flatten() {
        let coverage = runner_data.get("coverage").and_then(|c| c.as_object());
        for (file, file_data) in coverage.into_iter().flatten() {
            let file_id = fnv1a_hash(file);
            let lines = file_data.get("lines").and_then(|l| l.as_array());
            for (idx, val) in lines.into_iter().flatten().enumerate() {
                if let Some(count) = val.as_i64() {
                    let line = (idx + 1) as u32;
                    let bid = BranchId::new(file_id, line, 0, 0);
                    all.push(bid.clone());
                    if count > 0 {
                        covered.push(bid);
                    }
                }
                // null means non-executable line — skip
            }
        }
    }
    Ok((all, covered))
}

// ---------------------------------------------------------------------------
// Sandbox trait impl
// ---------------------------------------------------------------------------

#[async_trait]
impl Sandbox for RubyTestSandbox {
    async fn run(&self, input: &InputSeed) -> Result<ExecutionResult> {
        let start = Instant::now();

        let code = std::str::from_utf8(&input.data)
            .map_err(|e| ApexError::Sandbox(format!("candidate not valid UTF-8: {e}")))?;

        let tmp_dir =
            tempfile::tempdir().map_err(|e| ApexError::Sandbox(format!("tempdir: {e}")))?;

        let test_file = tmp_dir.path().join("apex_probe_test.rb");
        let coverage_dir = tmp_dir.path().join("coverage");

        // Prepend SimpleCov setup so any Ruby file gets coverage instrumentation.
        let wrapped = format!(
            "require 'simplecov'\nrequire 'simplecov-json'\n\
             SimpleCov.start do\n  formatter SimpleCov::Formatter::JSONFormatter\n  \
             coverage_dir '{}'\nend\n\n{}",
            coverage_dir.display(),
            code
        );

        std::fs::write(&test_file, &wrapped)
            .map_err(|e| ApexError::Sandbox(format!("write test: {e}")))?;

        // Validate ruby is available before spawning.
        let ruby_check = tokio::process::Command::new("ruby")
            .arg("--version")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .await;
        if ruby_check.map(|s| !s.success()).unwrap_or(true) {
            return Err(ApexError::Sandbox("ruby not found in PATH".into()));
        }

        let run_output = tokio::time::timeout(
            std::time::Duration::from_millis(self.timeout_ms),
            tokio::process::Command::new("ruby")
                .arg(test_file.to_string_lossy().as_ref())
                .current_dir(&self.target_dir)
                .output(),
        )
        .await;

        let duration_ms = start.elapsed().as_millis() as u64;

        let run_output = match run_output {
            Err(_) => {
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
            Ok(Err(e)) => return Err(ApexError::Sandbox(format!("spawn ruby: {e}"))),
            Ok(Ok(o)) => o,
        };

        let stdout = String::from_utf8_lossy(&run_output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&run_output.stderr).to_string();

        let status = match run_output.status.code() {
            Some(0) => ExecutionStatus::Pass,
            Some(code) if code < 0 => ExecutionStatus::Crash,
            _ => ExecutionStatus::Fail,
        };

        // Parse SimpleCov JSON output (best-effort; may not exist on syntax error)
        let cov_file = coverage_dir.join(".resultset.json");
        let new_branches = if cov_file.exists() {
            match std::fs::read_to_string(&cov_file)
                .map_err(|e| ApexError::Sandbox(format!("read coverage: {e}")))
                .and_then(|json| parse_simplecov_branches(&json))
            {
                Ok((_all, covered_branches)) => covered_branches
                    .into_iter()
                    .filter(|b| {
                        matches!(
                            self.oracle.state_of(b),
                            Some(apex_core::types::BranchState::Uncovered)
                        )
                    })
                    .collect(),
                Err(e) => {
                    warn!(error = %e, "failed to parse SimpleCov coverage JSON");
                    Vec::new()
                }
            }
        } else {
            debug!("SimpleCov JSON not produced for this candidate");
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
            "RubyTestSandbox does not support snapshots".into(),
        ))
    }

    async fn restore(&self, _id: SnapshotId) -> Result<()> {
        Err(ApexError::NotSupported(
            "RubyTestSandbox does not support restore".into(),
        ))
    }

    fn language(&self) -> Language {
        Language::Ruby
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

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
    fn ruby_sandbox_language() {
        let sandbox =
            RubyTestSandbox::new(make_oracle(), make_file_paths(), PathBuf::from("/tmp/test"));
        use apex_core::traits::Sandbox;
        assert_eq!(sandbox.language(), Language::Ruby);
    }

    #[test]
    fn ruby_sandbox_constructs() {
        let sandbox =
            RubyTestSandbox::new(make_oracle(), make_file_paths(), PathBuf::from("/tmp/test"));
        assert_eq!(sandbox.timeout_ms, 30_000);
    }

    #[test]
    fn with_timeout_overrides() {
        let sandbox =
            RubyTestSandbox::new(make_oracle(), make_file_paths(), PathBuf::from("/tmp/test"))
                .with_timeout(5_000);
        assert_eq!(sandbox.timeout_ms, 5_000);
    }

    #[test]
    fn with_timeout_chains() {
        let sandbox =
            RubyTestSandbox::new(make_oracle(), make_file_paths(), PathBuf::from("/tmp/test"))
                .with_timeout(1_000)
                .with_timeout(2_000);
        assert_eq!(sandbox.timeout_ms, 2_000);
    }

    #[test]
    fn parse_simplecov_json_extracts_branches() {
        let json = r#"{
            "RSpec": {
                "coverage": {
                    "/app/lib/foo.rb": {
                        "lines": [1, 1, null, 0, 1, 0, null]
                    }
                }
            }
        }"#;
        let (all, covered) = parse_simplecov_branches(json).unwrap();
        // Lines with values (non-null): indices 0,1,3,4,5 → lines 1,2,4,5,6
        assert!(all.len() >= 5);
        // Covered (>0): lines 1,2,5
        assert!(covered.len() >= 3);
    }

    #[test]
    fn parse_simplecov_empty_lines() {
        let json = r#"{"RSpec": {"coverage": {"app.rb": {"lines": []}}}}"#;
        let (all, covered) = parse_simplecov_branches(json).unwrap();
        assert!(all.is_empty());
        assert!(covered.is_empty());
    }

    #[test]
    fn parse_simplecov_all_null() {
        let json = r#"{"RSpec": {"coverage": {"app.rb": {"lines": [null, null, null]}}}}"#;
        let (all, covered) = parse_simplecov_branches(json).unwrap();
        assert!(all.is_empty());
        assert!(covered.is_empty());
    }

    #[test]
    fn parse_simplecov_invalid_json_errors() {
        let result = parse_simplecov_branches("not json at all");
        assert!(result.is_err());
    }

    #[test]
    fn parse_simplecov_multiple_files() {
        let json = r#"{
            "RSpec": {
                "coverage": {
                    "a.rb": {"lines": [1, 0]},
                    "b.rb": {"lines": [1, 1, 0]}
                }
            }
        }"#;
        let (all, covered) = parse_simplecov_branches(json).unwrap();
        assert_eq!(all.len(), 5); // 2 + 3
        assert_eq!(covered.len(), 3); // 1 + 2
    }

    #[test]
    fn parse_simplecov_zero_count_not_covered() {
        let json = r#"{"RSpec": {"coverage": {"f.rb": {"lines": [0, 0, 0]}}}}"#;
        let (all, covered) = parse_simplecov_branches(json).unwrap();
        assert_eq!(all.len(), 3);
        assert!(covered.is_empty());
    }

    #[test]
    fn parse_simplecov_multiple_runners() {
        // Multiple runner keys (e.g., both RSpec and Minitest in the same JSON)
        let json = r#"{
            "RSpec":    {"coverage": {"a.rb": {"lines": [1]}}},
            "Minitest": {"coverage": {"b.rb": {"lines": [1, 0]}}}
        }"#;
        let (all, covered) = parse_simplecov_branches(json).unwrap();
        assert_eq!(all.len(), 3);
        assert_eq!(covered.len(), 2);
    }

    #[test]
    fn parse_simplecov_file_id_is_stable() {
        let json = r#"{"RSpec": {"coverage": {"lib/foo.rb": {"lines": [1]}}}}"#;
        let (all, _covered) = parse_simplecov_branches(json).unwrap();
        assert_eq!(all.len(), 1);
        let expected_fid = fnv1a_hash("lib/foo.rb");
        assert_eq!(all[0].file_id, expected_fid);
    }

    #[test]
    fn snapshot_not_supported() {
        let sandbox =
            RubyTestSandbox::new(make_oracle(), make_file_paths(), PathBuf::from("/tmp/test"));
        let rt = tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap();
        let err = rt.block_on(async {
            use apex_core::traits::Sandbox;
            sandbox.snapshot().await
        });
        assert!(err.is_err());
    }

    #[test]
    fn restore_not_supported() {
        let sandbox =
            RubyTestSandbox::new(make_oracle(), make_file_paths(), PathBuf::from("/tmp/test"));
        let rt = tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap();
        let err = rt.block_on(async {
            use apex_core::traits::Sandbox;
            sandbox.restore(SnapshotId::new()).await
        });
        assert!(err.is_err());
    }

    #[tokio::test]
    async fn snapshot_error_message() {
        use apex_core::traits::Sandbox;
        let sb = RubyTestSandbox::new(make_oracle(), make_file_paths(), PathBuf::from("/p"));
        let err = sb.snapshot().await.unwrap_err();
        let msg = format!("{err}");
        assert!(msg.contains("RubyTestSandbox"), "error: {msg}");
    }

    #[tokio::test]
    async fn restore_error_message() {
        use apex_core::traits::Sandbox;
        let sb = RubyTestSandbox::new(make_oracle(), make_file_paths(), PathBuf::from("/p"));
        let err = sb.restore(SnapshotId::new()).await.unwrap_err();
        let msg = format!("{err}");
        assert!(msg.contains("RubyTestSandbox"), "error: {msg}");
    }
}
