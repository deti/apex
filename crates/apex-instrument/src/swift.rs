use apex_core::{
    command::{CommandRunner, CommandSpec, RealCommandRunner},
    config::InstrumentTimeouts,
    error::{ApexError, Result},
    hash::fnv1a_hash,
    traits::Instrumentor,
    types::{BranchId, InstrumentedTarget, Target},
};
use async_trait::async_trait;
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};
use tracing::{debug, info, warn};

pub struct SwiftInstrumentor<R: CommandRunner = RealCommandRunner> {
    runner: R,
    timeouts: InstrumentTimeouts,
}

impl SwiftInstrumentor {
    pub fn new() -> Self {
        SwiftInstrumentor {
            runner: RealCommandRunner,
            timeouts: InstrumentTimeouts::default(),
        }
    }
}

impl Default for SwiftInstrumentor {
    fn default() -> Self {
        Self::new()
    }
}

impl<R: CommandRunner> SwiftInstrumentor<R> {
    pub fn with_runner(runner: R) -> Self {
        SwiftInstrumentor {
            runner,
            timeouts: InstrumentTimeouts::default(),
        }
    }

    pub fn with_timeouts(mut self, timeouts: InstrumentTimeouts) -> Self {
        self.timeouts = timeouts;
        self
    }
}

/// Parse llvm-cov JSON export into branch entries.
///
/// The JSON format (from `llvm-cov export`) contains:
/// ```json
/// { "data": [{ "files": [{ "filename": "...", "segments": [[line, col, count, ...], ...] }] }] }
/// ```
pub fn parse_llvm_cov_json(
    content: &str,
    target_root: &Path,
) -> (Vec<BranchId>, Vec<BranchId>, HashMap<u64, PathBuf>) {
    let mut all_branches = Vec::new();
    let mut executed_branches = Vec::new();
    let mut file_paths: HashMap<u64, PathBuf> = HashMap::new();

    // Minimal JSON parsing using serde_json::Value
    let Ok(root) = serde_json::from_str::<serde_json::Value>(content) else {
        return (all_branches, executed_branches, file_paths);
    };

    let Some(data) = root.get("data").and_then(|d| d.as_array()) else {
        return (all_branches, executed_branches, file_paths);
    };

    for entry in data {
        let Some(files) = entry.get("files").and_then(|f| f.as_array()) else {
            continue;
        };

        for file in files {
            let Some(filename) = file.get("filename").and_then(|f| f.as_str()) else {
                continue;
            };
            let Some(segments) = file.get("segments").and_then(|s| s.as_array()) else {
                continue;
            };

            let rel_path = derive_relative_path(filename, target_root);
            let file_id = fnv1a_hash(&rel_path);
            file_paths
                .entry(file_id)
                .or_insert_with(|| PathBuf::from(&rel_path));

            for segment in segments {
                let Some(seg) = segment.as_array() else {
                    continue;
                };
                if seg.len() < 3 {
                    continue;
                }

                let Some(line) = seg[0].as_u64() else {
                    continue;
                };
                let Some(col) = seg[1].as_u64() else {
                    continue;
                };
                let Some(count) = seg[2].as_u64() else {
                    continue;
                };

                let start_line = line as u32;
                let start_col = col as u16;

                let branch_covered = BranchId::new(file_id, start_line, start_col, 0);
                let branch_uncovered = BranchId::new(file_id, start_line, start_col, 1);

                all_branches.push(branch_covered.clone());
                all_branches.push(branch_uncovered.clone());

                if count > 0 {
                    executed_branches.push(branch_covered);
                } else {
                    executed_branches.push(branch_uncovered);
                }
            }
        }
    }

    (all_branches, executed_branches, file_paths)
}

/// Derive a relative path from an absolute coverage path.
fn derive_relative_path(coverage_path: &str, target_root: &Path) -> String {
    let path = Path::new(coverage_path);
    if let Ok(rel) = path.strip_prefix(target_root) {
        return rel.to_string_lossy().to_string();
    }
    // Fallback: use basename segments
    coverage_path.to_string()
}

#[async_trait]
impl<R: CommandRunner> Instrumentor for SwiftInstrumentor<R> {
    async fn instrument(&self, target: &Target) -> Result<InstrumentedTarget> {
        let target_root = &target.root;
        info!(target = %target_root.display(), "running Swift coverage instrumentation");

        // Run: swift test --enable-code-coverage
        let spm_cache = target_root.join(".build").join("spm-cache");
        let spec = CommandSpec::new("swift", target_root)
            .args(["test", "--enable-code-coverage"])
            .env("SWIFTPM_CACHE_DIR", spm_cache.to_string_lossy())
            .timeout(self.timeouts.swift_test_ms);

        let output = self.runner.run_command(&spec).await.map_err(|e| {
            ApexError::Instrumentation(format!("swift test --enable-code-coverage: {e}"))
        })?;

        if output.exit_code != 0 {
            let stderr = String::from_utf8_lossy(&output.stderr);
            warn!(exit = output.exit_code, %stderr, "swift test --enable-code-coverage returned non-zero");
        }

        // Find the profdata and binary, then export with llvm-cov
        // swift test --show-codecov-path gives us the JSON path
        let codecov_spec = CommandSpec::new("swift", target_root)
            .args(["test", "--show-codecov-path"])
            .timeout(self.timeouts.swift_codecov_ms);
        let codecov_output = self.runner.run_command(&codecov_spec).await.map_err(|e| {
            ApexError::Instrumentation(format!("swift test --show-codecov-path: {e}"))
        })?;

        let codecov_path = String::from_utf8_lossy(&codecov_output.stdout)
            .trim()
            .to_string();

        let content = std::fs::read_to_string(&codecov_path).map_err(|e| {
            ApexError::Instrumentation(format!("failed to read {codecov_path}: {e}"))
        })?;

        let (all_branches, executed_branches, file_paths) =
            parse_llvm_cov_json(&content, target_root);

        debug!(
            total = all_branches.len(),
            executed = executed_branches.len(),
            "parsed Swift coverage"
        );

        Ok(InstrumentedTarget {
            target: target.clone(),
            branch_ids: all_branches,
            executed_branch_ids: executed_branches,
            file_paths,
            work_dir: target_root.to_path_buf(),
        })
    }

    fn branch_ids(&self) -> &[BranchId] {
        &[]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE_LLVM_COV: &str = r#"{
        "data": [{
            "files": [
                {
                    "filename": "/src/main.swift",
                    "segments": [
                        [10, 5, 3, true, true],
                        [14, 5, 0, true, true],
                        [20, 10, 1, true, true]
                    ]
                },
                {
                    "filename": "/src/helper.swift",
                    "segments": [
                        [5, 3, 0, true, true]
                    ]
                }
            ]
        }]
    }"#;

    #[test]
    fn parse_llvm_cov_basic() {
        let tmp = tempfile::tempdir().unwrap();
        let (all, executed, file_paths) = parse_llvm_cov_json(FIXTURE_LLVM_COV, tmp.path());

        // 4 segments -> 4 * 2 directions = 8 branches total
        assert_eq!(all.len(), 8);
        assert_eq!(executed.len(), 4);
        assert_eq!(file_paths.len(), 2);
    }

    #[test]
    fn parse_llvm_cov_counts_covered() {
        let tmp = tempfile::tempdir().unwrap();
        let (_, executed, _) = parse_llvm_cov_json(FIXTURE_LLVM_COV, tmp.path());

        let dirs: Vec<u8> = executed.iter().map(|b| b.direction).collect();
        // count=3 -> dir 0, count=0 -> dir 1, count=1 -> dir 0, count=0 -> dir 1
        assert_eq!(dirs, vec![0, 1, 0, 1]);
    }

    #[test]
    fn parse_llvm_cov_empty() {
        let tmp = tempfile::tempdir().unwrap();
        let (all, executed, file_paths) =
            parse_llvm_cov_json(r#"{"data": [{"files": []}]}"#, tmp.path());
        assert!(all.is_empty());
        assert!(executed.is_empty());
        assert!(file_paths.is_empty());
    }

    #[test]
    fn parse_llvm_cov_invalid_json() {
        let tmp = tempfile::tempdir().unwrap();
        let (all, executed, file_paths) = parse_llvm_cov_json("not json", tmp.path());
        assert!(all.is_empty());
        assert!(executed.is_empty());
        assert!(file_paths.is_empty());
    }

    #[test]
    fn parse_llvm_cov_line_col() {
        let json = r#"{"data": [{"files": [{"filename": "a.swift", "segments": [[42, 7, 5, true, true]]}]}]}"#;
        let tmp = tempfile::tempdir().unwrap();
        let (all, _, _) = parse_llvm_cov_json(json, tmp.path());
        assert_eq!(all.len(), 2);
        assert_eq!(all[0].line, 42);
        assert_eq!(all[0].col, 7);
    }

    #[test]
    fn derive_relative_path_strips_prefix() {
        let root = Path::new("/project");
        let result = derive_relative_path("/project/Sources/main.swift", root);
        assert_eq!(result, "Sources/main.swift");
    }

    // --- New tests targeting uncovered regions ---

    // Target: derive_relative_path — no prefix match returns original path
    #[test]
    fn derive_relative_path_no_match_returns_original() {
        let root = Path::new("/project");
        let result = derive_relative_path("/other/Sources/main.swift", root);
        assert_eq!(result, "/other/Sources/main.swift");
    }

    // Target: derive_relative_path — relative path unchanged when no prefix
    #[test]
    fn derive_relative_path_relative_unchanged() {
        let root = Path::new("/project");
        let result = derive_relative_path("Sources/main.swift", root);
        assert_eq!(result, "Sources/main.swift");
    }

    // Target: parse_llvm_cov_json — missing "data" key returns empty
    #[test]
    fn parse_llvm_cov_missing_data_key() {
        let tmp = tempfile::tempdir().unwrap();
        let (all, executed, file_paths) = parse_llvm_cov_json(r#"{"version": "2.0"}"#, tmp.path());
        assert!(all.is_empty());
        assert!(executed.is_empty());
        assert!(file_paths.is_empty());
    }

    // Target: parse_llvm_cov_json — data is not an array returns empty
    #[test]
    fn parse_llvm_cov_data_not_array() {
        let tmp = tempfile::tempdir().unwrap();
        let (all, executed, file_paths) =
            parse_llvm_cov_json(r#"{"data": "not-an-array"}"#, tmp.path());
        assert!(all.is_empty());
        assert!(executed.is_empty());
        assert!(file_paths.is_empty());
    }

    // Target: parse_llvm_cov_json — entry missing "files" key is skipped
    #[test]
    fn parse_llvm_cov_entry_missing_files_key() {
        let json = r#"{"data": [{"summary": {}}]}"#;
        let tmp = tempfile::tempdir().unwrap();
        let (all, executed, file_paths) = parse_llvm_cov_json(json, tmp.path());
        assert!(all.is_empty());
        assert!(executed.is_empty());
        assert!(file_paths.is_empty());
    }

    // Target: parse_llvm_cov_json — file entry missing "filename" is skipped
    #[test]
    fn parse_llvm_cov_file_missing_filename() {
        let json = r#"{"data": [{"files": [{"segments": [[10, 5, 1, true, true]]}]}]}"#;
        let tmp = tempfile::tempdir().unwrap();
        let (all, executed, file_paths) = parse_llvm_cov_json(json, tmp.path());
        assert!(all.is_empty());
        assert!(executed.is_empty());
        assert!(file_paths.is_empty());
    }

    // Target: parse_llvm_cov_json — file entry missing "segments" is skipped
    #[test]
    fn parse_llvm_cov_file_missing_segments() {
        let json = r#"{"data": [{"files": [{"filename": "foo.swift"}]}]}"#;
        let tmp = tempfile::tempdir().unwrap();
        let (all, executed, file_paths) = parse_llvm_cov_json(json, tmp.path());
        // file_paths may or may not be populated — but no branches
        assert!(all.is_empty());
        assert!(executed.is_empty());
    }

    // Target: parse_llvm_cov_json — segment that is not an array is skipped
    #[test]
    fn parse_llvm_cov_segment_not_array() {
        let json = r#"{"data": [{"files": [{"filename": "a.swift", "segments": ["bad", [10, 5, 1, true, true]]}]}]}"#;
        let tmp = tempfile::tempdir().unwrap();
        let (all, _, _) = parse_llvm_cov_json(json, tmp.path());
        // Only the valid segment produces branches
        assert_eq!(all.len(), 2);
    }

    // Target: parse_llvm_cov_json — segment with fewer than 3 elements is skipped
    #[test]
    fn parse_llvm_cov_segment_too_short() {
        let json = r#"{"data": [{"files": [{"filename": "a.swift", "segments": [[10, 5]]}]}]}"#;
        let tmp = tempfile::tempdir().unwrap();
        let (all, _, _) = parse_llvm_cov_json(json, tmp.path());
        assert!(all.is_empty());
    }

    // Target: parse_llvm_cov_json — segment where line is not u64 is skipped
    #[test]
    fn parse_llvm_cov_segment_line_not_u64() {
        let json = r#"{"data": [{"files": [{"filename": "a.swift", "segments": [["bad", 5, 1, true, true]]}]}]}"#;
        let tmp = tempfile::tempdir().unwrap();
        let (all, _, _) = parse_llvm_cov_json(json, tmp.path());
        assert!(all.is_empty());
    }

    // Target: parse_llvm_cov_json — segment where col is not u64 is skipped
    #[test]
    fn parse_llvm_cov_segment_col_not_u64() {
        let json = r#"{"data": [{"files": [{"filename": "a.swift", "segments": [[10, "bad", 1, true, true]]}]}]}"#;
        let tmp = tempfile::tempdir().unwrap();
        let (all, _, _) = parse_llvm_cov_json(json, tmp.path());
        assert!(all.is_empty());
    }

    // Target: parse_llvm_cov_json — segment where count is not u64 is skipped
    #[test]
    fn parse_llvm_cov_segment_count_not_u64() {
        let json = r#"{"data": [{"files": [{"filename": "a.swift", "segments": [[10, 5, "bad", true, true]]}]}]}"#;
        let tmp = tempfile::tempdir().unwrap();
        let (all, _, _) = parse_llvm_cov_json(json, tmp.path());
        assert!(all.is_empty());
    }

    // Target: parse_llvm_cov_json — count=0 produces direction=1 in executed
    #[test]
    fn parse_llvm_cov_zero_count_direction_one() {
        let json = r#"{"data": [{"files": [{"filename": "a.swift", "segments": [[1, 1, 0, true, true]]}]}]}"#;
        let tmp = tempfile::tempdir().unwrap();
        let (_, executed, _) = parse_llvm_cov_json(json, tmp.path());
        assert_eq!(executed.len(), 1);
        assert_eq!(executed[0].direction, 1);
    }

    // Target: parse_llvm_cov_json — multiple data entries both processed
    #[test]
    fn parse_llvm_cov_multiple_data_entries() {
        let json = r#"{"data": [
            {"files": [{"filename": "a.swift", "segments": [[1, 1, 1, true, true]]}]},
            {"files": [{"filename": "b.swift", "segments": [[2, 2, 0, true, true]]}]}
        ]}"#;
        let tmp = tempfile::tempdir().unwrap();
        let (all, _, file_paths) = parse_llvm_cov_json(json, tmp.path());
        assert_eq!(all.len(), 4);
        assert_eq!(file_paths.len(), 2);
    }

    // Target: parse_llvm_cov_json — duplicate filename in same run shares file_id
    #[test]
    fn parse_llvm_cov_duplicate_filename_same_file_id() {
        let json = r#"{"data": [{"files": [
            {"filename": "/src/a.swift", "segments": [[1, 1, 1, true, true]]},
            {"filename": "/src/a.swift", "segments": [[2, 1, 0, true, true]]}
        ]}]}"#;
        let root = Path::new("/src");
        let (all, _, file_paths) = parse_llvm_cov_json(json, root);
        assert_eq!(all[0].file_id, all[2].file_id);
        // entry().or_insert_with() means only one entry in file_paths
        assert_eq!(file_paths.len(), 1);
    }

    // Target: parse_llvm_cov_json — unicode filename
    #[test]
    fn parse_llvm_cov_unicode_filename() {
        let json = "{\"data\": [{\"files\": [{\"filename\": \"/src/\u{6587}\u{4ef6}.swift\", \"segments\": [[1, 1, 2, true, true]]}]}]}";
        let root = Path::new("/src");
        let (all, _, file_paths) = parse_llvm_cov_json(json, root);
        assert_eq!(all.len(), 2);
        assert_eq!(file_paths.len(), 1);
    }

    // Target: parse_llvm_cov_json — empty segments array produces no branches
    #[test]
    fn parse_llvm_cov_empty_segments() {
        let json = r#"{"data": [{"files": [{"filename": "a.swift", "segments": []}]}]}"#;
        let tmp = tempfile::tempdir().unwrap();
        let (all, executed, _) = parse_llvm_cov_json(json, tmp.path());
        assert!(all.is_empty());
        assert!(executed.is_empty());
    }
}
