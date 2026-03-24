//! Portfolio solver that wraps multiple solver backends.
//!
//! Tries each solver sequentially and returns the first SAT result.

use std::time::Duration;

use apex_core::{error::Result, types::InputSeed};

use crate::gradient::GradientSolver;
use crate::traits::{Solver, SolverLogic};

/// A solver that wraps multiple backends and returns the first SAT result.
pub struct PortfolioSolver {
    solvers: Vec<Box<dyn Solver>>,
    timeout: Duration,
}

impl PortfolioSolver {
    /// Create a new portfolio solver with the given backends and per-solver timeout.
    pub fn new(solvers: Vec<Box<dyn Solver>>, timeout: Duration) -> Self {
        PortfolioSolver { solvers, timeout }
    }

    /// Create a portfolio with GradientSolver as the first (fastest) backend.
    /// Additional solvers can be added with `add_solver()`.
    pub fn with_gradient_first(timeout: Duration) -> Self {
        let solvers: Vec<Box<dyn Solver>> = vec![Box::new(GradientSolver::new(100))];
        PortfolioSolver { solvers, timeout }
    }

    /// Add a solver backend to the portfolio.
    pub fn add_solver(&mut self, solver: Box<dyn Solver>) {
        self.solvers.push(solver);
    }

    /// Returns the configured per-solver timeout.
    pub fn timeout(&self) -> Duration {
        self.timeout
    }
}

impl Solver for PortfolioSolver {
    fn solve(&self, constraints: &[String], negate_last: bool) -> Result<Option<InputSeed>> {
        for solver in &self.solvers {
            let result = solver.solve(constraints, negate_last)?;
            if result.is_some() {
                return Ok(result);
            }
        }
        Ok(None)
    }

    fn solve_batch(
        &self,
        sets: &[Vec<String>],
        negate_last: bool,
    ) -> Vec<Result<Option<InputSeed>>> {
        sets.iter().map(|cs| self.solve(cs, negate_last)).collect()
    }

    fn set_logic(&mut self, logic: SolverLogic) {
        for solver in &mut self.solvers {
            solver.set_logic(logic);
        }
    }

    fn name(&self) -> &str {
        "portfolio"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A stub solver that always returns None (UNSAT / unknown).
    struct NullSolver {
        solver_name: String,
    }

    impl NullSolver {
        fn new(name: &str) -> Self {
            NullSolver {
                solver_name: name.to_string(),
            }
        }
    }

    impl Solver for NullSolver {
        fn solve(&self, _constraints: &[String], _negate_last: bool) -> Result<Option<InputSeed>> {
            Ok(None)
        }

        fn set_logic(&mut self, _logic: SolverLogic) {}

        fn name(&self) -> &str {
            &self.solver_name
        }
    }

    #[test]
    fn empty_portfolio_returns_none() {
        let portfolio = PortfolioSolver::new(vec![], Duration::from_secs(5));
        let result = portfolio.solve(&["x > 0".to_string()], false).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn single_null_solver_returns_none() {
        let solvers: Vec<Box<dyn Solver>> = vec![Box::new(NullSolver::new("null"))];
        let portfolio = PortfolioSolver::new(solvers, Duration::from_secs(5));
        let result = portfolio.solve(&["x > 0".to_string()], false).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn portfolio_name() {
        let portfolio = PortfolioSolver::new(vec![], Duration::from_secs(5));
        assert_eq!(portfolio.name(), "portfolio");
    }

    #[test]
    fn portfolio_solve_batch_delegates() {
        let solvers: Vec<Box<dyn Solver>> = vec![Box::new(NullSolver::new("null"))];
        let portfolio = PortfolioSolver::new(solvers, Duration::from_secs(5));
        let sets = vec![vec!["x > 0".to_string()], vec!["y < 10".to_string()]];
        let results = portfolio.solve_batch(&sets, false);
        assert_eq!(results.len(), 2);
        for r in results {
            assert!(r.unwrap().is_none());
        }
    }

    /// A stub solver that returns a fixed SAT result.
    struct SatSolver {
        seed: InputSeed,
    }

    impl SatSolver {
        fn new(data: Vec<u8>) -> Self {
            SatSolver {
                seed: InputSeed::new(data, apex_core::types::SeedOrigin::Symbolic),
            }
        }
    }

    impl Solver for SatSolver {
        fn solve(&self, _constraints: &[String], _negate_last: bool) -> Result<Option<InputSeed>> {
            Ok(Some(self.seed.clone()))
        }

        fn set_logic(&mut self, _logic: SolverLogic) {}

        fn name(&self) -> &str {
            "sat"
        }
    }

    /// A solver that returns an error.
    struct ErrorSolver;

    impl Solver for ErrorSolver {
        fn solve(&self, _constraints: &[String], _negate_last: bool) -> Result<Option<InputSeed>> {
            Err(apex_core::error::ApexError::Solver("test error".into()))
        }

        fn set_logic(&mut self, _logic: SolverLogic) {}

        fn name(&self) -> &str {
            "error"
        }
    }

    #[test]
    fn portfolio_returns_first_sat_result() {
        let solvers: Vec<Box<dyn Solver>> = vec![
            Box::new(NullSolver::new("null1")),
            Box::new(SatSolver::new(vec![1, 2, 3])),
            Box::new(NullSolver::new("null2")),
        ];
        let portfolio = PortfolioSolver::new(solvers, Duration::from_secs(5));
        let result = portfolio.solve(&["x > 0".to_string()], false).unwrap();
        assert!(result.is_some());
        assert_eq!(result.unwrap().data.as_ref(), &[1u8, 2, 3]);
    }

    #[test]
    fn portfolio_first_sat_wins_over_later_sat() {
        let solvers: Vec<Box<dyn Solver>> = vec![
            Box::new(SatSolver::new(vec![10])),
            Box::new(SatSolver::new(vec![20])),
        ];
        let portfolio = PortfolioSolver::new(solvers, Duration::from_secs(5));
        let result = portfolio.solve(&[], false).unwrap();
        assert_eq!(result.unwrap().data.as_ref(), &[10u8]);
    }

    #[test]
    fn portfolio_error_propagates() {
        let solvers: Vec<Box<dyn Solver>> = vec![Box::new(ErrorSolver)];
        let portfolio = PortfolioSolver::new(solvers, Duration::from_secs(5));
        let result = portfolio.solve(&["x > 0".to_string()], false);
        assert!(result.is_err());
    }

    #[test]
    fn portfolio_add_solver_increases_count() {
        let mut portfolio = PortfolioSolver::new(vec![], Duration::from_secs(5));
        portfolio.add_solver(Box::new(NullSolver::new("a")));
        portfolio.add_solver(Box::new(NullSolver::new("b")));
        assert_eq!(portfolio.solvers.len(), 2);
    }

    #[test]
    fn portfolio_add_solver_makes_sat_reachable() {
        let mut portfolio = PortfolioSolver::new(vec![], Duration::from_secs(5));
        portfolio.add_solver(Box::new(SatSolver::new(vec![42])));
        let result = portfolio.solve(&[], false).unwrap();
        assert!(result.is_some());
    }

    #[test]
    fn portfolio_timeout_accessor() {
        let duration = Duration::from_millis(1500);
        let portfolio = PortfolioSolver::new(vec![], duration);
        assert_eq!(portfolio.timeout(), duration);
    }

    #[test]
    fn portfolio_set_logic_propagates_to_all() {
        // set_logic on a portfolio with multiple solvers should not panic
        let solvers: Vec<Box<dyn Solver>> = vec![
            Box::new(NullSolver::new("n1")),
            Box::new(NullSolver::new("n2")),
        ];
        let mut portfolio = PortfolioSolver::new(solvers, Duration::from_secs(1));
        // Should propagate to all sub-solvers without error.
        portfolio.set_logic(SolverLogic::QfLia);
        portfolio.set_logic(SolverLogic::QfAbv);
        portfolio.set_logic(SolverLogic::QfS);
        portfolio.set_logic(SolverLogic::Auto);
    }

    #[test]
    fn portfolio_solve_batch_with_negate_last() {
        let solvers: Vec<Box<dyn Solver>> = vec![Box::new(NullSolver::new("null"))];
        let portfolio = PortfolioSolver::new(solvers, Duration::from_secs(5));
        let sets = vec![vec!["a = 1".to_string(), "b = 2".to_string()]];
        let results = portfolio.solve_batch(&sets, true);
        assert_eq!(results.len(), 1);
        assert!(results[0].as_ref().unwrap().is_none());
    }

    #[test]
    fn portfolio_solve_empty_constraints() {
        let solvers: Vec<Box<dyn Solver>> = vec![Box::new(NullSolver::new("null"))];
        let portfolio = PortfolioSolver::new(solvers, Duration::from_secs(5));
        let result = portfolio.solve(&[], false).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn portfolio_error_before_sat_propagates() {
        // Error solver first, SAT solver second — error should propagate
        let solvers: Vec<Box<dyn Solver>> =
            vec![Box::new(ErrorSolver), Box::new(SatSolver::new(vec![99]))];
        let portfolio = PortfolioSolver::new(solvers, Duration::from_secs(5));
        let result = portfolio.solve(&["x > 0".to_string()], false);
        assert!(result.is_err());
    }

    #[test]
    fn portfolio_null_then_null_returns_none() {
        let solvers: Vec<Box<dyn Solver>> = vec![
            Box::new(NullSolver::new("n1")),
            Box::new(NullSolver::new("n2")),
            Box::new(NullSolver::new("n3")),
        ];
        let portfolio = PortfolioSolver::new(solvers, Duration::from_secs(5));
        let result = portfolio.solve(&["x > 0".to_string()], false).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn portfolio_solve_batch_mixed_results() {
        let solvers: Vec<Box<dyn Solver>> = vec![Box::new(NullSolver::new("null"))];
        let portfolio = PortfolioSolver::new(solvers, Duration::from_secs(5));
        let sets = vec![
            vec!["a".to_string()],
            vec!["b".to_string()],
            vec!["c".to_string()],
        ];
        let results = portfolio.solve_batch(&sets, true);
        assert_eq!(results.len(), 3);
    }

    #[test]
    fn portfolio_solve_batch_with_sat_solver() {
        let solvers: Vec<Box<dyn Solver>> = vec![Box::new(SatSolver::new(vec![1, 2]))];
        let portfolio = PortfolioSolver::new(solvers, Duration::from_secs(5));
        let sets = vec![vec!["x > 0".to_string()], vec!["y < 5".to_string()]];
        let results = portfolio.solve_batch(&sets, false);
        assert_eq!(results.len(), 2);
        for r in &results {
            assert!(r.as_ref().unwrap().is_some());
        }
    }

    #[test]
    fn portfolio_solve_batch_empty_sets() {
        let solvers: Vec<Box<dyn Solver>> = vec![Box::new(NullSolver::new("null"))];
        let portfolio = PortfolioSolver::new(solvers, Duration::from_secs(5));
        let results = portfolio.solve_batch(&[], false);
        assert!(results.is_empty());
    }

    #[test]
    fn portfolio_with_gradient_first() {
        let portfolio = PortfolioSolver::with_gradient_first(Duration::from_secs(5));
        assert_eq!(portfolio.solvers.len(), 1);
        assert_eq!(portfolio.solvers[0].name(), "gradient");
        // Should solve simple constraint
        let result = portfolio.solve(&["(= x 42)".to_string()], false).unwrap();
        assert!(result.is_some());
    }
}
