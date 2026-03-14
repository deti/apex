//! AI agent orchestration for APEX — multi-agent ensemble strategies,
//! test generation, and coverage-driven refinement loops.

pub mod adversarial;
pub mod bandit;
pub mod budget;
pub mod cache;
pub mod classifier;
pub mod driller;
pub mod ensemble;
pub mod exchange;
pub mod feedback;
pub mod history;
pub mod ledger;
pub mod monitor;
pub mod mutation_guide;
pub mod orchestrator;
pub mod priority;
pub mod rotation;
pub mod router;
pub mod source;

pub use adversarial::{AdversarialConfig, AdversarialLoop, AdversarialRound};
pub use bandit::StrategyBandit;
pub use budget::BudgetAllocator;
pub use classifier::{BranchClassifier, BranchDifficulty};
pub use feedback::{FeedbackAggregator, StrategyFeedback};
pub use history::{ExplorationLog, LogEntry};
pub use ledger::BugLedger;
pub use mutation_guide::MutationGuide;
pub use orchestrator::{AgentCluster, OrchestratorConfig};
pub use rotation::RotationPolicy;
pub use router::{BranchClass, S2FRouter};
pub use source::{build_uncovered_with_lines, extract_source_contexts};
