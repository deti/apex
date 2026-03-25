use apex_core::config::SemanticConfig;
use apex_core::types::ExecutionResult;
use apex_coverage::semantic::extract_signals;

#[derive(Debug)]
pub struct SemanticFeedback {
    pub branch_weight: f64,
    pub semantic_weight: f64,
}

impl SemanticFeedback {
    pub fn new(branch_weight: f64, semantic_weight: f64) -> Self {
        Self {
            branch_weight,
            semantic_weight,
        }
    }
}

impl Default for SemanticFeedback {
    fn default() -> Self {
        let cfg = SemanticConfig::default();
        Self::new(cfg.branch_weight, cfg.semantic_weight)
    }
}

impl SemanticFeedback {
    /// Create from a [`SemanticConfig`].
    pub fn from_config(config: &SemanticConfig) -> Self {
        Self::new(config.branch_weight, config.semantic_weight)
    }
}

#[derive(Debug, Default)]
pub struct SemFeedbackScore {
    pub branch_score: f64,
    pub semantic_score: f64,
}

impl SemFeedbackScore {
    pub fn total(&self) -> f64 {
        self.branch_score + self.semantic_score
    }
}

impl SemanticFeedback {
    pub fn score(&self, result: &ExecutionResult) -> SemFeedbackScore {
        let branch_score = result.new_branches.len() as f64 * self.branch_weight;
        let sig = extract_signals(&[], &result.stderr);
        let semantic_score = sig.assertion_distance * self.semantic_weight;
        SemFeedbackScore {
            branch_score,
            semantic_score,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use apex_core::types::{ExecutionResult, ExecutionStatus, SeedId};

    fn make_result(new_branches: usize, stderr: &str) -> ExecutionResult {
        ExecutionResult {
            seed_id: SeedId::new(),
            status: ExecutionStatus::Pass,
            new_branches: (0..new_branches)
                .map(|i| apex_core::types::BranchId::new(1, i as u32, 0, 0))
                .collect(),
            trace: None,
            duration_ms: 1,
            stdout: String::new(),
            stderr: stderr.into(),
            input: None,
            resource_metrics: None,
        }
    }

    #[test]
    fn zero_score_on_no_coverage_no_stderr() {
        let fb = SemanticFeedback::default();
        let score = fb.score(&make_result(0, ""));
        assert_eq!(score.total(), 0.0);
    }

    #[test]
    fn new_branches_contribute_to_score() {
        let fb = SemanticFeedback::default();
        let score = fb.score(&make_result(3, ""));
        assert!(score.total() > 0.0);
    }

    #[test]
    fn assertion_distance_contributes_when_nonzero() {
        let fb = SemanticFeedback::default();
        let s1 = fb.score(&make_result(0, ""));
        let s2 = fb.score(&make_result(0, "AssertionError: expected 5 got 100"));
        assert!(s2.total() > s1.total());
    }
}
