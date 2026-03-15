use apex_core::hash::fnv1a_hash;
use apex_core::types::BranchId;
use std::collections::HashMap;
use std::path::PathBuf;

/// Parse SimpleCov JSON coverage into branch data for indexing.
///
/// Supports:
/// - Line coverage (`lines` array — all SimpleCov versions)
/// - Branch coverage (`branches` object — SimpleCov 0.18+ with `enable_coverage :branch`)
pub fn parse_simplecov_coverage(json: &str) -> RubyCoverageData {
    let mut data = RubyCoverageData::default();

    let parsed: serde_json::Value = match serde_json::from_str(json) {
        Ok(v) => v,
        Err(_) => return data,
    };

    let coverage = match parsed.get("coverage").and_then(|c| c.as_object()) {
        Some(c) => c,
        None => return data,
    };

    for (file_path, file_data) in coverage {
        let file_id = fnv1a_hash(file_path);
        data.file_paths.insert(file_id, PathBuf::from(file_path));

        // --- Line coverage ---
        let lines = file_data
            .get("lines")
            .and_then(|l| l.as_array())
            .cloned()
            .unwrap_or_default();

        for (i, val) in lines.iter().enumerate() {
            if val.is_null() {
                continue;
            }
            let count = val.as_u64().unwrap_or(0);
            let line = (i + 1) as u32;
            let branch = BranchId::new(file_id, line, 0, 0);
            data.all_branches.push(branch.clone());
            data.total += 1;
            if count > 0 {
                data.covered_branches.push(branch);
                data.covered += 1;
            }
        }

        // --- Branch coverage (SimpleCov 0.18+ with enable_coverage :branch) ---
        //
        // SimpleCov encodes branches as a nested object:
        //   "branches": {
        //     "type:line:branch_index": [positive_count, negative_count],
        //     ...
        //   }
        //
        // Alternatively (SimpleCov 0.22+), each branch condition is a separate key:
        //   "branches": {
        //     "[\"if\", 0, 5, 8, 5, 33]": [3, 0],
        //     ...
        //   }
        //
        // We treat each branch key as one condition: the first element of the value
        // array is the "true/taken" count, the second is the "false/not-taken" count.
        // When not present or null we default to 0.
        if let Some(branches_obj) = file_data.get("branches").and_then(|b| b.as_object()) {
            for (_branch_key, counts) in branches_obj {
                let arr = match counts.as_array() {
                    Some(a) => a,
                    None => continue,
                };

                // taken count — first element
                let taken_count = arr.first().and_then(|v| v.as_u64()).unwrap_or(0);
                // not-taken count — second element
                let not_taken_count = arr.get(1).and_then(|v| v.as_u64()).unwrap_or(0);

                // Derive a stable line number from the branch key.
                // Key formats vary; we extract a numeric component for the line.
                let line = extract_line_from_branch_key(_branch_key);

                // Encode "taken" direction as a separate BranchId (direction = 0)
                let branch_taken = BranchId::new(file_id, line, 0, 0);
                data.all_branches.push(branch_taken.clone());
                data.branch_total += 1;
                if taken_count > 0 {
                    data.covered_branches.push(branch_taken);
                    data.branch_covered += 1;
                }

                // Encode "not-taken" direction as a separate BranchId (direction = 1)
                let branch_not_taken = BranchId::new(file_id, line, 0, 1);
                data.all_branches.push(branch_not_taken.clone());
                data.branch_total += 1;
                if not_taken_count > 0 {
                    data.covered_branches.push(branch_not_taken);
                    data.branch_covered += 1;
                }
            }
        }
    }

    data
}

/// Parse per-test SimpleCov JSON runs into a test-to-branch mapping.
///
/// SimpleCov can be configured to emit per-test coverage files when running
/// one test at a time (e.g. via `RSpec::Core::Runner` or parallel-tests with
/// per-file SimpleCov output). This function parses a slice of `(test_name,
/// json)` pairs and returns a map from test name → covered `BranchId`s.
///
/// Each JSON blob must be a valid SimpleCov JSON file. Only covered branches
/// (count > 0 for line coverage, first/second element > 0 for branch coverage)
/// are included in the returned map.
pub fn parse_per_test_coverage(runs: &[(&str, &str)]) -> HashMap<String, Vec<BranchId>> {
    let mut result: HashMap<String, Vec<BranchId>> = HashMap::new();

    for (test_name, json) in runs {
        let cov = parse_simplecov_coverage(json);
        result.insert(test_name.to_string(), cov.covered_branches);
    }

    result
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Extract a line number from a SimpleCov branch key.
///
/// SimpleCov uses several key formats:
/// - `"if:5:0"` — type:line:index
/// - `"[\"if\", 0, 5, 8, 5, 33]"` — JSON-encoded array where index 2 is line
/// - `"condition_10_1"` — positional
///
/// We scan for the first numeric token ≥ 1 as the line number, falling back
/// to 0 if none is found (will map to a synthetic line 0 BranchId).
fn extract_line_from_branch_key(key: &str) -> u32 {
    // Try to find numeric substrings in the key
    let mut num_buf = String::new();
    let mut candidates: Vec<u32> = Vec::new();

    for ch in key.chars() {
        if ch.is_ascii_digit() {
            num_buf.push(ch);
        } else if !num_buf.is_empty() {
            if let Ok(n) = num_buf.parse::<u32>() {
                candidates.push(n);
            }
            num_buf.clear();
        }
    }
    if !num_buf.is_empty() {
        if let Ok(n) = num_buf.parse::<u32>() {
            candidates.push(n);
        }
    }

    // Return the first candidate ≥ 1, or 0 if none
    candidates.into_iter().find(|&n| n >= 1).unwrap_or(0)
}

#[derive(Debug, Default)]
pub struct RubyCoverageData {
    /// All branch points (line + explicit branch), covered and uncovered.
    pub all_branches: Vec<BranchId>,
    /// Only branches with a hit count > 0.
    pub covered_branches: Vec<BranchId>,
    /// Maps file_id → file path.
    pub file_paths: HashMap<u64, PathBuf>,
    /// Total executable line slots.
    pub total: usize,
    /// Covered line slots.
    pub covered: usize,
    /// Total explicit branch directions (from `branches` key).
    pub branch_total: usize,
    /// Covered explicit branch directions.
    pub branch_covered: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    // ----- basic line coverage -----

    #[test]
    fn parse_basic() {
        let json = r#"{"coverage":{"app.rb":{"lines":[null,1,0,1]}}}"#;
        let data = parse_simplecov_coverage(json);
        assert_eq!(data.total, 3);
        assert_eq!(data.covered, 2);
    }

    #[test]
    fn parse_empty() {
        let data = parse_simplecov_coverage(r#"{"coverage":{}}"#);
        assert_eq!(data.total, 0);
    }

    #[test]
    fn parse_invalid() {
        let data = parse_simplecov_coverage("bad");
        assert_eq!(data.total, 0);
    }

    #[test]
    fn parse_multiple_files() {
        let json = r#"{"coverage":{"a.rb":{"lines":[1]},"b.rb":{"lines":[0,1]}}}"#;
        let data = parse_simplecov_coverage(json);
        assert_eq!(data.total, 3);
        assert_eq!(data.file_paths.len(), 2);
    }

    // ----- branch coverage (SimpleCov 0.18+ format) -----

    #[test]
    fn parse_branch_coverage_basic() {
        let json = r#"{
            "coverage": {
                "app.rb": {
                    "lines": [null, 1, 0],
                    "branches": {
                        "if:2:0": [3, 0],
                        "if:2:1": [0, 3]
                    }
                }
            }
        }"#;
        let data = parse_simplecov_coverage(json);
        // 2 lines (lines 2 and 3), 4 branch directions (2 conditions × 2 directions)
        assert_eq!(data.total, 2);
        assert_eq!(data.branch_total, 4);
        // First condition: taken=3 (hit), not-taken=0 (miss)
        // Second condition: taken=0 (miss), not-taken=3 (hit)
        assert_eq!(data.branch_covered, 2);
    }

    #[test]
    fn parse_branch_coverage_all_hit() {
        let json = r#"{
            "coverage": {
                "lib.rb": {
                    "lines": [1, 1],
                    "branches": {
                        "if:1:0": [5, 3]
                    }
                }
            }
        }"#;
        let data = parse_simplecov_coverage(json);
        assert_eq!(data.branch_total, 2);
        assert_eq!(data.branch_covered, 2);
    }

    #[test]
    fn parse_branch_coverage_none_hit() {
        let json = r#"{
            "coverage": {
                "lib.rb": {
                    "lines": [0],
                    "branches": {
                        "if:1:0": [0, 0]
                    }
                }
            }
        }"#;
        let data = parse_simplecov_coverage(json);
        assert_eq!(data.branch_total, 2);
        assert_eq!(data.branch_covered, 0);
    }

    #[test]
    fn parse_branch_coverage_json_array_key_format() {
        // SimpleCov 0.22+ JSON-encoded array key: "[\"if\", 0, 5, 8, 5, 33]"
        let json = r#"{
            "coverage": {
                "app.rb": {
                    "lines": [null, null, null, null, null, 1],
                    "branches": {
                        "[\"if\", 0, 5, 8, 5, 33]": [2, 1]
                    }
                }
            }
        }"#;
        let data = parse_simplecov_coverage(json);
        assert_eq!(data.branch_total, 2);
        assert_eq!(data.branch_covered, 2);
        // Line extracted from key: first number ≥ 1 — depends on key content
        // "if", 0, 5, ... → first ≥ 1 is 5
        let line5_branches: Vec<_> = data.all_branches.iter().filter(|b| b.line == 5).collect();
        assert_eq!(
            line5_branches.len(),
            2,
            "expected 2 branch directions at line 5"
        );
    }

    #[test]
    fn parse_branch_no_branches_key() {
        // File without `branches` key — should not error and branch totals stay 0
        let json = r#"{"coverage":{"app.rb":{"lines":[1,0,1]}}}"#;
        let data = parse_simplecov_coverage(json);
        assert_eq!(data.branch_total, 0);
        assert_eq!(data.branch_covered, 0);
    }

    #[test]
    fn parse_branch_with_null_value_skipped() {
        // If branch value is not an array (e.g., null), it should be skipped
        let json = r#"{
            "coverage": {
                "app.rb": {
                    "lines": [1],
                    "branches": {
                        "if:1:0": null
                    }
                }
            }
        }"#;
        let data = parse_simplecov_coverage(json);
        assert_eq!(data.branch_total, 0);
    }

    // ----- per-test coverage mapping -----

    #[test]
    fn per_test_coverage_basic() {
        let test1_json = r#"{"coverage":{"a.rb":{"lines":[1,0]}}}"#;
        let test2_json = r#"{"coverage":{"a.rb":{"lines":[0,1]}}}"#;

        let runs = vec![("test_a", test1_json), ("test_b", test2_json)];
        let map = parse_per_test_coverage(&runs);

        assert_eq!(map.len(), 2);
        // test_a covers line 1, test_b covers line 2
        let test_a_branches = &map["test_a"];
        let test_b_branches = &map["test_b"];
        assert_eq!(test_a_branches.len(), 1);
        assert_eq!(test_b_branches.len(), 1);
        assert_ne!(
            test_a_branches[0].line, test_b_branches[0].line,
            "tests cover different lines"
        );
    }

    #[test]
    fn per_test_coverage_empty_runs() {
        let map = parse_per_test_coverage(&[]);
        assert!(map.is_empty());
    }

    #[test]
    fn per_test_coverage_invalid_json_yields_empty_branch_list() {
        let runs = vec![("bad_test", "not json")];
        let map = parse_per_test_coverage(&runs);
        assert!(map["bad_test"].is_empty());
    }

    #[test]
    fn per_test_coverage_with_branch_coverage() {
        let json = r#"{
            "coverage": {
                "lib.rb": {
                    "lines": [1, 0],
                    "branches": {
                        "if:1:0": [3, 0]
                    }
                }
            }
        }"#;
        let runs = vec![("spec_a", json)];
        let map = parse_per_test_coverage(&runs);
        // spec_a covers: line 1 (from lines), taken branch at line 1 (from branches)
        // not-taken branch (0) is NOT in covered_branches
        let branches = &map["spec_a"];
        assert!(!branches.is_empty());
        // At least the line coverage branch (line 1, direction 0) and branch taken direction
        let taken: Vec<_> = branches.iter().filter(|b| b.direction == 0).collect();
        assert!(!taken.is_empty());
    }

    // ----- extract_line_from_branch_key helper -----

    #[test]
    fn extract_line_colon_format() {
        // "if:5:0" → first numeric ≥ 1 = 5
        assert_eq!(extract_line_from_branch_key("if:5:0"), 5);
    }

    #[test]
    fn extract_line_json_array_format() {
        // "[\"if\", 0, 5, 8, 5, 33]" → first numeric ≥ 1 = 5 (skips 0)
        assert_eq!(extract_line_from_branch_key(r#"["if", 0, 5, 8, 5, 33]"#), 5);
    }

    #[test]
    fn extract_line_no_numbers() {
        assert_eq!(extract_line_from_branch_key("condition_key"), 0);
    }

    #[test]
    fn extract_line_all_zero() {
        // "0:0:0" — all zeros, returns 0 (no number ≥ 1)
        assert_eq!(extract_line_from_branch_key("0:0:0"), 0);
    }
}
