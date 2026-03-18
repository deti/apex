use apex_core::{
    command::{adaptive_timeout, count_source_files, CommandRunner, CommandSpec, OpKind, RealCommandRunner},
    error::{ApexError, Result},
    hash::fnv1a_hash,
    traits::Instrumentor,
    types::{BranchId, InstrumentedTarget, Language, Target},
};
use async_trait::async_trait;
use serde::Deserialize;
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::Arc,
};
use tracing::{debug, info, warn};

/// Resolve the python binary for a target directory.
///
/// Checks for `.apex-venv`, `.venv`, `venv` in order, falling back to `python3`.
fn resolve_venv_python(target: &Path) -> String {
    for venv_dir in &[".apex-venv", ".venv", "venv"] {
        let python_path = target.join(venv_dir).join("bin").join("python");
        if python_path.exists() {
            return python_path.to_string_lossy().into_owned();
        }
    }
    "python3".to_string()
}

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

/// Python helper script bundled at compile time.
const INSTRUMENT_SCRIPT: &str = include_str!("scripts/apex_instrument.py");

// ---------------------------------------------------------------------------
// coverage.py JSON schema
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct CoverageMeta {
    version: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ApexCoverageJson {
    #[serde(default)]
    meta: Option<CoverageMeta>,
    files: HashMap<String, FileData>,
}

impl ApexCoverageJson {
    /// Return the coverage.py version from the `meta` block, if present.
    fn meta_version(&self) -> Option<&str> {
        self.meta.as_ref().and_then(|m| m.version.as_deref())
    }
}

#[derive(Debug, Deserialize)]
struct FileData {
    executed_branches: Vec<[i64; 2]>,
    missing_branches: Vec<[i64; 2]>,
    #[allow(dead_code)]
    all_branches: Vec<[i64; 2]>,
}

// ---------------------------------------------------------------------------
// Instrumentor implementation
// ---------------------------------------------------------------------------

pub struct PythonInstrumentor {
    branch_ids: Vec<BranchId>,
    executed_branch_ids: Vec<BranchId>,
    file_paths: std::collections::HashMap<u64, PathBuf>,
    work_dir: Option<PathBuf>,
    runner: Arc<dyn CommandRunner>,
}

impl PythonInstrumentor {
    pub fn new() -> Self {
        PythonInstrumentor {
            branch_ids: Vec::new(),
            executed_branch_ids: Vec::new(),
            file_paths: std::collections::HashMap::new(),
            work_dir: None,
            runner: Arc::new(RealCommandRunner),
        }
    }

    /// Create a new instrumentor with a custom command runner (for testing).
    pub fn with_runner(runner: Arc<dyn CommandRunner>) -> Self {
        PythonInstrumentor {
            branch_ids: Vec::new(),
            executed_branch_ids: Vec::new(),
            file_paths: std::collections::HashMap::new(),
            work_dir: None,
            runner,
        }
    }

    /// Parse the coverage JSON written by `apex_instrument.py` and build
    /// `BranchId` entries for all discovered branches.
    fn parse_coverage_json(&mut self, json_path: &Path, repo_root: &Path) -> Result<()> {
        let content = std::fs::read_to_string(json_path)
            .map_err(|e| ApexError::Instrumentation(e.to_string()))?;
        let data: ApexCoverageJson = serde_json::from_str(&content)
            .map_err(|e| ApexError::Instrumentation(format!("parse coverage JSON: {e}")))?;

        if let Some(v) = data.meta_version() {
            debug!(version = %v, "coverage.py JSON version");
        } else {
            warn!("coverage JSON has no version metadata");
        }

        self.branch_ids.clear();
        self.executed_branch_ids.clear();
        self.file_paths.clear();

        for (file_path, fdata) in &data.files {
            // Normalise to repo-root-relative path for stable file_id.
            let rel = Path::new(file_path)
                .strip_prefix(repo_root)
                .unwrap_or(Path::new(file_path));
            let rel_str = rel.to_string_lossy();
            let file_id = fnv1a_hash(&rel_str);
            self.file_paths.insert(file_id, rel.to_path_buf());

            for pair in &fdata.missing_branches {
                let from_line = pair[0].unsigned_abs() as u32;
                // direction: 0 = true branch (positive to_line), 1 = false / exit
                let direction = if pair[1] < 0 { 1u8 } else { 0u8 };
                self.branch_ids
                    .push(BranchId::new(file_id, from_line, 0, direction));
            }
            for pair in &fdata.executed_branches {
                let from_line = pair[0].unsigned_abs() as u32;
                let direction = if pair[1] < 0 { 1u8 } else { 0u8 };
                let b = BranchId::new(file_id, from_line, 0, direction);
                self.branch_ids.push(b.clone());
                self.executed_branch_ids.push(b);
            }

            debug!(
                file = %file_path,
                executed = fdata.executed_branches.len(),
                missing = fdata.missing_branches.len(),
                "parsed coverage"
            );
        }

        Ok(())
    }
}

impl Default for PythonInstrumentor {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Instrumentor for PythonInstrumentor {
    async fn instrument(&self, target: &Target) -> Result<InstrumentedTarget> {
        // We need &mut self to populate branch_ids but the trait takes &self.
        // Work around by running in a fresh instance and returning results.
        let mut inner = PythonInstrumentor::with_runner(self.runner.clone());
        inner.instrument_impl(target).await?;

        let branch_ids = inner.branch_ids.clone();
        let executed_branch_ids = inner.executed_branch_ids.clone();
        let file_paths = inner.file_paths.clone();
        let work_dir = inner
            .work_dir
            .clone()
            .unwrap_or_else(|| target.root.clone());

        Ok(InstrumentedTarget {
            target: target.clone(),
            branch_ids,
            executed_branch_ids,
            file_paths,
            work_dir,
        })
    }

    fn branch_ids(&self) -> &[BranchId] {
        &self.branch_ids
    }
}

impl PythonInstrumentor {
    async fn instrument_impl(&mut self, target: &Target) -> Result<()> {
        // Write the helper script to a temp dir.
        let tmp =
            tempfile::tempdir().map_err(|e| ApexError::Instrumentation(format!("tempdir: {e}")))?;
        let script_path = tmp.path().join("apex_instrument.py");
        std::fs::write(&script_path, INSTRUMENT_SCRIPT)
            .map_err(|e| ApexError::Instrumentation(format!("write script: {e}")))?;

        // Build the test command. Default: pytest -q
        let test_cmd = if target.test_command.is_empty() {
            vec!["pytest".to_string(), "-q".to_string()]
        } else {
            target.test_command.clone()
        };

        info!(
            target = %target.root.display(),
            cmd = ?test_cmd,
            "running Python instrumentation"
        );

        // Run: python3 apex_instrument.py <test_cmd...>
        // Prefer `uv run --with coverage --with pytest python3` when uv is available
        // so that coverage.py is guaranteed importable (handles PEP 668 envs).
        // Fall back to .apex-venv/bin/python if the lang runner created one.
        let spec = if let Some(uv) = resolve_uv() {
            let mut args = vec![
                "run".to_string(),
                "--with".to_string(),
                "coverage".to_string(),
                "--with".to_string(),
                "pytest".to_string(),
                "--".to_string(),
                "python3".to_string(),
            ];
            args.push(script_path.to_string_lossy().to_string());
            args.extend(test_cmd);
            let file_count = count_source_files(&target.root);
            let timeout = adaptive_timeout(file_count, Language::Python, OpKind::TestRun);
            CommandSpec::new(&uv, &target.root).args(args).timeout(timeout)
        } else {
            // Use .apex-venv python if it exists (created by PEP 668 venv logic).
            let python = resolve_venv_python(&target.root);
            let mut args = vec![script_path.to_string_lossy().to_string()];
            args.extend(test_cmd);
            let file_count = count_source_files(&target.root);
            let timeout = adaptive_timeout(file_count, Language::Python, OpKind::TestRun);
            CommandSpec::new(&python, &target.root).args(args).timeout(timeout)
        };
        let output = self
            .runner
            .run_command(&spec)
            .await
            .map_err(|e| ApexError::Instrumentation(format!("spawn: {e}")))?;

        if output.exit_code != 0 {
            warn!(
                exit = output.exit_code,
                "instrumented test run returned non-zero (coverage data may still be valid)"
            );
        }

        // Parse resulting JSON.
        let json_path = target.root.join(".apex_coverage.json");
        if json_path.exists() {
            self.parse_coverage_json(&json_path, &target.root)?;
        } else {
            return Err(ApexError::Instrumentation(
                "coverage JSON not produced; is coverage.py installed?".into(),
            ));
        }

        self.work_dir = Some(target.root.clone());
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// LLVM instrumentation (feature-gated)
// ---------------------------------------------------------------------------

#[cfg(feature = "llvm-instrument")]
pub mod llvm;

#[cfg(test)]
mod tests {
    use super::*;
    use apex_core::command::CommandOutput;

    /// Build a minimal coverage.py JSON string for testing.
    fn sample_coverage_json(repo_root: &str) -> String {
        format!(
            r#"{{
  "files": {{
    "{repo_root}/src/app.py": {{
      "executed_branches": [[10, 12], [20, -1]],
      "missing_branches": [[10, -1], [30, 35]],
      "all_branches": [[10, 12], [10, -1], [20, -1], [30, 35]]
    }},
    "{repo_root}/src/lib.py": {{
      "executed_branches": [],
      "missing_branches": [[5, 8]],
      "all_branches": [[5, 8]]
    }}
  }}
}}"#
        )
    }

    #[test]
    fn test_fnv1a_deterministic() {
        let h1 = fnv1a_hash("src/app.py");
        let h2 = fnv1a_hash("src/app.py");
        assert_eq!(h1, h2);
    }

    #[test]
    fn test_fnv1a_different_inputs() {
        assert_ne!(fnv1a_hash("src/app.py"), fnv1a_hash("src/lib.py"));
    }

    #[test]
    fn test_parse_coverage_json_branch_counts() {
        let tmp = tempfile::tempdir().unwrap();
        let repo_root = tmp.path();
        let json_path = repo_root.join(".apex_coverage.json");
        let json = sample_coverage_json(repo_root.to_str().unwrap());
        std::fs::write(&json_path, &json).unwrap();

        let mut inst = PythonInstrumentor::new();
        inst.parse_coverage_json(&json_path, repo_root).unwrap();

        // app.py: 2 executed + 2 missing = 4 branches
        // lib.py: 0 executed + 1 missing = 1 branch
        assert_eq!(inst.branch_ids.len(), 5);
        assert_eq!(inst.executed_branch_ids.len(), 2);
        assert_eq!(inst.file_paths.len(), 2);
    }

    #[test]
    fn test_parse_coverage_json_direction_mapping() {
        let tmp = tempfile::tempdir().unwrap();
        let repo_root = tmp.path();
        let json_path = repo_root.join(".apex_coverage.json");
        let json = sample_coverage_json(repo_root.to_str().unwrap());
        std::fs::write(&json_path, &json).unwrap();

        let mut inst = PythonInstrumentor::new();
        inst.parse_coverage_json(&json_path, repo_root).unwrap();

        // Negative to_line -> direction 1, positive -> direction 0.
        let app_file_id = fnv1a_hash("src/app.py");
        let app_branches: Vec<_> = inst
            .branch_ids
            .iter()
            .filter(|b| b.file_id == app_file_id)
            .collect();

        assert!(app_branches.iter().any(|b| b.direction == 0));
        assert!(app_branches.iter().any(|b| b.direction == 1));
    }

    #[test]
    fn test_parse_coverage_json_empty_file() {
        let tmp = tempfile::tempdir().unwrap();
        let repo_root = tmp.path();
        let json_path = repo_root.join(".apex_coverage.json");
        let json = r#"{"files": {}}"#;
        std::fs::write(&json_path, json).unwrap();

        let mut inst = PythonInstrumentor::new();
        inst.parse_coverage_json(&json_path, repo_root).unwrap();

        assert_eq!(inst.branch_ids.len(), 0);
        assert_eq!(inst.executed_branch_ids.len(), 0);
    }

    #[test]
    fn test_parse_coverage_json_invalid_json() {
        let tmp = tempfile::tempdir().unwrap();
        let json_path = tmp.path().join("bad.json");
        std::fs::write(&json_path, "not valid json").unwrap();

        let mut inst = PythonInstrumentor::new();
        let result = inst.parse_coverage_json(&json_path, tmp.path());
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_coverage_json_path_normalization() {
        let tmp = tempfile::tempdir().unwrap();
        let repo_root = tmp.path();
        let json_path = repo_root.join(".apex_coverage.json");
        let json = sample_coverage_json(repo_root.to_str().unwrap());
        std::fs::write(&json_path, &json).unwrap();

        let mut inst = PythonInstrumentor::new();
        inst.parse_coverage_json(&json_path, repo_root).unwrap();

        for path in inst.file_paths.values() {
            assert!(
                !path.is_absolute(),
                "expected relative path, got: {}",
                path.display()
            );
        }
    }

    #[test]
    fn test_default_impl() {
        let inst = PythonInstrumentor::default();
        assert!(inst.branch_ids.is_empty());
        assert!(inst.executed_branch_ids.is_empty());
        assert!(inst.file_paths.is_empty());
        assert!(inst.work_dir.is_none());
    }

    #[test]
    fn test_branch_ids_accessor() {
        use apex_core::traits::Instrumentor;
        let inst = PythonInstrumentor::new();
        assert_eq!(inst.branch_ids().len(), 0);
    }

    #[test]
    fn test_branch_ids_after_parse() {
        use apex_core::traits::Instrumentor;
        let tmp = tempfile::tempdir().unwrap();
        let repo_root = tmp.path();
        let json_path = repo_root.join("cov.json");
        let json = sample_coverage_json(repo_root.to_str().unwrap());
        std::fs::write(&json_path, &json).unwrap();

        let mut inst = PythonInstrumentor::new();
        inst.parse_coverage_json(&json_path, repo_root).unwrap();
        assert_eq!(inst.branch_ids().len(), 5);
    }

    #[test]
    fn test_parse_coverage_json_file_not_found() {
        let mut inst = PythonInstrumentor::new();
        let result =
            inst.parse_coverage_json(Path::new("/nonexistent/file.json"), Path::new("/tmp"));
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_coverage_json_clears_previous_state() {
        let tmp = tempfile::tempdir().unwrap();
        let repo_root = tmp.path();
        let json_path = repo_root.join("cov.json");

        // First parse
        let json = sample_coverage_json(repo_root.to_str().unwrap());
        std::fs::write(&json_path, &json).unwrap();

        let mut inst = PythonInstrumentor::new();
        inst.parse_coverage_json(&json_path, repo_root).unwrap();
        assert_eq!(inst.branch_ids.len(), 5);

        // Second parse with empty files -- should clear previous results
        std::fs::write(&json_path, r#"{"files": {}}"#).unwrap();
        inst.parse_coverage_json(&json_path, repo_root).unwrap();
        assert_eq!(inst.branch_ids.len(), 0);
        assert_eq!(inst.executed_branch_ids.len(), 0);
        assert_eq!(inst.file_paths.len(), 0);
    }

    #[test]
    fn test_parse_coverage_json_path_without_prefix() {
        let tmp = tempfile::tempdir().unwrap();
        let json_path = tmp.path().join("cov.json");
        let json = r#"{
  "files": {
    "/some/other/path/mod.py": {
      "executed_branches": [[1, 2]],
      "missing_branches": [],
      "all_branches": [[1, 2]]
    }
  }
}"#;
        std::fs::write(&json_path, json).unwrap();

        let mut inst = PythonInstrumentor::new();
        inst.parse_coverage_json(&json_path, Path::new("/different/root"))
            .unwrap();

        assert_eq!(inst.branch_ids.len(), 1);
        assert_eq!(inst.executed_branch_ids.len(), 1);
        let expected_fid = fnv1a_hash("/some/other/path/mod.py");
        assert_eq!(inst.branch_ids[0].file_id, expected_fid);
    }

    #[test]
    fn test_parse_coverage_json_only_missing_branches() {
        let tmp = tempfile::tempdir().unwrap();
        let repo_root = tmp.path();
        let json_path = repo_root.join("cov.json");
        let json = format!(
            r#"{{
  "files": {{
    "{}/mod.py": {{
      "executed_branches": [],
      "missing_branches": [[10, 15], [20, -1]],
      "all_branches": [[10, 15], [20, -1]]
    }}
  }}
}}"#,
            repo_root.display()
        );
        std::fs::write(&json_path, &json).unwrap();

        let mut inst = PythonInstrumentor::new();
        inst.parse_coverage_json(&json_path, repo_root).unwrap();

        assert_eq!(inst.branch_ids.len(), 2);
        assert_eq!(inst.executed_branch_ids.len(), 0);
        let dirs: Vec<u8> = inst.branch_ids.iter().map(|b| b.direction).collect();
        assert!(dirs.contains(&0));
        assert!(dirs.contains(&1));
    }

    #[test]
    fn test_fnv1a_known_value() {
        assert_eq!(fnv1a_hash(""), 0xcbf2_9ce4_8422_2325);
    }

    #[test]
    fn test_parse_coverage_json_file_id_matches_file_paths_key() {
        let tmp = tempfile::tempdir().unwrap();
        let repo_root = tmp.path();
        let json_path = repo_root.join("cov.json");
        let json = sample_coverage_json(repo_root.to_str().unwrap());
        std::fs::write(&json_path, &json).unwrap();

        let mut inst = PythonInstrumentor::new();
        inst.parse_coverage_json(&json_path, repo_root).unwrap();

        for b in &inst.branch_ids {
            assert!(
                inst.file_paths.contains_key(&b.file_id),
                "file_id {} not in file_paths",
                b.file_id
            );
        }
    }

    // -----------------------------------------------------------------------
    // Mock-based instrument() tests
    // -----------------------------------------------------------------------

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

        fn failure(exit_code: i32) -> Self {
            FakeRunner {
                exit_code,
                stdout: Vec::new(),
                stderr: b"command failed".to_vec(),
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

    #[tokio::test]
    async fn test_instrument_success_with_mock() {
        let tmp = tempfile::tempdir().unwrap();
        let repo_root = tmp.path();

        // Pre-create the coverage JSON that instrument_impl expects to find
        let json = sample_coverage_json(repo_root.to_str().unwrap());
        std::fs::write(repo_root.join(".apex_coverage.json"), &json).unwrap();

        let runner = Arc::new(FakeRunner::success());
        let inst = PythonInstrumentor::with_runner(runner);

        let target = Target {
            root: repo_root.to_path_buf(),
            language: apex_core::types::Language::Python,
            test_command: vec!["pytest".into(), "-q".into()],
        };

        let result = inst.instrument(&target).await.unwrap();
        assert_eq!(result.branch_ids.len(), 5);
        assert_eq!(result.executed_branch_ids.len(), 2);
        assert_eq!(result.file_paths.len(), 2);
        assert_eq!(result.work_dir, repo_root.to_path_buf());
    }

    #[tokio::test]
    async fn test_instrument_nonzero_exit_still_parses() {
        let tmp = tempfile::tempdir().unwrap();
        let repo_root = tmp.path();

        let json = sample_coverage_json(repo_root.to_str().unwrap());
        std::fs::write(repo_root.join(".apex_coverage.json"), &json).unwrap();

        let runner = Arc::new(FakeRunner::failure(1));
        let inst = PythonInstrumentor::with_runner(runner);

        let target = Target {
            root: repo_root.to_path_buf(),
            language: apex_core::types::Language::Python,
            test_command: Vec::new(),
        };

        // Non-zero exit is a warning, not an error -- coverage may still exist
        let result = inst.instrument(&target).await.unwrap();
        assert_eq!(result.branch_ids.len(), 5);
    }

    #[tokio::test]
    async fn test_instrument_spawn_error() {
        let tmp = tempfile::tempdir().unwrap();
        let repo_root = tmp.path();

        let runner = Arc::new(FakeRunner::spawn_error());
        let inst = PythonInstrumentor::with_runner(runner);

        let target = Target {
            root: repo_root.to_path_buf(),
            language: apex_core::types::Language::Python,
            test_command: Vec::new(),
        };

        let result = inst.instrument(&target).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_instrument_missing_coverage_json() {
        let tmp = tempfile::tempdir().unwrap();
        let repo_root = tmp.path();
        // Do NOT create .apex_coverage.json

        let runner = Arc::new(FakeRunner::success());
        let inst = PythonInstrumentor::with_runner(runner);

        let target = Target {
            root: repo_root.to_path_buf(),
            language: apex_core::types::Language::Python,
            test_command: Vec::new(),
        };

        let result = inst.instrument(&target).await;
        assert!(result.is_err());
        let err_msg = format!("{}", result.unwrap_err());
        assert!(err_msg.contains("coverage JSON not produced"));
    }

    #[test]
    fn test_fnv1a_single_byte_values() {
        // Single-byte strings should produce unique hashes
        let hashes: Vec<u64> = (b'a'..=b'z')
            .map(|c| fnv1a_hash(&String::from(c as char)))
            .collect();
        let unique: std::collections::HashSet<u64> = hashes.iter().cloned().collect();
        assert_eq!(
            hashes.len(),
            unique.len(),
            "all single-char hashes should be unique"
        );
    }

    #[test]
    fn test_with_runner_constructor() {
        let runner = Arc::new(FakeRunner::success());
        let inst = PythonInstrumentor::with_runner(runner);
        assert!(inst.branch_ids.is_empty());
        assert!(inst.executed_branch_ids.is_empty());
        assert!(inst.file_paths.is_empty());
        assert!(inst.work_dir.is_none());
    }

    #[test]
    fn test_parse_coverage_json_multiple_files() {
        let tmp = tempfile::tempdir().unwrap();
        let repo_root = tmp.path();
        let json_path = repo_root.join("cov.json");
        let json = format!(
            r#"{{
  "files": {{
    "{root}/a.py": {{
      "executed_branches": [[1, 2]],
      "missing_branches": [[3, -1]],
      "all_branches": [[1, 2], [3, -1]]
    }},
    "{root}/b.py": {{
      "executed_branches": [],
      "missing_branches": [[10, 20], [30, -1]],
      "all_branches": [[10, 20], [30, -1]]
    }},
    "{root}/c.py": {{
      "executed_branches": [[5, 6], [7, 8]],
      "missing_branches": [],
      "all_branches": [[5, 6], [7, 8]]
    }}
  }}
}}"#,
            root = repo_root.display()
        );
        std::fs::write(&json_path, &json).unwrap();

        let mut inst = PythonInstrumentor::new();
        inst.parse_coverage_json(&json_path, repo_root).unwrap();

        // a.py: 1 exec + 1 miss = 2 total, b.py: 0 + 2 = 2, c.py: 2 + 0 = 2
        assert_eq!(inst.branch_ids.len(), 6);
        assert_eq!(inst.executed_branch_ids.len(), 3);
        assert_eq!(inst.file_paths.len(), 3);
    }

    #[test]
    fn test_parse_coverage_json_all_positive_to_lines() {
        let tmp = tempfile::tempdir().unwrap();
        let repo_root = tmp.path();
        let json_path = repo_root.join("cov.json");
        let json = format!(
            r#"{{
  "files": {{
    "{root}/x.py": {{
      "executed_branches": [[1, 5], [10, 15]],
      "missing_branches": [[20, 25]],
      "all_branches": [[1, 5], [10, 15], [20, 25]]
    }}
  }}
}}"#,
            root = repo_root.display()
        );
        std::fs::write(&json_path, &json).unwrap();

        let mut inst = PythonInstrumentor::new();
        inst.parse_coverage_json(&json_path, repo_root).unwrap();

        // All positive to_lines => direction=0 for all
        for b in &inst.branch_ids {
            assert_eq!(b.direction, 0);
        }
    }

    #[test]
    fn test_parse_coverage_json_all_negative_to_lines() {
        let tmp = tempfile::tempdir().unwrap();
        let repo_root = tmp.path();
        let json_path = repo_root.join("cov.json");
        let json = format!(
            r#"{{
  "files": {{
    "{root}/y.py": {{
      "executed_branches": [[1, -1]],
      "missing_branches": [[5, -2]],
      "all_branches": [[1, -1], [5, -2]]
    }}
  }}
}}"#,
            root = repo_root.display()
        );
        std::fs::write(&json_path, &json).unwrap();

        let mut inst = PythonInstrumentor::new();
        inst.parse_coverage_json(&json_path, repo_root).unwrap();

        for b in &inst.branch_ids {
            assert_eq!(b.direction, 1);
        }
    }

    #[tokio::test]
    async fn test_instrument_with_custom_test_command() {
        let tmp = tempfile::tempdir().unwrap();
        let repo_root = tmp.path();

        let json = r#"{"files": {}}"#;
        std::fs::write(repo_root.join(".apex_coverage.json"), json).unwrap();

        use std::sync::Mutex;

        struct CapturingRunner {
            captured_args: Mutex<Vec<String>>,
        }

        #[async_trait]
        impl CommandRunner for CapturingRunner {
            async fn run_command(
                &self,
                spec: &CommandSpec,
            ) -> apex_core::error::Result<CommandOutput> {
                let mut args = self.captured_args.lock().unwrap();
                *args = spec.args.clone();
                Ok(CommandOutput::success(Vec::new()))
            }
        }

        let runner = Arc::new(CapturingRunner {
            captured_args: Mutex::new(Vec::new()),
        });
        let runner_ref = runner.clone();
        let inst = PythonInstrumentor::with_runner(runner);

        let target = Target {
            root: repo_root.to_path_buf(),
            language: apex_core::types::Language::Python,
            test_command: vec!["python".into(), "-m".into(), "unittest".into()],
        };

        inst.instrument(&target).await.unwrap();

        let args = runner_ref.captured_args.lock().unwrap();
        assert!(
            args.iter().any(|a| a == "python"),
            "expected 'python' in args: {:?}",
            *args
        );
        assert!(
            args.iter().any(|a| a == "-m"),
            "expected '-m' in args: {:?}",
            *args
        );
        assert!(
            args.iter().any(|a| a == "unittest"),
            "expected 'unittest' in args: {:?}",
            *args
        );
        // The custom command itself should be "python -m unittest", not "pytest".
        // Note: when uv is available, "--with pytest" appears as a dep specifier
        // (before "--"), but "pytest" should NOT appear after the script path.
        let script_idx = args.iter().position(|a| a.contains("apex_instrument.py")).unwrap();
        let post_script: Vec<&String> = args[script_idx + 1..].iter().collect();
        assert!(
            !post_script.iter().any(|a| *a == "pytest"),
            "custom test command args should not contain 'pytest': {:?}",
            post_script
        );
    }

    #[tokio::test]
    async fn test_instrument_default_test_command() {
        // Verify that empty test_command defaults to pytest -q
        use std::sync::Mutex;

        struct CapturingRunner {
            captured_args: Mutex<Vec<String>>,
        }

        #[async_trait]
        impl CommandRunner for CapturingRunner {
            async fn run_command(
                &self,
                spec: &CommandSpec,
            ) -> apex_core::error::Result<CommandOutput> {
                let mut args = self.captured_args.lock().unwrap();
                *args = spec.args.clone();
                Ok(CommandOutput::success(Vec::new()))
            }
        }

        let tmp = tempfile::tempdir().unwrap();
        let repo_root = tmp.path();
        // Create coverage JSON so parsing succeeds
        std::fs::write(repo_root.join(".apex_coverage.json"), r#"{"files":{}}"#).unwrap();

        let runner = Arc::new(CapturingRunner {
            captured_args: Mutex::new(Vec::new()),
        });
        let runner_ref = runner.clone();
        let inst = PythonInstrumentor::with_runner(runner);

        let target = Target {
            root: repo_root.to_path_buf(),
            language: apex_core::types::Language::Python,
            test_command: Vec::new(), // empty -> should default to pytest -q
        };

        inst.instrument(&target).await.unwrap();

        let args = runner_ref.captured_args.lock().unwrap();
        // args should contain the script path, then "pytest", "-q"
        assert!(
            args.iter().any(|a| a == "pytest"),
            "expected 'pytest' in args: {:?}",
            *args
        );
        assert!(
            args.iter().any(|a| a == "-q"),
            "expected '-q' in args: {:?}",
            *args
        );
    }

    #[test]
    fn parse_coverage_json_warns_on_missing_version() {
        let tmp = tempfile::tempdir().unwrap();
        let repo_root = tmp.path();
        let json_path = repo_root.join("cov.json");
        let json = r#"{"files": {}}"#;
        std::fs::write(&json_path, json).unwrap();

        let mut inst = PythonInstrumentor::new();
        inst.parse_coverage_json(&json_path, repo_root).unwrap();

        // Parse the JSON directly to verify meta_version is None
        let data: ApexCoverageJson = serde_json::from_str(json).unwrap();
        assert!(data.meta_version().is_none());
    }

    #[test]
    fn parse_coverage_json_with_version() {
        let tmp = tempfile::tempdir().unwrap();
        let repo_root = tmp.path();
        let json_path = repo_root.join("cov.json");
        let json = r#"{"meta": {"version": "7.4.0"}, "files": {}}"#;
        std::fs::write(&json_path, json).unwrap();

        let mut inst = PythonInstrumentor::new();
        inst.parse_coverage_json(&json_path, repo_root).unwrap();

        let data: ApexCoverageJson = serde_json::from_str(json).unwrap();
        assert_eq!(data.meta_version(), Some("7.4.0"));
    }

    #[test]
    fn parse_coverage_json_with_meta_but_no_version() {
        let json = r#"{"meta": {}, "files": {}}"#;
        let data: ApexCoverageJson = serde_json::from_str(json).unwrap();
        assert!(data.meta_version().is_none());
    }

    #[test]
    fn test_instrument_script_has_source_filter() {
        // The embedded script must pass --source or --omit to coverage.py
        let script = include_str!("scripts/apex_instrument.py");
        assert!(
            script.contains("--source") || script.contains("--omit"),
            "apex_instrument.py must filter source vs test files"
        );
    }
}
