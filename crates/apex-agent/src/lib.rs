//! AI agent orchestration for APEX — multi-agent ensemble strategies,
//! test generation, and coverage-driven refinement loops.

pub mod bandit;
pub mod cache;
pub mod classifier;
pub mod driller;
pub mod ensemble;
pub mod exchange;
pub mod ledger;
pub mod monitor;
pub mod mutation_guide;
pub mod orchestrator;
pub mod priority;
pub mod source;

pub use bandit::StrategyBandit;
pub use classifier::{BranchClassifier, BranchDifficulty};
pub use ledger::BugLedger;
pub use mutation_guide::MutationGuide;
pub use orchestrator::{AgentCluster, OrchestratorConfig};
pub use source::{build_uncovered_with_lines, extract_source_contexts};
