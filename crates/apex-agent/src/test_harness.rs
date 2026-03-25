//! Test harness mocks for apex-agent unit tests.
//!
//! Provides [`ScriptedSandbox`] and [`ScriptedStrategy`] that return prescribed
//! sequences of results, enabling deterministic orchestrator loop testing without
//! spawning real processes.

use apex_core::{
    error::Result,
    traits::{Sandbox, Strategy},
    types::{
        ExecutionResult, ExecutionStatus, ExplorationContext, InputSeed, Language, SeedId,
        SeedOrigin, SnapshotId,
    },
};
use std::sync::Mutex;

// ---------------------------------------------------------------------------
// ScriptedSandbox
// ---------------------------------------------------------------------------

/// A [`Sandbox`] impl that returns prescribed [`ExecutionResult`]s in sequence.
///
/// When the queue is exhausted, returns a configurable fallback (default: Pass
/// with no new branches).  Setting `error_mode = true` makes every call return
/// `Err(apex_core::error::ApexError::Other("scripted error"))`.
pub struct ScriptedSandbox {
    queue: Mutex<std::collections::VecDeque<ExecutionResult>>,
    fallback: ExecutionResult,
    pub error_mode: bool,
}

impl ScriptedSandbox {
    /// Create a sandbox that drains `results` in order, then returns `fallback`.
    pub fn new(results: Vec<ExecutionResult>, fallback: ExecutionResult) -> Self {
        ScriptedSandbox {
            queue: Mutex::new(results.into()),
            fallback,
            error_mode: false,
        }
    }

    /// Create a sandbox that always returns `Err` for every `run()` call.
    pub fn error() -> Self {
        ScriptedSandbox {
            queue: Mutex::new(std::collections::VecDeque::new()),
            fallback: pass_result(SeedId::new()),
            error_mode: true,
        }
    }

    /// Convenience: create a sandbox with an empty queue, using a pass fallback.
    pub fn pass_fallback() -> Self {
        ScriptedSandbox::new(vec![], pass_result(SeedId::new()))
    }
}

#[async_trait::async_trait]
impl Sandbox for ScriptedSandbox {
    async fn run(&self, input: &InputSeed) -> Result<ExecutionResult> {
        if self.error_mode {
            return Err(apex_core::error::ApexError::Other(
                "scripted sandbox error".into(),
            ));
        }
        let mut q = self.queue.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(result) = q.pop_front() {
            Ok(result)
        } else {
            let mut r = self.fallback.clone();
            r.seed_id = input.id;
            Ok(r)
        }
    }

    async fn snapshot(&self) -> Result<SnapshotId> {
        Ok(SnapshotId::new())
    }

    async fn restore(&self, _id: SnapshotId) -> Result<()> {
        Ok(())
    }

    fn language(&self) -> Language {
        Language::Python
    }
}

// ---------------------------------------------------------------------------
// ScriptedStrategy
// ---------------------------------------------------------------------------

/// A [`Strategy`] impl that returns prescribed seed batches in sequence.
///
/// Each call to `suggest_inputs` pops the next batch.  When the queue is
/// exhausted, returns an empty vec (causing the orchestrator's stall counter
/// to increment).
pub struct ScriptedStrategy {
    queue: Mutex<std::collections::VecDeque<Vec<InputSeed>>>,
}

impl ScriptedStrategy {
    /// Create a strategy that yields each inner vec in sequence.
    pub fn new(batches: Vec<Vec<InputSeed>>) -> Self {
        ScriptedStrategy {
            queue: Mutex::new(batches.into()),
        }
    }

    /// Convenience: strategy that always returns an empty vec (stalls immediately).
    pub fn empty() -> Self {
        ScriptedStrategy::new(vec![])
    }

    /// Convenience: strategy that returns a single seed `n` times.
    pub fn repeating(seed: InputSeed, n: usize) -> Self {
        let batches = (0..n).map(|_| vec![seed.clone()]).collect();
        ScriptedStrategy::new(batches)
    }
}

#[async_trait::async_trait]
impl Strategy for ScriptedStrategy {
    fn name(&self) -> &str {
        "scripted"
    }

    async fn suggest_inputs(&self, _ctx: &ExplorationContext) -> Result<Vec<InputSeed>> {
        let mut q = self.queue.lock().unwrap_or_else(|e| e.into_inner());
        Ok(q.pop_front().unwrap_or_default())
    }

    async fn observe(&self, _result: &ExecutionResult) -> Result<()> {
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Build a Pass `ExecutionResult` with no new branches.
pub fn pass_result(seed_id: SeedId) -> ExecutionResult {
    ExecutionResult {
        seed_id,
        status: ExecutionStatus::Pass,
        new_branches: Vec::new(),
        trace: None,
        duration_ms: 0,
        stdout: String::new(),
        stderr: String::new(),
        input: None,
        resource_metrics: None,
    }
}

/// Build a Crash `ExecutionResult` with no new branches.
pub fn crash_result(seed_id: SeedId) -> ExecutionResult {
    ExecutionResult {
        seed_id,
        status: ExecutionStatus::Crash,
        new_branches: Vec::new(),
        trace: None,
        duration_ms: 0,
        stdout: String::new(),
        stderr: "segfault".into(),
        input: None,
        resource_metrics: None,
    }
}

/// Build a Pass `ExecutionResult` that reports `new_branches` as newly covered.
pub fn pass_with_branches(
    seed_id: SeedId,
    new_branches: Vec<apex_core::types::BranchId>,
) -> ExecutionResult {
    ExecutionResult {
        seed_id,
        status: ExecutionStatus::Pass,
        new_branches,
        trace: None,
        duration_ms: 0,
        stdout: String::new(),
        stderr: String::new(),
        input: None,
        resource_metrics: None,
    }
}

/// Create a minimal `InputSeed` for testing.
pub fn test_seed() -> InputSeed {
    InputSeed::new(vec![1, 2, 3], SeedOrigin::Agent)
}
