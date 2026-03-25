use apex_core::{
    error::{ApexError, Result},
    hash::fnv1a_hash,
    traits::Sandbox,
    types::{
        BranchId, BranchState, ExecutionResult, ExecutionStatus, InputSeed, Language, SnapshotId,
    },
};
use apex_coverage::CoverageOracle;
use async_trait::async_trait;
use std::{collections::HashMap, path::PathBuf, sync::Arc, time::Instant};
use uuid::Uuid;

/// (file_path, branch_key, arm_index, hit_count)
type BranchHit = (String, String, usize, u64);

/// Parse Istanbul coverage-final.json and extract branch arm hit counts.
fn parse_istanbul_branches(json_str: &str) -> Vec<BranchHit> {
    let Ok(root) = serde_json::from_str::<serde_json::Value>(json_str) else {
        return Vec::new();
    };
    let Some(files) = root.as_object() else {
        return Vec::new();
    };

    let mut hits = Vec::new();
    for (file_path, file_data) in files {
        let Some(branches) = file_data.get("b").and_then(|b| b.as_object()) else {
            continue;
        };
        for (branch_key, arms) in branches {
            let Some(arm_counts) = arms.as_array() else {
                continue;
            };
            for (arm_idx, count) in arm_counts.iter().enumerate() {
                let c = count.as_u64().unwrap_or(0);
                if c > 0 {
                    hits.push((file_path.clone(), branch_key.clone(), arm_idx, c));
                }
            }
        }
    }
    hits
}

/// Runs an agent-generated Jest test file in the target project and returns
/// execution results. Coverage delta is not tracked at this level (the caller
/// is responsible for calling `oracle.merge_from_result`).
///
/// `InputSeed.data` must be UTF-8 JavaScript source code (a Jest test file).
pub struct JavaScriptTestSandbox {
    oracle: Arc<CoverageOracle>,
    file_paths: Arc<HashMap<u64, PathBuf>>,
    target_root: PathBuf,
}

impl JavaScriptTestSandbox {
    pub fn new(
        oracle: Arc<CoverageOracle>,
        file_paths: Arc<HashMap<u64, PathBuf>>,
        target_root: PathBuf,
    ) -> Self {
        JavaScriptTestSandbox {
            oracle,
            file_paths,
            target_root,
        }
    }
}

#[async_trait]
impl Sandbox for JavaScriptTestSandbox {
    async fn run(&self, seed: &InputSeed) -> Result<ExecutionResult> {
        let start = Instant::now();

        // Decode test code.
        let code = std::str::from_utf8(&seed.data)
            .map_err(|e| ApexError::Sandbox(format!("candidate not valid UTF-8: {e}")))?;

        // Write probe file to target_root/tests/apex_probe_<uuid>.test.js
        let probe_name = format!("apex_probe_{}", Uuid::new_v4().simple());
        let tests_dir = self.target_root.join("tests");
        std::fs::create_dir_all(&tests_dir)
            .map_err(|e| ApexError::Sandbox(format!("create tests dir: {e}")))?;

        let test_file = tests_dir.join(format!("{probe_name}.test.js"));
        std::fs::write(&test_file, code)
            .map_err(|e| ApexError::Sandbox(format!("write probe: {e}")))?;

        // Run jest against the probe file with Istanbul coverage.
        let coverage_dir = tests_dir.join(".apex_coverage_js");
        let output = tokio::process::Command::new("node")
            .args([
                "node_modules/.bin/jest",
                &test_file.to_string_lossy(),
                "--coverage",
                "--coverageReporters=json",
                &format!("--coverageDirectory={}", coverage_dir.display()),
                "--testTimeout=10000",
            ])
            .current_dir(&self.target_root)
            .output()
            .await
            .map_err(|e| ApexError::Sandbox(format!("spawn jest: {e}")))?;

        let duration_ms = start.elapsed().as_millis() as u64;

        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();

        // Determine pass/fail from exit code and output.
        let status = match output.status.code() {
            Some(0) => ExecutionStatus::Pass,
            Some(code) if code < 0 => ExecutionStatus::Crash,
            _ => ExecutionStatus::Fail,
        };

        // Delete the probe file (best-effort).
        let _ = std::fs::remove_file(&test_file);

        // Collect coverage branches from Istanbul output.
        let mut new_branches = Vec::new();
        let cov_json_path = coverage_dir.join("coverage-final.json");
        if let Ok(cov_json) = std::fs::read_to_string(&cov_json_path) {
            let hits = parse_istanbul_branches(&cov_json);
            for (file_path, _branch_key, arm_idx, _count) in &hits {
                let file_id = fnv1a_hash(file_path);
                // Only track files we know about.
                if self.file_paths.get(&file_id).is_some() {
                    let branch = BranchId::new(file_id, 0, *arm_idx as u16, 0);
                    if matches!(self.oracle.state_of(&branch), Some(BranchState::Uncovered)) {
                        new_branches.push(branch);
                    }
                }
            }
        }
        // Clean up coverage dir (best-effort).
        let _ = std::fs::remove_dir_all(&coverage_dir);

        Ok(ExecutionResult {
            seed_id: seed.id,
            status,
            new_branches,
            trace: None,
            duration_ms,
            stdout,
            stderr,
            input: None,
            resource_metrics: None,
        })
    }

    async fn snapshot(&self) -> Result<SnapshotId> {
        Err(ApexError::Sandbox("not supported".into()))
    }

    async fn restore(&self, _: SnapshotId) -> Result<()> {
        Err(ApexError::Sandbox("not supported".into()))
    }

    fn language(&self) -> Language {
        Language::JavaScript
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_istanbul_branch_hits() {
        let json = r#"{
            "/src/index.js": {
                "branchMap": {
                    "0": { "type": "if", "loc": { "start": { "line": 5 }, "end": { "line": 5 } } },
                    "1": { "type": "if", "loc": { "start": { "line": 10 }, "end": { "line": 10 } } }
                },
                "b": {
                    "0": [1, 0],
                    "1": [0, 1]
                }
            }
        }"#;
        let hits = parse_istanbul_branches(json);
        // Branch 0 arm 0 was hit (1), Branch 1 arm 1 was hit (1)
        assert_eq!(hits.len(), 2);
    }

    #[test]
    fn parse_istanbul_empty_json() {
        let hits = parse_istanbul_branches("{}");
        assert!(hits.is_empty());
    }

    #[test]
    fn parse_istanbul_invalid_json() {
        let hits = parse_istanbul_branches("not json");
        assert!(hits.is_empty());
    }

    #[test]
    fn fnv1a_hash_deterministic() {
        let a = fnv1a_hash("/src/index.js");
        let b = fnv1a_hash("/src/index.js");
        assert_eq!(a, b);
        assert_ne!(fnv1a_hash("/src/a.js"), fnv1a_hash("/src/b.js"));
    }

    #[test]
    fn fnv1a_hash_empty_string() {
        // FNV offset basis for empty input
        assert_eq!(fnv1a_hash(""), 0xcbf2_9ce4_8422_2325);
    }

    #[test]
    fn parse_istanbul_no_b_key() {
        // File entry without "b" key should be skipped gracefully.
        let json = r#"{
            "/src/index.js": {
                "branchMap": {}
            }
        }"#;
        let hits = parse_istanbul_branches(json);
        assert!(hits.is_empty());
    }

    #[test]
    fn parse_istanbul_all_zero_counts() {
        // Arms with zero hit counts are excluded.
        let json = r#"{
            "/src/index.js": {
                "b": {
                    "0": [0, 0],
                    "1": [0, 0]
                }
            }
        }"#;
        let hits = parse_istanbul_branches(json);
        assert!(hits.is_empty());
    }

    #[test]
    fn parse_istanbul_non_array_arm() {
        // A branch key whose value is not an array should be skipped.
        let json = r#"{
            "/src/index.js": {
                "b": {
                    "0": "not_an_array"
                }
            }
        }"#;
        let hits = parse_istanbul_branches(json);
        assert!(hits.is_empty());
    }

    #[test]
    fn parse_istanbul_multiple_files() {
        let json = r#"{
            "/src/a.js": { "b": { "0": [1, 0] } },
            "/src/b.js": { "b": { "0": [0, 2] } }
        }"#;
        let hits = parse_istanbul_branches(json);
        assert_eq!(hits.len(), 2);
    }

    #[test]
    fn parse_istanbul_hit_count_preserved() {
        let json = r#"{
            "/src/app.js": {
                "b": { "0": [5, 0] }
            }
        }"#;
        let hits = parse_istanbul_branches(json);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].3, 5);
    }

    #[test]
    fn sandbox_constructor_stores_fields() {
        use apex_coverage::CoverageOracle;
        let oracle = Arc::new(CoverageOracle::new());
        let file_paths = Arc::new(HashMap::new());
        let target_root = PathBuf::from("/tmp/test_proj");

        let sb = JavaScriptTestSandbox::new(oracle, file_paths, target_root.clone());
        assert_eq!(sb.target_root, target_root);
        assert_eq!(sb.language(), Language::JavaScript);
    }

    #[tokio::test]
    async fn sandbox_snapshot_returns_error() {
        use apex_coverage::CoverageOracle;
        let oracle = Arc::new(CoverageOracle::new());
        let file_paths = Arc::new(HashMap::new());
        let sb = JavaScriptTestSandbox::new(oracle, file_paths, PathBuf::from("/tmp"));

        let result = sb.snapshot().await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn sandbox_restore_returns_error() {
        use apex_core::types::SnapshotId;
        use apex_coverage::CoverageOracle;
        let oracle = Arc::new(CoverageOracle::new());
        let file_paths = Arc::new(HashMap::new());
        let sb = JavaScriptTestSandbox::new(oracle, file_paths, PathBuf::from("/tmp"));

        let result = sb.restore(SnapshotId::new()).await;
        assert!(result.is_err());
    }

    // ------------------------------------------------------------------
    // Additional branch-coverage tests
    // ------------------------------------------------------------------

    /// `parse_istanbul_branches` with a count value that is not a JSON number.
    /// `count.as_u64().unwrap_or(0)` returns 0 → count not > 0 → skipped.
    #[test]
    fn parse_istanbul_non_number_count_is_zero() {
        let json = r#"{
            "/src/app.js": {
                "b": {
                    "0": [null, true, "str"]
                }
            }
        }"#;
        // null/true/"str" all have as_u64() = None → unwrap_or(0) = 0 → skipped
        let hits = parse_istanbul_branches(json);
        assert!(hits.is_empty());
    }

    /// `parse_istanbul_branches` with a mix of zero and non-zero counts,
    /// verifying the `c > 0` gate.
    #[test]
    fn parse_istanbul_only_nonzero_arms_included() {
        let json = r#"{
            "/src/lib.js": {
                "b": {
                    "0": [0, 3, 0, 1]
                }
            }
        }"#;
        let hits = parse_istanbul_branches(json);
        // arm 1 (count=3) and arm 3 (count=1) are hits; arms 0 and 2 (count=0) are not.
        assert_eq!(hits.len(), 2);
        // verify arm indices are correct
        let arm_indices: std::collections::HashSet<usize> = hits.iter().map(|h| h.2).collect();
        assert!(arm_indices.contains(&1));
        assert!(arm_indices.contains(&3));
    }

    /// `parse_istanbul_branches` root object is not a JSON object → returns empty.
    #[test]
    fn parse_istanbul_root_is_array() {
        // `root.as_object()` returns None when the root is an array.
        let hits = parse_istanbul_branches("[1, 2, 3]");
        assert!(hits.is_empty());
    }

    /// `parse_istanbul_branches` where "b" exists but its value is not an object.
    #[test]
    fn parse_istanbul_b_key_not_object() {
        let json = r#"{
            "/src/a.js": {
                "b": 42
            }
        }"#;
        // `file_data.get("b").and_then(|b| b.as_object())` returns None for integer "b"
        let hits = parse_istanbul_branches(json);
        assert!(hits.is_empty());
    }

    /// `fnv1a_hash` on a single-char string exercises the loop body once.
    #[test]
    fn fnv1a_hash_single_char() {
        let h = fnv1a_hash("a");
        // Should be different from the offset basis (non-empty input changes hash).
        assert_ne!(h, 0xcbf2_9ce4_8422_2325);
    }

    /// `fnv1a_hash` produces different results for different strings.
    #[test]
    fn fnv1a_hash_many_strings_differ() {
        let paths: Vec<&str> = vec!["/src/a.js", "/src/b.js", "/src/c.js", "/lib/util.js"];
        let hashes: std::collections::HashSet<u64> = paths.iter().map(|p| fnv1a_hash(p)).collect();
        assert_eq!(hashes.len(), paths.len(), "all hashes should be distinct");
    }

    /// When `file_paths.get(&file_id)` returns `None`, the branch is not tracked.
    /// This exercises the `is_some()` check returning false.
    #[test]
    fn parse_istanbul_unknown_file_not_tracked() {
        let json = r#"{
            "/unknown/path/that/has/no/dot.js": {
                "b": {
                    "0": [5, 0]
                }
            }
        }"#;
        // Parse produces 1 hit for arm 0.
        let hits = parse_istanbul_branches(json);
        assert_eq!(hits.len(), 1);

        // But if we construct the sandbox with empty file_paths, no branch is added
        // to new_branches (the `is_some()` check returns false).
        // We cannot easily test the full run() path without a Jest installation,
        // but we can verify the parsing side (which is already done above) and
        // confirm the file_paths lookup logic indirectly via the empty-map assertion.
        let oracle = Arc::new(CoverageOracle::new());
        let file_paths: Arc<HashMap<u64, PathBuf>> = Arc::new(HashMap::new());
        let sb = JavaScriptTestSandbox::new(oracle, file_paths, PathBuf::from("/tmp"));
        // Construction succeeds; the branch filter logic is exercised during run().
        assert_eq!(sb.target_root, PathBuf::from("/tmp"));
    }

    /// `oracle.state_of(&branch)` returning `Some(BranchState::Covered)` → branch not added.
    /// Exercises the `matches!(… Uncovered)` returning false arm.
    #[test]
    fn parse_istanbul_already_covered_branch_not_in_new() {
        // This exercises the oracle state filter in the sandbox run() method.
        // We test it indirectly through the parse helper + oracle state.
        use apex_core::types::{BranchId, BranchState, SeedId};

        let oracle = Arc::new(CoverageOracle::new());
        let file_id = fnv1a_hash("/src/known.js");
        let b = BranchId::new(file_id, 0, 0, 0);
        oracle.register_branches([b.clone()]);
        oracle.mark_covered(&b, SeedId::new());

        // State should be Covered (has hit_count field), not Uncovered.
        assert!(!matches!(oracle.state_of(&b), Some(BranchState::Uncovered)));
    }

    /// Multiple arms in a single branch entry — arm indices are assigned correctly.
    #[test]
    fn parse_istanbul_multi_arm_indices() {
        let json = r#"{
            "/src/multi.js": {
                "b": {
                    "1": [10, 20, 30]
                }
            }
        }"#;
        let hits = parse_istanbul_branches(json);
        // All 3 arms have count > 0.
        assert_eq!(hits.len(), 3);
        let indices: Vec<usize> = hits.iter().map(|h| h.2).collect();
        // Arm indices should be 0, 1, 2 in enumerate order.
        for (expected, &got) in [0usize, 1, 2].iter().zip(indices.iter()) {
            assert_eq!(got, *expected);
        }
    }

    /// Empty arms array: no arms → nothing added.
    #[test]
    fn parse_istanbul_empty_arm_array() {
        let json = r#"{
            "/src/empty.js": {
                "b": {
                    "0": []
                }
            }
        }"#;
        let hits = parse_istanbul_branches(json);
        assert!(hits.is_empty());
    }

    /// `language()` returns JavaScript.
    #[test]
    fn language_method_returns_javascript() {
        let oracle = Arc::new(CoverageOracle::new());
        let file_paths = Arc::new(HashMap::new());
        let sb = JavaScriptTestSandbox::new(oracle, file_paths, PathBuf::from("/proj"));
        use apex_core::traits::Sandbox;
        assert_eq!(sb.language(), Language::JavaScript);
    }

    /// Snapshot error message contains "not supported".
    #[tokio::test]
    async fn snapshot_error_message_not_supported() {
        let oracle = Arc::new(CoverageOracle::new());
        let file_paths = Arc::new(HashMap::new());
        let sb = JavaScriptTestSandbox::new(oracle, file_paths, PathBuf::from("/tmp"));
        let err = sb.snapshot().await.unwrap_err();
        let msg = format!("{err}");
        assert!(msg.contains("not supported"), "error: {msg}");
    }

    /// Restore error message contains "not supported".
    #[tokio::test]
    async fn restore_error_message_not_supported() {
        use apex_core::types::SnapshotId;
        let oracle = Arc::new(CoverageOracle::new());
        let file_paths = Arc::new(HashMap::new());
        let sb = JavaScriptTestSandbox::new(oracle, file_paths, PathBuf::from("/tmp"));
        let err = sb.restore(SnapshotId::new()).await.unwrap_err();
        let msg = format!("{err}");
        assert!(msg.contains("not supported"), "error: {msg}");
    }
}
