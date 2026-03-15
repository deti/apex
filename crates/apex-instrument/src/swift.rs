use apex_core::{
    command::{CommandRunner, CommandSpec, RealCommandRunner},
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
}

impl SwiftInstrumentor {
    pub fn new() -> Self {
        SwiftInstrumentor {
            runner: RealCommandRunner,
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
        SwiftInstrumentor { runner }
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
        let spec = CommandSpec::new("swift", target_root)
            .args(["test", "--enable-code-coverage"]);

        let output = self
            .runner
            .run_command(&spec)
            .await
            .map_err(|e| ApexError::Instrumentation(format!("swift test --enable-code-coverage: {e}")))?;

        if output.exit_code != 0 {
            let stderr = String::from_utf8_lossy(&output.stderr);
            warn!(exit = output.exit_code, %stderr, "swift test --enable-code-coverage returned non-zero");
        }

        // Find the profdata and binary, then export with llvm-cov
        // swift test --show-codecov-path gives us the JSON path
        let codecov_spec = CommandSpec::new("swift", target_root)
            .args(["test", "--show-codecov-path"]);
        let codecov_output = self
            .runner
            .run_command(&codecov_spec)
            .await
            .map_err(|e| ApexError::Instrumentation(format!("swift test --show-codecov-path: {e}")))?;

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
}
