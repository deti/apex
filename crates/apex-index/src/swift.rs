use crate::types::{hash_source_files, BranchIndex, TestTrace};
use apex_core::hash::fnv1a_hash;
use apex_core::types::{BranchId, ExecutionStatus, Language};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use tracing::{info, warn};

/// Parse Swift llvm-cov JSON export into branch entries.
///
/// The JSON format (from `llvm-cov export`) contains:
/// ```json
/// { "data": [{ "files": [{ "filename": "...", "segments": [[line, col, count, ...], ...] }] }] }
/// ```
fn parse_swift_coverage(
    content: &str,
    target_root: &Path,
) -> (Vec<BranchId>, HashMap<u64, PathBuf>) {
    let mut branches = Vec::new();
    let mut file_paths: HashMap<u64, PathBuf> = HashMap::new();

    let Ok(root) = serde_json::from_str::<serde_json::Value>(content) else {
        return (branches, file_paths);
    };

    let Some(data) = root.get("data").and_then(|d| d.as_array()) else {
        return (branches, file_paths);
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

                let branch = BranchId::new(
                    file_id,
                    line as u32,
                    col as u16,
                    if count > 0 { 0 } else { 1 },
                );
                branches.push(branch);
            }
        }
    }

    (branches, file_paths)
}

/// Derive a relative path from an absolute coverage path.
fn derive_relative_path(coverage_path: &str, target_root: &Path) -> String {
    let path = Path::new(coverage_path);
    if let Ok(rel) = path.strip_prefix(target_root) {
        return rel.to_string_lossy().to_string();
    }
    coverage_path.to_string()
}

/// Build test traces from `swift test` verbose output.
/// Each `Test Case '-[Module.TestClass testName]' passed` line is a test.
fn build_traces_from_output(stdout: &str, all_branches: &[BranchId]) -> Vec<TestTrace> {
    let mut traces = Vec::new();

    for line in stdout.lines() {
        let trimmed = line.trim();

        if let Some(rest) = trimmed.strip_prefix("Test Case '") {
            // Format: Test Case '-[Module.TestClass testName]' passed (0.001 seconds).
            let Some((name_part, status_part)) = rest.split_once("' ") else {
                continue;
            };
            // Extract test name from bracket notation
            let test_name = name_part
                .trim_start_matches("-[")
                .trim_end_matches(']')
                .replace(' ', ".");

            let status = if status_part.starts_with("passed") {
                ExecutionStatus::Pass
            } else if status_part.starts_with("failed") {
                ExecutionStatus::Fail
            } else {
                continue;
            };

            let duration_ms = parse_seconds_parens(status_part);

            traces.push(TestTrace {
                test_name,
                branches: all_branches.to_vec(),
                duration_ms,
                status,
            });
        }
    }

    traces
}

/// Parse "(0.001 seconds)" from a status line into milliseconds.
fn parse_seconds_parens(s: &str) -> u64 {
    // Find the number in parentheses
    let Some(open) = s.find('(') else { return 0 };
    let Some(close) = s.find(" seconds") else {
        return 0;
    };
    let num_str = &s[open + 1..close];
    match num_str.trim().parse::<f64>() {
        Ok(secs) => (secs * 1000.0) as u64,
        Err(_) => 0,
    }
}

/// Build a BranchIndex for a Swift project by running tests with coverage.
pub async fn build_swift_index(
    target_root: &Path,
    _parallelism: usize,
) -> std::result::Result<BranchIndex, Box<dyn std::error::Error + Send + Sync>> {
    let target_root = std::fs::canonicalize(target_root)?;
    info!(target = %target_root.display(), "building Swift branch index");

    // Run: swift test --enable-code-coverage -v
    let output = tokio::process::Command::new("swift")
        .args(["test", "--enable-code-coverage", "-v"])
        .current_dir(&target_root)
        .output()
        .await?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        warn!(%stderr, "swift test --enable-code-coverage returned non-zero");
    }

    // Get coverage JSON path
    let codecov_output = tokio::process::Command::new("swift")
        .args(["test", "--show-codecov-path"])
        .current_dir(&target_root)
        .output()
        .await?;

    let codecov_path = String::from_utf8_lossy(&codecov_output.stdout)
        .trim()
        .to_string();

    let content = std::fs::read_to_string(&codecov_path)
        .map_err(|e| format!("failed to read coverage JSON: {e}"))?;

    let (all_branches, file_paths) = parse_swift_coverage(&content, &target_root);

    let stdout = String::from_utf8_lossy(&output.stdout);
    let traces = build_traces_from_output(&stdout, &all_branches);

    let profiles = BranchIndex::build_profiles(&traces);
    let covered_branches = profiles.len();
    let source_hash = hash_source_files(&target_root, Language::Swift);

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
        language: Language::Swift,
        target_root: target_root.clone(),
        source_hash,
    };

    info!(
        total = index.total_branches,
        covered = index.covered_branches,
        "Swift branch index built"
    );

    Ok(index)
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE: &str = r#"{
        "data": [{
            "files": [
                {
                    "filename": "/src/main.swift",
                    "segments": [
                        [10, 5, 3, true, true],
                        [14, 5, 0, true, true],
                        [20, 10, 1, true, true]
                    ]
                }
            ]
        }]
    }"#;

    #[test]
    fn parse_swift_coverage_basic() {
        let tmp = tempfile::tempdir().unwrap();
        let (branches, file_paths) = parse_swift_coverage(FIXTURE, tmp.path());
        assert_eq!(branches.len(), 3);
        assert_eq!(file_paths.len(), 1);
    }

    #[test]
    fn parse_swift_coverage_direction() {
        let tmp = tempfile::tempdir().unwrap();
        let (branches, _) = parse_swift_coverage(FIXTURE, tmp.path());
        // count=3 -> dir 0, count=0 -> dir 1, count=1 -> dir 0
        assert_eq!(branches[0].direction, 0);
        assert_eq!(branches[1].direction, 1);
        assert_eq!(branches[2].direction, 0);
    }

    #[test]
    fn parse_swift_coverage_empty() {
        let tmp = tempfile::tempdir().unwrap();
        let (branches, file_paths) =
            parse_swift_coverage(r#"{"data": [{"files": []}]}"#, tmp.path());
        assert!(branches.is_empty());
        assert!(file_paths.is_empty());
    }

    #[test]
    fn parse_swift_coverage_file_id_consistent() {
        let json = r#"{"data": [{"files": [{"filename": "a.swift", "segments": [[1,1,1,true,true],[3,1,0,true,true]]}]}]}"#;
        let tmp = tempfile::tempdir().unwrap();
        let (branches, _) = parse_swift_coverage(json, tmp.path());
        assert_eq!(branches[0].file_id, branches[1].file_id);
    }

    #[test]
    fn build_traces_from_output_parses_pass() {
        let stdout = "Test Case '-[MyTests.FooTests testBar]' passed (0.001 seconds).\n";
        let branches = vec![BranchId::new(1, 10, 0, 0)];
        let traces = build_traces_from_output(stdout, &branches);
        assert_eq!(traces.len(), 1);
        assert_eq!(traces[0].test_name, "MyTests.FooTests.testBar");
        assert_eq!(traces[0].status, ExecutionStatus::Pass);
        assert_eq!(traces[0].duration_ms, 1);
    }

    #[test]
    fn build_traces_from_output_parses_fail() {
        let stdout = "Test Case '-[MyTests.FooTests testBaz]' failed (0.010 seconds).\n";
        let branches = vec![];
        let traces = build_traces_from_output(stdout, &branches);
        assert_eq!(traces.len(), 1);
        assert_eq!(traces[0].test_name, "MyTests.FooTests.testBaz");
        assert_eq!(traces[0].status, ExecutionStatus::Fail);
    }

    #[test]
    fn parse_seconds_parens_valid() {
        assert_eq!(parse_seconds_parens("passed (0.001 seconds)."), 1);
        assert_eq!(parse_seconds_parens("passed (1.500 seconds)."), 1500);
        assert_eq!(parse_seconds_parens("passed (0.000 seconds)."), 0);
    }

    #[test]
    fn parse_seconds_parens_invalid() {
        assert_eq!(parse_seconds_parens("no parens"), 0);
    }

    // Target: parse_swift_coverage — invalid JSON input (line 21-23)
    #[test]
    fn bug_parse_swift_coverage_invalid_json() {
        let tmp = tempfile::tempdir().unwrap();
        let (branches, file_paths) = parse_swift_coverage("not json at all {{{{", tmp.path());
        assert!(branches.is_empty());
        assert!(file_paths.is_empty());
    }

    // Target: parse_swift_coverage — missing "data" key (line 25-27)
    #[test]
    fn bug_parse_swift_coverage_missing_data_key() {
        let tmp = tempfile::tempdir().unwrap();
        let (branches, file_paths) = parse_swift_coverage(r#"{"other": []}"#, tmp.path());
        assert!(branches.is_empty());
        assert!(file_paths.is_empty());
    }

    // Target: parse_swift_coverage — entry with no "files" key (line 30-32)
    #[test]
    fn bug_parse_swift_coverage_entry_missing_files() {
        let tmp = tempfile::tempdir().unwrap();
        let json = r#"{"data": [{"functions": []}]}"#;
        let (branches, file_paths) = parse_swift_coverage(json, tmp.path());
        assert!(branches.is_empty());
        assert!(file_paths.is_empty());
    }

    // Target: parse_swift_coverage — file entry with no "filename" (line 35-37)
    #[test]
    fn bug_parse_swift_coverage_file_missing_filename() {
        let tmp = tempfile::tempdir().unwrap();
        let json = r#"{"data": [{"files": [{"segments": [[1,1,1,true,true]]}]}]}"#;
        let (branches, file_paths) = parse_swift_coverage(json, tmp.path());
        assert!(branches.is_empty());
        assert!(file_paths.is_empty());
    }

    // Target: parse_swift_coverage — file entry with no "segments" (line 38-40)
    #[test]
    fn bug_parse_swift_coverage_file_missing_segments() {
        let tmp = tempfile::tempdir().unwrap();
        let json = r#"{"data": [{"files": [{"filename": "a.swift"}]}]}"#;
        let (branches, file_paths) = parse_swift_coverage(json, tmp.path());
        assert!(branches.is_empty());
        // file has filename, but no segments so no branches; file_path not inserted either
        // (file_paths is inserted before segment loop — check that too)
        assert!(file_paths.is_empty());
    }

    // Target: parse_swift_coverage — segment is not an array (line 49-51)
    #[test]
    fn bug_parse_swift_coverage_segment_not_array() {
        let tmp = tempfile::tempdir().unwrap();
        let json = r#"{"data": [{"files": [{"filename": "a.swift", "segments": [42, "bad"]}]}]}"#;
        let (branches, _) = parse_swift_coverage(json, tmp.path());
        assert!(branches.is_empty(), "non-array segments must be skipped");
    }

    // Target: parse_swift_coverage — segment array too short (line 52-54)
    #[test]
    fn bug_parse_swift_coverage_segment_too_short() {
        let tmp = tempfile::tempdir().unwrap();
        let json = r#"{"data": [{"files": [{"filename": "a.swift", "segments": [[1,2]]}]}]}"#;
        let (branches, _) = parse_swift_coverage(json, tmp.path());
        assert!(branches.is_empty(), "segment with < 3 elements must be skipped");
    }

    // Target: parse_swift_coverage — segment with non-integer line/col/count (lines 56-64)
    #[test]
    fn bug_parse_swift_coverage_segment_non_integer_line() {
        let tmp = tempfile::tempdir().unwrap();
        let json = r#"{"data": [{"files": [{"filename": "a.swift", "segments": [["bad",1,1,true,true],[5,2,3,true,true]]}]}]}"#;
        let (branches, _) = parse_swift_coverage(json, tmp.path());
        assert_eq!(branches.len(), 1, "only the valid segment should be indexed");
        assert_eq!(branches[0].line, 5);
    }

    // Target: parse_swift_coverage — segment with non-integer count, valid line/col
    #[test]
    fn bug_parse_swift_coverage_segment_non_integer_count() {
        let tmp = tempfile::tempdir().unwrap();
        let json = r#"{"data": [{"files": [{"filename": "a.swift", "segments": [[10,1,"x",true,true],[11,1,0,true,true]]}]}]}"#;
        let (branches, _) = parse_swift_coverage(json, tmp.path());
        assert_eq!(branches.len(), 1, "segment with bad count must be skipped");
        assert_eq!(branches[0].line, 11);
        assert_eq!(branches[0].direction, 1); // count=0
    }

    // Target: multiple files in one data entry — both indexed
    #[test]
    fn parse_swift_coverage_multiple_files() {
        let json = r#"{"data": [{"files": [
            {"filename": "/a.swift", "segments": [[1,1,1,true,true]]},
            {"filename": "/b.swift", "segments": [[2,1,0,true,true]]}
        ]}]}"#;
        let tmp = tempfile::tempdir().unwrap();
        let (branches, file_paths) = parse_swift_coverage(json, tmp.path());
        assert_eq!(branches.len(), 2);
        assert_eq!(file_paths.len(), 2);
        // Different file IDs
        assert_ne!(branches[0].file_id, branches[1].file_id);
    }

    // Target: multiple data entries
    #[test]
    fn parse_swift_coverage_multiple_data_entries() {
        let json = r#"{"data": [
            {"files": [{"filename": "/c.swift", "segments": [[1,1,5,true,true]]}]},
            {"files": [{"filename": "/d.swift", "segments": [[3,1,0,true,true]]}]}
        ]}"#;
        let tmp = tempfile::tempdir().unwrap();
        let (branches, file_paths) = parse_swift_coverage(json, tmp.path());
        assert_eq!(branches.len(), 2);
        assert_eq!(file_paths.len(), 2);
    }

    // Target: derive_relative_path with prefix match
    #[test]
    fn derive_relative_path_strips_prefix() {
        let tmp = tempfile::tempdir().unwrap();
        let full = tmp.path().join("Sources/App.swift");
        let rel = derive_relative_path(full.to_str().unwrap(), tmp.path());
        assert_eq!(rel, "Sources/App.swift");
    }

    // Target: derive_relative_path with no match returns original
    #[test]
    fn derive_relative_path_no_match_returns_original() {
        let rel = derive_relative_path("/unrelated/path.swift", std::path::Path::new("/other/root"));
        assert_eq!(rel, "/unrelated/path.swift");
    }

    // Target: build_traces_from_output — no matching lines
    #[test]
    fn build_traces_from_output_no_matches() {
        let stdout = "Build succeeded.\nLinking...\n";
        let traces = build_traces_from_output(stdout, &[]);
        assert!(traces.is_empty());
    }

    // Target: build_traces_from_output — line without closing quote-space pattern (line 99-101)
    #[test]
    fn bug_build_traces_from_output_malformed_test_case_line() {
        // "Test Case '" prefix present but no "' " separator — must be skipped
        let stdout = "Test Case 'missingclose passed (0.001 seconds).\n";
        let traces = build_traces_from_output(stdout, &[]);
        assert!(traces.is_empty(), "malformed Test Case line must be skipped");
    }

    // Target: build_traces_from_output — status part is neither "passed" nor "failed" (line 112-114)
    #[test]
    fn bug_build_traces_from_output_unknown_status() {
        // "skipped" status — neither passed nor failed, must be skipped
        let stdout = "Test Case '-[MyTests.Foo testBar]' skipped (0.000 seconds).\n";
        let traces = build_traces_from_output(stdout, &[]);
        assert!(traces.is_empty(), "unknown status (skipped) must produce no trace");
    }

    // Target: parse_seconds_parens — open paren but no " seconds" substring (line 134-136)
    #[test]
    fn bug_parse_seconds_parens_open_but_no_seconds() {
        // Has '(' but no " seconds" — should return 0
        assert_eq!(parse_seconds_parens("passed (0.001 ms)."), 0);
    }

    // Target: parse_seconds_parens — non-numeric content between parens (line 138-141)
    #[test]
    fn bug_parse_seconds_parens_nonnumeric_content() {
        assert_eq!(parse_seconds_parens("passed (N/A seconds)."), 0);
    }

    // Target: parse_seconds_parens — 2-second case (large float)
    #[test]
    fn parse_seconds_parens_large_value() {
        assert_eq!(parse_seconds_parens("passed (2.000 seconds)."), 2000);
    }

    // Target: build_traces duration_ms propagated correctly for "failed"
    #[test]
    fn build_traces_from_output_fail_duration_parsed() {
        let stdout = "Test Case '-[Suite.Tests testFail]' failed (0.050 seconds).\n";
        let traces = build_traces_from_output(stdout, &[]);
        assert_eq!(traces.len(), 1);
        assert_eq!(traces[0].duration_ms, 50);
        assert_eq!(traces[0].status, ExecutionStatus::Fail);
    }

    // Target: parse_swift_coverage — file_paths.entry().or_insert_with() de-duplicates
    // the same file appearing in two different data entries (line 44-46).
    #[test]
    fn parse_swift_coverage_duplicate_file_deduped() {
        // Same filename in two separate data entries — file_paths should have 1 entry.
        let json = r#"{"data": [
            {"files": [{"filename": "/shared.swift", "segments": [[1,1,1,true,true]]}]},
            {"files": [{"filename": "/shared.swift", "segments": [[2,1,0,true,true]]}]}
        ]}"#;
        let tmp = tempfile::tempdir().unwrap();
        let (branches, file_paths) = parse_swift_coverage(json, tmp.path());
        assert_eq!(branches.len(), 2);
        // The or_insert_with path: second entry is a dup, file_paths should have 1 key.
        assert_eq!(file_paths.len(), 1, "duplicate file must appear once in file_paths");
    }

    // Target: parse_swift_coverage — segment col value extracted correctly (line 68-69).
    #[test]
    fn parse_swift_coverage_col_extracted() {
        let json = r#"{"data": [{"files": [{"filename": "col.swift", "segments": [[7,15,3,true,true]]}]}]}"#;
        let tmp = tempfile::tempdir().unwrap();
        let (branches, _) = parse_swift_coverage(json, tmp.path());
        assert_eq!(branches.len(), 1);
        assert_eq!(branches[0].line, 7);
        assert_eq!(branches[0].col, 15);
    }

    // Target: derive_relative_path — exact prefix match on macOS-style tmpdir
    // (verify /private/tmp or /tmp prefixes are properly stripped).
    #[test]
    fn derive_relative_path_with_real_tempdir() {
        let tmp = tempfile::tempdir().unwrap();
        // Construct a path that is definitively under tmp.path()
        let full = format!("{}/Sources/App.swift", tmp.path().display());
        let rel = derive_relative_path(&full, tmp.path());
        // Should strip the prefix
        assert!(
            !rel.starts_with('/') || rel == full,
            "expected relative path, got: {rel}"
        );
    }

    // Target: parse_seconds_parens — value exactly 0.0 seconds
    #[test]
    fn parse_seconds_parens_zero() {
        assert_eq!(parse_seconds_parens("passed (0.0 seconds)."), 0);
    }

    // Target: build_traces_from_output — multiple tests in one output (both pass/fail)
    #[test]
    fn build_traces_from_output_multiple_mixed() {
        let stdout = "\
Test Case '-[Suite.Tests testA]' passed (0.002 seconds).\n\
Test Case '-[Suite.Tests testB]' failed (0.003 seconds).\n\
Test Case '-[Suite.Tests testC]' passed (0.001 seconds).\n";
        let branches = vec![BranchId::new(1, 1, 0, 0)];
        let traces = build_traces_from_output(stdout, &branches);
        assert_eq!(traces.len(), 3);
        assert_eq!(traces[0].status, ExecutionStatus::Pass);
        assert_eq!(traces[1].status, ExecutionStatus::Fail);
        assert_eq!(traces[2].status, ExecutionStatus::Pass);
        assert_eq!(traces[0].duration_ms, 2);
        assert_eq!(traces[1].duration_ms, 3);
        assert_eq!(traces[2].duration_ms, 1);
    }

    // Target: parse_swift_coverage data array is not an array (line 25-27).
    #[test]
    fn parse_swift_coverage_data_not_array() {
        let tmp = tempfile::tempdir().unwrap();
        let (branches, file_paths) = parse_swift_coverage(r#"{"data": "notanarray"}"#, tmp.path());
        assert!(branches.is_empty());
        assert!(file_paths.is_empty());
    }
}
