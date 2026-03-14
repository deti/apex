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
        };
        assert!(!result.killed);
        assert!(result.killing_tests.is_empty());
    }
}
