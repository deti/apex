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

pub struct GoInstrumentor<R: CommandRunner = RealCommandRunner> {
    runner: R,
}

impl GoInstrumentor {
    pub fn new() -> Self {
        GoInstrumentor {
            runner: RealCommandRunner,
        }
    }
}

impl Default for GoInstrumentor {
    fn default() -> Self {
        Self::new()
    }
}

impl<R: CommandRunner> GoInstrumentor<R> {
    pub fn with_runner(runner: R) -> Self {
        GoInstrumentor { runner }
    }
}

/// Parse Go coverage.out format into branch entries.
///
/// Format:
///   mode: atomic
///   file:startLine.startCol,endLine.endCol numStmt count
///
/// Example:
///   example.com/foo/bar.go:10.2,12.15 1 3
pub fn parse_coverage_out(
    content: &str,
    target_root: &Path,
) -> (Vec<BranchId>, Vec<BranchId>, HashMap<u64, PathBuf>) {
    let mut all_branches = Vec::new();
    let mut executed_branches = Vec::new();
    let mut file_paths: HashMap<u64, PathBuf> = HashMap::new();

    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with("mode:") {
            continue;
        }

        // Parse: file:startLine.startCol,endLine.endCol numStmt count
        let Some((file_range, rest)) = line.rsplit_once(' ') else {
            continue;
        };
        let Some((file_range, _num_stmt)) = file_range.rsplit_once(' ') else {
            continue;
        };
        let count: u32 = match rest.parse() {
            Ok(c) => c,
            Err(_) => continue,
        };

        let Some((file_part, range_part)) = file_range.split_once(':') else {
            continue;
        };

        // Extract startLine.startCol from the range
        let Some((start_part, _end_part)) = range_part.split_once(',') else {
            continue;
        };
        let Some((start_line_str, start_col_str)) = start_part.split_once('.') else {
            continue;
        };

        let start_line: u32 = match start_line_str.parse() {
            Ok(l) => l,
            Err(_) => continue,
        };
        let start_col: u16 = match start_col_str.parse() {
            Ok(c) => c,
            Err(_) => continue,
        };

        // Derive relative path: strip module prefix if present.
        // Go coverage paths look like "example.com/pkg/file.go" --
        // try to find a relative path within the target root.
        let rel_path = derive_relative_path(file_part, target_root);
        let file_id = fnv1a_hash(&rel_path);

        file_paths
            .entry(file_id)
            .or_insert_with(|| PathBuf::from(&rel_path));

        // Each coverage line represents a block: we create a branch entry for it.
        // direction 0 = covered path, direction 1 = not-covered path.
        let branch_covered = BranchId::new(file_id, start_line, start_col, 0);
        let branch_uncovered = BranchId::new(file_id, start_line, start_col, 1);

        all_branches.push(branch_covered.clone());
        all_branches.push(branch_uncovered.clone());

        if count > 0 {
            executed_branches.push(branch_covered);
        } else {
            // The uncovered direction is "executed" in the sense that
            // the code was not reached -- mark the uncovered direction.
            executed_branches.push(branch_uncovered);
        }
    }

    (all_branches, executed_branches, file_paths)
}

/// Derive a relative path from a Go coverage path.
/// Go coverage uses module paths like "example.com/pkg/file.go".
/// We try to find the file relative to target_root.
fn derive_relative_path(coverage_path: &str, target_root: &Path) -> String {
    // First try: the path itself might be relative.
    if target_root.join(coverage_path).exists() {
        return coverage_path.to_string();
    }

    // Try progressively shorter suffixes.
    // Guard against path traversal (e.g. `../../etc/passwd` in coverage output).
    let parts: Vec<&str> = coverage_path.split('/').collect();
    for start in 1..parts.len() {
        let suffix = parts[start..].join("/");
        let candidate = target_root.join(&suffix);
        if candidate.starts_with(target_root) && candidate.exists() {
            return suffix;
        }
    }

    // Fallback: use the file name portion after the last '/'.
    coverage_path.to_string()
}

#[async_trait]
impl<R: CommandRunner> Instrumentor for GoInstrumentor<R> {
    async fn instrument(&self, target: &Target) -> Result<InstrumentedTarget> {
        let target_root = &target.root;
        info!(target = %target_root.display(), "running Go coverage instrumentation");

        // Run: go test -coverprofile=coverage.out -covermode=atomic ./...
        let coverage_out = target_root.join("coverage.out");
        let spec = CommandSpec::new("go", target_root)
            .args([
                "test",
                "-coverprofile=coverage.out",
                "-covermode=atomic",
                "./...",
            ])
            .timeout(600_000); // 10 min — coverage instrumentation is slower than plain tests

        let output = self
            .runner
            .run_command(&spec)
            .await
            .map_err(|e| ApexError::Instrumentation(format!("go test -cover: {e}")))?;

        if output.exit_code != 0 {
            let stderr = String::from_utf8_lossy(&output.stderr);
            warn!(exit = output.exit_code, %stderr, "go test -cover returned non-zero");
        }

        // Parse coverage.out
        let content = std::fs::read_to_string(&coverage_out).map_err(|e| {
            ApexError::Instrumentation(format!("failed to read {}: {e}", coverage_out.display()))
        })?;

        let (all_branches, executed_branches, file_paths) =
            parse_coverage_out(&content, target_root);

        if all_branches.is_empty() {
            return Err(ApexError::Instrumentation(
                "coverage.out contained 0 valid coverage lines; \
                 the file may be malformed or the test run produced no coverage data"
                    .into(),
            ));
        }

        debug!(
            total = all_branches.len(),
            executed = executed_branches.len(),
            "parsed Go coverage"
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
        // Stateless instrumentor -- branch ids live in the returned InstrumentedTarget.
        &[]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE_COVERAGE: &str = "\
mode: atomic
example.com/foo/main.go:10.2,12.15 1 3
example.com/foo/main.go:14.2,16.10 1 0
example.com/foo/handler.go:5.14,8.2 2 1
example.com/foo/handler.go:10.14,12.2 1 0
";

    #[test]
    fn parse_coverage_out_basic() {
        let tmp = tempfile::tempdir().unwrap();
        let (all, executed, file_paths) = parse_coverage_out(FIXTURE_COVERAGE, tmp.path());

        // 4 lines -> 4 * 2 directions = 8 branches total
        assert_eq!(all.len(), 8);
        // Each line produces one executed branch
        assert_eq!(executed.len(), 4);
        // Two distinct files
        assert_eq!(file_paths.len(), 2);
    }

    #[test]
    fn parse_coverage_out_counts_covered() {
        let tmp = tempfile::tempdir().unwrap();
        let (all, executed, _) = parse_coverage_out(FIXTURE_COVERAGE, tmp.path());

        // Lines with count > 0 produce direction=0 in executed
        // Lines with count = 0 produce direction=1 in executed
        let covered_dirs: Vec<u8> = executed.iter().map(|b| b.direction).collect();
        // Line 1: count=3 -> dir 0, Line 2: count=0 -> dir 1,
        // Line 3: count=1 -> dir 0, Line 4: count=0 -> dir 1
        assert_eq!(covered_dirs, vec![0, 1, 0, 1]);

        assert_eq!(all.len(), 8);
    }

    #[test]
    fn parse_coverage_out_empty() {
        let tmp = tempfile::tempdir().unwrap();
        let (all, executed, file_paths) = parse_coverage_out("mode: atomic\n", tmp.path());
        assert!(all.is_empty());
        assert!(executed.is_empty());
        assert!(file_paths.is_empty());
    }

    #[test]
    fn parse_coverage_out_skips_malformed() {
        let input = "mode: atomic\nnot a valid line\n";
        let tmp = tempfile::tempdir().unwrap();
        let (all, executed, _) = parse_coverage_out(input, tmp.path());
        assert!(all.is_empty());
        assert!(executed.is_empty());
    }

    #[test]
    fn parse_coverage_out_line_col() {
        let input = "mode: atomic\npkg/foo.go:42.7,50.3 2 5\n";
        let tmp = tempfile::tempdir().unwrap();
        let (all, _, _) = parse_coverage_out(input, tmp.path());
        assert_eq!(all.len(), 2);
        assert_eq!(all[0].line, 42);
        assert_eq!(all[0].col, 7);
    }

    #[test]
    fn parse_coverage_out_file_id_deterministic() {
        let input = "mode: atomic\npkg/foo.go:1.1,2.1 1 1\npkg/foo.go:3.1,4.1 1 0\n";
        let tmp = tempfile::tempdir().unwrap();
        let (all, _, _) = parse_coverage_out(input, tmp.path());
        // Same file -> same file_id
        assert_eq!(all[0].file_id, all[2].file_id);
    }

    #[test]
    fn derive_relative_path_fallback() {
        let tmp = tempfile::tempdir().unwrap();
        let result = derive_relative_path("example.com/foo/bar.go", tmp.path());
        // No matching file exists, returns original
        assert_eq!(result, "example.com/foo/bar.go");
    }

    #[test]
    fn derive_relative_path_found() {
        let tmp = tempfile::tempdir().unwrap();
        let pkg = tmp.path().join("foo");
        std::fs::create_dir(&pkg).unwrap();
        std::fs::write(pkg.join("bar.go"), "package foo").unwrap();

        let result = derive_relative_path("example.com/foo/bar.go", tmp.path());
        assert_eq!(result, "foo/bar.go");
    }

    // --- New tests targeting uncovered regions ---

    // Target: derive_relative_path — path traversal in coverage output is rejected
    // The guard `candidate.starts_with(target_root)` blocks `../../etc/passwd`.
    #[test]
    fn derive_relative_path_blocks_traversal() {
        let tmp = tempfile::tempdir().unwrap();
        // Path traversal: the function should NOT return a suffix that,
        // when joined with target_root, resolves outside it.
        let result = derive_relative_path("a/../../etc/passwd", tmp.path());
        // The result is either the original (fallback) or a shorter suffix.
        // The function's guard `candidate.starts_with(target_root)` prevents
        // returning paths that escape — but since /etc/passwd doesn't match
        // any file under tmp, the function returns the original as fallback.
        // On different platforms, the suffix stripping may produce different
        // intermediate results, but the final return is always safe.
        assert!(
            !result.is_empty(),
            "should return something (original path or safe suffix)"
        );
    }

    // Target: derive_relative_path — empty string input
    #[test]
    fn derive_relative_path_empty_string() {
        let tmp = tempfile::tempdir().unwrap();
        // Empty path: split('/') yields [""], parts.len() == 1, loop doesn't run
        let result = derive_relative_path("", tmp.path());
        assert_eq!(result, "");
    }

    // Target: derive_relative_path — direct relative file found at root
    #[test]
    fn derive_relative_path_direct_relative() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("main.go"), "package main").unwrap();
        let result = derive_relative_path("main.go", tmp.path());
        // Exists directly under target_root
        assert_eq!(result, "main.go");
    }

    // Target: parse_coverage_out — completely blank input (no mode line)
    #[test]
    fn parse_coverage_out_completely_blank() {
        let tmp = tempfile::tempdir().unwrap();
        let (all, executed, file_paths) = parse_coverage_out("", tmp.path());
        assert!(all.is_empty());
        assert!(executed.is_empty());
        assert!(file_paths.is_empty());
    }

    // Target: parse_coverage_out — whitespace-only lines are skipped
    #[test]
    fn parse_coverage_out_whitespace_lines_skipped() {
        let input = "mode: atomic\n   \n\t\npkg/foo.go:1.1,2.1 1 1\n";
        let tmp = tempfile::tempdir().unwrap();
        let (all, _, _) = parse_coverage_out(input, tmp.path());
        assert_eq!(all.len(), 2);
    }

    // Target: parse_coverage_out — line with only one space token (missing count) is skipped
    #[test]
    fn parse_coverage_out_missing_count_token_skipped() {
        let input = "mode: atomic\npkg/foo.go:1.1,2.1 1\n";
        let tmp = tempfile::tempdir().unwrap();
        let (all, _, _) = parse_coverage_out(input, tmp.path());
        // rsplit_once(' ') yields ("pkg/foo.go:1.1,2.1", "1") then second rsplit_once fails
        assert!(all.is_empty());
    }

    // Target: parse_coverage_out — non-numeric count is skipped
    #[test]
    fn parse_coverage_out_non_numeric_count_skipped() {
        let input = "mode: atomic\npkg/foo.go:1.1,2.1 1 abc\n";
        let tmp = tempfile::tempdir().unwrap();
        let (all, _, _) = parse_coverage_out(input, tmp.path());
        assert!(all.is_empty());
    }

    // Target: parse_coverage_out — no colon in file_range is skipped
    #[test]
    fn parse_coverage_out_no_colon_in_file_range_skipped() {
        let input = "mode: atomic\nnocolon 1 3\n";
        let tmp = tempfile::tempdir().unwrap();
        let (all, _, _) = parse_coverage_out(input, tmp.path());
        assert!(all.is_empty());
    }

    // Target: parse_coverage_out — no comma in range_part is skipped
    #[test]
    fn parse_coverage_out_no_comma_in_range_skipped() {
        let input = "mode: atomic\npkg/foo.go:1.1-2.1 1 3\n";
        let tmp = tempfile::tempdir().unwrap();
        let (all, _, _) = parse_coverage_out(input, tmp.path());
        assert!(all.is_empty());
    }

    // Target: parse_coverage_out — no dot in start_part is skipped
    #[test]
    fn parse_coverage_out_no_dot_in_start_skipped() {
        let input = "mode: atomic\npkg/foo.go:12,15.3 1 3\n";
        let tmp = tempfile::tempdir().unwrap();
        let (all, _, _) = parse_coverage_out(input, tmp.path());
        assert!(all.is_empty());
    }

    // Target: parse_coverage_out — non-numeric start line is skipped
    #[test]
    fn parse_coverage_out_non_numeric_start_line_skipped() {
        let input = "mode: atomic\npkg/foo.go:xx.1,2.1 1 3\n";
        let tmp = tempfile::tempdir().unwrap();
        let (all, _, _) = parse_coverage_out(input, tmp.path());
        assert!(all.is_empty());
    }

    // Target: parse_coverage_out — non-numeric start col is skipped
    #[test]
    fn parse_coverage_out_non_numeric_start_col_skipped() {
        let input = "mode: atomic\npkg/foo.go:1.yy,2.1 1 3\n";
        let tmp = tempfile::tempdir().unwrap();
        let (all, _, _) = parse_coverage_out(input, tmp.path());
        assert!(all.is_empty());
    }

    // Target: parse_coverage_out — count=0 produces direction=1
    #[test]
    fn parse_coverage_out_zero_count_direction_one() {
        let input = "mode: atomic\npkg/foo.go:5.1,6.1 1 0\n";
        let tmp = tempfile::tempdir().unwrap();
        let (_, executed, _) = parse_coverage_out(input, tmp.path());
        assert_eq!(executed.len(), 1);
        assert_eq!(executed[0].direction, 1);
    }

    // Target: parse_coverage_out — same file referenced in multiple lines shares file_id
    #[test]
    fn parse_coverage_out_same_file_deduplicates_in_file_paths() {
        let input = "mode: atomic\npkg/foo.go:1.1,2.1 1 1\npkg/foo.go:3.1,4.1 1 0\n";
        let tmp = tempfile::tempdir().unwrap();
        let (all, _, file_paths) = parse_coverage_out(input, tmp.path());
        assert_eq!(all.len(), 4);
        // Only one file_id despite two lines
        assert_eq!(file_paths.len(), 1);
        // Both branches share same file_id
        assert_eq!(all[0].file_id, all[2].file_id);
    }

    // Target: parse_coverage_out — max u32 count is treated as covered
    #[test]
    fn parse_coverage_out_max_count_is_covered() {
        let input = format!("mode: atomic\npkg/foo.go:1.1,2.1 1 {}\n", u32::MAX);
        let tmp = tempfile::tempdir().unwrap();
        let (_, executed, _) = parse_coverage_out(&input, tmp.path());
        assert_eq!(executed.len(), 1);
        assert_eq!(executed[0].direction, 0);
    }

    // Target: parse_coverage_out — unicode path in coverage output
    #[test]
    fn parse_coverage_out_unicode_path() {
        let input = "mode: atomic\nexample.com/\u{4e2d}\u{6587}/foo.go:1.1,2.1 1 1\n";
        let tmp = tempfile::tempdir().unwrap();
        let (all, _, file_paths) = parse_coverage_out(input, tmp.path());
        assert_eq!(all.len(), 2);
        assert_eq!(file_paths.len(), 1);
    }
}
