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
}
