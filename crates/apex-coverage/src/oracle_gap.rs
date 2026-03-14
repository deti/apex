use crate::mutation::MutationResult;

/// Gap metric: fraction of mutants that survived the test suite.
#[derive(Debug, Clone)]
pub struct OracleGapScore {
    total: usize,
    survivors: Vec<MutationResult>,
}

impl OracleGapScore {
    pub fn from_results(results: &[MutationResult]) -> Self {
        let mut survivors: Vec<MutationResult> =
            results.iter().filter(|r| !r.killed).cloned().collect();
        survivors.sort_by_key(|s| s.operator.line);
        Self {
            total: results.len(),
            survivors,
        }
    }

    /// Percentage of mutants NOT killed (0.0 = perfect, 100.0 = no test suite).
    pub fn gap_percent(&self) -> f64 {
        if self.total == 0 {
            return 0.0;
        }
        (self.survivors.len() as f64 / self.total as f64) * 100.0
    }

    pub fn survivors(&self) -> &[MutationResult] {
        &self.survivors
    }
    pub fn total(&self) -> usize {
        self.total
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mutation::{MutationKind, MutationOperator, MutationResult};

    fn op(kind: MutationKind, line: u32) -> MutationOperator {
        MutationOperator {
            kind,
            file: "lib.py".into(),
            line,
            original: "x".into(),
            replacement: "y".into(),
        }
    }

    #[test]
    fn score_zero_when_all_killed() {
        let results = vec![MutationResult {
            operator: op(MutationKind::BoundaryShift, 1),
            killed: true,
            killing_tests: vec!["t1".into()],
            detection_margin: 0.9,
        }];
        let score = OracleGapScore::from_results(&results);
        assert_eq!(score.gap_percent(), 0.0);
        assert!(score.survivors().is_empty());
    }

    #[test]
    fn score_fifty_percent_when_half_survive() {
        let results = vec![
            MutationResult {
                operator: op(MutationKind::BoundaryShift, 1),
                killed: true,
                killing_tests: vec![],
                detection_margin: 0.8,
            },
            MutationResult {
                operator: op(MutationKind::ConditionalNegation, 2),
                killed: false,
                killing_tests: vec![],
                detection_margin: 0.0,
            },
        ];
        let score = OracleGapScore::from_results(&results);
        assert!((score.gap_percent() - 50.0).abs() < 0.01);
        assert_eq!(score.survivors().len(), 1);
    }

    #[test]
    fn survivors_sorted_by_line() {
        let results = vec![
            MutationResult {
                operator: op(MutationKind::ArithmeticReplace, 10),
                killed: false,
                killing_tests: vec![],
                detection_margin: 0.0,
            },
            MutationResult {
                operator: op(MutationKind::ReturnValueChange, 3),
                killed: false,
                killing_tests: vec![],
                detection_margin: 0.0,
            },
        ];
        let score = OracleGapScore::from_results(&results);
        let lines: Vec<u32> = score.survivors().iter().map(|s| s.operator.line).collect();
        assert_eq!(lines, vec![3, 10]);
    }

    #[test]
    fn gap_percent_zero_when_empty() {
        let score = OracleGapScore::from_results(&[]);
        assert_eq!(score.gap_percent(), 0.0);
        assert_eq!(score.total(), 0);
        assert!(score.survivors().is_empty());
    }
}
