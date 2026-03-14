/// Bridges `OracleGapScore` into the agent's decision loop.
///
/// Based on Meta ACH (ICSE 2025): branches surrounded by surviving mutants
/// need tests that assert on specific values, not just code paths. Maps
/// surviving mutants to assertion hints for the test generation agent.
use std::collections::{HashMap, HashSet};
use apex_coverage::mutation::{MutationKind, MutationResult};

pub struct MutationGuide {
    survivors: Vec<MutationResult>,
    hints: HashMap<u32, String>,
}

impl MutationGuide {
    pub fn new(survivors: Vec<MutationResult>) -> Self {
        let mut hints = HashMap::new();
        for s in &survivors {
            let hint = match s.operator.kind {
                MutationKind::BoundaryShift => "Add assertion on boundary value (e.g., off-by-one).".into(),
                MutationKind::ReturnValueChange => "Assert on the exact return value.".into(),
                MutationKind::ConditionalNegation => "Test both true and false branch outcomes.".into(),
                MutationKind::ArithmeticReplace => "Assert on the computed numeric result.".into(),
                _ => "Add assertion on observable side effect.".into(),
            };
            hints.entry(s.operator.line).or_insert(hint);
        }
        Self { survivors, hints }
    }

    pub fn priority_lines(&self) -> HashSet<u32> {
        self.survivors.iter().map(|s| s.operator.line).collect()
    }

    pub fn hint_for_line(&self, line: u32) -> Option<&str> {
        self.hints.get(&line).map(|s| s.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use apex_coverage::mutation::{MutationKind, MutationOperator, MutationResult};

    fn survived(kind: MutationKind, line: u32) -> MutationResult {
        MutationResult {
            operator: MutationOperator { kind, file: "f.py".into(), line,
                original: "x".into(), replacement: "y".into() },
            killed: false, killing_tests: vec![],
        }
    }

    #[test]
    fn no_survivors_means_no_guidance() {
        let guide = MutationGuide::new(vec![]);
        assert!(guide.priority_lines().is_empty());
    }

    #[test]
    fn survivors_produce_guidance_hints() {
        let guide = MutationGuide::new(vec![
            survived(MutationKind::BoundaryShift, 10),
            survived(MutationKind::ReturnValueChange, 20),
        ]);
        assert!(guide.priority_lines().contains(&10));
        assert!(guide.priority_lines().contains(&20));
    }

    #[test]
    fn hint_for_boundary_suggests_value_assertion() {
        let guide = MutationGuide::new(vec![survived(MutationKind::BoundaryShift, 5)]);
        let hint = guide.hint_for_line(5).unwrap();
        assert!(hint.to_lowercase().contains("boundary") || hint.to_lowercase().contains("value"));
    }
}
