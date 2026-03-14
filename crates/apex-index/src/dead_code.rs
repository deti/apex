//! Dead code detection with optional LLM validation.
//! Identifies branches that are never hit across all test traces,
//! then optionally asks an LLM whether each is genuinely dead.

use crate::types::{branch_key, BranchIndex};
use apex_core::types::BranchId;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::PathBuf;

/// A branch suspected of being dead code.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeadCodeCandidate {
    pub branch: BranchId,
    pub file_path: Option<PathBuf>,
    pub reason: String,
}

/// Result of LLM validation of a dead code candidate.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeadCodeResult {
    pub candidate: DeadCodeCandidate,
    pub confirmed_dead: bool,
    pub llm_explanation: Option<String>,
}

/// Detects dead code by finding branches never hit in any test trace.
pub struct DeadCodeDetector;

impl DeadCodeDetector {
    /// Find branches from `all_branches` that appear in no test trace.
    pub fn detect(index: &BranchIndex, all_branches: &[BranchId]) -> Vec<DeadCodeCandidate> {
        let covered_keys: HashSet<String> = index.profiles.keys().cloned().collect();

        all_branches
            .iter()
            .filter(|b| !covered_keys.contains(&branch_key(b)))
            .map(|b| DeadCodeCandidate {
                branch: b.clone(),
                file_path: index.file_paths.get(&b.file_id).cloned(),
                reason: "never hit in any test trace".to_string(),
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::TestTrace;
    use apex_core::types::{ExecutionStatus, Language};
    use std::collections::HashMap;
    use std::path::PathBuf;

    fn make_branch(file_id: u64, line: u32) -> BranchId {
        BranchId::new(file_id, line, 0, 0)
    }

    #[test]
    fn dead_code_candidate_creation() {
        let c = DeadCodeCandidate {
            branch: make_branch(1, 42),
            file_path: Some(PathBuf::from("src/lib.py")),
            reason: "never hit in any test trace".to_string(),
        };
        assert_eq!(c.branch.line, 42);
    }

    #[test]
    fn detect_finds_uncovered_branches() {
        let all_branches = vec![make_branch(1, 10), make_branch(1, 20), make_branch(1, 30)];
        let traces = vec![TestTrace {
            test_name: "t1".into(),
            branches: vec![make_branch(1, 10)],
            duration_ms: 10,
            status: ExecutionStatus::Pass,
        }];
        let index = BranchIndex {
            traces: traces.clone(),
            profiles: BranchIndex::build_profiles(&traces),
            file_paths: HashMap::from([(1, PathBuf::from("src/lib.py"))]),
            total_branches: 3,
            covered_branches: 1,
            created_at: String::new(),
            language: Language::Python,
            target_root: PathBuf::new(),
            source_hash: String::new(),
        };
        let candidates = DeadCodeDetector::detect(&index, &all_branches);
        assert_eq!(candidates.len(), 2);
        let lines: Vec<u32> = candidates.iter().map(|c| c.branch.line).collect();
        assert!(lines.contains(&20));
        assert!(lines.contains(&30));
    }

    #[test]
    fn detect_no_dead_code() {
        let branches = vec![make_branch(1, 10)];
        let traces = vec![TestTrace {
            test_name: "t1".into(),
            branches: branches.clone(),
            duration_ms: 10,
            status: ExecutionStatus::Pass,
        }];
        let index = BranchIndex {
            traces: traces.clone(),
            profiles: BranchIndex::build_profiles(&traces),
            file_paths: HashMap::new(),
            total_branches: 1,
            covered_branches: 1,
            created_at: String::new(),
            language: Language::Python,
            target_root: PathBuf::new(),
            source_hash: String::new(),
        };
        let candidates = DeadCodeDetector::detect(&index, &branches);
        assert!(candidates.is_empty());
    }

    #[test]
    fn dead_code_result_creation() {
        let r = DeadCodeResult {
            candidate: DeadCodeCandidate {
                branch: make_branch(1, 42),
                file_path: None,
                reason: "never hit".into(),
            },
            confirmed_dead: true,
            llm_explanation: Some("This branch is guarded by an always-false condition.".into()),
        };
        assert!(r.confirmed_dead);
    }
}
