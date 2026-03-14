//! Mutation testing types for oracle gap analysis and metamorphic adequacy.

use serde::{Deserialize, Serialize};

/// Classification of source-code mutation operators.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum MutationKind {
    /// Remove a statement entirely.
    StatementDeletion,
    /// Negate a conditional expression (e.g., `if x` → `if !x`).
    ConditionalNegation,
    /// Change return value (e.g., `return x` → `return 0`).
    ReturnValueChange,
    /// Replace arithmetic operator (e.g., `+` → `-`).
    ArithmeticReplace,
    /// Shift comparison boundary (e.g., `<` → `<=`).
    BoundaryShift,
    /// Remove exception handler or try block.
    ExceptionRemoval,
    /// Replace constant with another value.
    ConstantReplace,
}

/// A single mutation applied to source code.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MutationOperator {
    /// What kind of mutation.
    pub kind: MutationKind,
    /// File where the mutation is applied.
    pub file: String,
    /// Line number of the mutation.
    pub line: u32,
    /// Original source text.
    pub original: String,
    /// Replacement source text.
    pub replacement: String,
}

/// Result of running a mutant against the test suite.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MutationResult {
    /// The mutation that was applied.
    pub operator: MutationOperator,
    /// Whether the test suite detected (killed) this mutant.
    pub killed: bool,
    /// Which tests killed this mutant (empty if survived).
    pub killing_tests: Vec<String>,
    /// How close was detection? 1.0 = strong kill, 0.01 = barely detected, 0.0 = not killed.
    #[serde(default)]
    pub detection_margin: f64,
}

/// Metamorphic adequacy score — goes beyond binary kill/survive.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetamorphicScore {
    /// Traditional mutation score: killed / total.
    pub mutation_score: f64,
    /// Ratio of killed mutants with detection_margin > 0.5.
    pub detection_ratio: f64,
    /// Mutants killed but with very low margin (detection_margin < 0.1).
    pub weak_mutations: Vec<MutationOperator>,
}

/// Compute metamorphic adequacy from mutation testing results.
///
/// Goes beyond binary kill/survive:
/// - `mutation_score`: fraction of killed mutants.
/// - `detection_ratio`: fraction of killed mutants with detection_margin > 0.5.
/// - `weak_mutations`: killed mutants with detection_margin < 0.1 (brittle detection).
pub fn metamorphic_adequacy(results: &[MutationResult]) -> MetamorphicScore {
    if results.is_empty() {
        return MetamorphicScore {
            mutation_score: 1.0,
            detection_ratio: 1.0,
            weak_mutations: vec![],
        };
    }

    let total = results.len() as f64;
    let killed: Vec<&MutationResult> = results.iter().filter(|r| r.killed).collect();
    let killed_count = killed.len() as f64;

    let mutation_score = killed_count / total;

    let strong_kills = killed.iter().filter(|r| r.detection_margin > 0.5).count() as f64;
    let detection_ratio = if killed_count > 0.0 {
        strong_kills / killed_count
    } else {
        0.0
    };

    let weak_mutations: Vec<MutationOperator> = killed
        .iter()
        .filter(|r| r.detection_margin < 0.1)
        .map(|r| r.operator.clone())
        .collect();

    MetamorphicScore {
        mutation_score,
        detection_ratio,
        weak_mutations,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mutation_kind_eq_and_hash() {
        use std::collections::HashSet;
        let mut set = HashSet::new();
        set.insert(MutationKind::StatementDeletion);
        set.insert(MutationKind::ConditionalNegation);
        set.insert(MutationKind::StatementDeletion); // duplicate
        assert_eq!(set.len(), 2);
    }

    #[test]
    fn mutation_kind_all_variants_distinct() {
        let variants = [
            MutationKind::StatementDeletion,
            MutationKind::ConditionalNegation,
            MutationKind::ReturnValueChange,
            MutationKind::ArithmeticReplace,
            MutationKind::BoundaryShift,
            MutationKind::ExceptionRemoval,
            MutationKind::ConstantReplace,
        ];
        for (i, a) in variants.iter().enumerate() {
            for (j, b) in variants.iter().enumerate() {
                if i == j {
                    assert_eq!(a, b);
                } else {
                    assert_ne!(a, b);
                }
            }
        }
    }

    #[test]
    fn mutation_operator_serde_roundtrip() {
        let op = MutationOperator {
            kind: MutationKind::ArithmeticReplace,
            file: "src/math.py".into(),
            line: 42,
            original: "a + b".into(),
            replacement: "a - b".into(),
        };
        let json = serde_json::to_string(&op).unwrap();
        let back: MutationOperator = serde_json::from_str(&json).unwrap();
        assert_eq!(back.kind, MutationKind::ArithmeticReplace);
        assert_eq!(back.file, "src/math.py");
        assert_eq!(back.line, 42);
        assert_eq!(back.original, "a + b");
        assert_eq!(back.replacement, "a - b");
    }

    #[test]
    fn mutation_result_killed() {
        let result = MutationResult {
            operator: MutationOperator {
                kind: MutationKind::BoundaryShift,
                file: "lib.py".into(),
                line: 10,
                original: "x < 5".into(),
                replacement: "x <= 5".into(),
            },
            killed: true,
            killing_tests: vec!["test_boundary".into()],
            detection_margin: 0.9,
        };
        assert!(result.killed);
        assert_eq!(result.killing_tests.len(), 1);
    }

    #[test]
    fn mutation_result_survived() {
        let result = MutationResult {
            operator: MutationOperator {
                kind: MutationKind::StatementDeletion,
                file: "lib.py".into(),
                line: 20,
                original: "log(msg)".into(),
                replacement: "pass".into(),
            },
            killed: false,
            killing_tests: vec![],
            detection_margin: 0.0,
        };
        assert!(!result.killed);
        assert!(result.killing_tests.is_empty());
    }

    fn make_result(
        kind: MutationKind,
        line: u32,
        killed: bool,
        detection_margin: f64,
    ) -> MutationResult {
        MutationResult {
            operator: MutationOperator {
                kind,
                file: "src/lib.py".into(),
                line,
                original: "x".into(),
                replacement: "y".into(),
            },
            killed,
            killing_tests: if killed { vec!["t1".into()] } else { vec![] },
            detection_margin,
        }
    }

    #[test]
    fn metamorphic_adequacy_all_killed() {
        let results = vec![
            make_result(MutationKind::ArithmeticReplace, 1, true, 0.9),
            make_result(MutationKind::ConditionalNegation, 2, true, 0.7),
        ];
        let score = metamorphic_adequacy(&results);
        assert!((score.mutation_score - 1.0).abs() < f64::EPSILON);
        assert!((score.detection_ratio - 1.0).abs() < f64::EPSILON);
        assert!(score.weak_mutations.is_empty());
    }

    #[test]
    fn metamorphic_adequacy_with_weak_and_survived() {
        let results = vec![
            make_result(MutationKind::ArithmeticReplace, 1, true, 0.05), // weak kill
            make_result(MutationKind::BoundaryShift, 2, false, 0.0),     // survived
        ];
        let score = metamorphic_adequacy(&results);
        assert!((score.mutation_score - 0.5).abs() < f64::EPSILON);
        assert!((score.detection_ratio - 0.0).abs() < f64::EPSILON); // neither has margin > 0.5
        assert_eq!(score.weak_mutations.len(), 1);
    }

    #[test]
    fn metamorphic_adequacy_empty() {
        let score = metamorphic_adequacy(&[]);
        assert!((score.mutation_score - 1.0).abs() < f64::EPSILON);
        assert!((score.detection_ratio - 1.0).abs() < f64::EPSILON);
        assert!(score.weak_mutations.is_empty());
    }
}
