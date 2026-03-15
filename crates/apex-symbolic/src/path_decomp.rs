//! Path decomposition for long constraint chains.
//! Based on the AutoBug paper — splits long chains into independent
//! sub-problems that can be solved separately.

use crate::smtlib::extract_variables;
use crate::traits::Solver;
use apex_core::error::Result;
use apex_core::types::InputSeed;
use std::collections::{HashMap, HashSet};

/// Decomposes long constraint chains into independent sub-problems.
pub struct PathDecomposer;

impl PathDecomposer {
    /// Split constraints into independent partitions based on shared variables.
    ///
    /// Two constraints are in the same partition if they share any variable,
    /// transitively. Uses union-find to group constraints.
    pub fn decompose(constraints: &[String]) -> Vec<Vec<String>> {
        if constraints.is_empty() {
            return vec![];
        }

        // Extract variables for each constraint
        let var_sets: Vec<HashSet<String>> = constraints
            .iter()
            .map(|c| extract_variables(c).into_iter().collect())
            .collect();

        // Union-find: parent[i] = representative of constraint i's group
        let n = constraints.len();
        let mut parent: Vec<usize> = (0..n).collect();

        // Find with path compression
        fn find(parent: &mut [usize], i: usize) -> usize {
            if parent[i] != i {
                parent[i] = find(parent, parent[i]);
            }
            parent[i]
        }

        // Union constraints that share variables
        for i in 0..n {
            for j in (i + 1)..n {
                if !var_sets[i].is_disjoint(&var_sets[j]) {
                    let ri = find(&mut parent, i);
                    let rj = find(&mut parent, j);
                    if ri != rj {
                        parent[ri] = rj;
                    }
                }
            }
        }

        // Group constraints by representative
        let mut groups: HashMap<usize, Vec<String>> = HashMap::new();
        for (i, constraint) in constraints.iter().enumerate().take(n) {
            let root = find(&mut parent, i);
            groups.entry(root).or_default().push(constraint.clone());
        }

        let mut result: Vec<Vec<String>> = groups.into_values().collect();
        result.sort_by_key(|v| v.len());
        result
    }

    /// Solve each partition independently and merge results.
    pub fn solve_decomposed(
        parts: &[Vec<String>],
        solver: &dyn Solver,
    ) -> Result<Option<InputSeed>> {
        if parts.is_empty() {
            return Ok(Some(InputSeed::new(
                vec![],
                apex_core::types::SeedOrigin::Symbolic,
            )));
        }
        let mut combined_data: Vec<u8> = Vec::new();

        for part in parts {
            match solver.solve(part, false)? {
                Some(seed) => combined_data.extend_from_slice(&seed.data),
                None => return Ok(None), // One partition is UNSAT => whole thing is UNSAT
            }
        }

        if combined_data.is_empty() {
            Ok(None)
        } else {
            Ok(Some(InputSeed::new(
                combined_data,
                apex_core::types::SeedOrigin::Symbolic,
            )))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decompose_independent_constraints() {
        let constraints = vec![
            "(> x 0)".to_string(),
            "(< y 10)".to_string(),
            "(= z 5)".to_string(),
        ];
        let parts = PathDecomposer::decompose(&constraints);
        // x, y, z are independent — each gets its own partition
        assert_eq!(parts.len(), 3);
    }

    #[test]
    fn decompose_shared_variable_groups() {
        let constraints = vec![
            "(> x 0)".to_string(),
            "(< x 10)".to_string(), // shares x with first
            "(= y 5)".to_string(),  // independent
        ];
        let parts = PathDecomposer::decompose(&constraints);
        // x constraints grouped, y separate => 2 partitions
        assert_eq!(parts.len(), 2);
    }

    #[test]
    fn decompose_all_shared() {
        let constraints = vec![
            "(> x 0)".to_string(),
            "(and (> x 0) (< y 10))".to_string(), // links x and y
            "(< y 5)".to_string(),
        ];
        let parts = PathDecomposer::decompose(&constraints);
        // All linked through x-y chain => 1 partition
        assert_eq!(parts.len(), 1);
        assert_eq!(parts[0].len(), 3);
    }

    #[test]
    fn decompose_empty() {
        let parts = PathDecomposer::decompose(&[]);
        assert!(parts.is_empty());
    }

    #[test]
    fn decompose_single_constraint() {
        let constraints = vec!["(> x 0)".to_string()];
        let parts = PathDecomposer::decompose(&constraints);
        assert_eq!(parts.len(), 1);
        assert_eq!(parts[0].len(), 1);
    }

    // ==================================================================
    // Bug-hunting tests
    // ==================================================================

    /// Empty parts is trivially SAT — returns Some with empty data.
    #[test]
    fn solve_decomposed_empty_returns_some() {
        use crate::traits::{Solver, SolverLogic};
        use apex_core::types::InputSeed;

        struct DummySolver;
        impl Solver for DummySolver {
            fn solve(
                &self,
                _constraints: &[String],
                _negate_last: bool,
            ) -> Result<Option<InputSeed>> {
                Ok(Some(InputSeed::new(
                    vec![42],
                    apex_core::types::SeedOrigin::Symbolic,
                )))
            }
            fn set_logic(&mut self, _logic: SolverLogic) {}
            fn name(&self) -> &str {
                "dummy"
            }
        }

        let result = PathDecomposer::solve_decomposed(&[], &DummySolver).unwrap();
        // BUG: returns None for empty parts, treating it as UNSAT
        // An empty constraint set should be trivially SAT
        assert!(
            result.is_some(),
            "empty parts should return Some (trivially SAT)"
        );
        assert!(result.unwrap().data.is_empty());
    }

    /// Constraints with no variables (e.g., "(true)") should each get
    /// their own partition since they share no variables.
    #[test]
    fn decompose_no_variable_constraints() {
        let constraints = vec!["(true)".to_string(), "(false)".to_string()];
        let parts = PathDecomposer::decompose(&constraints);
        // "true" and "false" are keywords, so they have no extracted variables.
        // Two constraints with empty variable sets: are they in the same partition?
        // They are NOT disjoint (empty sets are not disjoint in Rust's definition:
        // is_disjoint returns true for two empty sets), so they should be separate.
        assert_eq!(
            parts.len(),
            2,
            "no-variable constraints should be in separate partitions"
        );
    }

    /// Transitive variable sharing: a-b share x, b-c share y,
    /// so all three should be in one partition.
    #[test]
    fn decompose_transitive_sharing() {
        let constraints = vec![
            "(> x 0)".to_string(),
            "(and (< x 10) (> y 0))".to_string(), // links x and y
            "(< y 5)".to_string(),
        ];
        let parts = PathDecomposer::decompose(&constraints);
        assert_eq!(parts.len(), 1);
        assert_eq!(parts[0].len(), 3);
    }

    /// Two completely disjoint groups should produce two partitions.
    #[test]
    fn decompose_two_disjoint_groups() {
        let constraints = vec![
            "(> x 0)".to_string(),
            "(< x 10)".to_string(),
            "(> y 0)".to_string(),
            "(< y 10)".to_string(),
        ];
        let parts = PathDecomposer::decompose(&constraints);
        assert_eq!(parts.len(), 2);
        // Sorted by length, both groups have 2 constraints
        assert_eq!(parts[0].len(), 2);
        assert_eq!(parts[1].len(), 2);
    }

    /// BUG: solve_decomposed returns None when ANY partition is UNSAT,
    /// which is correct for conjunction. But verify the short-circuit:
    /// if the first partition is UNSAT, remaining partitions are never solved.
    #[test]
    fn solve_decomposed_short_circuits_on_unsat() {
        use crate::traits::{Solver, SolverLogic};
        use apex_core::types::InputSeed;
        use std::sync::atomic::{AtomicUsize, Ordering};

        static CALL_COUNT: AtomicUsize = AtomicUsize::new(0);

        struct CountingSolver;
        impl Solver for CountingSolver {
            fn solve(
                &self,
                _constraints: &[String],
                _negate_last: bool,
            ) -> Result<Option<InputSeed>> {
                CALL_COUNT.fetch_add(1, Ordering::SeqCst);
                Ok(None) // always UNSAT
            }
            fn set_logic(&mut self, _logic: SolverLogic) {}
            fn name(&self) -> &str {
                "counting"
            }
        }

        CALL_COUNT.store(0, Ordering::SeqCst);
        let parts = vec![vec!["(> x 0)".to_string()], vec!["(> y 0)".to_string()]];
        let result = PathDecomposer::solve_decomposed(&parts, &CountingSolver).unwrap();
        assert!(result.is_none());
        // Should have short-circuited after first UNSAT
        assert_eq!(
            CALL_COUNT.load(Ordering::SeqCst),
            1,
            "should short-circuit after first UNSAT partition"
        );
    }

    /// solve_decomposed concatenates data from all partitions.
    #[test]
    fn solve_decomposed_concatenates_partition_results() {
        use crate::traits::{Solver, SolverLogic};
        use apex_core::types::InputSeed;

        struct FixedSolver;
        impl Solver for FixedSolver {
            fn solve(
                &self,
                constraints: &[String],
                _negate_last: bool,
            ) -> Result<Option<InputSeed>> {
                // Return data based on the constraint content
                let byte = if constraints[0].contains('x') {
                    0xAA
                } else {
                    0xBB
                };
                Ok(Some(InputSeed::new(
                    vec![byte],
                    apex_core::types::SeedOrigin::Symbolic,
                )))
            }
            fn set_logic(&mut self, _logic: SolverLogic) {}
            fn name(&self) -> &str {
                "fixed"
            }
        }

        let parts = vec![vec!["(> x 0)".to_string()], vec!["(> y 0)".to_string()]];
        let result = PathDecomposer::solve_decomposed(&parts, &FixedSolver)
            .unwrap()
            .unwrap();
        assert_eq!(result.data, vec![0xAA, 0xBB]);
    }

    /// Decompose result is sorted by partition size (ascending).
    #[test]
    fn decompose_result_sorted_by_size() {
        let constraints = vec![
            "(> a 0)".to_string(), // alone
            "(> x 0)".to_string(),
            "(< x 10)".to_string(),
            "(and (> x 5) (< x 8))".to_string(), // 3 constraints share x
        ];
        let parts = PathDecomposer::decompose(&constraints);
        // a is alone (1 constraint), x group has 3
        assert_eq!(parts.len(), 2);
        assert!(
            parts[0].len() <= parts[1].len(),
            "partitions should be sorted by size: {} vs {}",
            parts[0].len(),
            parts[1].len()
        );
    }
}
