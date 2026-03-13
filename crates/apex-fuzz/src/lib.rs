//! Coverage-guided fuzzing for APEX with MOpt mutator scheduling,
//! corpus management, grammar-aware mutation, and optional LibAFL backend.

pub mod cmplog;
pub mod corpus;
pub mod directed;
pub mod grammar;
pub mod libafl_backend;
pub mod mutators;
pub mod plugin;
pub mod scheduler;
pub mod traits;

use crate::corpus::Corpus;
use crate::scheduler::MOptScheduler;
use apex_core::{
    error::{ApexError, Result},
    traits::Strategy,
    types::{ExecutionResult, ExplorationContext, InputSeed, SeedOrigin},
};
use apex_coverage::CoverageOracle;
use async_trait::async_trait;
use rand::{rngs::StdRng, Rng, SeedableRng};
use std::sync::{Arc, Mutex};
use tracing::{debug, info};

const CORPUS_MAX: usize = 10_000;
const MUTATIONS_PER_INPUT: usize = 8; // inputs suggested per scheduler tick

// ---------------------------------------------------------------------------
// FuzzStrategy — coverage-guided mutation fuzzer
// ---------------------------------------------------------------------------

#[allow(dead_code)]
pub struct FuzzStrategy {
    oracle: Arc<CoverageOracle>,
    corpus: Mutex<Corpus>,
    rng: Mutex<StdRng>,
    scheduler: Mutex<MOptScheduler>,
}

impl FuzzStrategy {
    pub fn new(oracle: Arc<CoverageOracle>) -> Self {
        FuzzStrategy {
            oracle,
            corpus: Mutex::new(Corpus::new(CORPUS_MAX)),
            rng: Mutex::new(StdRng::from_entropy()),
            scheduler: Mutex::new(MOptScheduler::new(mutators::builtin_mutators())),
        }
    }

    /// Seed the corpus with known-good inputs (e.g. existing test vectors).
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
}

#[async_trait]
impl Strategy for FuzzStrategy {
    fn name(&self) -> &str {
        "fuzz"
    }

    /// Suggest mutated inputs for the next exploration tick.
    ///
    /// When `--features libafl-backend` is active, callers may instead
    /// construct a [`libafl_backend::LibAflFuzzer`] and call its `generate()`
    /// method, which routes through libafl's HavocMutationalStage pipeline.
    /// This method remains the default path (no libafl dependency required).
    async fn suggest_inputs(&self, _ctx: &ExplorationContext) -> Result<Vec<InputSeed>> {
        let corpus_len = self
            .corpus
            .lock()
            .map_err(|e| ApexError::Other(format!("corpus mutex poisoned: {e}")))?
            .len();
        if corpus_len == 0 {
            // Corpus empty — generate a few random seeds to bootstrap.
            let mut rng = self
                .rng
                .lock()
                .map_err(|e| ApexError::Other(format!("rng mutex poisoned: {e}")))?;
            return Ok((0..MUTATIONS_PER_INPUT)
                .map(|_| {
                    let len = rng.gen_range(1..=64);
                    let data: Vec<u8> = (0..len).map(|_| rng.gen()).collect();
                    InputSeed::new(data, SeedOrigin::Fuzzer)
                })
                .collect());
        }

        let mut inputs = Vec::with_capacity(MUTATIONS_PER_INPUT + 1);

        // Standard mutations from sampled corpus entries.
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

        // Occasional splice for diversity.
        if let Some(spliced) = self.splice_two()? {
            inputs.push(InputSeed::new(spliced, SeedOrigin::Fuzzer));
        }

        debug!(generated = inputs.len(), corpus = corpus_len, "fuzz inputs");
        Ok(inputs)
    }

    async fn observe(&self, result: &ExecutionResult) -> Result<()> {
        // Add to corpus any input that found new coverage.
        if !result.new_branches.is_empty() {
            info!(
                newly_covered = result.new_branches.len(),
                "fuzzer: interesting input added to corpus"
            );
            // The seed data is not stored in ExecutionResult; the orchestrator
            // must call seed_corpus() separately with the winning input.
            // TODO(phase3): thread the winning InputSeed back through result.
        }
        Ok(())
    }
}

/// Compute energy boost from near-miss branch heuristics.
/// Near-miss = heuristic > 0.5 but < 1.0 (close to flipping but not yet covered).
#[allow(dead_code)]
fn near_miss_energy_boost(oracle: &CoverageOracle, uncovered: &[apex_core::types::BranchId]) -> f64 {
    let mut boost = 0.0;
    for branch in uncovered {
        let h = oracle.best_heuristic(branch);
        if h > 0.5 && h < 1.0 {
            boost += h;
        }
    }
    boost
}

// ---------------------------------------------------------------------------
// libafl backend (optional feature) — see libafl_backend.rs
// ---------------------------------------------------------------------------
// When compiled with `--features libafl-backend`, libafl_backend::LibAflFuzzer
// provides a full StdFuzzer pipeline (HavocMutationalStage + MaxMapFeedback +
// QueueScheduler + InMemoryCorpus) as a drop-in complement to FuzzStrategy.

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn empty_corpus_generates_random_seeds() {
        let oracle = Arc::new(CoverageOracle::new());
        let strategy = FuzzStrategy::new(oracle);
        let ctx = ExplorationContext {
            target: apex_core::types::Target {
                root: std::path::PathBuf::from("/tmp"),
                language: apex_core::types::Language::C,
                test_command: vec![],
            },
            uncovered_branches: vec![],
            iteration: 0,
        };
        let inputs = strategy.suggest_inputs(&ctx).await.unwrap();
        assert_eq!(inputs.len(), MUTATIONS_PER_INPUT);
        for seed in &inputs {
            assert_eq!(seed.origin, SeedOrigin::Fuzzer);
            assert!(!seed.data.is_empty());
        }
    }

    #[tokio::test]
    async fn seeded_corpus_produces_mutations() {
        let oracle = Arc::new(CoverageOracle::new());
        let strategy = FuzzStrategy::new(oracle);
        strategy
            .seed_corpus(vec![b"hello world".to_vec(), b"test data".to_vec()])
            .unwrap();

        let ctx = ExplorationContext {
            target: apex_core::types::Target {
                root: std::path::PathBuf::from("/tmp"),
                language: apex_core::types::Language::C,
                test_command: vec![],
            },
            uncovered_branches: vec![],
            iteration: 1,
        };
        let inputs = strategy.suggest_inputs(&ctx).await.unwrap();
        // Should have MUTATIONS_PER_INPUT mutated + possibly 1 splice
        assert!(inputs.len() >= MUTATIONS_PER_INPUT);
    }

    #[test]
    fn strategy_name() {
        let oracle = Arc::new(CoverageOracle::new());
        let strategy = FuzzStrategy::new(oracle);
        assert_eq!(strategy.name(), "fuzz");
    }

    fn make_ctx() -> ExplorationContext {
        ExplorationContext {
            target: apex_core::types::Target {
                root: std::path::PathBuf::from("/tmp"),
                language: apex_core::types::Language::C,
                test_command: vec![],
            },
            uncovered_branches: vec![],
            iteration: 0,
        }
    }

    fn make_result_with_branches(branches: Vec<apex_core::types::BranchId>) -> ExecutionResult {
        ExecutionResult {
            seed_id: apex_core::types::SeedId::new(),
            status: apex_core::types::ExecutionStatus::Pass,
            new_branches: branches,
            trace: None,
            duration_ms: 10,
            stdout: String::new(),
            stderr: String::new(),
        }
    }

    #[tokio::test]
    async fn observe_with_new_branches() {
        let oracle = Arc::new(CoverageOracle::new());
        let strategy = FuzzStrategy::new(oracle);
        let branch = apex_core::types::BranchId::new(1, 10, 0, 0);
        let result = make_result_with_branches(vec![branch]);
        assert!(strategy.observe(&result).await.is_ok());
    }

    #[tokio::test]
    async fn observe_with_empty_branches() {
        let oracle = Arc::new(CoverageOracle::new());
        let strategy = FuzzStrategy::new(oracle);
        let result = make_result_with_branches(vec![]);
        assert!(strategy.observe(&result).await.is_ok());
    }

    #[tokio::test]
    async fn suggest_inputs_varies_across_iterations() {
        let oracle = Arc::new(CoverageOracle::new());
        let strategy = FuzzStrategy::new(oracle);
        strategy
            .seed_corpus(vec![b"aaaa".to_vec(), b"bbbb".to_vec()])
            .unwrap();

        let ctx = make_ctx();
        let first = strategy.suggest_inputs(&ctx).await.unwrap();
        let second = strategy.suggest_inputs(&ctx).await.unwrap();

        // With different RNG draws, outputs should differ
        let first_data: Vec<_> = first.iter().map(|s| &s.data).collect();
        let second_data: Vec<_> = second.iter().map(|s| &s.data).collect();
        assert_ne!(first_data, second_data);
    }

    #[tokio::test]
    async fn splice_not_possible_with_single_entry() {
        let oracle = Arc::new(CoverageOracle::new());
        let strategy = FuzzStrategy::new(oracle);
        strategy.seed_corpus(vec![b"only one".to_vec()]).unwrap();

        let ctx = make_ctx();
        let inputs = strategy.suggest_inputs(&ctx).await.unwrap();
        // With only 1 corpus entry, splice_two returns None, so len == MUTATIONS_PER_INPUT
        assert_eq!(inputs.len(), MUTATIONS_PER_INPUT);
    }

    #[tokio::test]
    async fn fuzz_strategy_uses_scheduler() {
        let oracle = Arc::new(CoverageOracle::new());
        let strategy = FuzzStrategy::new(oracle);
        strategy.seed_corpus(vec![b"test".to_vec()]).unwrap();

        let ctx = ExplorationContext {
            target: apex_core::types::Target {
                root: std::path::PathBuf::from("/tmp/test"),
                language: apex_core::types::Language::Rust,
                test_command: vec!["cargo".into(), "test".into()],
            },
            uncovered_branches: vec![apex_core::types::BranchId::new(1, 1, 0, 0)],
            iteration: 0,
        };
        let inputs = strategy.suggest_inputs(&ctx).await.unwrap();
        assert!(!inputs.is_empty());
    }

    #[test]
    fn near_miss_energy_boost_calculation() {
        let oracle = Arc::new(CoverageOracle::new());
        let b1 = apex_core::types::BranchId::new(1, 1, 0, 0);
        let b2 = apex_core::types::BranchId::new(1, 2, 0, 0);
        let b3 = apex_core::types::BranchId::new(1, 3, 0, 0);

        oracle.record_heuristic(apex_coverage::BranchHeuristic {
            branch_id: b1.clone(),
            score: 0.8,
            operand_a: None,
            operand_b: None,
        });
        oracle.record_heuristic(apex_coverage::BranchHeuristic {
            branch_id: b2.clone(),
            score: 0.3,
            operand_a: None,
            operand_b: None,
        });
        // b3 has no heuristic (0.0 default)

        let boost = near_miss_energy_boost(&oracle, &[b1, b2, b3]);
        assert!((boost - 0.8).abs() < 0.001); // only b1 qualifies (0.5 < 0.8 < 1.0)
    }

    #[tokio::test]
    async fn splice_possible_with_two_entries() {
        let oracle = Arc::new(CoverageOracle::new());
        let strategy = FuzzStrategy::new(oracle);
        strategy
            .seed_corpus(vec![b"alpha".to_vec(), b"bravo".to_vec()])
            .unwrap();

        let ctx = make_ctx();
        let inputs = strategy.suggest_inputs(&ctx).await.unwrap();
        // With 2 corpus entries, splice_two can succeed => len > MUTATIONS_PER_INPUT
        assert!(inputs.len() >= MUTATIONS_PER_INPUT);
        // At least one invocation should produce a splice (MUTATIONS_PER_INPUT + 1)
        // Run multiple times to confirm splice fires at least once
        let mut saw_splice = false;
        for _ in 0..10 {
            let inputs = strategy.suggest_inputs(&ctx).await.unwrap();
            if inputs.len() > MUTATIONS_PER_INPUT {
                saw_splice = true;
                break;
            }
        }
        assert!(
            saw_splice,
            "splice_two should succeed with 2 corpus entries"
        );
    }
}
