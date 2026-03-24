use apex_core::{
    command::{CommandRunner, CommandSpec, RealCommandRunner},
    config::InstrumentTimeouts,
    error::{ApexError, Result},
    traits::Instrumentor,
    types::{BranchId, InstrumentedTarget, Target},
};
use async_trait::async_trait;
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};
use tracing::{debug, info, warn};

use crate::llvm_coverage::{parse_llvm_cov_export, FileFilter};

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
/// Delegates to the unified [`crate::llvm_coverage::parse_llvm_cov_export`] parser.
/// This fixes the previous dual-direction bug where every segment produced 2 BranchIds
/// (direction=0 and direction=1), inflating branch counts by 2x.
pub fn parse_llvm_cov_json(
    content: &str,
    target_root: &Path,
) -> (Vec<BranchId>, Vec<BranchId>, HashMap<u64, PathBuf>) {
    let filter = FileFilter {
        require_under_root: false,
        skip_test_files: false,
    };
    match parse_llvm_cov_export(content.as_bytes(), target_root, &filter) {
        Ok(result) => (
            result.branch_ids,
            result.executed_branch_ids,
            result.file_paths,
        ),
        Err(e) => {
            warn!("failed to parse llvm-cov JSON: {e}");
            (Vec::new(), Vec::new(), HashMap::new())
        }
    }
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
            return Err(ApexError::Instrumentation(format!(
                "swift test --enable-code-coverage failed (exit {}): {}",
                output.exit_code, stderr
            )));
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

        if codecov_path.is_empty() || !Path::new(&codecov_path).exists() {
            return Err(ApexError::Instrumentation(format!(
                "codecov JSON file does not exist: '{}'; \
                 swift test --show-codecov-path returned a path that could not be found",
                codecov_path
            )));
        }

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

    // Updated fixture: 6-field segments (with is_gap=false) matching the unified parser's
    // requirements. The old fixture used 5-field segments which the unified parser correctly
    // skips (requires has_count, is_region_entry, AND is_gap fields).
    const FIXTURE_LLVM_COV: &str = r#"{
        "data": [{
            "files": [
                {
                    "filename": "/src/main.swift",
                    "segments": [
                        [10, 5, 3, true, true, false],
                        [14, 5, 0, true, true, false],
                        [20, 10, 1, true, true, false]
                    ]
                },
                {
                    "filename": "/src/helper.swift",
                    "segments": [
                        [5, 3, 0, true, true, false]
                    ]
                }
            ]
        }]
    }"#;

    #[test]
    fn parse_llvm_cov_basic() {
        let tmp = tempfile::tempdir().unwrap();
        let (all, executed, file_paths) = parse_llvm_cov_json(FIXTURE_LLVM_COV, tmp.path());

        // 4 segments -> 4 branches (1 per segment, direction=0 only)
        // Previously was 8 due to dual-direction bug
        assert_eq!(all.len(), 4);
        // 2 segments have count > 0 (count=3, count=1)
        // Previously was 4 because the old parser also "executed" direction=1 for count=0
        assert_eq!(executed.len(), 2);
        assert_eq!(file_paths.len(), 2);
    }

    #[test]
    fn parse_llvm_cov_counts_covered() {
        let tmp = tempfile::tempdir().unwrap();
        let (_, executed, _) = parse_llvm_cov_json(FIXTURE_LLVM_COV, tmp.path());

        // All executed branches have direction=0 (unified parser uses direction=0 only)
        for b in &executed {
            assert_eq!(b.direction, 0, "all branches should have direction=0");
        }
        // count=3 -> executed, count=0 -> not executed, count=1 -> executed
        assert_eq!(executed.len(), 2);
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
        let json = r#"{"data": [{"files": [{"filename": "a.swift", "segments": [[42, 7, 5, true, true, false]]}]}]}"#;
        let tmp = tempfile::tempdir().unwrap();
        let (all, _, _) = parse_llvm_cov_json(json, tmp.path());
        // 1 branch per segment (was 2 with old dual-direction parser)
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].line, 42);
        assert_eq!(all[0].col, 7);
        assert_eq!(all[0].direction, 0);
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

    // Target: parse_llvm_cov_json — entry missing "files" key returns empty
    // (unified parser returns Err, wrapper maps to empty)
    #[test]
    fn parse_llvm_cov_entry_missing_files_key() {
        let json = r#"{"data": [{"summary": {}}]}"#;
        let tmp = tempfile::tempdir().unwrap();
        let (all, executed, file_paths) = parse_llvm_cov_json(json, tmp.path());
        assert!(all.is_empty());
        assert!(executed.is_empty());
        assert!(file_paths.is_empty());
    }

    // Target: parse_llvm_cov_json — file entry missing "filename" returns empty
    #[test]
    fn parse_llvm_cov_file_missing_filename() {
        let json = r#"{"data": [{"files": [{"segments": [[10, 5, 1, true, true, false]]}]}]}"#;
        let tmp = tempfile::tempdir().unwrap();
        let (all, executed, file_paths) = parse_llvm_cov_json(json, tmp.path());
        assert!(all.is_empty());
        assert!(executed.is_empty());
        assert!(file_paths.is_empty());
    }

    // Target: parse_llvm_cov_json — file entry missing "segments" returns empty
    #[test]
    fn parse_llvm_cov_file_missing_segments() {
        let json = r#"{"data": [{"files": [{"filename": "foo.swift"}]}]}"#;
        let tmp = tempfile::tempdir().unwrap();
        let (all, executed, _file_paths) = parse_llvm_cov_json(json, tmp.path());
        assert!(all.is_empty());
        assert!(executed.is_empty());
    }

    // Target: parse_llvm_cov_json — segment that is not an array causes parse error
    // The unified parser returns Err (stricter than the old parser which skipped),
    // and the wrapper maps Err to empty results.
    #[test]
    fn parse_llvm_cov_segment_not_array() {
        let json = r#"{"data": [{"files": [{"filename": "a.swift", "segments": ["bad", [10, 5, 1, true, true, false]]}]}]}"#;
        let tmp = tempfile::tempdir().unwrap();
        let (all, _, _) = parse_llvm_cov_json(json, tmp.path());
        // Unified parser errors on non-array segment, wrapper returns empty
        assert!(all.is_empty());
    }

    // Target: parse_llvm_cov_json — segment with fewer than 6 elements is skipped
    #[test]
    fn parse_llvm_cov_segment_too_short() {
        let json = r#"{"data": [{"files": [{"filename": "a.swift", "segments": [[10, 5]]}]}]}"#;
        let tmp = tempfile::tempdir().unwrap();
        let (all, _, _) = parse_llvm_cov_json(json, tmp.path());
        assert!(all.is_empty());
    }

    // Target: 5-field segments are now skipped (require 6 fields)
    #[test]
    fn parse_llvm_cov_five_field_segment_skipped() {
        let json = r#"{"data": [{"files": [{"filename": "a.swift", "segments": [[10, 5, 1, true, true]]}]}]}"#;
        let tmp = tempfile::tempdir().unwrap();
        let (all, _, _) = parse_llvm_cov_json(json, tmp.path());
        // Unified parser requires 6 fields — 5-field segments are skipped
        assert!(all.is_empty());
    }

    // Target: count=0 is NOT executed (no direction=1 trick)
    #[test]
    fn parse_llvm_cov_zero_count_not_executed() {
        let json = r#"{"data": [{"files": [{"filename": "a.swift", "segments": [[1, 1, 0, true, true, false]]}]}]}"#;
        let tmp = tempfile::tempdir().unwrap();
        let (all, executed, _) = parse_llvm_cov_json(json, tmp.path());
        // 1 branch total (direction=0), 0 executed (count=0 means not executed)
        assert_eq!(all.len(), 1);
        assert_eq!(executed.len(), 0);
    }

    // Target: parse_llvm_cov_json — multiple data entries both processed
    #[test]
    fn parse_llvm_cov_multiple_data_entries() {
        let json = r#"{"data": [
            {"files": [{"filename": "a.swift", "segments": [[1, 1, 1, true, true, false]]}]},
            {"files": [{"filename": "b.swift", "segments": [[2, 2, 0, true, true, false]]}]}
        ]}"#;
        let tmp = tempfile::tempdir().unwrap();
        let (all, _, file_paths) = parse_llvm_cov_json(json, tmp.path());
        // 2 branches (1 per segment, was 4 with dual-direction)
        assert_eq!(all.len(), 2);
        assert_eq!(file_paths.len(), 2);
    }

    // Target: parse_llvm_cov_json — duplicate filename in same run shares file_id
    #[test]
    fn parse_llvm_cov_duplicate_filename_same_file_id() {
        let json = r#"{"data": [{"files": [
            {"filename": "/src/a.swift", "segments": [[1, 1, 1, true, true, false]]},
            {"filename": "/src/a.swift", "segments": [[2, 1, 0, true, true, false]]}
        ]}]}"#;
        let root = Path::new("/src");
        let (all, _, file_paths) = parse_llvm_cov_json(json, root);
        assert_eq!(all[0].file_id, all[1].file_id);
        assert_eq!(file_paths.len(), 1);
    }

    // Target: parse_llvm_cov_json — unicode filename
    #[test]
    fn parse_llvm_cov_unicode_filename() {
        let json = "{\"data\": [{\"files\": [{\"filename\": \"/src/\u{6587}\u{4ef6}.swift\", \"segments\": [[1, 1, 2, true, true, false]]}]}]}";
        let root = Path::new("/src");
        let (all, _, file_paths) = parse_llvm_cov_json(json, root);
        // 1 branch (was 2 with dual-direction)
        assert_eq!(all.len(), 1);
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

    // Target: integer booleans (LLVM version compat)
    #[test]
    fn parse_llvm_cov_integer_booleans() {
        let json = r#"{"data": [{"files": [{"filename": "a.swift", "segments": [[10, 5, 3, 1, 1, 0]]}]}]}"#;
        let tmp = tempfile::tempdir().unwrap();
        let (all, executed, _) = parse_llvm_cov_json(json, tmp.path());
        assert_eq!(all.len(), 1);
        assert_eq!(executed.len(), 1);
    }

    // Target: gap regions are skipped
    #[test]
    fn parse_llvm_cov_gap_region_skipped() {
        let json = r#"{"data": [{"files": [{"filename": "a.swift", "segments": [
            [1, 1, 5, true, true, true],
            [2, 1, 5, true, true, false]
        ]}]}]}"#;
        let tmp = tempfile::tempdir().unwrap();
        let (all, _, _) = parse_llvm_cov_json(json, tmp.path());
        // Only the non-gap segment
        assert_eq!(all.len(), 1);
    }
}
