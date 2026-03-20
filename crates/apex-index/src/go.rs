use crate::types::{hash_source_files, BranchIndex, TestTrace};
use apex_core::hash::fnv1a_hash;
use apex_core::types::{BranchId, ExecutionStatus, Language};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use tracing::{info, warn};

/// Parse Go `coverage.out` format into branch entries.
///
/// Format:
///   mode: atomic
///   file:startLine.startCol,endLine.endCol numStmt count
fn parse_go_coverage(content: &str, target_root: &Path) -> (Vec<BranchId>, HashMap<u64, PathBuf>) {
    let mut branches = Vec::new();
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

        let rel_path = derive_relative_path(file_part, target_root);
        let file_id = fnv1a_hash(&rel_path);

        file_paths
            .entry(file_id)
            .or_insert_with(|| PathBuf::from(&rel_path));

        let branch = BranchId::new(
            file_id,
            start_line,
            start_col,
            if count > 0 { 0 } else { 1 },
        );
        branches.push(branch);
    }

    (branches, file_paths)
}

/// Derive a relative path from a Go coverage path.
fn derive_relative_path(coverage_path: &str, target_root: &Path) -> String {
    if target_root.join(coverage_path).exists() {
        return coverage_path.to_string();
    }
    let parts: Vec<&str> = coverage_path.split('/').collect();
    for start in 1..parts.len() {
        let suffix = parts[start..].join("/");
        if target_root.join(&suffix).exists() {
            return suffix;
        }
    }
    coverage_path.to_string()
}

/// Build a BranchIndex for a Go project by running tests with coverage.
pub async fn build_go_index(
    target_root: &Path,
    _parallelism: usize,
) -> std::result::Result<BranchIndex, Box<dyn std::error::Error + Send + Sync>> {
    let target_root = std::fs::canonicalize(target_root)?;
    info!(target = %target_root.display(), "building Go branch index");

    // Run: go test -coverprofile=coverage.out -covermode=atomic -v ./...
    let output = tokio::process::Command::new("go")
        .args([
            "test",
            "-coverprofile=coverage.out",
            "-covermode=atomic",
            "-v",
            "./...",
        ])
        .current_dir(&target_root)
        .output()
        .await?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        warn!(%stderr, "go test -cover returned non-zero");
    }

    let coverage_path = target_root.join("coverage.out");
    let content = std::fs::read_to_string(&coverage_path)
        .map_err(|e| format!("failed to read coverage.out: {e}"))?;

    let (all_branches, file_paths) = parse_go_coverage(&content, &target_root);

    // Parse verbose output to build per-test traces.
    let stdout = String::from_utf8_lossy(&output.stdout);
    let traces = build_traces_from_verbose(&stdout, &all_branches);

    let profiles = BranchIndex::build_profiles(&traces);
    let covered_branches = profiles.len();
    let source_hash = hash_source_files(&target_root, Language::Go);

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    let index = BranchIndex {
        traces,
        profiles,
        file_paths,
        total_branches: all_branches.len(),
        covered_branches,
        created_at: format!("{now}"),
        language: Language::Go,
        target_root: target_root.clone(),
        source_hash,
    };

    info!(
        total = index.total_branches,
        covered = index.covered_branches,
        "Go branch index built"
    );

    Ok(index)
}

/// Build test traces from `go test -v` output.
/// Each `--- PASS: TestFoo (0.00s)` or `--- FAIL: TestFoo` line is a test.
fn build_traces_from_verbose(stdout: &str, all_branches: &[BranchId]) -> Vec<TestTrace> {
    let mut traces = Vec::new();

    for line in stdout.lines() {
        let trimmed = line.trim();

        if let Some(rest) = trimmed.strip_prefix("--- PASS: ") {
            // Format: --- PASS: TestFoo (0.01s)
            if let Some((name, duration_part)) = rest.split_once(' ') {
                let duration_ms = parse_duration_parens(duration_part);
                traces.push(TestTrace {
                    test_name: name.to_string(),
                    branches: all_branches.to_vec(),
                    duration_ms,
                    status: ExecutionStatus::Pass,
                });
            }
        } else if let Some(rest) = trimmed.strip_prefix("--- FAIL: ") {
            let name = rest.split_whitespace().next().unwrap_or(rest);
            traces.push(TestTrace {
                test_name: name.to_string(),
                branches: all_branches.to_vec(),
                duration_ms: 0,
                status: ExecutionStatus::Fail,
            });
        }
    }

    traces
}

/// Parse "(0.01s)" into milliseconds.
fn parse_duration_parens(s: &str) -> u64 {
    let s = s.trim().trim_start_matches('(').trim_end_matches(')');
    let s = s.trim_end_matches('s');
    match s.parse::<f64>() {
        Ok(secs) => (secs * 1000.0) as u64,
        Err(_) => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE: &str = "\
mode: atomic
example.com/foo/main.go:10.2,12.15 1 3
example.com/foo/main.go:14.2,16.10 1 0
example.com/foo/handler.go:5.14,8.2 2 1
";

    #[test]
    fn parse_go_coverage_basic() {
        let tmp = tempfile::tempdir().unwrap();
        let (branches, file_paths) = parse_go_coverage(FIXTURE, tmp.path());
        assert_eq!(branches.len(), 3);
        assert_eq!(file_paths.len(), 2);
    }

    #[test]
    fn parse_go_coverage_direction() {
        let tmp = tempfile::tempdir().unwrap();
        let (branches, _) = parse_go_coverage(FIXTURE, tmp.path());
        // count=3 -> dir 0, count=0 -> dir 1, count=1 -> dir 0
        assert_eq!(branches[0].direction, 0);
        assert_eq!(branches[1].direction, 1);
        assert_eq!(branches[2].direction, 0);
    }

    #[test]
    fn parse_go_coverage_empty() {
        let tmp = tempfile::tempdir().unwrap();
        let (branches, file_paths) = parse_go_coverage("mode: atomic\n", tmp.path());
        assert!(branches.is_empty());
        assert!(file_paths.is_empty());
    }

    #[test]
    fn parse_go_coverage_file_id_consistent() {
        let tmp = tempfile::tempdir().unwrap();
        let input = "mode: atomic\npkg/a.go:1.1,2.1 1 1\npkg/a.go:3.1,4.1 1 0\n";
        let (branches, _) = parse_go_coverage(input, tmp.path());
        assert_eq!(branches[0].file_id, branches[1].file_id);
    }

    #[test]
    fn build_traces_from_verbose_parses_pass() {
        let stdout = "=== RUN   TestFoo\n--- PASS: TestFoo (0.01s)\n";
        let branches = vec![BranchId::new(1, 10, 0, 0)];
        let traces = build_traces_from_verbose(stdout, &branches);
        assert_eq!(traces.len(), 1);
        assert_eq!(traces[0].test_name, "TestFoo");
        assert_eq!(traces[0].status, ExecutionStatus::Pass);
        assert_eq!(traces[0].duration_ms, 10);
    }

    #[test]
    fn build_traces_from_verbose_parses_fail() {
        let stdout = "=== RUN   TestBar\n--- FAIL: TestBar (0.00s)\n";
        let branches = vec![];
        let traces = build_traces_from_verbose(stdout, &branches);
        assert_eq!(traces.len(), 1);
        assert_eq!(traces[0].test_name, "TestBar");
        assert_eq!(traces[0].status, ExecutionStatus::Fail);
    }

    #[test]
    fn parse_duration_parens_valid() {
        assert_eq!(parse_duration_parens("(0.01s)"), 10);
        assert_eq!(parse_duration_parens("(1.5s)"), 1500);
        assert_eq!(parse_duration_parens("(0.00s)"), 0);
    }

    #[test]
    fn parse_duration_parens_invalid() {
        assert_eq!(parse_duration_parens("invalid"), 0);
    }

    // Target: lines 24-26 — line with no spaces (no rsplit_once ' ' succeeds) — skipped
    #[test]
    fn bug_parse_go_coverage_line_no_spaces() {
        let tmp = tempfile::tempdir().unwrap();
        let input = "mode: atomic\nnoseparatorhere\n";
        let (branches, _) = parse_go_coverage(input, tmp.path());
        assert!(branches.is_empty(), "line with no spaces must be skipped");
    }

    // Target: lines 27-29 — file_range has no second space (can't split num_stmt)
    #[test]
    fn bug_parse_go_coverage_line_one_space_only() {
        let tmp = tempfile::tempdir().unwrap();
        // Only one space: "file:1.1,2.2 3" — second rsplit fails
        let input = "mode: atomic\nfile:1.1,2.2 3\n";
        let (branches, _) = parse_go_coverage(input, tmp.path());
        assert!(branches.is_empty(), "line with one space must be skipped");
    }

    // Target: lines 30-33 — non-numeric count field
    #[test]
    fn bug_parse_go_coverage_nonnumeric_count() {
        let tmp = tempfile::tempdir().unwrap();
        let input = "mode: atomic\nexample.com/pkg/main.go:10.2,12.15 1 bad\nexample.com/pkg/main.go:14.2,16.10 1 0\n";
        let (branches, _) = parse_go_coverage(input, tmp.path());
        assert_eq!(branches.len(), 1, "non-numeric count must be skipped");
        assert_eq!(branches[0].direction, 1); // count=0
    }

    // Target: lines 35-37 — file_range has no ':' separator
    #[test]
    fn bug_parse_go_coverage_no_colon_in_file_range() {
        let tmp = tempfile::tempdir().unwrap();
        // file_range = "nocoLon" (no ':')
        let input = "mode: atomic\nnoColon 1 3\n";
        let (branches, _) = parse_go_coverage(input, tmp.path());
        assert!(
            branches.is_empty(),
            "file range without ':' must be skipped"
        );
    }

    // Target: lines 39-41 — range_part has no ',' separator
    #[test]
    fn bug_parse_go_coverage_no_comma_in_range() {
        let tmp = tempfile::tempdir().unwrap();
        // No comma in range: "pkg/main.go:10.2-12.15 1 3"
        let input = "mode: atomic\npkg/main.go:10.2-12.15 1 3\n";
        let (branches, _) = parse_go_coverage(input, tmp.path());
        assert!(branches.is_empty(), "range without comma must be skipped");
    }

    // Target: lines 42-44 — start_part has no '.' separator
    #[test]
    fn bug_parse_go_coverage_no_dot_in_start_part() {
        let tmp = tempfile::tempdir().unwrap();
        // Start part lacks '.': "pkg/main.go:10,12.15 1 3"
        let input = "mode: atomic\npkg/main.go:10,12.15 1 3\n";
        let (branches, _) = parse_go_coverage(input, tmp.path());
        assert!(
            branches.is_empty(),
            "start part without '.' must be skipped"
        );
    }

    // Target: lines 46-48 — non-numeric start_line
    #[test]
    fn bug_parse_go_coverage_nonnumeric_start_line() {
        let tmp = tempfile::tempdir().unwrap();
        let input = "mode: atomic\npkg/main.go:x.2,12.15 1 3\n";
        let (branches, _) = parse_go_coverage(input, tmp.path());
        assert!(
            branches.is_empty(),
            "non-numeric start_line must be skipped"
        );
    }

    // Target: lines 50-52 — non-numeric start_col
    #[test]
    fn bug_parse_go_coverage_nonnumeric_start_col() {
        let tmp = tempfile::tempdir().unwrap();
        let input = "mode: atomic\npkg/main.go:10.col,12.15 1 3\n";
        let (branches, _) = parse_go_coverage(input, tmp.path());
        assert!(branches.is_empty(), "non-numeric start_col must be skipped");
    }

    // Target: lines 17-20 — blank lines and mode: line skipped
    #[test]
    fn parse_go_coverage_skips_blank_and_mode() {
        let tmp = tempfile::tempdir().unwrap();
        let input = "\nmode: atomic\n\nexample.com/pkg/a.go:1.1,2.2 1 1\n";
        let (branches, _) = parse_go_coverage(input, tmp.path());
        assert_eq!(branches.len(), 1);
    }

    // Target: lines 75-87 — derive_relative_path suffix matching
    #[test]
    fn derive_relative_path_suffix_matching() {
        let tmp = tempfile::tempdir().unwrap();
        // Create a real file so the join+exists check succeeds
        let src_dir = tmp.path().join("pkg");
        std::fs::create_dir(&src_dir).unwrap();
        let file = src_dir.join("main.go");
        std::fs::write(&file, "package main").unwrap();

        // Coverage path uses a module prefix that doesn't match the root
        let cov_path = "github.com/user/repo/pkg/main.go";
        let result = derive_relative_path(cov_path, tmp.path());
        assert_eq!(
            result, "pkg/main.go",
            "suffix matching should find pkg/main.go"
        );
    }

    // Target: lines 75-87 — derive_relative_path falls through to return original
    #[test]
    fn derive_relative_path_no_match_returns_original() {
        let tmp = tempfile::tempdir().unwrap();
        // Nothing in tmp matches "github.com/user/nosuchfile.go"
        let cov_path = "github.com/user/nosuchfile.go";
        let result = derive_relative_path(cov_path, tmp.path());
        assert_eq!(result, cov_path);
    }

    // Target: derive_relative_path — direct file join matches (line 76-78)
    #[test]
    fn derive_relative_path_direct_join_match() {
        let tmp = tempfile::tempdir().unwrap();
        let file = tmp.path().join("main.go");
        std::fs::write(&file, "package main").unwrap();
        let result = derive_relative_path("main.go", tmp.path());
        assert_eq!(result, "main.go");
    }

    // Target: lines 163-173 — build_traces_from_verbose PASS without duration part
    #[test]
    fn bug_build_traces_from_verbose_pass_no_space_after_name() {
        // "--- PASS: TestFoo" with no space after test name — split_once fails, no trace
        let stdout = "--- PASS: TestFoo\n";
        let traces = build_traces_from_verbose(stdout, &[]);
        assert!(
            traces.is_empty(),
            "PASS line without duration part must be skipped"
        );
    }

    // Target: lines 174-182 — FAIL line with no whitespace after name
    #[test]
    fn build_traces_from_verbose_fail_no_duration() {
        // "--- FAIL: TestBar" with no trailing content — split_whitespace().next() still works
        let stdout = "--- FAIL: TestBaz\n";
        let traces = build_traces_from_verbose(stdout, &[]);
        assert_eq!(traces.len(), 1);
        assert_eq!(traces[0].test_name, "TestBaz");
        assert_eq!(traces[0].status, ExecutionStatus::Fail);
        assert_eq!(traces[0].duration_ms, 0);
    }

    // Target: build_traces_from_verbose — non-test lines ignored
    #[test]
    fn build_traces_from_verbose_ignores_other_lines() {
        let stdout = "=== RUN   TestFoo\nsome output\nok  pkg  0.012s\n";
        let traces = build_traces_from_verbose(stdout, &[]);
        assert!(traces.is_empty());
    }

    // Target: parse_duration_parens — no opening paren
    #[test]
    fn bug_parse_duration_parens_no_paren() {
        // trim_start_matches('(') is a no-op, trim_end_matches(')') is a no-op
        // "0.01s" becomes "0.01" after trim_end_matches('s'), parses fine
        assert_eq!(parse_duration_parens("0.01s"), 10);
    }

    // Target: parse_duration_parens — empty string
    #[test]
    fn bug_parse_duration_parens_empty() {
        assert_eq!(parse_duration_parens(""), 0);
    }

    // Target: build_traces_from_verbose PASS with branches propagated
    #[test]
    fn build_traces_from_verbose_branches_propagated() {
        let branches = vec![BranchId::new(42, 5, 0, 0), BranchId::new(42, 10, 0, 1)];
        let stdout = "--- PASS: TestX (0.005s)\n";
        let traces = build_traces_from_verbose(stdout, &branches);
        assert_eq!(traces.len(), 1);
        assert_eq!(traces[0].branches.len(), 2);
        assert_eq!(traces[0].duration_ms, 5);
    }

    // Target: start_col u16 overflow — large column value (> 65535)
    // BranchId::new takes u16 col, so very large col parse::<u16>() fails -> skipped
    #[test]
    fn bug_parse_go_coverage_col_overflow_u16() {
        let tmp = tempfile::tempdir().unwrap();
        // Column 70000 overflows u16 (max 65535)
        let input = "mode: atomic\nexample.com/pkg/a.go:10.70000,12.15 1 3\n";
        let (branches, _) = parse_go_coverage(input, tmp.path());
        assert!(branches.is_empty(), "u16 column overflow must be skipped");
    }

    // Target: file_paths.entry().or_insert_with() — same file gets only one entry
    // (lines 58-60: the or_insert_with branch on repeated file_id).
    #[test]
    fn parse_go_coverage_file_deduped_in_file_paths() {
        let tmp = tempfile::tempdir().unwrap();
        let input = "mode: atomic\n\
pkg/main.go:1.1,2.2 1 1\n\
pkg/main.go:3.1,4.2 1 0\n\
pkg/main.go:5.1,6.2 1 2\n";
        let (branches, file_paths) = parse_go_coverage(input, tmp.path());
        assert_eq!(branches.len(), 3);
        // All three lines belong to the same file — file_paths should have exactly 1 key.
        assert_eq!(
            file_paths.len(),
            1,
            "same file must appear once in file_paths"
        );
    }

    // Target: parse_go_coverage — count=0 produces direction=1 (line 66).
    #[test]
    fn parse_go_coverage_count_zero_direction_one() {
        let tmp = tempfile::tempdir().unwrap();
        let input = "mode: atomic\npkg/x.go:10.5,15.10 2 0\n";
        let (branches, _) = parse_go_coverage(input, tmp.path());
        assert_eq!(branches.len(), 1);
        assert_eq!(branches[0].direction, 1, "count=0 must yield direction=1");
    }

    // Target: build_traces_from_verbose — PASS line with branches propagated (lines 167-172).
    #[test]
    fn build_traces_from_verbose_pass_branches_propagated() {
        let branches = vec![BranchId::new(5, 1, 0, 0), BranchId::new(5, 2, 0, 1)];
        let stdout = "--- PASS: TestSomething (0.100s)\n";
        let traces = build_traces_from_verbose(stdout, &branches);
        assert_eq!(traces.len(), 1);
        assert_eq!(traces[0].branches.len(), 2);
        assert_eq!(traces[0].duration_ms, 100);
    }

    // Target: build_traces_from_verbose — FAIL line has duration_ms=0 (lines 174-182).
    #[test]
    fn build_traces_from_verbose_fail_zero_duration() {
        let stdout = "--- FAIL: TestBroken (0.050s)\n";
        let traces = build_traces_from_verbose(stdout, &[]);
        assert_eq!(traces.len(), 1);
        assert_eq!(
            traces[0].duration_ms, 0,
            "FAIL traces always have duration_ms=0"
        );
        assert_eq!(traces[0].status, ExecutionStatus::Fail);
    }

    // Target: build_traces_from_verbose — multiple PASS and FAIL lines (full loop).
    #[test]
    fn build_traces_from_verbose_multiple_results() {
        let stdout = "\
=== RUN   TestA\n\
--- PASS: TestA (0.001s)\n\
=== RUN   TestB\n\
--- FAIL: TestB (0.002s)\n\
=== RUN   TestC\n\
--- PASS: TestC (0.003s)\n";
        let traces = build_traces_from_verbose(stdout, &[]);
        assert_eq!(traces.len(), 3);
        assert_eq!(traces[0].test_name, "TestA");
        assert_eq!(traces[0].status, ExecutionStatus::Pass);
        assert_eq!(traces[1].test_name, "TestB");
        assert_eq!(traces[1].status, ExecutionStatus::Fail);
        assert_eq!(traces[2].test_name, "TestC");
        assert_eq!(traces[2].status, ExecutionStatus::Pass);
    }

    // Target: parse_duration_parens — whitespace trimming (line 190).
    #[test]
    fn parse_duration_parens_with_surrounding_whitespace() {
        assert_eq!(parse_duration_parens("  (0.01s)  "), 10);
    }

    // Target: derive_relative_path — empty suffix from split (edge: single-component path)
    #[test]
    fn derive_relative_path_single_component() {
        let tmp = tempfile::tempdir().unwrap();
        // A path with no '/' slashes — split produces ["file.go"], loop starts at 1..1
        // which is empty, so falls through to return original.
        let result = derive_relative_path("file.go", tmp.path());
        // No match in suffix loop since start=1..1 is empty
        assert_eq!(result, "file.go");
    }
}
