/// Source-code context extraction helpers shared by the orchestrator and CLI.
use apex_core::types::{BranchId, SourceContext, UncoveredBranch};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};
use tracing::warn;

/// Lines of context shown around each uncovered cluster.
const WINDOW: u32 = 15;
/// Maximum files shown per round (keeps agent prompts focused).
pub const MAX_FILES_PER_ROUND: usize = 3;

/// Build `SourceContext` slices for the files with the most uncovered branches.
/// The agent uses these as the seed prompt for each round.
pub fn extract_source_contexts(
    uncovered: &[BranchId],
    file_paths: &HashMap<u64, PathBuf>,
    target_root: &Path,
) -> Vec<SourceContext> {
    let mut by_file: HashMap<u64, Vec<u32>> = HashMap::new();
    for b in uncovered {
        by_file.entry(b.file_id).or_default().push(b.line);
    }
    // Sort: most-uncovered files first.
    let mut files: Vec<(u64, Vec<u32>)> = by_file.into_iter().collect();
    files.sort_by(|a, b| b.1.len().cmp(&a.1.len()));

    let mut contexts = Vec::new();
    for (file_id, mut lines) in files.into_iter().take(MAX_FILES_PER_ROUND) {
        let Some(rel_path) = file_paths.get(&file_id) else {
            continue;
        };
        let abs = target_root.join(rel_path);
        let Ok(source) = std::fs::read_to_string(&abs) else {
            warn!(path = %abs.display(), "could not read source file for context");
            continue;
        };
        let source_lines: Vec<String> = source.lines().map(String::from).collect();
        let total = source_lines.len() as u32;
        lines.sort_unstable();
        let min_line = lines[0].saturating_sub(WINDOW).max(1);
        let max_line = (lines[lines.len() - 1] + WINDOW).min(total);
        let slice = source_lines[(min_line - 1) as usize..max_line as usize].to_vec();
        contexts.push(SourceContext {
            file_path: rel_path.clone(),
            lines: slice,
            start_line: min_line,
        });
    }
    contexts
}

/// Annotate uncovered BranchIds with their source file path and source line text.
pub fn build_uncovered_with_lines(
    uncovered: &[BranchId],
    file_paths: &HashMap<u64, PathBuf>,
    target_root: &Path,
) -> Vec<UncoveredBranch> {
    let mut file_cache: HashMap<u64, Vec<String>> = HashMap::new();
    uncovered
        .iter()
        .map(|b| {
            let source_line = file_paths.get(&b.file_id).and_then(|rel| {
                let lines = file_cache.entry(b.file_id).or_insert_with(|| {
                    std::fs::read_to_string(target_root.join(rel))
                        .map(|s| s.lines().map(String::from).collect())
                        .unwrap_or_default()
                });
                lines.get(b.line.saturating_sub(1) as usize).cloned()
            });
            UncoveredBranch {
                branch: b.clone(),
                file_path: file_paths
                    .get(&b.file_id)
                    .cloned()
                    .unwrap_or_else(|| PathBuf::from(format!("{:016x}", b.file_id))),
                source_line,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper: write a file under `root` and return its repo-relative path.
    fn write_file(root: &Path, rel: &str, content: &str) -> PathBuf {
        let abs = root.join(rel);
        if let Some(parent) = abs.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(&abs, content).unwrap();
        PathBuf::from(rel)
    }

    fn make_branch(file_id: u64, line: u32) -> BranchId {
        BranchId::new(file_id, line, 0, 0)
    }

    // ------------------------------------------------------------------
    // extract_source_contexts
    // ------------------------------------------------------------------

    #[test]
    fn extract_contexts_returns_correct_file_paths() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let rel = write_file(root, "src/foo.py", "line1\nline2\nline3\n");

        let mut file_paths = HashMap::new();
        file_paths.insert(1u64, rel.clone());

        let uncovered = vec![make_branch(1, 2)];
        let ctxs = extract_source_contexts(&uncovered, &file_paths, root);

        assert_eq!(ctxs.len(), 1);
        assert_eq!(ctxs[0].file_path, rel);
    }

    #[test]
    fn extract_contexts_includes_surrounding_lines() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        // 30 lines, uncovered branch at line 20
        let content: String = (1..=30).map(|i| format!("line {i}\n")).collect();
        let rel = write_file(root, "lib.py", &content);

        let mut file_paths = HashMap::new();
        file_paths.insert(1u64, rel);

        let uncovered = vec![make_branch(1, 20)];
        let ctxs = extract_source_contexts(&uncovered, &file_paths, root);

        assert_eq!(ctxs.len(), 1);
        let ctx = &ctxs[0];
        // Window is 15, so start_line = max(1, 20-15) = 5
        assert_eq!(ctx.start_line, 5);
        // Lines slice should contain from line 5 to min(30, 20+15)=30 → 26 lines
        assert_eq!(ctx.lines.len(), 26);
        assert_eq!(ctx.lines[0], "line 5");
        assert_eq!(ctx.lines[15], "line 20"); // the uncovered line
        assert_eq!(*ctx.lines.last().unwrap(), "line 30");
    }

    #[test]
    fn extract_contexts_sorted_most_uncovered_first() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let rel_a = write_file(root, "a.py", "a1\na2\na3\n");
        let rel_b = write_file(root, "b.py", "b1\nb2\nb3\n");

        let mut file_paths = HashMap::new();
        file_paths.insert(1u64, rel_a.clone());
        file_paths.insert(2u64, rel_b.clone());

        // File 2 has more uncovered branches (2 vs 1).
        let uncovered = vec![make_branch(1, 1), make_branch(2, 1), make_branch(2, 2)];
        let ctxs = extract_source_contexts(&uncovered, &file_paths, root);

        assert_eq!(ctxs.len(), 2);
        // Most-uncovered first → file_id 2 (b.py) should come first.
        assert_eq!(ctxs[0].file_path, rel_b);
        assert_eq!(ctxs[1].file_path, rel_a);
    }

    #[test]
    fn extract_contexts_limits_to_max_files() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();

        let mut file_paths = HashMap::new();
        // Create more files than MAX_FILES_PER_ROUND.
        let mut uncovered = Vec::new();
        for i in 0..5u64 {
            let rel = write_file(root, &format!("f{i}.py"), "x\n");
            file_paths.insert(i, rel);
            uncovered.push(make_branch(i, 1));
        }

        let ctxs = extract_source_contexts(&uncovered, &file_paths, root);
        assert_eq!(ctxs.len(), MAX_FILES_PER_ROUND);
    }

    #[test]
    fn extract_contexts_skips_missing_files() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();

        let mut file_paths = HashMap::new();
        file_paths.insert(1u64, PathBuf::from("does_not_exist.py"));

        let uncovered = vec![make_branch(1, 1)];
        let ctxs = extract_source_contexts(&uncovered, &file_paths, root);
        assert!(ctxs.is_empty());
    }

    // ------------------------------------------------------------------
    // build_uncovered_with_lines
    // ------------------------------------------------------------------

    #[test]
    fn build_uncovered_correct_file_path_and_source_line() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let rel = write_file(root, "mod.py", "def foo():\n    return 42\n");

        let mut file_paths = HashMap::new();
        file_paths.insert(1u64, rel.clone());

        let uncovered = vec![make_branch(1, 2)];
        let result = build_uncovered_with_lines(&uncovered, &file_paths, root);

        assert_eq!(result.len(), 1);
        assert_eq!(result[0].file_path, rel);
        assert_eq!(result[0].source_line.as_deref(), Some("    return 42"));
        assert_eq!(result[0].branch.line, 2);
    }

    #[test]
    fn build_uncovered_missing_file_produces_none_source_line() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();

        let mut file_paths = HashMap::new();
        file_paths.insert(99u64, PathBuf::from("ghost.py"));

        let uncovered = vec![make_branch(99, 5)];
        let result = build_uncovered_with_lines(&uncovered, &file_paths, root);

        assert_eq!(result.len(), 1);
        assert!(result[0].source_line.is_none());
        assert_eq!(result[0].file_path, PathBuf::from("ghost.py"));
    }

    #[test]
    fn build_uncovered_unknown_file_id_uses_hex_path() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();

        let file_paths: HashMap<u64, PathBuf> = HashMap::new();
        let uncovered = vec![make_branch(0xDEAD, 1)];
        let result = build_uncovered_with_lines(&uncovered, &file_paths, root);

        assert_eq!(result.len(), 1);
        assert_eq!(result[0].file_path, PathBuf::from("000000000000dead"));
        assert!(result[0].source_line.is_none());
    }

    #[test]
    fn build_uncovered_line_out_of_range_returns_none() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let rel = write_file(root, "short.py", "only one line\n");

        let mut file_paths = HashMap::new();
        file_paths.insert(1u64, rel);

        // Line 100 does not exist in a 1-line file.
        let uncovered = vec![make_branch(1, 100)];
        let result = build_uncovered_with_lines(&uncovered, &file_paths, root);

        assert_eq!(result.len(), 1);
        assert!(result[0].source_line.is_none());
    }

    // ------------------------------------------------------------------
    // Additional branch-coverage tests
    // ------------------------------------------------------------------

    /// `extract_source_contexts` with an empty uncovered slice returns empty.
    #[test]
    fn extract_contexts_empty_uncovered_returns_empty() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let file_paths: HashMap<u64, PathBuf> = HashMap::new();
        let ctxs = extract_source_contexts(&[], &file_paths, root);
        assert!(ctxs.is_empty());
    }

    /// `build_uncovered_with_lines` with an empty uncovered slice returns empty.
    #[test]
    fn build_uncovered_empty_returns_empty() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let file_paths: HashMap<u64, PathBuf> = HashMap::new();
        let result = build_uncovered_with_lines(&[], &file_paths, root);
        assert!(result.is_empty());
    }

    /// Branch at line 0 (or line < WINDOW) — saturating_sub prevents underflow,
    /// so start_line is clamped to 1.
    #[test]
    fn extract_contexts_branch_near_start_of_file() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let content: String = (1..=5).map(|i| format!("line {i}\n")).collect();
        let rel = write_file(root, "small.py", &content);
        let mut file_paths = HashMap::new();
        file_paths.insert(1u64, rel.clone());

        // Branch at line 2 — window of 15 would go below 1, saturating to 1.
        let uncovered = vec![make_branch(1, 2)];
        let ctxs = extract_source_contexts(&uncovered, &file_paths, root);
        assert_eq!(ctxs.len(), 1);
        assert_eq!(ctxs[0].start_line, 1); // saturating_sub(15) → 0 → max(1) = 1
    }

    /// Branch at the last line — window + line would exceed total, clamped to total.
    #[test]
    fn extract_contexts_branch_near_end_of_file() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let content: String = (1..=5).map(|i| format!("line {i}\n")).collect();
        let rel = write_file(root, "tail.py", &content);
        let mut file_paths = HashMap::new();
        file_paths.insert(1u64, rel);

        // Branch at line 5 — line + WINDOW(15) = 20 > total(5), clamped to 5.
        let uncovered = vec![make_branch(1, 5)];
        let ctxs = extract_source_contexts(&uncovered, &file_paths, root);
        assert_eq!(ctxs.len(), 1);
        // slice goes from max(1, 5-15) = 1 to min(5, 5+15) = 5 → 5 lines.
        assert_eq!(ctxs[0].lines.len(), 5);
    }

    /// `extract_source_contexts` skips a file_id that has no entry in file_paths.
    #[test]
    fn extract_contexts_skips_unknown_file_id() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        // file_paths has no entry for file_id 99.
        let file_paths: HashMap<u64, PathBuf> = HashMap::new();
        let uncovered = vec![make_branch(99, 5)];
        let ctxs = extract_source_contexts(&uncovered, &file_paths, root);
        assert!(ctxs.is_empty());
    }

    /// Multiple branches in the same file collapse into one SourceContext.
    #[test]
    fn extract_contexts_multiple_branches_same_file() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let content: String = (1..=50).map(|i| format!("line {i}\n")).collect();
        let rel = write_file(root, "multi.py", &content);
        let mut file_paths = HashMap::new();
        file_paths.insert(1u64, rel);

        let uncovered = vec![
            make_branch(1, 10),
            make_branch(1, 20),
            make_branch(1, 30),
        ];
        let ctxs = extract_source_contexts(&uncovered, &file_paths, root);
        // All branches in the same file → one SourceContext.
        assert_eq!(ctxs.len(), 1);
    }

    /// `build_uncovered_with_lines` uses the file cache for repeated accesses
    /// to the same file_id (exercises the `or_insert_with` path on hit vs miss).
    #[test]
    fn build_uncovered_file_cache_reuse() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let rel = write_file(root, "cached.py", "first\nsecond\nthird\n");
        let mut file_paths = HashMap::new();
        file_paths.insert(1u64, rel.clone());

        // Two branches from the same file — second access should use the cache.
        let uncovered = vec![make_branch(1, 1), make_branch(1, 3)];
        let result = build_uncovered_with_lines(&uncovered, &file_paths, root);

        assert_eq!(result.len(), 2);
        assert_eq!(result[0].source_line.as_deref(), Some("first"));
        assert_eq!(result[1].source_line.as_deref(), Some("third"));
    }

    /// `build_uncovered_with_lines` with line = 0 (saturating_sub(1) = 0, valid index).
    #[test]
    fn build_uncovered_line_zero_saturates_to_index_zero() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let rel = write_file(root, "zero.py", "first line\nsecond line\n");
        let mut file_paths = HashMap::new();
        file_paths.insert(1u64, rel);

        // line=0 → saturating_sub(1) = 0 → lines.get(0) = Some("first line")
        let uncovered = vec![make_branch(1, 0)];
        let result = build_uncovered_with_lines(&uncovered, &file_paths, root);
        assert_eq!(result.len(), 1);
        // line 0 with saturating_sub(1) → index 0 → "first line"
        assert_eq!(result[0].source_line.as_deref(), Some("first line"));
    }

    /// MAX_FILES_PER_ROUND constant is accessible and equals 3.
    #[test]
    fn max_files_per_round_constant() {
        assert_eq!(MAX_FILES_PER_ROUND, 3);
    }

    /// When two files have the same uncovered branch count, both appear in result.
    #[test]
    fn extract_contexts_tie_in_uncovered_count() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let rel_a = write_file(root, "tie_a.py", "a1\na2\n");
        let rel_b = write_file(root, "tie_b.py", "b1\nb2\n");
        let mut file_paths = HashMap::new();
        file_paths.insert(1u64, rel_a);
        file_paths.insert(2u64, rel_b);

        // Each file has exactly 1 uncovered branch.
        let uncovered = vec![make_branch(1, 1), make_branch(2, 1)];
        let ctxs = extract_source_contexts(&uncovered, &file_paths, root);
        assert_eq!(ctxs.len(), 2);
    }
}
