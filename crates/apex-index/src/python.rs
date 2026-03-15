use crate::types::{hash_source_files, BranchIndex, TestTrace};
use apex_core::command::{CommandRunner, CommandSpec, RealCommandRunner};
use apex_core::types::{BranchId, ExecutionStatus, Language};
use serde::Deserialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use tracing::{debug, info, warn};

// ---------------------------------------------------------------------------
// Generic builder for testability
// ---------------------------------------------------------------------------

/// Builder that delegates subprocess calls through a generic [`CommandRunner`],
/// allowing tests to inject mocks without spawning real processes.
pub struct PythonIndexBuilder<R: CommandRunner> {
    runner: R,
}

impl<R: CommandRunner> PythonIndexBuilder<R> {
    pub fn new(runner: R) -> Self {
        Self { runner }
    }

    /// Enumerate Python tests by running `pytest --collect-only` through the runner.
    pub async fn enumerate_tests(
        &self,
        target_root: &Path,
    ) -> Result<Vec<String>, Box<dyn std::error::Error + Send + Sync>> {
        let spec = CommandSpec::new("python3", target_root)
            .args(["-m", "pytest", "--collect-only", "-q", "--no-header"])
            .timeout(120_000);
        let output = self.runner.run_command(&spec).await?;
        let stdout = String::from_utf8_lossy(&output.stdout);
        let mut tests = Vec::new();

        for line in stdout.lines() {
            let line = line.trim();
            if line.contains("::") && !line.starts_with('=') && !line.starts_with('-') {
                tests.push(line.to_string());
            }
        }

        if output.exit_code != 0 && tests.is_empty() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            warn!(stderr = %stderr, "pytest --collect-only failed");
        }

        Ok(tests)
    }
}

// ---------------------------------------------------------------------------
// coverage.py JSON schema (reused from apex-instrument)
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct ApexCoverageJson {
    files: HashMap<String, FileData>,
}

#[derive(Debug, Deserialize)]
struct FileData {
    executed_branches: Vec<[i64; 2]>,
    #[allow(dead_code)]
    missing_branches: Vec<[i64; 2]>,
    #[allow(dead_code)]
    all_branches: Vec<[i64; 2]>,
}

/// FNV-1a 64-bit hash (must match apex-instrument's implementation).
fn fnv1a_hash(s: &str) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in s.bytes() {
        hash ^= byte as u64;
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Build a BranchIndex for a Python project by running each test individually
/// under coverage.py and collecting per-test branch data.
pub async fn build_python_index(
    target_root: &Path,
    parallelism: usize,
) -> Result<BranchIndex, Box<dyn std::error::Error + Send + Sync>> {
    let target_root = std::fs::canonicalize(target_root)?;
    info!(target = %target_root.display(), "building Python branch index");

    // 1. Enumerate tests
    let test_names = enumerate_python_tests(&target_root).await?;
    info!(count = test_names.len(), "discovered tests");

    if test_names.is_empty() {
        return Ok(empty_index(&target_root));
    }

    // 2. Run full suite once to get total branch set
    let (all_branches, file_paths) = run_full_coverage(&target_root).await?;
    info!(total = all_branches.len(), "total branches discovered");

    // 3. Run each test individually and collect traces
    let traces = run_per_test_coverage(&target_root, &test_names, parallelism, 0).await?;

    // 4. Build profiles and index
    let profiles = BranchIndex::build_profiles(&traces);
    let covered_branches = profiles.len();
    let source_hash = hash_source_files(&target_root, Language::Python);

    let index = BranchIndex {
        traces,
        profiles,
        file_paths,
        total_branches: all_branches.len(),
        covered_branches,
        created_at: chrono_now(),
        language: Language::Python,
        target_root: target_root.clone(),
        source_hash,
    };

    info!(
        total = index.total_branches,
        covered = index.covered_branches,
        tests = index.traces.len(),
        "index built: {:.1}% coverage",
        index.coverage_percent()
    );

    Ok(index)
}

// ---------------------------------------------------------------------------
// Test enumeration
// ---------------------------------------------------------------------------

/// Enumerate all Python tests via `pytest --collect-only`.
///
/// Convenience wrapper around [`PythonIndexBuilder::enumerate_tests`] using
/// the real subprocess runner.
pub async fn enumerate_python_tests(
    target_root: &Path,
) -> Result<Vec<String>, Box<dyn std::error::Error + Send + Sync>> {
    PythonIndexBuilder::new(RealCommandRunner)
        .enumerate_tests(target_root)
        .await
}

// ---------------------------------------------------------------------------
// Full-suite coverage (for total branch set)
// ---------------------------------------------------------------------------

async fn run_full_coverage(
    target_root: &Path,
) -> Result<(Vec<BranchId>, HashMap<u64, PathBuf>), Box<dyn std::error::Error + Send + Sync>> {
    let data_file = target_root.join(".apex_index_full_cov");
    let json_out = target_root.join(".apex_index_full_cov.json");

    // Run coverage on full suite
    let status = tokio::process::Command::new("python3")
        .args([
            "-m",
            "coverage",
            "run",
            "--branch",
            &format!("--data-file={}", data_file.display()),
            "-m",
            "pytest",
            "-q",
            "--tb=no",
            "--no-header",
        ])
        .current_dir(target_root)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .await?;

    if !status.success() {
        debug!("full suite returned non-zero (coverage data may still exist)");
    }

    // Export to JSON
    let _ = tokio::process::Command::new("python3")
        .args([
            "-m",
            "coverage",
            "json",
            &format!("--data-file={}", data_file.display()),
            "-o",
            &json_out.to_string_lossy(),
        ])
        .current_dir(target_root)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .await;

    let (branches, file_paths) = parse_coverage_all_branches(&json_out, target_root)?;

    // Cleanup temp files
    let _ = std::fs::remove_file(&data_file);
    let _ = std::fs::remove_file(&json_out);

    Ok((branches, file_paths))
}

// ---------------------------------------------------------------------------
// Per-test coverage
// ---------------------------------------------------------------------------

/// Run each test individually with instrumentation and collect per-test traces.
/// `idx_offset` offsets temp file names to avoid collisions across multiple invocations.
pub async fn run_python_per_test(
    target_root: &Path,
    test_names: &[String],
    parallelism: usize,
    idx_offset: usize,
) -> Result<Vec<TestTrace>, Box<dyn std::error::Error + Send + Sync>> {
    run_per_test_coverage(target_root, test_names, parallelism, idx_offset).await
}

async fn run_per_test_coverage(
    target_root: &Path,
    test_names: &[String],
    parallelism: usize,
    idx_offset: usize,
) -> Result<Vec<TestTrace>, Box<dyn std::error::Error + Send + Sync>> {
    use std::sync::Arc;
    use tokio::sync::Semaphore;

    let semaphore = Arc::new(Semaphore::new(parallelism.max(1)));
    let mut handles = Vec::with_capacity(test_names.len());

    for (i, test_name) in test_names.iter().enumerate() {
        let sem = semaphore.clone();
        let root = target_root.to_path_buf();
        let name = test_name.clone();
        let idx = i + idx_offset;

        let handle = tokio::spawn(async move {
            let _permit = sem.acquire().await.map_err(|e| {
                apex_core::error::ApexError::Agent(format!("semaphore closed: {e}"))
            })?;
            run_single_test(&root, &name, idx).await
        });
        handles.push(handle);
    }

    let mut traces = Vec::with_capacity(test_names.len());
    for handle in handles {
        match handle.await? {
            Ok(trace) => traces.push(trace),
            Err(e) => warn!(error = %e, "failed to collect trace for one test"),
        }
    }

    Ok(traces)
}

async fn run_single_test(
    target_root: &Path,
    test_name: &str,
    idx: usize,
) -> Result<TestTrace, Box<dyn std::error::Error + Send + Sync>> {
    let data_file = target_root.join(format!(".apex_idx_test_{idx}"));
    let json_out = target_root.join(format!(".apex_idx_test_{idx}.json"));

    let start = std::time::Instant::now();

    let output = tokio::process::Command::new("python3")
        .args([
            "-m",
            "coverage",
            "run",
            "--branch",
            &format!("--data-file={}", data_file.display()),
            "-m",
            "pytest",
            "-q",
            "--tb=no",
            "--no-header",
            test_name,
        ])
        .current_dir(target_root)
        .output()
        .await?;

    let duration_ms = start.elapsed().as_millis() as u64;
    let status = if output.status.success() {
        ExecutionStatus::Pass
    } else {
        ExecutionStatus::Fail
    };

    // Export to JSON
    let _ = tokio::process::Command::new("python3")
        .args([
            "-m",
            "coverage",
            "json",
            &format!("--data-file={}", data_file.display()),
            "-o",
            &json_out.to_string_lossy(),
        ])
        .current_dir(target_root)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .await;

    let branches = parse_coverage_executed(&json_out, target_root).unwrap_or_default();

    debug!(
        test = test_name,
        branches = branches.len(),
        duration_ms,
        "collected trace"
    );

    // Cleanup
    let _ = std::fs::remove_file(&data_file);
    let _ = std::fs::remove_file(&json_out);

    Ok(TestTrace {
        test_name: test_name.to_string(),
        branches,
        duration_ms,
        status,
    })
}

// ---------------------------------------------------------------------------
// Coverage JSON parsing
// ---------------------------------------------------------------------------

type BranchResult = (Vec<BranchId>, HashMap<u64, PathBuf>);

/// Parse coverage JSON and return ALL branches (executed + missing).
fn parse_coverage_all_branches(
    json_path: &Path,
    repo_root: &Path,
) -> Result<BranchResult, Box<dyn std::error::Error + Send + Sync>> {
    let content = std::fs::read_to_string(json_path)?;
    let data: CoverageJsonRaw = serde_json::from_str(&content)?;
    let mut branches = Vec::new();
    let mut file_paths = HashMap::new();

    for (file_path, fdata) in &data.files {
        let rel = Path::new(file_path)
            .strip_prefix(repo_root)
            .unwrap_or(Path::new(file_path));
        let rel_str = rel.to_string_lossy();
        let file_id = fnv1a_hash(&rel_str);
        file_paths.insert(file_id, rel.to_path_buf());

        if let Some(executed) = fdata.get("executed_branches") {
            if let Some(arr) = executed.as_array() {
                for pair in arr {
                    if let Some(pair_arr) = pair.as_array() {
                        if pair_arr.len() == 2 {
                            let from = pair_arr[0].as_i64().unwrap_or(0);
                            let to = pair_arr[1].as_i64().unwrap_or(0);
                            let direction = if to < 0 { 1u8 } else { 0u8 };
                            branches.push(BranchId::new(
                                file_id,
                                from.unsigned_abs() as u32,
                                0,
                                direction,
                            ));
                        }
                    }
                }
            }
        }
        if let Some(missing) = fdata.get("missing_branches") {
            if let Some(arr) = missing.as_array() {
                for pair in arr {
                    if let Some(pair_arr) = pair.as_array() {
                        if pair_arr.len() == 2 {
                            let from = pair_arr[0].as_i64().unwrap_or(0);
                            let to = pair_arr[1].as_i64().unwrap_or(0);
                            let direction = if to < 0 { 1u8 } else { 0u8 };
                            branches.push(BranchId::new(
                                file_id,
                                from.unsigned_abs() as u32,
                                0,
                                direction,
                            ));
                        }
                    }
                }
            }
        }
    }

    Ok((branches, file_paths))
}

/// Raw coverage.py JSON envelope: {"files": {path: {data}}, "totals": {...}}
#[derive(Debug, Deserialize)]
struct CoverageJsonRaw {
    #[serde(default)]
    files: HashMap<String, HashMap<String, serde_json::Value>>,
}

/// Parse coverage JSON and return only EXECUTED branches.
fn parse_coverage_executed(
    json_path: &Path,
    repo_root: &Path,
) -> Result<Vec<BranchId>, Box<dyn std::error::Error + Send + Sync>> {
    let content = std::fs::read_to_string(json_path)?;

    // Try APEX format first (from apex_instrument.py)
    if let Ok(data) = serde_json::from_str::<ApexCoverageJson>(&content) {
        return Ok(parse_apex_format(&data, repo_root));
    }

    // Fall back to raw coverage.py JSON format
    let data: CoverageJsonRaw = serde_json::from_str(&content)?;
    let mut branches = Vec::new();

    for (file_path, fdata) in &data.files {
        let rel = Path::new(file_path)
            .strip_prefix(repo_root)
            .unwrap_or(Path::new(file_path));
        let file_id = fnv1a_hash(&rel.to_string_lossy());

        if let Some(executed) = fdata.get("executed_branches") {
            if let Some(arr) = executed.as_array() {
                for pair in arr {
                    if let Some(pair_arr) = pair.as_array() {
                        if pair_arr.len() == 2 {
                            let from = pair_arr[0].as_i64().unwrap_or(0);
                            let to = pair_arr[1].as_i64().unwrap_or(0);
                            let direction = if to < 0 { 1u8 } else { 0u8 };
                            branches.push(BranchId::new(
                                file_id,
                                from.unsigned_abs() as u32,
                                0,
                                direction,
                            ));
                        }
                    }
                }
            }
        }
    }

    Ok(branches)
}

fn parse_apex_format(data: &ApexCoverageJson, repo_root: &Path) -> Vec<BranchId> {
    let mut branches = Vec::new();
    for (file_path, fdata) in &data.files {
        let rel = Path::new(file_path)
            .strip_prefix(repo_root)
            .unwrap_or(Path::new(file_path));
        let file_id = fnv1a_hash(&rel.to_string_lossy());

        for pair in &fdata.executed_branches {
            let from = pair[0].unsigned_abs() as u32;
            let direction = if pair[1] < 0 { 1u8 } else { 0u8 };
            branches.push(BranchId::new(file_id, from, 0, direction));
        }
    }
    branches
}

fn chrono_now() -> String {
    // Simple ISO 8601 without chrono dependency
    let dur = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    format!("{}", dur.as_secs())
}

fn empty_index(target_root: &Path) -> BranchIndex {
    BranchIndex {
        traces: vec![],
        profiles: HashMap::new(),
        file_paths: HashMap::new(),
        total_branches: 0,
        covered_branches: 0,
        created_at: chrono_now(),
        language: Language::Python,
        target_root: target_root.to_path_buf(),
        source_hash: hash_source_files(target_root, Language::Python),
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use apex_core::command::{CommandOutput, CommandRunner, CommandSpec};
    use apex_core::error::Result as CoreResult;

    /// Simple mock runner that returns a fixed output.
    struct FakeRunner {
        stdout: Vec<u8>,
    }

    #[async_trait::async_trait]
    impl CommandRunner for FakeRunner {
        async fn run_command(&self, _spec: &CommandSpec) -> CoreResult<CommandOutput> {
            Ok(CommandOutput::success(self.stdout.clone()))
        }
    }

    #[tokio::test]
    async fn builder_enumerate_tests_with_mock() {
        let runner = FakeRunner {
            stdout:
                b"tests/test_foo.py::test_bar\ntests/test_foo.py::test_baz\n\n2 tests collected\n"
                    .to_vec(),
        };

        let builder = PythonIndexBuilder::new(runner);
        let tests = builder.enumerate_tests(Path::new("/fake")).await.unwrap();
        assert_eq!(
            tests,
            vec!["tests/test_foo.py::test_bar", "tests/test_foo.py::test_baz"]
        );
    }

    #[tokio::test]
    async fn builder_enumerate_tests_empty_output() {
        let runner = FakeRunner {
            stdout: b"".to_vec(),
        };

        let builder = PythonIndexBuilder::new(runner);
        let tests = builder.enumerate_tests(Path::new("/fake")).await.unwrap();
        assert!(tests.is_empty());
    }

    #[tokio::test]
    async fn builder_enumerate_tests_filters_separator_lines() {
        let runner = FakeRunner {
            stdout: b"tests/test_a.py::test_one\n======= 1 test =======\n------- short -------\n"
                .to_vec(),
        };

        let builder = PythonIndexBuilder::new(runner);
        let tests = builder.enumerate_tests(Path::new("/fake")).await.unwrap();
        assert_eq!(tests, vec!["tests/test_a.py::test_one"]);
    }

    #[test]
    fn fnv1a_matches_instrument_crate() {
        // Must match apex-instrument's FNV-1a implementation
        assert_eq!(fnv1a_hash(""), 0xcbf2_9ce4_8422_2325);
        let h = fnv1a_hash("src/app.py");
        assert_ne!(h, 0);
    }

    #[test]
    fn parse_apex_format_works() {
        let json = r#"{
            "files": {
                "src/app.py": {
                    "executed_branches": [[10, 12], [20, -1]],
                    "missing_branches": [[10, -1]],
                    "all_branches": [[10, 12], [20, -1], [10, -1]]
                }
            }
        }"#;

        let data: ApexCoverageJson = serde_json::from_str(json).unwrap();
        let branches = parse_apex_format(&data, Path::new("/tmp"));
        assert_eq!(branches.len(), 2);
        assert_eq!(branches[0].direction, 0); // 12 > 0 → true branch
        assert_eq!(branches[1].direction, 1); // -1 < 0 → false branch
    }

    #[test]
    fn parse_coverage_executed_apex_format() {
        let tmp = tempfile::tempdir().unwrap();
        let json_path = tmp.path().join("cov.json");
        let json = r#"{
            "files": {
                "mod.py": {
                    "executed_branches": [[5, 8], [10, -1]],
                    "missing_branches": [],
                    "all_branches": [[5, 8], [10, -1]]
                }
            }
        }"#;
        std::fs::write(&json_path, json).unwrap();

        let branches = parse_coverage_executed(&json_path, tmp.path()).unwrap();
        assert_eq!(branches.len(), 2);
    }

    #[test]
    fn parse_coverage_executed_missing_file() {
        let result = parse_coverage_executed(Path::new("/nonexistent.json"), Path::new("/tmp"));
        assert!(result.is_err());
    }

    // -----------------------------------------------------------------------
    // fnv1a_hash edge cases
    // -----------------------------------------------------------------------

    #[test]
    fn fnv1a_hash_deterministic() {
        let h1 = fnv1a_hash("hello.py");
        let h2 = fnv1a_hash("hello.py");
        assert_eq!(h1, h2);
    }

    #[test]
    fn fnv1a_hash_different_inputs_differ() {
        assert_ne!(fnv1a_hash("a.py"), fnv1a_hash("b.py"));
    }

    #[test]
    fn fnv1a_hash_single_byte() {
        let h = fnv1a_hash("a");
        assert_ne!(h, 0);
        assert_ne!(h, 0xcbf2_9ce4_8422_2325); // differs from empty
    }

    #[test]
    fn fnv1a_hash_long_string() {
        let long = "x".repeat(10000);
        let h = fnv1a_hash(&long);
        assert_ne!(h, 0);
    }

    // -----------------------------------------------------------------------
    // chrono_now
    // -----------------------------------------------------------------------

    #[test]
    fn chrono_now_returns_nonzero_string() {
        let ts = chrono_now();
        assert!(!ts.is_empty());
        let val: u64 = ts.parse().expect("should be numeric");
        assert!(val > 0);
    }

    // -----------------------------------------------------------------------
    // empty_index
    // -----------------------------------------------------------------------

    #[test]
    fn empty_index_has_zero_branches() {
        let tmp = tempfile::tempdir().unwrap();
        let idx = empty_index(tmp.path());
        assert_eq!(idx.total_branches, 0);
        assert_eq!(idx.covered_branches, 0);
        assert!(idx.traces.is_empty());
        assert!(idx.profiles.is_empty());
        assert!(idx.file_paths.is_empty());
        assert!(matches!(idx.language, Language::Python));
        assert_eq!(idx.target_root, tmp.path().to_path_buf());
        assert!(!idx.created_at.is_empty());
    }

    // -----------------------------------------------------------------------
    // parse_coverage_all_branches
    // -----------------------------------------------------------------------

    #[test]
    fn parse_coverage_all_branches_basic() {
        let tmp = tempfile::tempdir().unwrap();
        let json_path = tmp.path().join("cov.json");
        let json = r#"{
            "files": {
                "src/foo.py": {
                    "executed_branches": [[10, 12], [20, -1]],
                    "missing_branches": [[30, 32], [40, -5]]
                }
            }
        }"#;
        std::fs::write(&json_path, json).unwrap();
        let (branches, file_paths) =
            parse_coverage_all_branches(&json_path, Path::new("/nonexistent_root")).unwrap();
        // 2 executed + 2 missing = 4 branches
        assert_eq!(branches.len(), 4);
        // file_paths should have 1 entry
        assert_eq!(file_paths.len(), 1);
    }

    #[test]
    fn parse_coverage_all_branches_negative_to_sets_direction_1() {
        let tmp = tempfile::tempdir().unwrap();
        let json_path = tmp.path().join("cov.json");
        let json = r#"{
            "files": {
                "mod.py": {
                    "executed_branches": [[10, -3]],
                    "missing_branches": [[20, -7]]
                }
            }
        }"#;
        std::fs::write(&json_path, json).unwrap();
        let (branches, _) = parse_coverage_all_branches(&json_path, Path::new("/")).unwrap();
        assert_eq!(branches.len(), 2);
        assert_eq!(branches[0].direction, 1); // to < 0
        assert_eq!(branches[1].direction, 1); // to < 0
    }

    #[test]
    fn parse_coverage_all_branches_positive_to_sets_direction_0() {
        let tmp = tempfile::tempdir().unwrap();
        let json_path = tmp.path().join("cov.json");
        let json = r#"{
            "files": {
                "mod.py": {
                    "executed_branches": [[10, 15]],
                    "missing_branches": [[20, 25]]
                }
            }
        }"#;
        std::fs::write(&json_path, json).unwrap();
        let (branches, _) = parse_coverage_all_branches(&json_path, Path::new("/")).unwrap();
        assert_eq!(branches.len(), 2);
        assert_eq!(branches[0].direction, 0);
        assert_eq!(branches[1].direction, 0);
    }

    #[test]
    fn parse_coverage_all_branches_empty_files() {
        let tmp = tempfile::tempdir().unwrap();
        let json_path = tmp.path().join("cov.json");
        let json = r#"{"files": {}}"#;
        std::fs::write(&json_path, json).unwrap();
        let (branches, file_paths) =
            parse_coverage_all_branches(&json_path, Path::new("/")).unwrap();
        assert!(branches.is_empty());
        assert!(file_paths.is_empty());
    }

    #[test]
    fn parse_coverage_all_branches_missing_keys() {
        // File data has neither executed_branches nor missing_branches
        let tmp = tempfile::tempdir().unwrap();
        let json_path = tmp.path().join("cov.json");
        let json = r#"{"files": {"a.py": {"some_other_key": 42}}}"#;
        std::fs::write(&json_path, json).unwrap();
        let (branches, file_paths) =
            parse_coverage_all_branches(&json_path, Path::new("/")).unwrap();
        assert!(branches.is_empty());
        assert_eq!(file_paths.len(), 1); // file_path entry still created
    }

    #[test]
    fn parse_coverage_all_branches_non_array_executed() {
        // executed_branches is not an array
        let tmp = tempfile::tempdir().unwrap();
        let json_path = tmp.path().join("cov.json");
        let json =
            r#"{"files": {"a.py": {"executed_branches": "not_array", "missing_branches": 42}}}"#;
        std::fs::write(&json_path, json).unwrap();
        let (branches, _) = parse_coverage_all_branches(&json_path, Path::new("/")).unwrap();
        assert!(branches.is_empty());
    }

    #[test]
    fn parse_coverage_all_branches_non_array_pairs() {
        // Pairs within executed_branches are not arrays
        let tmp = tempfile::tempdir().unwrap();
        let json_path = tmp.path().join("cov.json");
        let json = r#"{"files": {"a.py": {"executed_branches": [42, "hello"]}}}"#;
        std::fs::write(&json_path, json).unwrap();
        let (branches, _) = parse_coverage_all_branches(&json_path, Path::new("/")).unwrap();
        assert!(branches.is_empty()); // pairs are not arrays, skipped
    }

    #[test]
    fn parse_coverage_all_branches_short_pair() {
        // Pair has only 1 element (not 2)
        let tmp = tempfile::tempdir().unwrap();
        let json_path = tmp.path().join("cov.json");
        let json = r#"{"files": {"a.py": {"executed_branches": [[10]], "missing_branches": [[20, 30, 40]]}}}"#;
        std::fs::write(&json_path, json).unwrap();
        let (branches, _) = parse_coverage_all_branches(&json_path, Path::new("/")).unwrap();
        // [10] has len 1, skipped. [20, 30, 40] has len 3, skipped.
        assert!(branches.is_empty());
    }

    #[test]
    fn parse_coverage_all_branches_strip_prefix_works() {
        let tmp = tempfile::tempdir().unwrap();
        let json_path = tmp.path().join("cov.json");
        // File path starts with repo_root so strip_prefix removes it
        let json = r#"{"files": {"/repo/root/src/foo.py": {"executed_branches": [[1, 2]]}}}"#;
        std::fs::write(&json_path, json).unwrap();
        let (branches, file_paths) =
            parse_coverage_all_branches(&json_path, Path::new("/repo/root")).unwrap();
        assert_eq!(branches.len(), 1);
        // The file_path should be relative
        let first_path = file_paths.values().next().unwrap();
        assert_eq!(first_path, &PathBuf::from("src/foo.py"));
    }

    #[test]
    fn parse_coverage_all_branches_no_strip_prefix() {
        // File path does NOT start with repo_root, so strip_prefix falls back
        let tmp = tempfile::tempdir().unwrap();
        let json_path = tmp.path().join("cov.json");
        let json = r#"{"files": {"other/path.py": {"executed_branches": [[1, 2]]}}}"#;
        std::fs::write(&json_path, json).unwrap();
        let (_, file_paths) =
            parse_coverage_all_branches(&json_path, Path::new("/different/root")).unwrap();
        let first_path = file_paths.values().next().unwrap();
        assert_eq!(first_path, &PathBuf::from("other/path.py"));
    }

    #[test]
    fn parse_coverage_all_branches_invalid_json() {
        let tmp = tempfile::tempdir().unwrap();
        let json_path = tmp.path().join("cov.json");
        std::fs::write(&json_path, "not json").unwrap();
        let result = parse_coverage_all_branches(&json_path, Path::new("/"));
        assert!(result.is_err());
    }

    #[test]
    fn parse_coverage_all_branches_missing_file() {
        let result = parse_coverage_all_branches(Path::new("/no/such/file.json"), Path::new("/"));
        assert!(result.is_err());
    }

    #[test]
    fn parse_coverage_all_branches_multiple_files() {
        let tmp = tempfile::tempdir().unwrap();
        let json_path = tmp.path().join("cov.json");
        let json = r#"{
            "files": {
                "a.py": {
                    "executed_branches": [[1, 2]],
                    "missing_branches": [[3, -4]]
                },
                "b.py": {
                    "executed_branches": [[10, 20], [30, -40]],
                    "missing_branches": []
                }
            }
        }"#;
        std::fs::write(&json_path, json).unwrap();
        let (branches, file_paths) =
            parse_coverage_all_branches(&json_path, Path::new("/")).unwrap();
        // a.py: 1 executed + 1 missing = 2; b.py: 2 executed + 0 missing = 2; total = 4
        assert_eq!(branches.len(), 4);
        assert_eq!(file_paths.len(), 2);
    }

    #[test]
    fn parse_coverage_all_branches_zero_to_value() {
        // to == 0 is not < 0, so direction = 0
        let tmp = tempfile::tempdir().unwrap();
        let json_path = tmp.path().join("cov.json");
        let json = r#"{"files": {"a.py": {"executed_branches": [[5, 0]]}}}"#;
        std::fs::write(&json_path, json).unwrap();
        let (branches, _) = parse_coverage_all_branches(&json_path, Path::new("/")).unwrap();
        assert_eq!(branches.len(), 1);
        assert_eq!(branches[0].direction, 0);
    }

    // -----------------------------------------------------------------------
    // parse_coverage_executed — raw coverage.py format (fallback path)
    // -----------------------------------------------------------------------

    #[test]
    fn parse_coverage_executed_raw_format() {
        // This JSON does NOT match ApexCoverageJson (no all_branches field),
        // so it falls back to the raw coverage.py parsing path.
        let tmp = tempfile::tempdir().unwrap();
        let json_path = tmp.path().join("cov.json");
        let json = r#"{
            "files": {
                "src/mod.py": {
                    "executed_branches": [[10, 15], [20, -3]]
                }
            }
        }"#;
        std::fs::write(&json_path, json).unwrap();
        let branches = parse_coverage_executed(&json_path, tmp.path()).unwrap();
        assert_eq!(branches.len(), 2);
        assert_eq!(branches[0].direction, 0); // 15 > 0
        assert_eq!(branches[1].direction, 1); // -3 < 0
    }

    #[test]
    fn parse_coverage_executed_raw_format_empty_executed() {
        let tmp = tempfile::tempdir().unwrap();
        let json_path = tmp.path().join("cov.json");
        let json = r#"{"files": {"a.py": {"executed_branches": []}}}"#;
        std::fs::write(&json_path, json).unwrap();
        let branches = parse_coverage_executed(&json_path, tmp.path()).unwrap();
        assert!(branches.is_empty());
    }

    #[test]
    fn parse_coverage_executed_raw_format_no_executed_key() {
        let tmp = tempfile::tempdir().unwrap();
        let json_path = tmp.path().join("cov.json");
        let json = r#"{"files": {"a.py": {"other_key": 1}}}"#;
        std::fs::write(&json_path, json).unwrap();
        let branches = parse_coverage_executed(&json_path, tmp.path()).unwrap();
        assert!(branches.is_empty());
    }

    #[test]
    fn parse_coverage_executed_raw_format_non_array_executed() {
        let tmp = tempfile::tempdir().unwrap();
        let json_path = tmp.path().join("cov.json");
        let json = r#"{"files": {"a.py": {"executed_branches": "bad"}}}"#;
        std::fs::write(&json_path, json).unwrap();
        let branches = parse_coverage_executed(&json_path, tmp.path()).unwrap();
        assert!(branches.is_empty());
    }

    #[test]
    fn parse_coverage_executed_raw_format_non_array_pair() {
        let tmp = tempfile::tempdir().unwrap();
        let json_path = tmp.path().join("cov.json");
        let json = r#"{"files": {"a.py": {"executed_branches": [42, "x"]}}}"#;
        std::fs::write(&json_path, json).unwrap();
        let branches = parse_coverage_executed(&json_path, tmp.path()).unwrap();
        assert!(branches.is_empty());
    }

    #[test]
    fn parse_coverage_executed_raw_format_short_pair() {
        let tmp = tempfile::tempdir().unwrap();
        let json_path = tmp.path().join("cov.json");
        let json = r#"{"files": {"a.py": {"executed_branches": [[10]]}}}"#;
        std::fs::write(&json_path, json).unwrap();
        let branches = parse_coverage_executed(&json_path, tmp.path()).unwrap();
        assert!(branches.is_empty());
    }

    #[test]
    fn parse_coverage_executed_raw_format_strip_prefix() {
        let tmp = tempfile::tempdir().unwrap();
        let json_path = tmp.path().join("cov.json");
        let root_str = tmp.path().to_string_lossy();
        let json = format!(
            r#"{{"files": {{"{root_str}/src/foo.py": {{"executed_branches": [[1, 2]]}}}}}}"#
        );
        std::fs::write(&json_path, &json).unwrap();
        let branches = parse_coverage_executed(&json_path, tmp.path()).unwrap();
        assert_eq!(branches.len(), 1);
        // The file_id should be computed from the relative path
        let expected_file_id = fnv1a_hash("src/foo.py");
        assert_eq!(branches[0].file_id, expected_file_id);
    }

    #[test]
    fn parse_coverage_executed_raw_format_no_strip_prefix() {
        let tmp = tempfile::tempdir().unwrap();
        let json_path = tmp.path().join("cov.json");
        let json = r#"{"files": {"other/path.py": {"executed_branches": [[1, 2]]}}}"#;
        std::fs::write(&json_path, json).unwrap();
        let branches = parse_coverage_executed(&json_path, Path::new("/different")).unwrap();
        assert_eq!(branches.len(), 1);
        let expected_file_id = fnv1a_hash("other/path.py");
        assert_eq!(branches[0].file_id, expected_file_id);
    }

    #[test]
    fn parse_coverage_executed_raw_format_multiple_files() {
        let tmp = tempfile::tempdir().unwrap();
        let json_path = tmp.path().join("cov.json");
        let json = r#"{
            "files": {
                "a.py": {"executed_branches": [[1, 2]]},
                "b.py": {"executed_branches": [[3, 4], [5, -6]]}
            }
        }"#;
        std::fs::write(&json_path, json).unwrap();
        let branches = parse_coverage_executed(&json_path, tmp.path()).unwrap();
        assert_eq!(branches.len(), 3);
    }

    #[test]
    fn parse_coverage_executed_invalid_json() {
        let tmp = tempfile::tempdir().unwrap();
        let json_path = tmp.path().join("cov.json");
        std::fs::write(&json_path, "{{not valid}}").unwrap();
        let result = parse_coverage_executed(&json_path, tmp.path());
        assert!(result.is_err());
    }

    #[test]
    fn parse_coverage_executed_empty_files() {
        let tmp = tempfile::tempdir().unwrap();
        let json_path = tmp.path().join("cov.json");
        let json = r#"{"files": {}}"#;
        std::fs::write(&json_path, json).unwrap();
        let branches = parse_coverage_executed(&json_path, tmp.path()).unwrap();
        assert!(branches.is_empty());
    }

    // -----------------------------------------------------------------------
    // parse_apex_format edge cases
    // -----------------------------------------------------------------------

    #[test]
    fn parse_apex_format_empty_files() {
        let json = r#"{"files": {}}"#;
        let data: ApexCoverageJson = serde_json::from_str(json).unwrap();
        let branches = parse_apex_format(&data, Path::new("/"));
        assert!(branches.is_empty());
    }

    #[test]
    fn parse_apex_format_empty_executed_branches() {
        let json = r#"{
            "files": {
                "a.py": {
                    "executed_branches": [],
                    "missing_branches": [],
                    "all_branches": []
                }
            }
        }"#;
        let data: ApexCoverageJson = serde_json::from_str(json).unwrap();
        let branches = parse_apex_format(&data, Path::new("/"));
        assert!(branches.is_empty());
    }

    #[test]
    fn parse_apex_format_strip_prefix() {
        let json = r#"{
            "files": {
                "/repo/src/a.py": {
                    "executed_branches": [[1, 2]],
                    "missing_branches": [],
                    "all_branches": [[1, 2]]
                }
            }
        }"#;
        let data: ApexCoverageJson = serde_json::from_str(json).unwrap();
        let branches = parse_apex_format(&data, Path::new("/repo"));
        assert_eq!(branches.len(), 1);
        let expected_id = fnv1a_hash("src/a.py");
        assert_eq!(branches[0].file_id, expected_id);
    }

    #[test]
    fn parse_apex_format_no_strip_prefix() {
        let json = r#"{
            "files": {
                "relative/path.py": {
                    "executed_branches": [[5, 10]],
                    "missing_branches": [],
                    "all_branches": [[5, 10]]
                }
            }
        }"#;
        let data: ApexCoverageJson = serde_json::from_str(json).unwrap();
        let branches = parse_apex_format(&data, Path::new("/other/root"));
        assert_eq!(branches.len(), 1);
        let expected_id = fnv1a_hash("relative/path.py");
        assert_eq!(branches[0].file_id, expected_id);
    }

    #[test]
    fn parse_apex_format_multiple_files() {
        let json = r#"{
            "files": {
                "a.py": {
                    "executed_branches": [[1, 2]],
                    "missing_branches": [],
                    "all_branches": [[1, 2]]
                },
                "b.py": {
                    "executed_branches": [[3, -4], [5, 6]],
                    "missing_branches": [],
                    "all_branches": [[3, -4], [5, 6]]
                }
            }
        }"#;
        let data: ApexCoverageJson = serde_json::from_str(json).unwrap();
        let branches = parse_apex_format(&data, Path::new("/"));
        assert_eq!(branches.len(), 3);
    }

    #[test]
    fn parse_apex_format_zero_direction() {
        // pair[1] == 0 => direction = 0 (not < 0)
        let json = r#"{
            "files": {
                "a.py": {
                    "executed_branches": [[10, 0]],
                    "missing_branches": [],
                    "all_branches": [[10, 0]]
                }
            }
        }"#;
        let data: ApexCoverageJson = serde_json::from_str(json).unwrap();
        let branches = parse_apex_format(&data, Path::new("/"));
        assert_eq!(branches[0].direction, 0);
    }

    #[test]
    fn parse_apex_format_uses_unsigned_abs_for_from() {
        // negative from value: unsigned_abs should give positive line
        let json = r#"{
            "files": {
                "a.py": {
                    "executed_branches": [[-10, 5]],
                    "missing_branches": [],
                    "all_branches": [[-10, 5]]
                }
            }
        }"#;
        let data: ApexCoverageJson = serde_json::from_str(json).unwrap();
        let branches = parse_apex_format(&data, Path::new("/"));
        assert_eq!(branches[0].line, 10); // unsigned_abs(-10) = 10
    }

    // -----------------------------------------------------------------------
    // CoverageJsonRaw deserialization
    // -----------------------------------------------------------------------

    #[test]
    fn coverage_json_raw_default_files() {
        // "files" is missing entirely — serde(default) gives empty HashMap
        let json = r#"{}"#;
        let data: CoverageJsonRaw = serde_json::from_str(json).unwrap();
        assert!(data.files.is_empty());
    }

    #[test]
    fn coverage_json_raw_with_totals() {
        // Extra fields like "totals" should be ignored
        let json = r#"{"files": {}, "totals": {"percent_covered": 50.0}}"#;
        let data: CoverageJsonRaw = serde_json::from_str(json).unwrap();
        assert!(data.files.is_empty());
    }

    // -----------------------------------------------------------------------
    // parse_coverage_all_branches — unwrap_or(0) paths for non-numeric values
    // -----------------------------------------------------------------------

    #[test]
    fn parse_coverage_all_branches_non_numeric_pair_values() {
        // Pair values are not numbers — as_i64() returns None, falls back to 0
        let tmp = tempfile::tempdir().unwrap();
        let json_path = tmp.path().join("cov.json");
        let json = r#"{"files": {"a.py": {"executed_branches": [["x", "y"]]}}}"#;
        std::fs::write(&json_path, json).unwrap();
        let (branches, _) = parse_coverage_all_branches(&json_path, Path::new("/")).unwrap();
        assert_eq!(branches.len(), 1);
        // Both from and to are 0 (from unwrap_or(0)); to=0 is not < 0, so direction=0
        assert_eq!(branches[0].line, 0);
        assert_eq!(branches[0].direction, 0);
    }

    // -----------------------------------------------------------------------
    // parse_coverage_executed — raw format, non-numeric pair values
    // -----------------------------------------------------------------------

    #[test]
    fn parse_coverage_executed_raw_non_numeric_pair() {
        let tmp = tempfile::tempdir().unwrap();
        let json_path = tmp.path().join("cov.json");
        let json = r#"{"files": {"a.py": {"executed_branches": [["a", "b"]]}}}"#;
        std::fs::write(&json_path, json).unwrap();
        let branches = parse_coverage_executed(&json_path, tmp.path()).unwrap();
        assert_eq!(branches.len(), 1);
        assert_eq!(branches[0].line, 0);
        assert_eq!(branches[0].direction, 0);
    }

    #[test]
    fn parse_coverage_executed_raw_zero_to() {
        let tmp = tempfile::tempdir().unwrap();
        let json_path = tmp.path().join("cov.json");
        let json = r#"{"files": {"a.py": {"executed_branches": [[5, 0]]}}}"#;
        std::fs::write(&json_path, json).unwrap();
        let branches = parse_coverage_executed(&json_path, tmp.path()).unwrap();
        assert_eq!(branches.len(), 1);
        assert_eq!(branches[0].direction, 0); // 0 is not < 0
    }

    #[test]
    fn parse_coverage_executed_raw_long_pair_skipped() {
        let tmp = tempfile::tempdir().unwrap();
        let json_path = tmp.path().join("cov.json");
        let json = r#"{"files": {"a.py": {"executed_branches": [[1, 2, 3]]}}}"#;
        std::fs::write(&json_path, json).unwrap();
        let branches = parse_coverage_executed(&json_path, tmp.path()).unwrap();
        assert!(branches.is_empty()); // len != 2
    }

    // -----------------------------------------------------------------------
    // Bug-hunting tests
    // -----------------------------------------------------------------------

    /// BUG: Large line numbers (> u32::MAX) get silently truncated via
    /// `unsigned_abs() as u32` in parse_apex_format. While Python files
    /// won't realistically have 4 billion lines, the code should not silently
    /// produce wrong line numbers for any valid i64 input.
    #[test]
    fn bug_line_number_truncation_in_apex_format() {
        // pair[0] = 4_294_967_296 (u32::MAX + 1) — unsigned_abs gives
        // 4_294_967_296u64, which truncates to 0u32.
        let json = r#"{
            "files": {
                "big.py": {
                    "executed_branches": [[4294967296, 5]],
                    "missing_branches": [],
                    "all_branches": [[4294967296, 5]]
                }
            }
        }"#;
        let data: ApexCoverageJson = serde_json::from_str(json).unwrap();
        let branches = parse_apex_format(&data, Path::new("/"));
        assert_eq!(branches.len(), 1);
        // BUG: line should be 4294967296 but gets truncated to 0
        // This demonstrates that `as u32` silently wraps.
        assert_eq!(branches[0].line, 0, "line was silently truncated to 0 — data loss");
    }

    /// Same truncation bug in parse_coverage_all_branches (raw format path).
    #[test]
    fn bug_line_number_truncation_in_raw_format() {
        let tmp = tempfile::tempdir().unwrap();
        let json_path = tmp.path().join("cov.json");
        let json = r#"{"files": {"big.py": {"executed_branches": [[4294967296, 5]]}}}"#;
        std::fs::write(&json_path, json).unwrap();
        let (branches, _) = parse_coverage_all_branches(&json_path, Path::new("/")).unwrap();
        assert_eq!(branches.len(), 1);
        // BUG: line truncated from 4294967296 to 0
        assert_eq!(branches[0].line, 0, "line was silently truncated to 0 — data loss");
    }

    /// BUG: parse_coverage_all_branches double-counts branches that appear in
    /// both executed_branches and missing_branches. While coverage.py normally
    /// keeps these disjoint, the code provides no deduplication, so a malformed
    /// or manually-constructed coverage report inflates total_branches.
    #[test]
    fn bug_duplicate_branches_in_all_branches() {
        let tmp = tempfile::tempdir().unwrap();
        let json_path = tmp.path().join("cov.json");
        // Same branch [10, 12] appears in both executed and missing
        let json = r#"{
            "files": {
                "dup.py": {
                    "executed_branches": [[10, 12]],
                    "missing_branches": [[10, 12]]
                }
            }
        }"#;
        std::fs::write(&json_path, json).unwrap();
        let (branches, _) = parse_coverage_all_branches(&json_path, Path::new("/")).unwrap();
        // BUG: Returns 2 branches when logically there's only 1 unique branch.
        // This inflates total_branches and gives wrong coverage percentage.
        assert_eq!(branches.len(), 2, "duplicate branch counted twice — inflates total_branches");
    }

    /// BUG: parse_coverage_executed prefers APEX format, but if a raw
    /// coverage.py JSON happens to include all three fields (executed_branches,
    /// missing_branches, all_branches) as proper typed arrays, it gets parsed
    /// as APEX format silently. This test documents the implicit priority.
    #[test]
    fn bug_format_detection_ambiguity() {
        let tmp = tempfile::tempdir().unwrap();
        let json_path = tmp.path().join("cov.json");
        // This JSON is valid for BOTH ApexCoverageJson and CoverageJsonRaw
        let json = r#"{
            "files": {
                "mod.py": {
                    "executed_branches": [[5, 8], [10, -1]],
                    "missing_branches": [[15, 20]],
                    "all_branches": [[5, 8], [10, -1], [15, 20]]
                }
            }
        }"#;
        std::fs::write(&json_path, json).unwrap();
        let branches = parse_coverage_executed(&json_path, tmp.path()).unwrap();
        // Both parsers would return the same 2 executed branches,
        // so the result is the same. But the APEX path is taken.
        assert_eq!(branches.len(), 2);
    }

    /// BUG: negative `from` values produce unsigned line numbers via
    /// unsigned_abs(). coverage.py uses negative "to" values to signal
    /// exits, but negative "from" is unusual. The code silently converts
    /// e.g. -10 to line 10.
    #[test]
    fn bug_negative_from_in_raw_format() {
        let tmp = tempfile::tempdir().unwrap();
        let json_path = tmp.path().join("cov.json");
        // negative from = -10, coverage.py shouldn't produce this, but
        // if it does the code silently maps it to line 10
        let json = r#"{"files": {"a.py": {"executed_branches": [[-10, 5]]}}}"#;
        std::fs::write(&json_path, json).unwrap();
        let branches = parse_coverage_executed(&json_path, tmp.path()).unwrap();
        assert_eq!(branches.len(), 1);
        // unsigned_abs(-10) = 10 — silent sign flip
        assert_eq!(branches[0].line, 10, "negative from silently became positive via unsigned_abs");
    }

    /// Test that parse_coverage_all_branches handles a file path that is
    /// exactly the repo root (edge case for strip_prefix).
    #[test]
    fn bug_file_path_equals_repo_root() {
        let tmp = tempfile::tempdir().unwrap();
        let json_path = tmp.path().join("cov.json");
        // File path equals repo root — strip_prefix gives ""
        let json = r#"{"files": {"/repo": {"executed_branches": [[1, 2]]}}}"#;
        std::fs::write(&json_path, json).unwrap();
        let (branches, file_paths) =
            parse_coverage_all_branches(&json_path, Path::new("/repo")).unwrap();
        assert_eq!(branches.len(), 1);
        // strip_prefix("/repo", "/repo") = "" — empty path
        let path = file_paths.values().next().unwrap();
        assert_eq!(path, &PathBuf::from(""), "strip_prefix produces empty path for exact match");
        // file_id is hash of "" which is the same as empty string hash
        assert_eq!(branches[0].file_id, fnv1a_hash(""));
    }

    /// Test that CoverageJsonRaw gracefully handles null values in branch arrays.
    #[test]
    fn bug_null_values_in_branch_pair() {
        let tmp = tempfile::tempdir().unwrap();
        let json_path = tmp.path().join("cov.json");
        let json = r#"{"files": {"a.py": {"executed_branches": [[null, null]]}}}"#;
        std::fs::write(&json_path, json).unwrap();
        let (branches, _) = parse_coverage_all_branches(&json_path, Path::new("/")).unwrap();
        // null.as_i64() returns None, unwrap_or(0) gives 0
        assert_eq!(branches.len(), 1);
        assert_eq!(branches[0].line, 0);
        assert_eq!(branches[0].direction, 0);
    }

    /// Test parse_coverage_executed with empty string file path.
    #[test]
    fn bug_empty_file_path_in_coverage() {
        let tmp = tempfile::tempdir().unwrap();
        let json_path = tmp.path().join("cov.json");
        let json = r#"{"files": {"": {"executed_branches": [[1, 2]]}}}"#;
        std::fs::write(&json_path, json).unwrap();
        let branches = parse_coverage_executed(&json_path, tmp.path()).unwrap();
        assert_eq!(branches.len(), 1);
        // file_id from empty string
        assert_eq!(branches[0].file_id, fnv1a_hash(""));
    }

    /// BUG: Very large negative `to` values don't affect direction logic
    /// but unsigned_abs on `from` with i64::MIN would overflow in debug mode.
    /// In release mode, i64::MIN.unsigned_abs() = 2^63 which truncates to 0u32.
    #[test]
    fn bug_i64_min_from_value() {
        let json = r#"{
            "files": {
                "edge.py": {
                    "executed_branches": [[-9223372036854775808, 1]],
                    "missing_branches": [],
                    "all_branches": [[-9223372036854775808, 1]]
                }
            }
        }"#;
        let data: ApexCoverageJson = serde_json::from_str(json).unwrap();
        let branches = parse_apex_format(&data, Path::new("/"));
        assert_eq!(branches.len(), 1);
        // i64::MIN.unsigned_abs() = 9223372036854775808u64, as u32 = 0
        assert_eq!(branches[0].line, 0, "i64::MIN truncated to 0 via as u32");
    }

    /// Test that multiple files with same relative path (after different
    /// strip_prefix results) map to the same file_id — a collision risk.
    #[test]
    fn bug_file_id_collision_different_abs_paths_same_relative() {
        let tmp = tempfile::tempdir().unwrap();
        let json_path = tmp.path().join("cov.json");
        // Two different absolute paths that both strip to "src/app.py"
        let json = r#"{
            "files": {
                "/repo/src/app.py": {"executed_branches": [[1, 2]]},
                "/other/repo/src/app.py": {"executed_branches": [[3, 4]]}
            }
        }"#;
        std::fs::write(&json_path, json).unwrap();
        // strip_prefix with "/repo" — first strips to "src/app.py", second doesn't strip
        let (branches, file_paths) =
            parse_coverage_all_branches(&json_path, Path::new("/repo")).unwrap();
        assert_eq!(branches.len(), 2);
        // One path strips to "src/app.py", the other stays as "/other/repo/src/app.py"
        // So they get different file_ids — no collision in this case
        assert_eq!(file_paths.len(), 2);
    }

    /// Test that parse_coverage_all_branches handles float values in pairs.
    /// coverage.py always outputs integers, but JSON allows floats.
    #[test]
    fn bug_float_values_in_branch_pair() {
        let tmp = tempfile::tempdir().unwrap();
        let json_path = tmp.path().join("cov.json");
        let json = r#"{"files": {"a.py": {"executed_branches": [[10.5, 20.7]]}}}"#;
        std::fs::write(&json_path, json).unwrap();
        let (branches, _) = parse_coverage_all_branches(&json_path, Path::new("/")).unwrap();
        // as_i64() on a float returns None (serde_json behavior), falls back to 0
        assert_eq!(branches.len(), 1);
        assert_eq!(branches[0].line, 0, "float from-value silently becomes line 0");
        assert_eq!(branches[0].direction, 0, "float to-value silently becomes direction 0");
    }
}
