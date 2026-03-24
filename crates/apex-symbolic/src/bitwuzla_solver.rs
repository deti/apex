//! Bitwuzla solver backend for apex-symbolic.
//!
//! Bitwuzla specialises in quantifier-free bit-vector and floating-point
//! arithmetic (QF_ABV, QF_BVFP), making it a strong partner for Z3 in a
//! parallel portfolio targeting compiled C/Rust/Wasm binaries.
//!
//! ## Feature flag
//!
//! The real Bitwuzla C-API integration is compiled only when the crate feature
//! `bitwuzla` is enabled (`--features bitwuzla`).  Without that feature the
//! struct still exists but every `solve()` call returns
//! `Err("bitwuzla not available")` so callers (e.g. `PortfolioSolver`) can
//! degrade gracefully at runtime.
//!
//! ## Design rationale
//!
//! The feature-flag / stub pattern mirrors how `Z3Solver` handles the
//! `z3-solver` flag: the trait implementation is always present; only the
//! inner backend is conditionally compiled.  This keeps the public API stable
//! regardless of which features the downstream crate selects.

use apex_core::{
    error::{ApexError, Result},
    types::InputSeed,
};

use crate::traits::{Solver, SolverLogic};

// ---------------------------------------------------------------------------
// BitwuzlaSolver struct
// ---------------------------------------------------------------------------

/// Bitwuzla-backed solver implementing the [`Solver`] trait.
///
/// Without the `bitwuzla` crate feature this is a zero-cost stub — the struct
/// exists, compiles, and can be placed into a [`crate::portfolio::PortfolioSolver`],
/// but every `solve()` call returns an `Err` immediately.
pub struct BitwuzlaSolver {
    logic: SolverLogic,
}

impl BitwuzlaSolver {
    /// Create a new `BitwuzlaSolver` with the given SMT logic hint.
    pub fn new(logic: SolverLogic) -> Self {
        BitwuzlaSolver { logic }
    }

    /// Factory: pick a logic appropriate for the target language, mirroring
    /// `Z3Solver::for_language`.
    pub fn for_language(lang: apex_core::types::Language) -> Self {
        use apex_core::types::Language;
        let logic = match lang {
            Language::C | Language::Rust | Language::Wasm => SolverLogic::QfAbv,
            Language::Python => SolverLogic::QfLia,
            Language::JavaScript => SolverLogic::QfS,
            _ => SolverLogic::Auto,
        };
        BitwuzlaSolver::new(logic)
    }
}

// ---------------------------------------------------------------------------
// Solver trait implementation
// ---------------------------------------------------------------------------

impl Solver for BitwuzlaSolver {
    fn name(&self) -> &str {
        "bitwuzla"
    }

    fn set_logic(&mut self, logic: SolverLogic) {
        self.logic = logic;
    }

    fn solve(&self, constraints: &[String], negate_last: bool) -> Result<Option<InputSeed>> {
        if constraints.is_empty() {
            return Ok(None);
        }

        #[cfg(feature = "bitwuzla")]
        {
            solve_bitwuzla(constraints, negate_last, self.logic)
        }

        #[cfg(not(feature = "bitwuzla"))]
        {
            let _ = negate_last;
            Err(ApexError::Solver(
                "bitwuzla not available: recompile with --features bitwuzla".into(),
            ))
        }
    }
}

// ---------------------------------------------------------------------------
// Real Bitwuzla backend (only compiled with --features bitwuzla)
// ---------------------------------------------------------------------------

#[cfg(feature = "bitwuzla")]
fn solve_bitwuzla(
    constraints: &[String],
    negate_last: bool,
    _logic: SolverLogic,
) -> Result<Option<InputSeed>> {
    // bitwuzla-sys is the low-level C-API binding.  A full integration would:
    //   1. Create a Bitwuzla options object and a solver instance.
    //   2. Parse each SMT-LIB2 constraint string via the API.
    //   3. Negate the last constraint when `negate_last` is true.
    //   4. Call `bitwuzla_check_sat()` and extract the model.
    //
    // This skeleton returns `Ok(None)` (UNSAT / unknown) so that the feature
    // flag plumbing compiles correctly.  Replace with real API calls once
    // bitwuzla-sys bindings stabilise.
    let _ = (constraints, negate_last);
    tracing::warn!("bitwuzla feature compiled in but full API integration is a stub");
    Ok(None)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::traits::{Solver as SolverTrait, SolverLogic};

    // ------------------------------------------------------------------
    // Constructor / accessor tests
    // ------------------------------------------------------------------

    #[test]
    fn name_is_bitwuzla() {
        let s = BitwuzlaSolver::new(SolverLogic::Auto);
        assert_eq!(s.name(), "bitwuzla");
    }

    #[test]
    fn set_logic_does_not_panic() {
        let mut s = BitwuzlaSolver::new(SolverLogic::Auto);
        for logic in [
            SolverLogic::QfLia,
            SolverLogic::QfAbv,
            SolverLogic::QfS,
            SolverLogic::Auto,
        ] {
            s.set_logic(logic);
        }
    }

    // ------------------------------------------------------------------
    // Stub behaviour (no `bitwuzla` feature)
    // ------------------------------------------------------------------

    /// Without the `bitwuzla` feature the solver must return an Err for
    /// non-empty constraint sets.
    #[cfg(not(feature = "bitwuzla"))]
    #[test]
    fn stub_returns_err_for_non_empty_constraints() {
        let s = BitwuzlaSolver::new(SolverLogic::Auto);
        let result = SolverTrait::solve(&s, &["(> x 0)".to_string()], false);
        assert!(
            result.is_err(),
            "expected Err when bitwuzla feature is absent"
        );
        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("bitwuzla not available"),
            "error message should mention bitwuzla: {err_msg}"
        );
    }

    #[cfg(not(feature = "bitwuzla"))]
    #[test]
    fn stub_returns_err_negate_last() {
        let s = BitwuzlaSolver::new(SolverLogic::QfAbv);
        let result = SolverTrait::solve(&s, &["(> x 0)".to_string(), "(< y 10)".to_string()], true);
        assert!(result.is_err());
    }

    // Empty constraints always return Ok(None) regardless of feature flag.
    #[test]
    fn empty_constraints_returns_ok_none() {
        let s = BitwuzlaSolver::new(SolverLogic::Auto);
        let result = SolverTrait::solve(&s, &[], false).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn empty_constraints_negate_last_returns_ok_none() {
        let s = BitwuzlaSolver::new(SolverLogic::Auto);
        let result = SolverTrait::solve(&s, &[], true).unwrap();
        assert!(result.is_none());
    }

    // ------------------------------------------------------------------
    // for_language factory
    // ------------------------------------------------------------------

    #[test]
    fn for_language_name_is_bitwuzla() {
        use apex_core::types::Language;
        for lang in [
            Language::C,
            Language::Rust,
            Language::Python,
            Language::JavaScript,
            Language::Java,
            Language::Wasm,
            Language::Ruby,
        ] {
            let s = BitwuzlaSolver::for_language(lang);
            assert_eq!(s.name(), "bitwuzla");
        }
    }

    // ------------------------------------------------------------------
    // solve_batch (default implementation from trait)
    // ------------------------------------------------------------------

    #[test]
    fn solve_batch_empty_returns_empty() {
        let s = BitwuzlaSolver::new(SolverLogic::Auto);
        let results = SolverTrait::solve_batch(&s, &[], false);
        assert!(results.is_empty());
    }

    /// With empty per-set, each call returns Ok(None).
    #[test]
    fn solve_batch_empty_per_set_returns_ok_none() {
        let s = BitwuzlaSolver::new(SolverLogic::Auto);
        let sets: Vec<Vec<String>> = vec![vec![], vec![]];
        let results = SolverTrait::solve_batch(&s, &sets, false);
        assert_eq!(results.len(), 2);
        for r in results {
            assert!(r.unwrap().is_none());
        }
    }
}
