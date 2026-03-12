//! Solver trait abstraction for SMT backends.

use apex_core::{error::Result, types::InputSeed};

/// Which SMT logic to set on the solver. Guides solver heuristics.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SolverLogic {
    /// Quantifier-free linear integer arithmetic (Python targets).
    QfLia,
    /// Quantifier-free arrays + bitvectors (C/Rust compiled targets).
    QfAbv,
    /// Quantifier-free strings (JavaScript/web targets).
    QfS,
    /// Let the solver auto-detect (default).
    Auto,
}

/// Abstraction over SMT solver backends (Z3, Bitwuzla, CVC5, etc.).
pub trait Solver: Send + Sync {
    /// Solve a constraint set. If `negate_last` is true, negate the final constraint
    /// to find an input that takes the opposite branch.
    fn solve(&self, constraints: &[String], negate_last: bool) -> Result<Option<InputSeed>>;

    /// Solve multiple constraint sets in one batch. Default implementation
    /// calls `solve()` for each set. Backends can override for efficiency.
    fn solve_batch(
        &self,
        sets: &[Vec<String>],
        negate_last: bool,
    ) -> Vec<Result<Option<InputSeed>>> {
        sets.iter().map(|cs| self.solve(cs, negate_last)).collect()
    }

    /// Set the SMT logic for this solver instance.
    fn set_logic(&mut self, logic: SolverLogic);

    /// Human-readable name for logging.
    fn name(&self) -> &str;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn solver_logic_debug() {
        assert_eq!(format!("{:?}", SolverLogic::QfLia), "QfLia");
        assert_eq!(format!("{:?}", SolverLogic::QfAbv), "QfAbv");
        assert_eq!(format!("{:?}", SolverLogic::QfS), "QfS");
        assert_eq!(format!("{:?}", SolverLogic::Auto), "Auto");
    }

    #[test]
    fn solver_logic_eq() {
        assert_eq!(SolverLogic::QfLia, SolverLogic::QfLia);
        assert_ne!(SolverLogic::QfLia, SolverLogic::QfAbv);
    }

    #[test]
    fn solver_logic_clone_and_copy() {
        let a = SolverLogic::QfS;
        let b = a; // Copy
        let c = a.clone(); // Clone
        assert_eq!(a, b);
        assert_eq!(a, c);
    }

    #[test]
    fn solver_logic_all_variants_ne() {
        let variants = [
            SolverLogic::QfLia,
            SolverLogic::QfAbv,
            SolverLogic::QfS,
            SolverLogic::Auto,
        ];
        for i in 0..variants.len() {
            for j in (i + 1)..variants.len() {
                assert_ne!(variants[i], variants[j]);
            }
        }
    }

    /// Test the default solve_batch implementation.
    struct DummySolver;
    impl Solver for DummySolver {
        fn solve(&self, constraints: &[String], _negate_last: bool) -> Result<Option<InputSeed>> {
            if constraints.is_empty() {
                Ok(None)
            } else {
                Ok(Some(InputSeed::new(
                    vec![constraints.len() as u8],
                    apex_core::types::SeedOrigin::Symbolic,
                )))
            }
        }
        fn set_logic(&mut self, _logic: SolverLogic) {}
        fn name(&self) -> &str {
            "dummy"
        }
    }

    #[test]
    fn solve_batch_default_impl() {
        let solver = DummySolver;
        let sets = vec![
            vec!["a".to_string()],
            vec![],
            vec!["b".to_string(), "c".to_string()],
        ];
        let results = solver.solve_batch(&sets, false);
        assert_eq!(results.len(), 3);
        assert!(results[0].as_ref().unwrap().is_some());
        assert!(results[1].as_ref().unwrap().is_none());
        assert!(results[2].as_ref().unwrap().is_some());
        assert_eq!(results[2].as_ref().unwrap().as_ref().unwrap().data.as_ref(), &[2u8]);
    }

    #[test]
    fn solve_batch_empty_sets() {
        let solver = DummySolver;
        let results = solver.solve_batch(&[], false);
        assert!(results.is_empty());
    }

    // ------------------------------------------------------------------
    // Additional gap-filling tests
    // ------------------------------------------------------------------

    #[test]
    fn solve_batch_negate_last_propagated() {
        // Verify negate_last is forwarded to each solve() call
        struct NegateCapture {
            negate_seen: std::sync::Mutex<Vec<bool>>,
        }
        impl Solver for NegateCapture {
            fn solve(&self, constraints: &[String], negate_last: bool) -> Result<Option<InputSeed>> {
                self.negate_seen.lock().unwrap().push(negate_last);
                if constraints.is_empty() { Ok(None) } else { Ok(None) }
            }
            fn set_logic(&mut self, _logic: SolverLogic) {}
            fn name(&self) -> &str { "capture" }
        }
        let solver = NegateCapture { negate_seen: std::sync::Mutex::new(Vec::new()) };
        let sets = vec![vec!["x".to_string()], vec!["y".to_string()]];
        let _ = solver.solve_batch(&sets, true);
        let seen = solver.negate_seen.lock().unwrap();
        assert!(seen.iter().all(|&v| v), "negate_last=true should be forwarded");
    }

    #[test]
    fn solve_batch_single_set_non_empty() {
        let solver = DummySolver;
        let results = solver.solve_batch(&[vec!["a".to_string()]], false);
        assert_eq!(results.len(), 1);
        assert!(results[0].as_ref().unwrap().is_some());
    }

    #[test]
    fn solver_logic_ne_exhaustive() {
        // Ensure all variants differ from each other
        let all = [SolverLogic::QfLia, SolverLogic::QfAbv, SolverLogic::QfS, SolverLogic::Auto];
        for (i, a) in all.iter().enumerate() {
            for (j, b) in all.iter().enumerate() {
                if i == j {
                    assert_eq!(a, b);
                } else {
                    assert_ne!(a, b);
                }
            }
        }
    }
}
