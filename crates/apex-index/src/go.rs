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
fn parse_go_coverage(
    content: &str,
    target_root: &Path,
) -> (Vec<BranchId>, HashMap<u64, PathBuf>) {
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

        let branch = BranchId::new(file_id, start_line, start_col, if count > 0 { 0 } else { 1 });
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
}
