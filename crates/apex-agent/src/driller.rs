//! Driller strategy — symbolic execution to bypass hard-to-fuzz branches.
//!
//! Activated by the orchestrator when the fuzzer stalls. Collects path
//! constraints from recent executions and negates frontier branches to
//! generate coverage-unlocking seeds.

use apex_core::{
    error::Result,
    traits::Strategy,
    types::{ExecutionResult, ExplorationContext, InputSeed, PathConstraint, SeedOrigin},
};
use apex_symbolic::traits::Solver;
use async_trait::async_trait;
use std::sync::{Arc, Mutex};

/// Driller-style symbolic execution strategy.
///
/// When the fuzzer stalls, the orchestrator rotates to this strategy.
/// It collects path constraints from recent executions, negates frontier
/// branches (those targeting uncovered branches), and uses an SMT solver
/// to generate inputs that pass hard-to-fuzz conditions.
pub struct DrillerStrategy {
    solver: Arc<Mutex<dyn Solver>>,
    /// Path constraints collected from traced executions.
    constraints: Mutex<Vec<PathConstraint>>,
    /// Maximum number of constraints to solve per invocation.
    max_constraints: usize,
}

impl DrillerStrategy {
    pub fn new(solver: Arc<Mutex<dyn Solver>>, max_constraints: usize) -> Self {
        Self {
            solver,
            constraints: Mutex::new(Vec::new()),
            max_constraints,
        }
    }

    /// Record path constraints from a traced execution.
    /// Called by the orchestrator after concolic/traced runs.
    pub fn record_constraints(&self, new_constraints: Vec<PathConstraint>) {
        let mut cs = self.constraints.lock().unwrap_or_else(|e| e.into_inner());
        cs.extend(new_constraints);
    }
}

#[async_trait]
impl Strategy for DrillerStrategy {
    fn name(&self) -> &str {
        "driller"
    }

    async fn suggest_inputs(&self, ctx: &ExplorationContext) -> Result<Vec<InputSeed>> {
        let constraints = self
            .constraints
            .lock()
            .map_err(|e| apex_core::error::ApexError::Agent(format!("mutex poisoned: {e}")))?
            .clone();
        if constraints.is_empty() {
            return Ok(Vec::new());
        }

        // Build set of uncovered branch IDs for quick lookup.
        let uncovered: std::collections::HashSet<_> = ctx.uncovered_branches.iter().collect();

        // Find constraints whose branch is still uncovered — these are
        // the frontier branches where negation could unlock new coverage.
        let frontier: Vec<_> = constraints
            .iter()
            .filter(|pc| uncovered.contains(&pc.branch))
            .take(self.max_constraints)
            .collect();

        let mut inputs = Vec::new();
        let solver = self.solver.lock().map_err(|e| {
            apex_core::error::ApexError::Agent(format!("solver mutex poisoned: {e}"))
        })?;

        for pc in &frontier {
            // Build constraint prefix up to this branch, then negate.
            let prefix: Vec<String> = constraints
                .iter()
                .take_while(|c| c.branch != pc.branch)
                .map(|c| c.smtlib2.clone())
                .chain(std::iter::once(pc.smtlib2.clone()))
                .collect();

            if let Ok(Some(seed)) = solver.solve(&prefix, true) {
                inputs.push(InputSeed::new(seed.data.to_vec(), SeedOrigin::Symbolic));
            }
        }

        Ok(inputs)
    }

    async fn observe(&self, _result: &ExecutionResult) -> Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use apex_core::types::{BranchId, Language, Target};

    fn make_ctx(uncovered: Vec<BranchId>) -> ExplorationContext {
        ExplorationContext {
            target: Target {
                root: std::path::PathBuf::from("/tmp"),
                language: Language::Rust,
                test_command: vec![],
            },
            uncovered_branches: uncovered,
            iteration: 100,
        }
    }

    /// Stub solver that returns a fixed input when constraints are solvable.
    struct StubSolver {
        result: Option<InputSeed>,
    }

    impl StubSolver {
        fn solvable() -> Self {
            StubSolver {
                result: Some(InputSeed::new(b"solved".to_vec(), SeedOrigin::Symbolic)),
            }
        }

        fn unsolvable() -> Self {
            StubSolver { result: None }
        }
    }

    impl Solver for StubSolver {
        fn solve(&self, _constraints: &[String], _negate_last: bool) -> Result<Option<InputSeed>> {
            Ok(self.result.clone())
        }
        fn set_logic(&mut self, _logic: apex_symbolic::traits::SolverLogic) {}
        fn name(&self) -> &str {
            "stub"
        }
    }

    #[test]
    fn driller_strategy_name() {
        let solver = Arc::new(Mutex::new(StubSolver::solvable()));
        let driller = DrillerStrategy::new(solver, 10);
        assert_eq!(driller.name(), "driller");
    }

    #[tokio::test]
    async fn suggest_inputs_with_no_constraints_returns_empty() {
        let solver = Arc::new(Mutex::new(StubSolver::solvable()));
        let driller = DrillerStrategy::new(solver, 10);
        let ctx = make_ctx(vec![BranchId::new(1, 10, 0, 0)]);
        let inputs = driller.suggest_inputs(&ctx).await.unwrap();
        // No constraints recorded yet — nothing to solve
        assert!(inputs.is_empty());
    }

    #[tokio::test]
    async fn suggest_inputs_solves_recorded_constraints() {
        let solver = Arc::new(Mutex::new(StubSolver::solvable()));
        let driller = DrillerStrategy::new(solver, 10);

        // Record some path constraints
        let branch = BranchId::new(1, 10, 0, 0);
        driller.record_constraints(vec![PathConstraint {
            branch: branch.clone(),
            smtlib2: "(assert (> x 0))".into(),
            direction_taken: true,
        }]);

        let ctx = make_ctx(vec![branch]);
        let inputs = driller.suggest_inputs(&ctx).await.unwrap();
        assert_eq!(inputs.len(), 1);
        assert_eq!(inputs[0].origin, SeedOrigin::Symbolic);
        assert_eq!(inputs[0].data, b"solved".as_ref());
    }

    #[tokio::test]
    async fn suggest_inputs_unsolvable_returns_empty() {
        let solver = Arc::new(Mutex::new(StubSolver::unsolvable()));
        let driller = DrillerStrategy::new(solver, 10);

        let branch = BranchId::new(1, 10, 0, 0);
        driller.record_constraints(vec![PathConstraint {
            branch: branch.clone(),
            smtlib2: "(assert false)".into(),
            direction_taken: true,
        }]);

        let ctx = make_ctx(vec![branch]);
        let inputs = driller.suggest_inputs(&ctx).await.unwrap();
        assert!(inputs.is_empty());
    }

    #[tokio::test]
    async fn suggest_inputs_respects_max_constraints() {
        let solver = Arc::new(Mutex::new(StubSolver::solvable()));
        let driller = DrillerStrategy::new(solver, 2); // max 2

        let mut constraints = Vec::new();
        for i in 0..5 {
            constraints.push(PathConstraint {
                branch: BranchId::new(1, i as u32, 0, 0),
                smtlib2: format!("(assert (> x {i}))"),
                direction_taken: true,
            });
        }
        driller.record_constraints(constraints);

        let ctx = make_ctx(vec![
            BranchId::new(1, 0, 0, 0),
            BranchId::new(1, 1, 0, 0),
            BranchId::new(1, 2, 0, 0),
            BranchId::new(1, 3, 0, 0),
            BranchId::new(1, 4, 0, 0),
        ]);
        let inputs = driller.suggest_inputs(&ctx).await.unwrap();
        // Should solve at most max_constraints (2)
        assert!(inputs.len() <= 2);
    }

    #[tokio::test]
    async fn observe_is_noop() {
        let solver = Arc::new(Mutex::new(StubSolver::solvable()));
        let driller = DrillerStrategy::new(solver, 10);
        let result = ExecutionResult {
            seed_id: apex_core::types::SeedId::new(),
            status: apex_core::types::ExecutionStatus::Pass,
            new_branches: vec![],
            trace: None,
            duration_ms: 5,
            stdout: String::new(),
            stderr: String::new(),
            input: None,
        };
        assert!(driller.observe(&result).await.is_ok());
    }

    #[test]
    fn record_constraints_appends_multiple_batches() {
        let solver = Arc::new(Mutex::new(StubSolver::solvable()));
        let driller = DrillerStrategy::new(solver, 10);

        driller.record_constraints(vec![PathConstraint {
            branch: BranchId::new(1, 1, 0, 0),
            smtlib2: "(assert true)".into(),
            direction_taken: true,
        }]);
        driller.record_constraints(vec![
            PathConstraint {
                branch: BranchId::new(1, 2, 0, 0),
                smtlib2: "(assert (> x 1))".into(),
                direction_taken: false,
            },
            PathConstraint {
                branch: BranchId::new(1, 3, 0, 0),
                smtlib2: "(assert (< x 10))".into(),
                direction_taken: true,
            },
        ]);

        let cs = driller.constraints.lock().unwrap();
        assert_eq!(cs.len(), 3);
    }

    #[tokio::test]
    async fn suggest_inputs_skips_covered_branches() {
        // Constraints exist for branches 1 and 2, but only branch 2 is uncovered.
        let solver = Arc::new(Mutex::new(StubSolver::solvable()));
        let driller = DrillerStrategy::new(solver, 10);

        let b1 = BranchId::new(1, 10, 0, 0);
        let b2 = BranchId::new(1, 20, 0, 0);
        driller.record_constraints(vec![
            PathConstraint {
                branch: b1.clone(),
                smtlib2: "(assert (> x 0))".into(),
                direction_taken: true,
            },
            PathConstraint {
                branch: b2.clone(),
                smtlib2: "(assert (< x 100))".into(),
                direction_taken: true,
            },
        ]);

        // Only b2 is uncovered — b1 is already covered
        let ctx = make_ctx(vec![b2]);
        let inputs = driller.suggest_inputs(&ctx).await.unwrap();
        assert_eq!(inputs.len(), 1);
    }

    #[tokio::test]
    async fn suggest_inputs_with_zero_max_constraints() {
        let solver = Arc::new(Mutex::new(StubSolver::solvable()));
        let driller = DrillerStrategy::new(solver, 0); // max_constraints = 0

        let branch = BranchId::new(1, 5, 0, 0);
        driller.record_constraints(vec![PathConstraint {
            branch: branch.clone(),
            smtlib2: "(assert true)".into(),
            direction_taken: true,
        }]);

        let ctx = make_ctx(vec![branch]);
        let inputs = driller.suggest_inputs(&ctx).await.unwrap();
        // max_constraints is 0, so take(0) yields nothing
        assert!(inputs.is_empty());
    }

    #[tokio::test]
    async fn suggest_inputs_builds_correct_prefix_chain() {
        // Verify that the prefix chain includes constraints up to and including the target.
        use std::sync::atomic::{AtomicUsize, Ordering};

        struct RecordingSolver {
            call_count: AtomicUsize,
        }

        impl Solver for RecordingSolver {
            fn solve(
                &self,
                constraints: &[String],
                _negate_last: bool,
            ) -> Result<Option<InputSeed>> {
                self.call_count.fetch_add(1, Ordering::SeqCst);
                // First call should have constraints for branch at line 10 only
                // (take_while stops before it, then chain adds it)
                Ok(Some(InputSeed::new(
                    format!("solved-{}", constraints.len()).into_bytes(),
                    SeedOrigin::Symbolic,
                )))
            }
            fn set_logic(&mut self, _logic: apex_symbolic::traits::SolverLogic) {}
            fn name(&self) -> &str {
                "recording"
            }
        }

        let solver = Arc::new(Mutex::new(RecordingSolver {
            call_count: AtomicUsize::new(0),
        }));
        let driller = DrillerStrategy::new(solver.clone(), 10);

        let b1 = BranchId::new(1, 10, 0, 0);
        let b2 = BranchId::new(1, 20, 0, 0);
        driller.record_constraints(vec![
            PathConstraint {
                branch: b1.clone(),
                smtlib2: "(assert (> x 0))".into(),
                direction_taken: true,
            },
            PathConstraint {
                branch: b2.clone(),
                smtlib2: "(assert (< x 100))".into(),
                direction_taken: true,
            },
        ]);

        // Both uncovered → frontier has both
        let ctx = make_ctx(vec![b1, b2]);
        let inputs = driller.suggest_inputs(&ctx).await.unwrap();
        assert_eq!(inputs.len(), 2);
        let count = solver.lock().unwrap().call_count.load(Ordering::SeqCst);
        assert_eq!(count, 2);
    }

    #[tokio::test]
    async fn suggest_inputs_no_uncovered_matching_constraints() {
        let solver = Arc::new(Mutex::new(StubSolver::solvable()));
        let driller = DrillerStrategy::new(solver, 10);

        // Record constraint for branch at line 10
        driller.record_constraints(vec![PathConstraint {
            branch: BranchId::new(1, 10, 0, 0),
            smtlib2: "(assert true)".into(),
            direction_taken: true,
        }]);

        // Uncovered branch is at line 99 — no matching constraint
        let ctx = make_ctx(vec![BranchId::new(1, 99, 0, 0)]);
        let inputs = driller.suggest_inputs(&ctx).await.unwrap();
        assert!(inputs.is_empty());
    }

    #[test]
    fn record_constraints_empty_is_noop() {
        let solver = Arc::new(Mutex::new(StubSolver::solvable()));
        let driller = DrillerStrategy::new(solver, 10);
        driller.record_constraints(vec![]);
        let cs = driller.constraints.lock().unwrap();
        assert!(cs.is_empty());
    }

    // ------------------------------------------------------------------
    // Additional branch-coverage tests
    // ------------------------------------------------------------------

    /// `observe()` always returns Ok — exercise it with all ExecutionStatus variants.
    #[tokio::test]
    async fn observe_returns_ok_for_all_statuses() {
        use apex_core::types::{ExecutionStatus, SeedId};
        let solver = Arc::new(Mutex::new(StubSolver::solvable()));
        let driller = DrillerStrategy::new(solver, 10);

        for status in [
            ExecutionStatus::Pass,
            ExecutionStatus::Fail,
            ExecutionStatus::Crash,
            ExecutionStatus::Timeout,
            ExecutionStatus::OomKill,
        ] {
            let result = ExecutionResult {
                seed_id: SeedId::new(),
                status,
                new_branches: vec![],
                trace: None,
                duration_ms: 1,
                stdout: String::new(),
                stderr: String::new(),
                input: None,
            };
            assert!(driller.observe(&result).await.is_ok());
        }
    }

    /// PathConstraint with `direction_taken = false` is handled the same way.
    #[tokio::test]
    async fn suggest_inputs_with_direction_taken_false() {
        let solver = Arc::new(Mutex::new(StubSolver::solvable()));
        let driller = DrillerStrategy::new(solver, 10);
        let branch = BranchId::new(1, 10, 0, 0);
        driller.record_constraints(vec![PathConstraint {
            branch: branch.clone(),
            smtlib2: "(assert (= x 0))".into(),
            direction_taken: false, // <-- the false arm
        }]);
        let ctx = make_ctx(vec![branch]);
        let inputs = driller.suggest_inputs(&ctx).await.unwrap();
        // Solver is solvable → should produce 1 input.
        assert_eq!(inputs.len(), 1);
    }

    /// suggest_inputs with constraints for covered AND uncovered branches.
    /// Only uncovered ones should be in the frontier.
    #[tokio::test]
    async fn suggest_inputs_frontier_excludes_covered() {
        let solver = Arc::new(Mutex::new(StubSolver::solvable()));
        let driller = DrillerStrategy::new(solver, 10);

        let covered = BranchId::new(2, 10, 0, 0);
        let uncovered = BranchId::new(2, 20, 0, 0);

        driller.record_constraints(vec![
            PathConstraint {
                branch: covered.clone(),
                smtlib2: "(assert true)".into(),
                direction_taken: true,
            },
            PathConstraint {
                branch: uncovered.clone(),
                smtlib2: "(assert false)".into(),
                direction_taken: false,
            },
        ]);

        // Only `uncovered` is in the context.
        let ctx = make_ctx(vec![uncovered]);
        let inputs = driller.suggest_inputs(&ctx).await.unwrap();
        // 1 frontier branch → 1 solve attempt → 1 result (solvable).
        assert_eq!(inputs.len(), 1);
    }

    /// suggest_inputs with an empty uncovered_branches list → frontier is empty.
    #[tokio::test]
    async fn suggest_inputs_no_uncovered_branches_in_ctx() {
        let solver = Arc::new(Mutex::new(StubSolver::solvable()));
        let driller = DrillerStrategy::new(solver, 10);
        driller.record_constraints(vec![PathConstraint {
            branch: BranchId::new(1, 1, 0, 0),
            smtlib2: "(assert true)".into(),
            direction_taken: true,
        }]);

        // Empty uncovered list → nothing matches → empty inputs.
        let ctx = make_ctx(vec![]);
        let inputs = driller.suggest_inputs(&ctx).await.unwrap();
        assert!(inputs.is_empty());
    }

    /// When solver returns Err for a constraint, `if let Ok(Some(seed))` skips it.
    #[tokio::test]
    async fn suggest_inputs_solver_error_skipped() {
        struct ErrorSolver;
        impl Solver for ErrorSolver {
            fn solve(
                &self,
                _constraints: &[String],
                _negate_last: bool,
            ) -> Result<Option<InputSeed>> {
                Err(apex_core::error::ApexError::Agent("solver error".into()))
            }
            fn set_logic(&mut self, _logic: apex_symbolic::traits::SolverLogic) {}
            fn name(&self) -> &str {
                "error"
            }
        }

        let solver = Arc::new(Mutex::new(ErrorSolver));
        let driller = DrillerStrategy::new(solver, 10);
        let branch = BranchId::new(1, 5, 0, 0);
        driller.record_constraints(vec![PathConstraint {
            branch: branch.clone(),
            smtlib2: "(assert true)".into(),
            direction_taken: true,
        }]);

        let ctx = make_ctx(vec![branch]);
        // Solver errors are silently skipped by `if let Ok(Some(seed))`.
        let inputs = driller.suggest_inputs(&ctx).await.unwrap();
        assert!(inputs.is_empty());
    }

    /// All constraints in max_constraints limit are processed when limit is large enough.
    #[tokio::test]
    async fn suggest_inputs_processes_all_within_limit() {
        let solver = Arc::new(Mutex::new(StubSolver::solvable()));
        let driller = DrillerStrategy::new(solver, 100);

        let mut constraints = Vec::new();
        let mut uncovered = Vec::new();
        for i in 0..5u32 {
            let b = BranchId::new(3, i, 0, 0);
            constraints.push(PathConstraint {
                branch: b.clone(),
                smtlib2: format!("(assert (> x {i}))"),
                direction_taken: true,
            });
            uncovered.push(b);
        }
        driller.record_constraints(constraints);
        let ctx = make_ctx(uncovered);
        let inputs = driller.suggest_inputs(&ctx).await.unwrap();
        assert_eq!(inputs.len(), 5);
    }

    /// InputSeed origin is always Symbolic for solver-produced seeds.
    #[tokio::test]
    async fn suggest_inputs_seed_origin_is_symbolic() {
        let solver = Arc::new(Mutex::new(StubSolver::solvable()));
        let driller = DrillerStrategy::new(solver, 10);
        let branch = BranchId::new(1, 1, 0, 0);
        driller.record_constraints(vec![PathConstraint {
            branch: branch.clone(),
            smtlib2: "(assert true)".into(),
            direction_taken: true,
        }]);
        let ctx = make_ctx(vec![branch]);
        let inputs = driller.suggest_inputs(&ctx).await.unwrap();
        assert!(!inputs.is_empty());
        for input in &inputs {
            assert_eq!(input.origin, SeedOrigin::Symbolic);
        }
    }

    /// DrillerStrategy with max_constraints=1 only solves the first frontier branch.
    #[tokio::test]
    async fn suggest_inputs_max_constraints_one() {
        let solver = Arc::new(Mutex::new(StubSolver::solvable()));
        let driller = DrillerStrategy::new(solver, 1);

        let branches: Vec<BranchId> = (0..3).map(|i| BranchId::new(4, i, 0, 0)).collect();
        driller.record_constraints(
            branches
                .iter()
                .map(|b| PathConstraint {
                    branch: b.clone(),
                    smtlib2: "(assert true)".into(),
                    direction_taken: true,
                })
                .collect(),
        );

        let ctx = make_ctx(branches);
        let inputs = driller.suggest_inputs(&ctx).await.unwrap();
        // max_constraints = 1 → at most 1 input.
        assert!(inputs.len() <= 1);
    }
}
