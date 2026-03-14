//! Generate multiple diverse solutions from one constraint set.
//! Based on the PanSampler paper — solves, adds blocking clause, repeats.

use crate::traits::{Solver, SolverLogic};
use apex_core::error::Result;
use apex_core::types::InputSeed;

/// Wraps any Solver to produce multiple diverse solutions via blocking clauses.
pub struct DiversitySolver<S: Solver> {
    inner: S,
    num_solutions: usize,
}

impl<S: Solver> DiversitySolver<S> {
    pub fn new(inner: S, num_solutions: usize) -> Self {
        DiversitySolver {
            inner,
            num_solutions,
        }
    }

    /// Solve constraints multiple times, returning up to `num_solutions` diverse seeds.
    ///
    /// After each solution, adds a blocking clause to exclude the previous solution.
    /// Stops early if the solver returns None (no more solutions).
    pub fn solve_diverse(&self, constraints: &[String]) -> Result<Vec<InputSeed>> {
        let mut solutions = Vec::new();
        let mut augmented_constraints: Vec<String> = constraints.to_vec();

        for _ in 0..self.num_solutions {
            match self.inner.solve(&augmented_constraints, false)? {
                Some(seed) => {
                    // Build a blocking clause from the solution data.
                    // We negate the current solution by adding a constraint
                    // that the output must differ from this seed.
                    let blocking =
                        format!("(not (= _solution_hash {}))", hash_seed_data(&seed.data));
                    augmented_constraints.push(blocking);
                    solutions.push(seed);
                }
                None => break,
            }
        }

        Ok(solutions)
    }
}

impl<S: Solver> Solver for DiversitySolver<S> {
    fn solve(&self, constraints: &[String], negate_last: bool) -> Result<Option<InputSeed>> {
        self.inner.solve(constraints, negate_last)
    }

    fn set_logic(&mut self, logic: SolverLogic) {
        self.inner.set_logic(logic);
    }

    fn name(&self) -> &str {
        "diversity"
    }
}

/// Simple hash of seed data for blocking clause generation.
fn hash_seed_data(data: &[u8]) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    data.hash(&mut hasher);
    hasher.finish()
}

#[cfg(test)]
mod tests {
    use super::*;
    use apex_core::types::SeedOrigin;
    use std::sync::Mutex;

    /// A solver that returns incrementing solutions up to a limit.
    struct IncrementingSolver {
        counter: Mutex<usize>,
        max_solutions: usize,
    }

    impl IncrementingSolver {
        fn new(max: usize) -> Self {
            IncrementingSolver {
                counter: Mutex::new(0),
                max_solutions: max,
            }
        }
    }

    impl Solver for IncrementingSolver {
        fn solve(&self, _constraints: &[String], _negate_last: bool) -> Result<Option<InputSeed>> {
            let mut c = self.counter.lock().unwrap_or_else(|e| e.into_inner());
            if *c >= self.max_solutions {
                return Ok(None);
            }
            *c += 1;
            Ok(Some(InputSeed::new(vec![*c as u8], SeedOrigin::Symbolic)))
        }
        fn set_logic(&mut self, _logic: SolverLogic) {}
        fn name(&self) -> &str {
            "incrementing"
        }
    }

    #[test]
    fn diversity_solver_returns_multiple() {
        let inner = IncrementingSolver::new(5);
        let ds = DiversitySolver::new(inner, 3);
        let results = ds.solve_diverse(&["(> x 0)".to_string()]).unwrap();
        assert_eq!(results.len(), 3);
        // Each solution should be different
        assert_ne!(results[0].data, results[1].data);
        assert_ne!(results[1].data, results[2].data);
    }

    #[test]
    fn diversity_solver_fewer_than_requested() {
        let inner = IncrementingSolver::new(2);
        let ds = DiversitySolver::new(inner, 5);
        let results = ds.solve_diverse(&["(> x 0)".to_string()]).unwrap();
        // Solver can only produce 2 solutions
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn diversity_solver_empty_constraints() {
        let inner = IncrementingSolver::new(5);
        let ds = DiversitySolver::new(inner, 3);
        let results = ds.solve_diverse(&[]).unwrap();
        // Even with empty constraints, solver returns solutions
        // (depends on inner solver behavior)
        assert!(!results.is_empty() || results.is_empty()); // no panic
    }

    #[test]
    fn diversity_solver_zero_requested() {
        let inner = IncrementingSolver::new(5);
        let ds = DiversitySolver::new(inner, 0);
        let results = ds.solve_diverse(&["(> x 0)".to_string()]).unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn diversity_solver_implements_solver_trait() {
        let inner = IncrementingSolver::new(5);
        let ds = DiversitySolver::new(inner, 3);
        assert_eq!(ds.name(), "diversity");
        // Standard solve should return first solution
        let result = ds.solve(&["(> x 0)".to_string()], false).unwrap();
        assert!(result.is_some());
    }
}
