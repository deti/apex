//! Slice-based change impact analysis.
//! Given a set of changed source lines, identify which tests are affected.
//! Based on arXiv:2508.19056.

use crate::types::BranchIndex;
use std::collections::HashSet;

/// Given a set of changed source lines `(file_id, line)`, return the names
/// of all tests whose branch traces intersect any changed line.
///
/// This is a lightweight approximation of program slicing: any test that
/// executed a branch on a changed line is considered affected.
pub fn change_impact(changed_lines: &[(u64, u32)], index: &BranchIndex) -> Vec<String> {
    if changed_lines.is_empty() {
        return vec![];
    }

    let changed_set: HashSet<(u64, u32)> = changed_lines.iter().copied().collect();

    let mut affected: HashSet<String> = HashSet::new();

    for trace in &index.traces {
        for branch in &trace.branches {
            if changed_set.contains(&(branch.file_id, branch.line)) {
                affected.insert(trace.test_name.clone());
                break; // no need to check more branches in this trace
            }
        }
    }

    let mut result: Vec<String> = affected.into_iter().collect();
    result.sort();
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::TestTrace;
    use apex_core::types::{BranchId, ExecutionStatus};
    use std::collections::HashMap;
    use std::path::PathBuf;

    fn make_branch(file_id: u64, line: u32) -> BranchId {
        BranchId::new(file_id, line, 0, 0)
    }

    fn make_index(traces: Vec<TestTrace>) -> BranchIndex {
        let profiles = BranchIndex::build_profiles(&traces);
        BranchIndex {
            traces,
            profiles,
            file_paths: HashMap::from([(1, PathBuf::from("src/lib.py"))]),
            total_branches: 10,
            covered_branches: 5,
            created_at: String::new(),
            language: apex_core::types::Language::Python,
            target_root: PathBuf::new(),
            source_hash: String::new(),
        }
    }

    #[test]
    fn change_impact_finds_affected_tests() {
        let traces = vec![
            TestTrace {
                test_name: "test_a".into(),
                branches: vec![make_branch(1, 10), make_branch(1, 20)],
                duration_ms: 10,
                status: ExecutionStatus::Pass,
            },
            TestTrace {
                test_name: "test_b".into(),
                branches: vec![make_branch(1, 30), make_branch(2, 5)],
                duration_ms: 10,
                status: ExecutionStatus::Pass,
            },
        ];
        let index = make_index(traces);
        // Changed line 10 in file_id 1 -> should affect test_a
        let changed = vec![(1u64, 10u32)];
        let affected = change_impact(&changed, &index);
        assert!(affected.contains(&"test_a".to_string()));
        assert!(!affected.contains(&"test_b".to_string()));
    }

    #[test]
    fn change_impact_no_match() {
        let traces = vec![TestTrace {
            test_name: "test_a".into(),
            branches: vec![make_branch(1, 10)],
            duration_ms: 10,
            status: ExecutionStatus::Pass,
        }];
        let index = make_index(traces);
        let changed = vec![(99u64, 999u32)];
        let affected = change_impact(&changed, &index);
        assert!(affected.is_empty());
    }

    #[test]
    fn change_impact_multiple_tests_affected() {
        let traces = vec![
            TestTrace {
                test_name: "test_a".into(),
                branches: vec![make_branch(1, 10)],
                duration_ms: 10,
                status: ExecutionStatus::Pass,
            },
            TestTrace {
                test_name: "test_b".into(),
                branches: vec![make_branch(1, 10), make_branch(1, 20)],
                duration_ms: 10,
                status: ExecutionStatus::Pass,
            },
        ];
        let index = make_index(traces);
        let changed = vec![(1u64, 10u32)];
        let affected = change_impact(&changed, &index);
        assert_eq!(affected.len(), 2);
        assert!(affected.contains(&"test_a".to_string()));
        assert!(affected.contains(&"test_b".to_string()));
    }

    #[test]
    fn change_impact_empty_changes() {
        let traces = vec![TestTrace {
            test_name: "test_a".into(),
            branches: vec![make_branch(1, 10)],
            duration_ms: 10,
            status: ExecutionStatus::Pass,
        }];
        let index = make_index(traces);
        let affected = change_impact(&[], &index);
        assert!(affected.is_empty());
    }

    #[test]
    fn change_impact_deduplicates() {
        let traces = vec![TestTrace {
            test_name: "test_a".into(),
            branches: vec![make_branch(1, 10), make_branch(1, 20)],
            duration_ms: 10,
            status: ExecutionStatus::Pass,
        }];
        let index = make_index(traces);
        // Both changed lines are in test_a — should still appear only once
        let changed = vec![(1u64, 10u32), (1u64, 20u32)];
        let affected = change_impact(&changed, &index);
        assert_eq!(affected.len(), 1);
    }
}
