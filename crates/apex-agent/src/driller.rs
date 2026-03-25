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

        // Build the list of (prefix, smtlib2) pairs while NOT holding the solver
        // lock — the lock is only acquired per solve call to avoid holding a
        // std::sync::Mutex across what may be I/O-bound solver work.
        let solve_tasks: Vec<Vec<String>> = frontier
            .iter()
            .map(|pc| {
                constraints
                    .iter()
                    .take_while(|c| c.branch != pc.branch)
                    .map(|c| c.smtlib2.clone())
                    .chain(std::iter::once(pc.smtlib2.clone()))
                    .collect()
            })
            .collect();

        let mut inputs = Vec::new();
        for prefix in &solve_tasks {
            // Lock, solve, unlock — do not hold across the full loop.
            let result = {
                let solver = self.solver.lock().map_err(|e| {
                    apex_core::error::ApexError::Agent(format!("solver mutex poisoned: {e}"))
                })?;
                solver.solve(prefix, true)
            };
            if let Ok(Some(seed)) = result {
                inputs.push(InputSeed::new(seed.data.to_vec(), SeedOrigin::Symbolic));
            }
        }

        Ok(inputs)
    }

    async fn observe(&self, _result: &ExecutionResult) -> Result<()> {
        Ok(())
    }
}

/// Detects coverage plateaus to trigger driller escalation.
pub struct StuckDetector {
    /// How many iterations to look back.
    plateau_window: usize,
    /// Coverage count below which we consider stuck.
    threshold: usize,
    /// Recent coverage observations: (iteration, total_covered).
    history: Vec<(u64, usize)>,
}

impl StuckDetector {
    pub fn new(plateau_window: usize, threshold: usize) -> Self {
        Self {
            plateau_window,
            threshold,
            history: Vec::new(),
        }
    }

    /// Record an observation. Call each iteration.
    pub fn record(&mut self, iteration: u64, covered: usize) {
        self.history.push((iteration, covered));
    }

    /// True if coverage hasn't grown by more than `threshold` over the last
    /// `plateau_window` observations.
    pub fn is_stuck(&self) -> bool {
        if self.history.len() < self.plateau_window {
            return false;
        }
        let window = &self.history[self.history.len() - self.plateau_window..];
        let first = window[0].1;
        let last = window[window.len() - 1].1;
        let growth = last.saturating_sub(first);
        growth <= self.threshold
    }

    /// Reset after escalation succeeds (new coverage found).
    pub fn reset(&mut self) {
        self.history.clear();
    }
}

/// Wraps StuckDetector + DrillerStrategy for escalation decisions.
///
/// `DrillerStrategy` manages all mutable state via its own internal `Mutex`
/// guards (`constraints`, `solver`). The outer `Arc<Mutex<…>>` that existed
/// here was redundant and caused a mutex-across-await hazard: `escalate()`
/// held the `std::sync::Mutex` guard while calling `suggest_inputs().await`,
/// risking deadlock when Tokio parks the task. Changed to `Arc<DrillerStrategy>`
/// — interior mutability is already handled inside `DrillerStrategy`.
pub struct DrillerEscalation {
    detector: StuckDetector,
    strategy: Arc<DrillerStrategy>,
}

impl DrillerEscalation {
    pub fn new(strategy: Arc<DrillerStrategy>, plateau_window: usize, threshold: usize) -> Self {
        Self {
            detector: StuckDetector::new(plateau_window, threshold),
            strategy,
        }
    }

    /// Record coverage observation (delegates to StuckDetector).
    pub fn record(&mut self, iteration: u64, covered: usize) {
        self.detector.record(iteration, covered);
    }

    /// Should we escalate to symbolic execution?
    pub fn should_escalate(&self) -> bool {
        self.detector.is_stuck()
    }

    /// Perform escalation: use DrillerStrategy to generate constraint-solving seeds.
    ///
    /// No `std::sync::Mutex` guard is held across the `.await` — `strategy` is
    /// an `Arc<DrillerStrategy>` and `suggest_inputs` takes `&self`, acquiring
    /// its own fine-grained locks only for the duration of each synchronous step.
    pub async fn escalate(&mut self, ctx: &ExplorationContext) -> Result<Vec<InputSeed>> {
        let seeds = self.strategy.suggest_inputs(ctx).await?;
        if !seeds.is_empty() {
            self.detector.reset();
        }
        Ok(seeds)
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
            resource_metrics: None,
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
                resource_metrics: None,
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

    // ------------------------------------------------------------------
    // Poisoned-mutex and additional edge-case branch coverage tests
    // ------------------------------------------------------------------

    /// record_constraints recovers from a poisoned constraints mutex
    /// via `unwrap_or_else(|e| e.into_inner())`.
    #[test]
    fn record_constraints_recovers_from_poisoned_mutex() {
        let solver = Arc::new(Mutex::new(StubSolver::solvable()));
        let driller = Arc::new(DrillerStrategy::new(solver, 10));

        // Poison the constraints mutex by panicking while holding the lock.
        let driller_clone = Arc::clone(&driller);
        let _ = std::thread::spawn(move || {
            let _guard = driller_clone.constraints.lock().unwrap();
            panic!("intentional panic to poison mutex");
        })
        .join();

        // The mutex is now poisoned. record_constraints should recover.
        driller.record_constraints(vec![PathConstraint {
            branch: BranchId::new(9, 1, 0, 0),
            smtlib2: "(assert true)".into(),
            direction_taken: true,
        }]);

        // Verify the constraint was recorded despite the poisoned mutex.
        let cs = driller
            .constraints
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        assert_eq!(cs.len(), 1);
    }

    /// suggest_inputs returns an error when the constraints mutex is poisoned.
    #[tokio::test]
    async fn suggest_inputs_errors_on_poisoned_constraints_mutex() {
        let solver = Arc::new(Mutex::new(StubSolver::solvable()));
        let driller = Arc::new(DrillerStrategy::new(solver, 10));

        // First record a constraint so we have data.
        driller.record_constraints(vec![PathConstraint {
            branch: BranchId::new(9, 2, 0, 0),
            smtlib2: "(assert true)".into(),
            direction_taken: true,
        }]);

        // Poison the constraints mutex.
        let driller_clone = Arc::clone(&driller);
        let _ = std::thread::spawn(move || {
            let _guard = driller_clone.constraints.lock().unwrap();
            panic!("intentional panic to poison mutex");
        })
        .join();

        let ctx = make_ctx(vec![BranchId::new(9, 2, 0, 0)]);
        let result = driller.suggest_inputs(&ctx).await;
        assert!(result.is_err());
        let err_msg = format!("{}", result.unwrap_err());
        assert!(err_msg.contains("mutex poisoned"), "got: {err_msg}");
    }

    /// suggest_inputs returns an error when the solver mutex is poisoned.
    #[tokio::test]
    async fn suggest_inputs_errors_on_poisoned_solver_mutex() {
        let solver: Arc<Mutex<dyn Solver>> = Arc::new(Mutex::new(StubSolver::solvable()));
        let driller = DrillerStrategy::new(Arc::clone(&solver), 10);

        let branch = BranchId::new(9, 3, 0, 0);
        driller.record_constraints(vec![PathConstraint {
            branch: branch.clone(),
            smtlib2: "(assert true)".into(),
            direction_taken: true,
        }]);

        // Poison the solver mutex.
        let solver_clone = Arc::clone(&solver);
        let _ = std::thread::spawn(move || {
            let _guard = solver_clone.lock().unwrap();
            panic!("intentional panic to poison solver mutex");
        })
        .join();

        let ctx = make_ctx(vec![branch]);
        let result = driller.suggest_inputs(&ctx).await;
        assert!(result.is_err());
        let err_msg = format!("{}", result.unwrap_err());
        assert!(err_msg.contains("solver mutex poisoned"), "got: {err_msg}");
    }

    /// Solver that alternates between returning a result and returning None,
    /// exercising the Ok(Some) / Ok(None) branches within the same loop.
    #[tokio::test]
    async fn suggest_inputs_mixed_solvable_unsolvable() {
        use std::sync::atomic::{AtomicUsize, Ordering};

        struct AlternatingSolver {
            call_count: AtomicUsize,
        }
        impl Solver for AlternatingSolver {
            fn solve(
                &self,
                _constraints: &[String],
                _negate_last: bool,
            ) -> Result<Option<InputSeed>> {
                let n = self.call_count.fetch_add(1, Ordering::SeqCst);
                if n % 2 == 0 {
                    Ok(Some(InputSeed::new(b"even".to_vec(), SeedOrigin::Symbolic)))
                } else {
                    Ok(None)
                }
            }
            fn set_logic(&mut self, _logic: apex_symbolic::traits::SolverLogic) {}
            fn name(&self) -> &str {
                "alternating"
            }
        }

        let solver = Arc::new(Mutex::new(AlternatingSolver {
            call_count: AtomicUsize::new(0),
        }));
        let driller = DrillerStrategy::new(solver, 10);

        let branches: Vec<BranchId> = (0..4).map(|i| BranchId::new(5, i, 0, 0)).collect();
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
        // Even calls (0, 2) produce seeds; odd calls (1, 3) return None.
        assert_eq!(inputs.len(), 2);
    }

    /// Solver alternates between Ok(Some), Err, and Ok(None) to exercise
    /// all three arms of `if let Ok(Some(seed))` in the same loop.
    #[tokio::test]
    async fn suggest_inputs_all_three_solve_outcomes() {
        use std::sync::atomic::{AtomicUsize, Ordering};

        struct TriStateSolver {
            call_count: AtomicUsize,
        }
        impl Solver for TriStateSolver {
            fn solve(
                &self,
                _constraints: &[String],
                _negate_last: bool,
            ) -> Result<Option<InputSeed>> {
                let n = self.call_count.fetch_add(1, Ordering::SeqCst);
                match n % 3 {
                    0 => Ok(Some(InputSeed::new(
                        b"found".to_vec(),
                        SeedOrigin::Symbolic,
                    ))),
                    1 => Err(apex_core::error::ApexError::Agent("nope".into())),
                    _ => Ok(None),
                }
            }
            fn set_logic(&mut self, _logic: apex_symbolic::traits::SolverLogic) {}
            fn name(&self) -> &str {
                "tristate"
            }
        }

        let solver = Arc::new(Mutex::new(TriStateSolver {
            call_count: AtomicUsize::new(0),
        }));
        let driller = DrillerStrategy::new(solver, 10);

        let branches: Vec<BranchId> = (0..6).map(|i| BranchId::new(6, i, 0, 0)).collect();
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
        // Calls 0, 3 → Ok(Some) → 2 seeds; calls 1,4 → Err; calls 2,5 → Ok(None).
        assert_eq!(inputs.len(), 2);
    }

    /// When the target branch is the very first constraint,
    /// `take_while(|c| c.branch != pc.branch)` yields an empty prefix,
    /// and the chain only includes the target's smtlib2.
    #[tokio::test]
    async fn suggest_inputs_target_is_first_constraint() {
        use std::sync::atomic::{AtomicUsize, Ordering};

        struct PrefixCheckSolver {
            call_count: AtomicUsize,
        }
        impl Solver for PrefixCheckSolver {
            fn solve(
                &self,
                constraints: &[String],
                _negate_last: bool,
            ) -> Result<Option<InputSeed>> {
                self.call_count.fetch_add(1, Ordering::SeqCst);
                // When target is first, prefix should contain just 1 element.
                Ok(Some(InputSeed::new(
                    format!("len={}", constraints.len()).into_bytes(),
                    SeedOrigin::Symbolic,
                )))
            }
            fn set_logic(&mut self, _logic: apex_symbolic::traits::SolverLogic) {}
            fn name(&self) -> &str {
                "prefix-check"
            }
        }

        let solver = Arc::new(Mutex::new(PrefixCheckSolver {
            call_count: AtomicUsize::new(0),
        }));
        let driller = DrillerStrategy::new(solver, 10);

        let b1 = BranchId::new(7, 1, 0, 0);
        let b2 = BranchId::new(7, 2, 0, 0);
        driller.record_constraints(vec![
            PathConstraint {
                branch: b1.clone(),
                smtlib2: "(assert (> x 0))".into(),
                direction_taken: true,
            },
            PathConstraint {
                branch: b2.clone(),
                smtlib2: "(assert (< x 10))".into(),
                direction_taken: true,
            },
        ]);

        // Only b1 is uncovered (it's the first constraint).
        let ctx = make_ctx(vec![b1]);
        let inputs = driller.suggest_inputs(&ctx).await.unwrap();
        assert_eq!(inputs.len(), 1);
        // Prefix should be just the target constraint itself (take_while yields nothing).
        assert_eq!(std::str::from_utf8(&inputs[0].data).unwrap(), "len=1");
    }

    /// When a later branch is the target, the prefix should include
    /// all constraints before it plus itself.
    #[tokio::test]
    async fn suggest_inputs_prefix_includes_prior_constraints() {
        use std::sync::atomic::{AtomicUsize, Ordering};

        struct PrefixLenSolver {
            call_count: AtomicUsize,
        }
        impl Solver for PrefixLenSolver {
            fn solve(
                &self,
                constraints: &[String],
                _negate_last: bool,
            ) -> Result<Option<InputSeed>> {
                self.call_count.fetch_add(1, Ordering::SeqCst);
                Ok(Some(InputSeed::new(
                    format!("len={}", constraints.len()).into_bytes(),
                    SeedOrigin::Symbolic,
                )))
            }
            fn set_logic(&mut self, _logic: apex_symbolic::traits::SolverLogic) {}
            fn name(&self) -> &str {
                "prefix-len"
            }
        }

        let solver = Arc::new(Mutex::new(PrefixLenSolver {
            call_count: AtomicUsize::new(0),
        }));
        let driller = DrillerStrategy::new(solver, 10);

        let b1 = BranchId::new(8, 1, 0, 0);
        let b2 = BranchId::new(8, 2, 0, 0);
        let b3 = BranchId::new(8, 3, 0, 0);
        driller.record_constraints(vec![
            PathConstraint {
                branch: b1.clone(),
                smtlib2: "c1".into(),
                direction_taken: true,
            },
            PathConstraint {
                branch: b2.clone(),
                smtlib2: "c2".into(),
                direction_taken: true,
            },
            PathConstraint {
                branch: b3.clone(),
                smtlib2: "c3".into(),
                direction_taken: true,
            },
        ]);

        // Only b3 uncovered → prefix = [c1, c2, c3] (2 from take_while + 1 chain).
        let ctx = make_ctx(vec![b3]);
        let inputs = driller.suggest_inputs(&ctx).await.unwrap();
        assert_eq!(inputs.len(), 1);
        assert_eq!(std::str::from_utf8(&inputs[0].data).unwrap(), "len=3");
    }

    /// Exercise `new()` with max_constraints = usize::MAX (boundary value).
    #[test]
    fn new_with_max_constraints_max_value() {
        let solver = Arc::new(Mutex::new(StubSolver::solvable()));
        let driller = DrillerStrategy::new(solver, usize::MAX);
        assert_eq!(driller.max_constraints, usize::MAX);
        assert_eq!(driller.name(), "driller");
    }

    /// Multiple frontier branches where only some match uncovered set.
    #[tokio::test]
    async fn suggest_inputs_partial_frontier_overlap() {
        let solver = Arc::new(Mutex::new(StubSolver::solvable()));
        let driller = DrillerStrategy::new(solver, 10);

        let b1 = BranchId::new(10, 1, 0, 0);
        let b2 = BranchId::new(10, 2, 0, 0);
        let b3 = BranchId::new(10, 3, 0, 0);
        let b4 = BranchId::new(10, 4, 0, 0);

        driller.record_constraints(vec![
            PathConstraint {
                branch: b1.clone(),
                smtlib2: "c1".into(),
                direction_taken: true,
            },
            PathConstraint {
                branch: b2.clone(),
                smtlib2: "c2".into(),
                direction_taken: false,
            },
            PathConstraint {
                branch: b3.clone(),
                smtlib2: "c3".into(),
                direction_taken: true,
            },
            PathConstraint {
                branch: b4.clone(),
                smtlib2: "c4".into(),
                direction_taken: false,
            },
        ]);

        // Only b2 and b4 are uncovered (alternating).
        let ctx = make_ctx(vec![b2, b4]);
        let inputs = driller.suggest_inputs(&ctx).await.unwrap();
        assert_eq!(inputs.len(), 2);
    }

    // ------------------------------------------------------------------
    // StuckDetector tests
    // ------------------------------------------------------------------

    #[test]
    fn stuck_detector_not_stuck_initially() {
        let d = StuckDetector::new(5, 0);
        assert!(!d.is_stuck());
    }

    #[test]
    fn stuck_detector_becomes_stuck_on_plateau() {
        let mut d = StuckDetector::new(5, 0);
        for i in 0..5 {
            d.record(i, 100); // same coverage each time
        }
        assert!(d.is_stuck());
    }

    #[test]
    fn stuck_detector_not_stuck_when_growing() {
        let mut d = StuckDetector::new(5, 0);
        for i in 0..5 {
            d.record(i, i as usize * 10);
        }
        assert!(!d.is_stuck());
    }

    #[test]
    fn stuck_detector_reset_clears_history() {
        let mut d = StuckDetector::new(5, 0);
        for i in 0..5 {
            d.record(i, 100);
        }
        assert!(d.is_stuck());
        d.reset();
        assert!(!d.is_stuck());
    }

    #[test]
    fn stuck_detector_threshold_matters() {
        let mut d = StuckDetector::new(5, 2);
        // Growth of 2 within threshold → still stuck
        d.record(0, 100);
        d.record(1, 100);
        d.record(2, 101);
        d.record(3, 101);
        d.record(4, 102);
        assert!(d.is_stuck());
    }

    #[test]
    fn stuck_detector_growth_beyond_threshold() {
        let mut d = StuckDetector::new(5, 2);
        // Growth of 3 exceeds threshold → not stuck
        d.record(0, 100);
        d.record(1, 100);
        d.record(2, 101);
        d.record(3, 102);
        d.record(4, 103);
        assert!(!d.is_stuck());
    }

    #[test]
    fn stuck_detector_empty_window() {
        // Fewer observations than plateau_window → not stuck
        let mut d = StuckDetector::new(10, 0);
        for i in 0..5 {
            d.record(i, 50);
        }
        assert!(!d.is_stuck());
    }

    #[test]
    fn stuck_detector_partial_growth() {
        // Some growth early but last entries flat → stuck
        let mut d = StuckDetector::new(5, 0);
        d.record(0, 100);
        d.record(1, 110);
        d.record(2, 120);
        d.record(3, 120);
        d.record(4, 120);
        // Window: [100, 110, 120, 120, 120] → growth = 120-100 = 20 > 0 → not stuck
        // But if we add more flat entries, the window shifts
        d.record(5, 120);
        d.record(6, 120);
        // Window of last 5: [120, 120, 120, 120, 120] → growth = 0 → stuck
        assert!(d.is_stuck());
    }

    // ------------------------------------------------------------------
    // DrillerEscalation tests
    // ------------------------------------------------------------------

    #[test]
    fn escalation_not_stuck_initially() {
        let solver = Arc::new(Mutex::new(StubSolver::solvable()));
        let strategy = Arc::new(DrillerStrategy::new(solver, 10));
        let esc = DrillerEscalation::new(strategy, 5, 0);
        assert!(!esc.should_escalate());
    }

    #[test]
    fn escalation_delegates_to_stuck_detector() {
        let solver = Arc::new(Mutex::new(StubSolver::solvable()));
        let strategy = Arc::new(DrillerStrategy::new(solver, 10));
        let mut esc = DrillerEscalation::new(strategy, 3, 0);
        for i in 0..3 {
            esc.record(i, 50);
        }
        assert!(esc.should_escalate());
    }

    #[test]
    fn escalation_record_forwards_to_detector() {
        let solver = Arc::new(Mutex::new(StubSolver::solvable()));
        let strategy = Arc::new(DrillerStrategy::new(solver, 10));
        let mut esc = DrillerEscalation::new(strategy, 5, 0);
        // Record fewer than window → not stuck
        esc.record(0, 10);
        esc.record(1, 10);
        assert!(!esc.should_escalate());
    }

    #[test]
    fn escalation_becomes_stuck() {
        let solver = Arc::new(Mutex::new(StubSolver::solvable()));
        let strategy = Arc::new(DrillerStrategy::new(solver, 10));
        let mut esc = DrillerEscalation::new(strategy, 4, 0);
        for i in 0..4 {
            esc.record(i, 100);
        }
        assert!(esc.should_escalate());
    }

    #[tokio::test]
    async fn escalation_resets_on_successful_escalate() {
        let solver = Arc::new(Mutex::new(StubSolver::solvable()));
        let strategy = Arc::new(DrillerStrategy::new(solver, 10));

        // Record a constraint so the solver returns seeds
        let branch = BranchId::new(1, 10, 0, 0);
        strategy.record_constraints(vec![PathConstraint {
            branch,
            smtlib2: "(assert true)".into(),
            direction_taken: true,
        }]);

        let mut esc = DrillerEscalation::new(strategy, 3, 0);
        for i in 0..3 {
            esc.record(i, 50);
        }
        assert!(esc.should_escalate());

        let ctx = make_ctx(vec![BranchId::new(1, 10, 0, 0)]);
        let seeds = esc.escalate(&ctx).await.unwrap();
        assert!(!seeds.is_empty());
        // After successful escalation, detector is reset
        assert!(!esc.should_escalate());
    }
}
