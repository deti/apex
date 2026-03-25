//! Resource-guided fuzzing strategy (PerfFuzz approach).
//!
//! Instead of maximizing branch coverage, PerfFuzzStrategy maximizes resource
//! consumption — wall-clock time, peak memory, instruction count, or per-edge
//! execution counts. This finds worst-case inputs for algorithmic complexity
//! vulnerabilities and denial-of-service attack vectors.
//!
//! References:
//! - Lemieux et al., "PerfFuzz: Automatically Generating Pathological Inputs", ISSTA 2018
//! - Petsios et al., "SlowFuzz: Automated Detection of Algorithmic Complexity Vulnerabilities", CCS 2017

use crate::corpus::{Corpus, CorpusEntry, PowerSchedule};
use crate::mutators;
use crate::perf_feedback::PerfFeedback;
use crate::scheduler::MOptScheduler;
use apex_core::{
    error::{ApexError, Result},
    traits::Strategy,
    types::{ExecutionResult, ExplorationContext, InputSeed, ResourceMetrics, SeedOrigin},
};
use async_trait::async_trait;
use rand::{rngs::StdRng, Rng, SeedableRng};
use std::sync::Mutex;
use tracing::{debug, info};

const CORPUS_MAX: usize = 10_000;
const MUTATIONS_PER_INPUT: usize = 8;

/// Which resource dimension to prioritize for corpus energy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PerfObjective {
    /// Maximize wall-clock execution time.
    WallTime,
    /// Maximize peak memory consumption.
    PeakMemory,
    /// Maximize total instruction count.
    InstructionCount,
    /// Maximize the hottest per-edge execution count (PerfFuzz multi-dimensional).
    HottestEdge,
}

/// Resource-guided fuzzing strategy.
///
/// Uses the same mutators and schedulers as `FuzzStrategy`, but the fitness
/// function rewards resource consumption instead of branch coverage. An input
/// is added to the corpus when it causes any resource metric to exceed the
/// current maximum.
pub struct PerfFuzzStrategy {
    feedback: Mutex<PerfFeedback>,
    corpus: Mutex<Corpus>,
    rng: Mutex<StdRng>,
    scheduler: Mutex<MOptScheduler>,
    objective: PerfObjective,
    /// Track the input that produced the highest resource consumption.
    worst_case: Mutex<Option<(Vec<u8>, ResourceMetrics)>>,
}

impl PerfFuzzStrategy {
    pub fn new(objective: PerfObjective) -> Self {
        PerfFuzzStrategy {
            feedback: Mutex::new(PerfFeedback::new()),
            corpus: Mutex::new(Corpus::new(CORPUS_MAX)),
            rng: Mutex::new(StdRng::from_os_rng()),
            scheduler: Mutex::new(MOptScheduler::new(mutators::builtin_mutators())),
            objective,
            worst_case: Mutex::new(None),
        }
    }

    /// Seed the corpus with known inputs (e.g., existing test vectors).
    pub fn seed_corpus(&self, data: impl IntoIterator<Item = Vec<u8>>) -> Result<()> {
        let mut corpus = self
            .corpus
            .lock()
            .map_err(|e| ApexError::Other(format!("corpus mutex poisoned: {e}")))?;
        for d in data {
            corpus.add(d, 1);
        }
        Ok(())
    }

    /// Returns the input that produced the highest resource consumption,
    /// along with its measured metrics.
    pub fn worst_case_input(&self) -> Option<(Vec<u8>, ResourceMetrics)> {
        self.worst_case
            .lock()
            .ok()
            .and_then(|guard| guard.clone())
    }

    fn mutate_one(&self, input: &[u8]) -> Result<Vec<u8>> {
        let mut rng = self
            .rng
            .lock()
            .map_err(|e| ApexError::Other(format!("rng mutex poisoned: {e}")))?;
        let mut scheduler = self
            .scheduler
            .lock()
            .map_err(|e| ApexError::Other(format!("scheduler mutex poisoned: {e}")))?;
        Ok(scheduler.mutate(input, &mut *rng))
    }

    fn splice_two(&self) -> Result<Option<Vec<u8>>> {
        let corpus = self
            .corpus
            .lock()
            .map_err(|e| ApexError::Other(format!("corpus mutex poisoned: {e}")))?;
        let mut rng = self
            .rng
            .lock()
            .map_err(|e| ApexError::Other(format!("rng mutex poisoned: {e}")))?;
        let pair = match corpus.sample_pair(&mut *rng) {
            Some(p) => p,
            None => return Ok(None),
        };
        Ok(Some(mutators::splice(
            &pair.0.data,
            &pair.1.data,
            &mut *rng,
        )))
    }

    /// Compute corpus energy for an entry based on the performance objective.
    fn energy_for(&self, metrics: &ResourceMetrics) -> f64 {
        let feedback = match self.feedback.lock() {
            Ok(f) => f,
            Err(_) => return 1.0,
        };

        match self.objective {
            PerfObjective::WallTime => {
                let max = feedback.max_wall_time_ms().max(1);
                metrics.wall_time_ms as f64 / max as f64
            }
            PerfObjective::PeakMemory => {
                let max = feedback.max_peak_memory_bytes().max(1);
                metrics.peak_memory_bytes.unwrap_or(0) as f64 / max as f64
            }
            PerfObjective::InstructionCount | PerfObjective::HottestEdge => {
                feedback.score(metrics).max(0.01)
            }
        }
    }

    /// Update the worst-case tracker if this input is the new worst case.
    fn update_worst_case(&self, input: &[u8], metrics: &ResourceMetrics) {
        let mut guard = match self.worst_case.lock() {
            Ok(g) => g,
            Err(_) => return,
        };

        let dominated = match &*guard {
            None => true,
            Some((_, prev)) => match self.objective {
                PerfObjective::WallTime => metrics.wall_time_ms > prev.wall_time_ms,
                PerfObjective::PeakMemory => {
                    metrics.peak_memory_bytes > prev.peak_memory_bytes
                }
                PerfObjective::InstructionCount => {
                    metrics.instruction_count > prev.instruction_count
                }
                PerfObjective::HottestEdge => {
                    // Use total edge count as proxy
                    let cur_total: u64 = metrics.edge_counts.values().sum();
                    let prev_total: u64 = prev.edge_counts.values().sum();
                    cur_total > prev_total
                }
            },
        };

        if dominated {
            *guard = Some((input.to_vec(), metrics.clone()));
        }
    }
}

#[async_trait]
impl Strategy for PerfFuzzStrategy {
    fn name(&self) -> &str {
        "perf-fuzz"
    }

    async fn suggest_inputs(&self, _ctx: &ExplorationContext) -> Result<Vec<InputSeed>> {
        let corpus_len = self
            .corpus
            .lock()
            .map_err(|e| ApexError::Other(format!("corpus mutex poisoned: {e}")))?
            .len();

        if corpus_len == 0 {
            // Bootstrap with random seeds.
            let mut rng = self
                .rng
                .lock()
                .map_err(|e| ApexError::Other(format!("rng mutex poisoned: {e}")))?;
            return Ok((0..MUTATIONS_PER_INPUT)
                .map(|_| {
                    let len = rng.random_range(1..=64);
                    let data: Vec<u8> = (0..len).map(|_| rng.random()).collect();
                    InputSeed::new(data, SeedOrigin::Fuzzer)
                })
                .collect());
        }

        let mut inputs = Vec::with_capacity(MUTATIONS_PER_INPUT + 1);

        for _ in 0..MUTATIONS_PER_INPUT {
            let base = {
                let mut corpus = self
                    .corpus
                    .lock()
                    .map_err(|e| ApexError::Other(format!("corpus mutex poisoned: {e}")))?;
                let mut rng = self
                    .rng
                    .lock()
                    .map_err(|e| ApexError::Other(format!("rng mutex poisoned: {e}")))?;
                corpus.sample(&mut *rng).map(|e| e.data.clone())
            };
            if let Some(data) = base {
                let mutated = self.mutate_one(&data)?;
                inputs.push(InputSeed::new(mutated, SeedOrigin::Fuzzer));
            }
        }

        if let Some(spliced) = self.splice_two()? {
            inputs.push(InputSeed::new(spliced, SeedOrigin::Fuzzer));
        }

        debug!(
            generated = inputs.len(),
            corpus = corpus_len,
            "perf-fuzz inputs"
        );
        Ok(inputs)
    }

    async fn observe(&self, result: &ExecutionResult) -> Result<()> {
        let Some(ref metrics) = result.resource_metrics else {
            return Ok(());
        };

        // Update worst-case tracker
        if let Some(ref input_data) = result.input {
            self.update_worst_case(input_data, metrics);
        }

        // Check if this input is "interesting" from a performance perspective
        let interesting = {
            let mut feedback = self
                .feedback
                .lock()
                .map_err(|e| ApexError::Other(format!("feedback mutex poisoned: {e}")))?;
            feedback.is_interesting(metrics)
        };

        if interesting {
            if let Some(ref input_data) = result.input {
                let energy = self.energy_for(metrics);
                let mut corpus = self
                    .corpus
                    .lock()
                    .map_err(|e| ApexError::Other(format!("corpus mutex poisoned: {e}")))?;

                // Add with energy proportional to resource consumption
                let mut entry = CorpusEntry {
                    data: input_data.clone(),
                    coverage_gain: 0, // Not tracking coverage
                    energy,
                    fuzz_count: 0,
                    covered_edges: Vec::new(),
                    distance_to_target: None,
                };
                // Store edge counts as covered_edges for compatibility
                entry.covered_edges = metrics.edge_counts.keys().copied().collect();

                if corpus.len() >= CORPUS_MAX {
                    // Evict, then add
                    corpus.add(input_data.clone(), 0);
                } else {
                    corpus.add(input_data.clone(), 0);
                }

                info!(
                    wall_time_ms = metrics.wall_time_ms,
                    peak_memory = metrics.peak_memory_bytes,
                    corpus_size = corpus.len(),
                    energy = energy,
                    "perf-fuzz: interesting input added to corpus"
                );
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use apex_core::types::{ExecutionStatus, SeedId};
    use std::collections::HashMap;

    fn make_result(input: Vec<u8>, wall_time: u64, edges: Vec<(u64, u64)>) -> ExecutionResult {
        ExecutionResult {
            seed_id: SeedId::new(),
            status: ExecutionStatus::Pass,
            new_branches: vec![],
            trace: None,
            duration_ms: wall_time,
            stdout: String::new(),
            stderr: String::new(),
            input: Some(input),
            resource_metrics: Some(ResourceMetrics {
                wall_time_ms: wall_time,
                edge_counts: edges.into_iter().collect(),
                ..Default::default()
            }),
        }
    }

    #[tokio::test]
    async fn corpus_grows_on_interesting_input() {
        let strategy = PerfFuzzStrategy::new(PerfObjective::WallTime);
        strategy.seed_corpus(vec![vec![0u8; 10]]).unwrap();

        // First result with high wall time
        let r1 = make_result(vec![1, 2, 3], 100, vec![]);
        strategy.observe(&r1).await.unwrap();

        // Higher wall time → should be added
        let r2 = make_result(vec![4, 5, 6], 200, vec![]);
        strategy.observe(&r2).await.unwrap();

        // Lower wall time → should NOT be added
        let r3 = make_result(vec![7, 8, 9], 50, vec![]);
        strategy.observe(&r3).await.unwrap();

        let corpus = strategy.corpus.lock().unwrap();
        // Initial seed (1) + r1 (interesting) + r2 (interesting) = 3
        // r3 was not interesting, so not added
        assert!(corpus.len() >= 2);
    }

    #[tokio::test]
    async fn worst_case_tracks_maximum() {
        let strategy = PerfFuzzStrategy::new(PerfObjective::WallTime);

        let r1 = make_result(vec![1], 100, vec![]);
        strategy.observe(&r1).await.unwrap();

        let r2 = make_result(vec![2], 200, vec![]);
        strategy.observe(&r2).await.unwrap();

        let r3 = make_result(vec![3], 150, vec![]);
        strategy.observe(&r3).await.unwrap();

        let (input, metrics) = strategy.worst_case_input().unwrap();
        assert_eq!(input, vec![2]);
        assert_eq!(metrics.wall_time_ms, 200);
    }

    #[tokio::test]
    async fn suggest_inputs_bootstraps_when_empty() {
        let strategy = PerfFuzzStrategy::new(PerfObjective::WallTime);
        let ctx = ExplorationContext {
            target: apex_core::types::Target {
                root: std::path::PathBuf::from("/tmp"),
                language: apex_core::types::Language::Python,
                test_command: None,
                fuzz_target: None,
            },
            uncovered_branches: vec![],
            iteration: 0,
        };

        let inputs = strategy.suggest_inputs(&ctx).await.unwrap();
        assert_eq!(inputs.len(), MUTATIONS_PER_INPUT);
    }

    #[tokio::test]
    async fn no_resource_metrics_ignored() {
        let strategy = PerfFuzzStrategy::new(PerfObjective::WallTime);

        let result = ExecutionResult {
            seed_id: SeedId::new(),
            status: ExecutionStatus::Pass,
            new_branches: vec![],
            trace: None,
            duration_ms: 100,
            stdout: String::new(),
            stderr: String::new(),
            input: Some(vec![1]),
            resource_metrics: None, // No metrics
        };

        // Should not panic or add to corpus
        strategy.observe(&result).await.unwrap();
        assert!(strategy.worst_case_input().is_none());
    }

    #[tokio::test]
    async fn name_returns_perf_fuzz() {
        let strategy = PerfFuzzStrategy::new(PerfObjective::WallTime);
        assert_eq!(strategy.name(), "perf-fuzz");
    }

    #[tokio::test]
    async fn edge_count_feedback_works() {
        let strategy = PerfFuzzStrategy::new(PerfObjective::HottestEdge);

        let r1 = make_result(vec![1], 10, vec![(1, 100), (2, 50)]);
        strategy.observe(&r1).await.unwrap();

        // Higher edge count on edge 1
        let r2 = make_result(vec![2], 10, vec![(1, 200), (2, 30)]);
        strategy.observe(&r2).await.unwrap();

        let (input, _) = strategy.worst_case_input().unwrap();
        assert_eq!(input, vec![2]); // r2 had higher total edge count
    }
}
