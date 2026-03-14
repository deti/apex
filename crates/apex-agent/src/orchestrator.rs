use crate::driller::DrillerEscalation;
use crate::ledger::BugLedger;
use crate::monitor::{CoverageMonitor, MonitorAction};
use apex_core::{
    error::Result,
    traits::{Sandbox, Strategy},
    types::ExplorationContext,
};
use apex_coverage::CoverageOracle;
use std::sync::Mutex;
use std::{collections::HashMap, path::PathBuf, sync::Arc, time::Instant};
use tracing::{info, warn};

const STALL_THRESHOLD: u64 = 10;

pub struct OrchestratorConfig {
    pub coverage_target: f64,
    pub deadline_secs: Option<u64>,
    pub stall_threshold: u64,
}

impl Default for OrchestratorConfig {
    fn default() -> Self {
        OrchestratorConfig {
            coverage_target: 1.0,
            deadline_secs: None,
            stall_threshold: STALL_THRESHOLD,
        }
    }
}

pub struct AgentCluster {
    pub oracle: Arc<CoverageOracle>,
    pub strategies: Vec<Box<dyn Strategy>>,
    pub sandbox: Arc<dyn Sandbox>,
    pub config: OrchestratorConfig,
    pub target: apex_core::types::Target,
    /// Maps FNV-1a file_id → repo-relative path — used to annotate gap reports
    /// and extract source context for agent prompts.
    pub file_paths: HashMap<u64, PathBuf>,
    /// Accumulates bugs found during exploration.
    pub ledger: Arc<BugLedger>,
    /// Sliding-window coverage growth monitor for stall detection.
    pub monitor: Mutex<CoverageMonitor>,
    /// Optional driller escalation for hybrid fuzzing.
    pub driller_escalation: Option<Mutex<DrillerEscalation>>,
}

impl AgentCluster {
    pub fn new(
        oracle: Arc<CoverageOracle>,
        sandbox: Arc<dyn Sandbox>,
        target: apex_core::types::Target,
    ) -> Self {
        AgentCluster {
            oracle,
            strategies: Vec::new(),
            sandbox,
            config: OrchestratorConfig::default(),
            target,
            file_paths: HashMap::new(),
            ledger: Arc::new(BugLedger::new()),
            monitor: Mutex::new(CoverageMonitor::new(10)),
            driller_escalation: None,
        }
    }

    pub fn with_strategy(mut self, strategy: Box<dyn Strategy>) -> Self {
        self.strategies.push(strategy);
        self
    }

    pub fn with_config(mut self, config: OrchestratorConfig) -> Self {
        self.config = config;
        self
    }

    pub fn with_driller_escalation(mut self, escalation: DrillerEscalation) -> Self {
        self.driller_escalation = Some(Mutex::new(escalation));
        self
    }

    pub fn with_file_paths(mut self, file_paths: HashMap<u64, PathBuf>) -> Self {
        self.file_paths = file_paths;
        self
    }

    pub async fn run(&self) -> Result<()> {
        let start = Instant::now();
        let mut iteration: u64 = 0;
        let mut stall_count: u64 = 0;

        loop {
            let coverage = self.oracle.coverage_percent() / 100.0;
            if coverage >= self.config.coverage_target {
                info!(coverage = %format!("{:.1}%", coverage * 100.0), "coverage target reached");
                break;
            }
            if let Some(deadline) = self.config.deadline_secs {
                if start.elapsed().as_secs() >= deadline {
                    warn!("deadline reached");
                    break;
                }
            }
            let uncovered = self.oracle.uncovered_branches();
            if uncovered.is_empty() {
                info!("all branches covered");
                break;
            }

            let ctx = ExplorationContext {
                target: self.target.clone(),
                uncovered_branches: uncovered.clone(),
                iteration,
            };

            // Run all strategies in parallel.
            let suggestions: Vec<_> =
                futures::future::join_all(self.strategies.iter().map(|s| s.suggest_inputs(&ctx)))
                    .await
                    .into_iter()
                    .filter_map(|r| r.ok())
                    .flatten()
                    .collect();

            if suggestions.is_empty() {
                stall_count += 1;
            } else {
                let results: Vec<_> = futures::future::join_all(
                    suggestions.iter().map(|seed| self.sandbox.run(seed)),
                )
                .await
                .into_iter()
                .filter_map(|r| r.ok())
                .collect();

                let mut new_coverage = false;
                for result in &results {
                    let delta = self.oracle.merge_from_result(result);
                    if !delta.newly_covered.is_empty() {
                        new_coverage = true;
                        info!(
                            newly_covered = delta.newly_covered.len(),
                            total_covered = self.oracle.covered_count(),
                            "new coverage"
                        );
                    }
                    // Record any bugs found.
                    if self.ledger.record_from_result(result, iteration) {
                        info!(
                            class = %apex_core::types::BugClass::from_status(result.status)
                                .map_or("unknown".to_string(), |c| c.to_string()),
                            total_bugs = self.ledger.count(),
                            "bug found"
                        );
                    }
                    for strategy in &self.strategies {
                        let _ = strategy.observe(result).await;
                    }
                }
                stall_count = if new_coverage { 0 } else { stall_count + 1 };
            }

            if self.config.stall_threshold > 0 && stall_count >= self.config.stall_threshold {
                warn!(
                    "coverage stalled after {} iterations with no improvement",
                    stall_count
                );
                break;
            }

            iteration += 1;
        }

        let bug_count = self.ledger.count();
        info!(
            coverage = %format!("{:.1}%", self.oracle.coverage_percent()),
            iterations = iteration,
            bugs_found = bug_count,
            "exploration complete"
        );
        Ok(())
    }

    /// Get the bug summary accumulated during exploration.
    pub fn bug_summary(&self) -> apex_core::types::BugSummary {
        self.ledger.summary()
    }

    pub fn strategy_count(&self) -> usize {
        self.strategies.len()
    }

    pub fn monitor_action(&self) -> MonitorAction {
        self.monitor
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .action()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use apex_core::types::{
        ExecutionResult, ExecutionStatus, InputSeed, Language, SnapshotId, Target,
    };

    // Minimal mock sandbox for construction tests (no actual execution).
    struct StubSandbox;

    #[async_trait::async_trait]
    impl Sandbox for StubSandbox {
        async fn run(&self, input: &InputSeed) -> apex_core::error::Result<ExecutionResult> {
            Ok(ExecutionResult {
                seed_id: input.id,
                status: ExecutionStatus::Pass,
                new_branches: Vec::new(),
                trace: None,
                duration_ms: 0,
                stdout: String::new(),
                stderr: String::new(),
                input: None,
            })
        }
        async fn snapshot(&self) -> apex_core::error::Result<SnapshotId> {
            Ok(SnapshotId::new())
        }
        async fn restore(&self, _id: SnapshotId) -> apex_core::error::Result<()> {
            Ok(())
        }
        fn language(&self) -> Language {
            Language::Python
        }
    }

    fn test_target() -> Target {
        Target {
            root: PathBuf::from("/tmp/test-project"),
            language: Language::Python,
            test_command: vec!["pytest".into()],
        }
    }

    // ------------------------------------------------------------------
    // OrchestratorConfig
    // ------------------------------------------------------------------

    #[test]
    fn config_default_coverage_target() {
        let cfg = OrchestratorConfig::default();
        assert!((cfg.coverage_target - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn config_default_no_deadline() {
        let cfg = OrchestratorConfig::default();
        assert!(cfg.deadline_secs.is_none());
    }

    #[test]
    fn config_default_stall_threshold() {
        let cfg = OrchestratorConfig::default();
        assert_eq!(cfg.stall_threshold, STALL_THRESHOLD);
        assert_eq!(cfg.stall_threshold, 10);
    }

    // ------------------------------------------------------------------
    // AgentCluster construction and builder methods
    // ------------------------------------------------------------------

    #[test]
    fn new_cluster_has_empty_strategies() {
        let oracle = Arc::new(CoverageOracle::new());
        let sandbox: Arc<dyn Sandbox> = Arc::new(StubSandbox);
        let cluster = AgentCluster::new(oracle, sandbox, test_target());
        assert_eq!(cluster.strategy_count(), 0);
    }

    #[test]
    fn new_cluster_has_default_config() {
        let oracle = Arc::new(CoverageOracle::new());
        let sandbox: Arc<dyn Sandbox> = Arc::new(StubSandbox);
        let cluster = AgentCluster::new(oracle, sandbox, test_target());
        assert!((cluster.config.coverage_target - 1.0).abs() < f64::EPSILON);
        assert!(cluster.config.deadline_secs.is_none());
    }

    #[test]
    fn new_cluster_has_empty_file_paths() {
        let oracle = Arc::new(CoverageOracle::new());
        let sandbox: Arc<dyn Sandbox> = Arc::new(StubSandbox);
        let cluster = AgentCluster::new(oracle, sandbox, test_target());
        assert!(cluster.file_paths.is_empty());
    }

    #[test]
    fn with_config_overrides_defaults() {
        let oracle = Arc::new(CoverageOracle::new());
        let sandbox: Arc<dyn Sandbox> = Arc::new(StubSandbox);
        let custom_cfg = OrchestratorConfig {
            coverage_target: 0.75,
            deadline_secs: Some(300),
            stall_threshold: 5,
        };
        let cluster = AgentCluster::new(oracle, sandbox, test_target()).with_config(custom_cfg);
        assert!((cluster.config.coverage_target - 0.75).abs() < f64::EPSILON);
        assert_eq!(cluster.config.deadline_secs, Some(300));
        assert_eq!(cluster.config.stall_threshold, 5);
    }

    #[test]
    fn with_file_paths_sets_map() {
        let oracle = Arc::new(CoverageOracle::new());
        let sandbox: Arc<dyn Sandbox> = Arc::new(StubSandbox);
        let mut paths = HashMap::new();
        paths.insert(42u64, PathBuf::from("src/main.py"));
        paths.insert(99u64, PathBuf::from("src/util.py"));

        let cluster = AgentCluster::new(oracle, sandbox, test_target()).with_file_paths(paths);
        assert_eq!(cluster.file_paths.len(), 2);
        assert_eq!(
            cluster.file_paths.get(&42),
            Some(&PathBuf::from("src/main.py"))
        );
    }

    #[test]
    fn with_strategy_increments_count() {
        use apex_core::types::ExplorationContext;

        struct DummyStrategy;

        #[async_trait::async_trait]
        impl Strategy for DummyStrategy {
            fn name(&self) -> &str {
                "dummy"
            }
            async fn suggest_inputs(
                &self,
                _ctx: &ExplorationContext,
            ) -> apex_core::error::Result<Vec<InputSeed>> {
                Ok(Vec::new())
            }
            async fn observe(&self, _result: &ExecutionResult) -> apex_core::error::Result<()> {
                Ok(())
            }
        }

        let oracle = Arc::new(CoverageOracle::new());
        let sandbox: Arc<dyn Sandbox> = Arc::new(StubSandbox);
        let cluster = AgentCluster::new(oracle, sandbox, test_target())
            .with_strategy(Box::new(DummyStrategy))
            .with_strategy(Box::new(DummyStrategy));
        assert_eq!(cluster.strategy_count(), 2);
    }

    #[test]
    fn cluster_target_matches_construction() {
        let oracle = Arc::new(CoverageOracle::new());
        let sandbox: Arc<dyn Sandbox> = Arc::new(StubSandbox);
        let target = test_target();
        let cluster = AgentCluster::new(oracle, sandbox, target.clone());
        assert_eq!(cluster.target.root, PathBuf::from("/tmp/test-project"));
        assert_eq!(cluster.target.language, Language::Python);
    }

    #[test]
    fn cluster_oracle_is_shared() {
        let oracle = Arc::new(CoverageOracle::new());
        let sandbox: Arc<dyn Sandbox> = Arc::new(StubSandbox);
        let cluster = AgentCluster::new(oracle.clone(), sandbox, test_target());
        // Mutating through oracle should be visible via cluster.oracle.
        let b = apex_core::types::BranchId::new(1, 1, 0, 0);
        oracle.register_branches([b]);
        assert_eq!(cluster.oracle.total_count(), 1);
    }

    // ------------------------------------------------------------------
    // OrchestratorConfig builder edge cases
    // ------------------------------------------------------------------

    #[test]
    fn config_custom_coverage_target() {
        let cfg = OrchestratorConfig {
            coverage_target: 1.0,
            deadline_secs: None,
            stall_threshold: STALL_THRESHOLD,
        };
        assert!((cfg.coverage_target - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn config_zero_coverage_target() {
        let cfg = OrchestratorConfig {
            coverage_target: 0.0,
            deadline_secs: None,
            stall_threshold: 0,
        };
        assert!((cfg.coverage_target - 0.0).abs() < f64::EPSILON);
        assert_eq!(cfg.stall_threshold, 0);
    }

    #[test]
    fn config_with_deadline() {
        let cfg = OrchestratorConfig {
            coverage_target: 0.9,
            deadline_secs: Some(60),
            stall_threshold: STALL_THRESHOLD,
        };
        assert_eq!(cfg.deadline_secs, Some(60));
    }

    #[test]
    fn config_with_zero_deadline() {
        let cfg = OrchestratorConfig {
            coverage_target: 0.9,
            deadline_secs: Some(0),
            stall_threshold: STALL_THRESHOLD,
        };
        assert_eq!(cfg.deadline_secs, Some(0));
    }

    #[test]
    fn config_custom_stall_threshold() {
        let cfg = OrchestratorConfig {
            coverage_target: 0.9,
            deadline_secs: None,
            stall_threshold: 42,
        };
        assert_eq!(cfg.stall_threshold, 42);
    }

    // ------------------------------------------------------------------
    // AgentCluster builder chaining
    // ------------------------------------------------------------------

    #[test]
    fn builder_chain_all_methods() {
        use apex_core::types::ExplorationContext;

        struct DummyStrategy;

        #[async_trait::async_trait]
        impl Strategy for DummyStrategy {
            fn name(&self) -> &str {
                "dummy"
            }
            async fn suggest_inputs(
                &self,
                _ctx: &ExplorationContext,
            ) -> apex_core::error::Result<Vec<InputSeed>> {
                Ok(Vec::new())
            }
            async fn observe(&self, _result: &ExecutionResult) -> apex_core::error::Result<()> {
                Ok(())
            }
        }

        let oracle = Arc::new(CoverageOracle::new());
        let sandbox: Arc<dyn Sandbox> = Arc::new(StubSandbox);
        let mut paths = HashMap::new();
        paths.insert(1u64, PathBuf::from("src/lib.rs"));
        let custom_cfg = OrchestratorConfig {
            coverage_target: 0.5,
            deadline_secs: Some(120),
            stall_threshold: 3,
        };
        let cluster = AgentCluster::new(oracle, sandbox, test_target())
            .with_strategy(Box::new(DummyStrategy))
            .with_config(custom_cfg)
            .with_file_paths(paths);
        assert_eq!(cluster.strategy_count(), 1);
        assert_eq!(cluster.file_paths.len(), 1);
        assert!((cluster.config.coverage_target - 0.5).abs() < f64::EPSILON);
        assert_eq!(cluster.config.deadline_secs, Some(120));
        assert_eq!(cluster.config.stall_threshold, 3);
    }

    #[test]
    fn with_file_paths_replaces_previous() {
        let oracle = Arc::new(CoverageOracle::new());
        let sandbox: Arc<dyn Sandbox> = Arc::new(StubSandbox);
        let mut paths1 = HashMap::new();
        paths1.insert(1u64, PathBuf::from("a.py"));
        let mut paths2 = HashMap::new();
        paths2.insert(2u64, PathBuf::from("b.py"));
        paths2.insert(3u64, PathBuf::from("c.py"));

        let cluster = AgentCluster::new(oracle, sandbox, test_target())
            .with_file_paths(paths1)
            .with_file_paths(paths2);
        // Second call replaces the first.
        assert_eq!(cluster.file_paths.len(), 2);
        assert!(cluster.file_paths.get(&1).is_none());
        assert_eq!(cluster.file_paths.get(&2), Some(&PathBuf::from("b.py")));
    }

    #[test]
    fn with_config_replaces_previous() {
        let oracle = Arc::new(CoverageOracle::new());
        let sandbox: Arc<dyn Sandbox> = Arc::new(StubSandbox);
        let cfg1 = OrchestratorConfig {
            coverage_target: 0.5,
            deadline_secs: Some(10),
            stall_threshold: 1,
        };
        let cfg2 = OrchestratorConfig {
            coverage_target: 0.99,
            deadline_secs: None,
            stall_threshold: 50,
        };
        let cluster = AgentCluster::new(oracle, sandbox, test_target())
            .with_config(cfg1)
            .with_config(cfg2);
        assert!((cluster.config.coverage_target - 0.99).abs() < f64::EPSILON);
        assert!(cluster.config.deadline_secs.is_none());
        assert_eq!(cluster.config.stall_threshold, 50);
    }

    #[test]
    fn strategy_count_zero_for_new_cluster() {
        let oracle = Arc::new(CoverageOracle::new());
        let sandbox: Arc<dyn Sandbox> = Arc::new(StubSandbox);
        let cluster = AgentCluster::new(oracle, sandbox, test_target());
        assert_eq!(cluster.strategy_count(), 0);
    }

    // ------------------------------------------------------------------
    // Constants
    // ------------------------------------------------------------------

    #[test]
    fn stall_threshold_constant() {
        assert_eq!(STALL_THRESHOLD, 10);
    }

    // ------------------------------------------------------------------
    // Target stored correctly
    // ------------------------------------------------------------------

    #[test]
    fn cluster_stores_test_command() {
        let oracle = Arc::new(CoverageOracle::new());
        let sandbox: Arc<dyn Sandbox> = Arc::new(StubSandbox);
        let target = test_target();
        let cluster = AgentCluster::new(oracle, sandbox, target);
        assert_eq!(cluster.target.test_command, vec!["pytest".to_string()]);
    }

    // ------------------------------------------------------------------
    // Async run() tests
    // ------------------------------------------------------------------

    #[tokio::test]
    async fn run_exits_immediately_at_100_percent_coverage() {
        let oracle = Arc::new(CoverageOracle::new());
        // No branches registered → coverage is 100%
        let sandbox: Arc<dyn Sandbox> = Arc::new(StubSandbox);
        let cluster = AgentCluster::new(oracle, sandbox, test_target());
        cluster.run().await.unwrap();
        // Should exit immediately without error
    }

    #[tokio::test]
    async fn run_exits_when_all_branches_covered() {
        let oracle = Arc::new(CoverageOracle::new());
        let b = apex_core::types::BranchId::new(1, 1, 0, 0);
        oracle.register_branches([b.clone()]);
        oracle.mark_covered(&b, apex_core::types::SeedId::new());
        let sandbox: Arc<dyn Sandbox> = Arc::new(StubSandbox);
        let cluster =
            AgentCluster::new(oracle, sandbox, test_target()).with_config(OrchestratorConfig {
                coverage_target: 0.5,
                deadline_secs: None,
                stall_threshold: STALL_THRESHOLD,
            });
        cluster.run().await.unwrap();
    }

    #[tokio::test]
    async fn run_respects_deadline() {
        let oracle = Arc::new(CoverageOracle::new());
        let b = apex_core::types::BranchId::new(1, 1, 0, 0);
        oracle.register_branches([b]);
        // Branch uncovered, but deadline is 0 so should exit immediately
        let sandbox: Arc<dyn Sandbox> = Arc::new(StubSandbox);
        let cluster =
            AgentCluster::new(oracle, sandbox, test_target()).with_config(OrchestratorConfig {
                coverage_target: 1.0,
                deadline_secs: Some(0),
                stall_threshold: STALL_THRESHOLD,
            });
        cluster.run().await.unwrap();
    }

    #[tokio::test]
    async fn run_stalls_without_strategies() {
        let oracle = Arc::new(CoverageOracle::new());
        let b = apex_core::types::BranchId::new(1, 1, 0, 0);
        oracle.register_branches([b]);
        // No strategies → suggestions always empty → stalls → exits via stall threshold
        let sandbox: Arc<dyn Sandbox> = Arc::new(StubSandbox);
        let cluster =
            AgentCluster::new(oracle, sandbox, test_target()).with_config(OrchestratorConfig {
                coverage_target: 1.0,
                deadline_secs: Some(1), // safety net
                stall_threshold: 2,
            });
        cluster.run().await.unwrap();
    }

    #[tokio::test]
    async fn run_with_strategy_that_yields_no_new_coverage() {
        use apex_core::types::ExplorationContext;

        struct EmptyStrategy;
        #[async_trait::async_trait]
        impl Strategy for EmptyStrategy {
            fn name(&self) -> &str {
                "empty"
            }
            async fn suggest_inputs(
                &self,
                _ctx: &ExplorationContext,
            ) -> apex_core::error::Result<Vec<InputSeed>> {
                // Return a seed, but sandbox returns no new branches
                Ok(vec![InputSeed::new(
                    b"test".to_vec(),
                    apex_core::types::SeedOrigin::Fuzzer,
                )])
            }
            async fn observe(&self, _result: &ExecutionResult) -> apex_core::error::Result<()> {
                Ok(())
            }
        }

        let oracle = Arc::new(CoverageOracle::new());
        let b = apex_core::types::BranchId::new(1, 1, 0, 0);
        oracle.register_branches([b]);
        let sandbox: Arc<dyn Sandbox> = Arc::new(StubSandbox);
        let cluster = AgentCluster::new(oracle, sandbox, test_target())
            .with_strategy(Box::new(EmptyStrategy))
            .with_config(OrchestratorConfig {
                coverage_target: 1.0,
                deadline_secs: Some(1),
                stall_threshold: 2,
            });
        cluster.run().await.unwrap();
    }

    // ------------------------------------------------------------------
    // Mock-based run() loop tests — exercise the full exploration loop
    // ------------------------------------------------------------------

    /// Strategy that returns seeds with new_branches matching a given set of BranchIds.
    struct CoveringStrategy {
        branches: Vec<apex_core::types::BranchId>,
    }

    #[async_trait::async_trait]
    impl Strategy for CoveringStrategy {
        fn name(&self) -> &str {
            "covering"
        }
        async fn suggest_inputs(
            &self,
            _ctx: &ExplorationContext,
        ) -> apex_core::error::Result<Vec<InputSeed>> {
            Ok(vec![InputSeed::new(
                b"covering".to_vec(),
                apex_core::types::SeedOrigin::Fuzzer,
            )])
        }
        async fn observe(&self, _result: &ExecutionResult) -> apex_core::error::Result<()> {
            Ok(())
        }
    }

    /// Sandbox that returns results covering the specified branches (one-shot).
    struct CoveringSandbox {
        branches: std::sync::Mutex<Vec<apex_core::types::BranchId>>,
    }

    impl CoveringSandbox {
        fn new(branches: Vec<apex_core::types::BranchId>) -> Self {
            CoveringSandbox {
                branches: std::sync::Mutex::new(branches),
            }
        }
    }

    #[async_trait::async_trait]
    impl Sandbox for CoveringSandbox {
        async fn run(&self, input: &InputSeed) -> apex_core::error::Result<ExecutionResult> {
            // Return all remaining branches on first call, then empty
            let new_branches = {
                let mut guard = self.branches.lock().unwrap();
                std::mem::take(&mut *guard)
            };
            Ok(ExecutionResult {
                seed_id: input.id,
                status: ExecutionStatus::Pass,
                new_branches,
                trace: None,
                duration_ms: 1,
                stdout: String::new(),
                stderr: String::new(),
                input: None,
            })
        }
        async fn snapshot(&self) -> apex_core::error::Result<SnapshotId> {
            Ok(SnapshotId::new())
        }
        async fn restore(&self, _id: SnapshotId) -> apex_core::error::Result<()> {
            Ok(())
        }
        fn language(&self) -> Language {
            Language::Python
        }
    }

    #[tokio::test]
    async fn run_happy_path_reaches_coverage_target() {
        let oracle = Arc::new(CoverageOracle::new());
        let b1 = apex_core::types::BranchId::new(1, 1, 0, 0);
        let b2 = apex_core::types::BranchId::new(1, 2, 0, 0);
        oracle.register_branches([b1.clone(), b2.clone()]);

        let sandbox: Arc<dyn Sandbox> =
            Arc::new(CoveringSandbox::new(vec![b1.clone(), b2.clone()]));
        let strategy = CoveringStrategy {
            branches: vec![b1, b2],
        };

        let cluster = AgentCluster::new(oracle.clone(), sandbox, test_target())
            .with_strategy(Box::new(strategy))
            .with_config(OrchestratorConfig {
                coverage_target: 0.9,
                deadline_secs: Some(5),
                stall_threshold: 100,
            });

        cluster.run().await.unwrap();
        assert_eq!(oracle.coverage_percent(), 100.0);
    }

    #[tokio::test]
    async fn run_with_coverage_producing_strategy_resets_stall() {
        // First iteration produces coverage, resetting stall counter.
        // Then sandbox stops producing coverage → eventually exits via deadline.
        let oracle = Arc::new(CoverageOracle::new());
        let b1 = apex_core::types::BranchId::new(1, 1, 0, 0);
        let b2 = apex_core::types::BranchId::new(1, 2, 0, 0);
        oracle.register_branches([b1.clone(), b2.clone()]);

        // Sandbox covers b1 on first run, then nothing more.
        let sandbox: Arc<dyn Sandbox> = Arc::new(CoveringSandbox::new(vec![b1.clone()]));

        struct AlwaysSuggest;
        #[async_trait::async_trait]
        impl Strategy for AlwaysSuggest {
            fn name(&self) -> &str {
                "suggest"
            }
            async fn suggest_inputs(
                &self,
                _ctx: &ExplorationContext,
            ) -> apex_core::error::Result<Vec<InputSeed>> {
                Ok(vec![InputSeed::new(
                    b"data".to_vec(),
                    apex_core::types::SeedOrigin::Fuzzer,
                )])
            }
            async fn observe(&self, _result: &ExecutionResult) -> apex_core::error::Result<()> {
                Ok(())
            }
        }

        let cluster = AgentCluster::new(oracle.clone(), sandbox, test_target())
            .with_strategy(Box::new(AlwaysSuggest))
            .with_config(OrchestratorConfig {
                coverage_target: 1.0,
                deadline_secs: Some(2),
                stall_threshold: 3,
            });

        cluster.run().await.unwrap();
        assert_eq!(oracle.covered_count(), 1); // b1 was covered
    }

    #[tokio::test]
    async fn run_with_multiple_strategies() {
        let oracle = Arc::new(CoverageOracle::new());
        let b1 = apex_core::types::BranchId::new(1, 1, 0, 0);
        oracle.register_branches([b1.clone()]);

        struct SimpleStrategy(&'static str);
        #[async_trait::async_trait]
        impl Strategy for SimpleStrategy {
            fn name(&self) -> &str {
                self.0
            }
            async fn suggest_inputs(
                &self,
                _ctx: &ExplorationContext,
            ) -> apex_core::error::Result<Vec<InputSeed>> {
                Ok(vec![InputSeed::new(
                    b"s".to_vec(),
                    apex_core::types::SeedOrigin::Fuzzer,
                )])
            }
            async fn observe(&self, _result: &ExecutionResult) -> apex_core::error::Result<()> {
                Ok(())
            }
        }

        let sandbox: Arc<dyn Sandbox> = Arc::new(CoveringSandbox::new(vec![b1]));
        let cluster = AgentCluster::new(oracle.clone(), sandbox, test_target())
            .with_strategy(Box::new(SimpleStrategy("s1")))
            .with_strategy(Box::new(SimpleStrategy("s2")))
            .with_config(OrchestratorConfig {
                coverage_target: 0.9,
                deadline_secs: Some(5),
                stall_threshold: 100,
            });

        cluster.run().await.unwrap();
        assert_eq!(oracle.coverage_percent(), 100.0);
    }

    #[tokio::test]
    async fn run_with_failing_strategy_continues() {
        let oracle = Arc::new(CoverageOracle::new());
        let b = apex_core::types::BranchId::new(1, 1, 0, 0);
        oracle.register_branches([b]);

        struct FailingStrategy;
        #[async_trait::async_trait]
        impl Strategy for FailingStrategy {
            fn name(&self) -> &str {
                "failing"
            }
            async fn suggest_inputs(
                &self,
                _ctx: &ExplorationContext,
            ) -> apex_core::error::Result<Vec<InputSeed>> {
                Err(apex_core::error::ApexError::Other("strategy error".into()))
            }
            async fn observe(&self, _result: &ExecutionResult) -> apex_core::error::Result<()> {
                Ok(())
            }
        }

        let sandbox: Arc<dyn Sandbox> = Arc::new(StubSandbox);
        let cluster = AgentCluster::new(oracle, sandbox, test_target())
            .with_strategy(Box::new(FailingStrategy))
            .with_config(OrchestratorConfig {
                coverage_target: 1.0,
                deadline_secs: Some(1),
                stall_threshold: 2,
            });

        // Should not panic — strategy errors are filtered out
        cluster.run().await.unwrap();
    }

    #[test]
    fn orchestrator_has_monitor() {
        let oracle = Arc::new(CoverageOracle::new());
        let target = Target {
            root: PathBuf::from("/tmp"),
            language: Language::Rust,
            test_command: vec![],
        };
        let cluster = AgentCluster::new(oracle, Arc::new(StubSandbox), target);
        assert_eq!(
            cluster.monitor_action(),
            crate::monitor::MonitorAction::Normal
        );
    }

    // ------------------------------------------------------------------
    // Additional branch-coverage tests
    // ------------------------------------------------------------------

    /// A sandbox that always returns Err — exercises the filter_map(|r| r.ok())
    /// path in the run() loop where sandbox results are discarded on error.
    struct ErrorSandbox;

    #[async_trait::async_trait]
    impl Sandbox for ErrorSandbox {
        async fn run(&self, _input: &InputSeed) -> apex_core::error::Result<ExecutionResult> {
            Err(apex_core::error::ApexError::Other("sandbox failure".into()))
        }
        async fn snapshot(&self) -> apex_core::error::Result<SnapshotId> {
            Ok(SnapshotId::new())
        }
        async fn restore(&self, _id: SnapshotId) -> apex_core::error::Result<()> {
            Ok(())
        }
        fn language(&self) -> Language {
            Language::Python
        }
    }

    #[tokio::test]
    async fn run_with_failing_sandbox_does_not_panic() {
        // Strategy returns a seed, but sandbox always errors.
        // The filter_map(|r| r.ok()) silently drops the errors; stall_count
        // increments (suggestions non-empty but no coverage) until threshold.
        struct ConstantSeeder;
        #[async_trait::async_trait]
        impl Strategy for ConstantSeeder {
            fn name(&self) -> &str {
                "seeder"
            }
            async fn suggest_inputs(
                &self,
                _ctx: &ExplorationContext,
            ) -> apex_core::error::Result<Vec<InputSeed>> {
                Ok(vec![InputSeed::new(
                    b"x".to_vec(),
                    apex_core::types::SeedOrigin::Fuzzer,
                )])
            }
            async fn observe(&self, _result: &ExecutionResult) -> apex_core::error::Result<()> {
                Ok(())
            }
        }

        let oracle = Arc::new(CoverageOracle::new());
        let b = apex_core::types::BranchId::new(1, 1, 0, 0);
        oracle.register_branches([b]);

        let cluster = AgentCluster::new(oracle, Arc::new(ErrorSandbox), test_target())
            .with_strategy(Box::new(ConstantSeeder))
            .with_config(OrchestratorConfig {
                coverage_target: 1.0,
                deadline_secs: Some(5),
                stall_threshold: 2,
            });

        // Must complete without panic and return Ok.
        cluster.run().await.unwrap();
    }

    #[tokio::test]
    async fn run_stall_count_increments_when_suggestions_produce_no_coverage() {
        // Strategy returns seeds but sandbox returns no new branches →
        // stall_count increments on the "else" arm (suggestions non-empty,
        // but new_coverage = false).  With stall_threshold=2 the loop exits.
        struct AlwaysSeeds;
        #[async_trait::async_trait]
        impl Strategy for AlwaysSeeds {
            fn name(&self) -> &str {
                "seeds"
            }
            async fn suggest_inputs(
                &self,
                _ctx: &ExplorationContext,
            ) -> apex_core::error::Result<Vec<InputSeed>> {
                Ok(vec![InputSeed::new(
                    b"no-cover".to_vec(),
                    apex_core::types::SeedOrigin::Fuzzer,
                )])
            }
            async fn observe(&self, _result: &ExecutionResult) -> apex_core::error::Result<()> {
                Ok(())
            }
        }

        let oracle = Arc::new(CoverageOracle::new());
        let b = apex_core::types::BranchId::new(10, 1, 0, 0);
        oracle.register_branches([b]);

        // StubSandbox returns Pass with no new_branches → new_coverage stays false.
        let cluster = AgentCluster::new(oracle.clone(), Arc::new(StubSandbox), test_target())
            .with_strategy(Box::new(AlwaysSeeds))
            .with_config(OrchestratorConfig {
                coverage_target: 1.0,
                deadline_secs: None,
                stall_threshold: 2,
            });

        cluster.run().await.unwrap();
        // Branch should remain uncovered — sandbox never produced new branches.
        assert_eq!(oracle.covered_count(), 0);
    }

    #[tokio::test]
    async fn run_stall_count_resets_on_new_coverage() {
        // First iteration: CoveringSandbox returns branch b1 → new_coverage = true
        //   → stall_count reset to 0.
        // Second iteration: sandbox returns nothing → stall_count increments.
        // Eventually exits via stall threshold.
        let oracle = Arc::new(CoverageOracle::new());
        let b1 = apex_core::types::BranchId::new(20, 1, 0, 0);
        let b2 = apex_core::types::BranchId::new(20, 2, 0, 0);
        oracle.register_branches([b1.clone(), b2.clone()]);

        // CoveringSandbox only returns branches on the first call.
        let sandbox: Arc<dyn Sandbox> = Arc::new(CoveringSandbox::new(vec![b1.clone()]));

        struct AlwaysSeeds2;
        #[async_trait::async_trait]
        impl Strategy for AlwaysSeeds2 {
            fn name(&self) -> &str {
                "seeds2"
            }
            async fn suggest_inputs(
                &self,
                _ctx: &ExplorationContext,
            ) -> apex_core::error::Result<Vec<InputSeed>> {
                Ok(vec![InputSeed::new(
                    b"data".to_vec(),
                    apex_core::types::SeedOrigin::Fuzzer,
                )])
            }
            async fn observe(&self, _result: &ExecutionResult) -> apex_core::error::Result<()> {
                Ok(())
            }
        }

        let cluster = AgentCluster::new(oracle.clone(), sandbox, test_target())
            .with_strategy(Box::new(AlwaysSeeds2))
            .with_config(OrchestratorConfig {
                coverage_target: 1.0,
                deadline_secs: None,
                stall_threshold: 3,
            });

        cluster.run().await.unwrap();
        // b1 was covered on the first iteration.
        assert!(oracle.covered_count() >= 1);
    }

    #[tokio::test]
    async fn run_observe_error_is_silently_ignored() {
        // Strategy.observe() returns Err — the `let _ = strategy.observe(result).await`
        // suppresses the error; the loop must continue normally.
        struct ErrorObserve;
        #[async_trait::async_trait]
        impl Strategy for ErrorObserve {
            fn name(&self) -> &str {
                "err-observe"
            }
            async fn suggest_inputs(
                &self,
                _ctx: &ExplorationContext,
            ) -> apex_core::error::Result<Vec<InputSeed>> {
                Ok(vec![InputSeed::new(
                    b"e".to_vec(),
                    apex_core::types::SeedOrigin::Fuzzer,
                )])
            }
            async fn observe(&self, _result: &ExecutionResult) -> apex_core::error::Result<()> {
                Err(apex_core::error::ApexError::Other("observe failed".into()))
            }
        }

        let oracle = Arc::new(CoverageOracle::new());
        let b = apex_core::types::BranchId::new(30, 1, 0, 0);
        oracle.register_branches([b]);

        let cluster = AgentCluster::new(oracle, Arc::new(StubSandbox), test_target())
            .with_strategy(Box::new(ErrorObserve))
            .with_config(OrchestratorConfig {
                coverage_target: 1.0,
                deadline_secs: Some(5),
                stall_threshold: 2,
            });

        cluster.run().await.unwrap();
    }

    #[tokio::test]
    async fn run_no_deadline_exits_via_coverage_target() {
        // Exercises the `deadline_secs = None` branch: the `if let Some(deadline)`
        // guard is skipped entirely; loop exits via coverage target instead.
        let oracle = Arc::new(CoverageOracle::new());
        let b = apex_core::types::BranchId::new(40, 1, 0, 0);
        oracle.register_branches([b.clone()]);

        let sandbox: Arc<dyn Sandbox> = Arc::new(CoveringSandbox::new(vec![b.clone()]));

        struct OneShot2;
        #[async_trait::async_trait]
        impl Strategy for OneShot2 {
            fn name(&self) -> &str {
                "oneshot2"
            }
            async fn suggest_inputs(
                &self,
                _ctx: &ExplorationContext,
            ) -> apex_core::error::Result<Vec<InputSeed>> {
                Ok(vec![InputSeed::new(
                    b"nd".to_vec(),
                    apex_core::types::SeedOrigin::Fuzzer,
                )])
            }
            async fn observe(&self, _result: &ExecutionResult) -> apex_core::error::Result<()> {
                Ok(())
            }
        }

        let cluster = AgentCluster::new(oracle.clone(), sandbox, test_target())
            .with_strategy(Box::new(OneShot2))
            .with_config(OrchestratorConfig {
                coverage_target: 0.9,
                deadline_secs: None, // <-- the None branch under test
                stall_threshold: 100,
            });

        cluster.run().await.unwrap();
        assert_eq!(oracle.coverage_percent(), 100.0);
    }

    #[tokio::test]
    async fn run_exits_via_stall_threshold_no_deadline() {
        // Exercises stall_threshold exit without a deadline being set.
        // No strategies → suggestions always empty → stall increments until threshold.
        let oracle = Arc::new(CoverageOracle::new());
        let b = apex_core::types::BranchId::new(50, 1, 0, 0);
        oracle.register_branches([b]);

        let cluster = AgentCluster::new(oracle, Arc::new(StubSandbox), test_target()).with_config(
            OrchestratorConfig {
                coverage_target: 1.0,
                deadline_secs: None,
                stall_threshold: 1,
            },
        );

        cluster.run().await.unwrap();
    }

    #[tokio::test]
    async fn run_bug_summary_empty_when_no_bugs() {
        // After a run where sandbox only returns Pass, ledger should be empty.
        let oracle = Arc::new(CoverageOracle::new());
        // No branches → immediately exits via coverage target (0/0 = 100%).
        let cluster = AgentCluster::new(oracle, Arc::new(StubSandbox), test_target());
        cluster.run().await.unwrap();
        let summary = cluster.bug_summary();
        assert_eq!(summary.total, 0);
    }

    #[tokio::test]
    async fn run_records_timeout_bug() {
        // Sandbox returns Timeout status → BugLedger records it as a bug.
        struct TimeoutSandbox;
        #[async_trait::async_trait]
        impl Sandbox for TimeoutSandbox {
            async fn run(&self, input: &InputSeed) -> apex_core::error::Result<ExecutionResult> {
                Ok(ExecutionResult {
                    seed_id: input.id,
                    status: ExecutionStatus::Timeout,
                    new_branches: Vec::new(),
                    trace: None,
                    duration_ms: 5000,
                    stdout: String::new(),
                    stderr: String::new(),
                    input: None,
                })
            }
            async fn snapshot(&self) -> apex_core::error::Result<SnapshotId> {
                Ok(SnapshotId::new())
            }
            async fn restore(&self, _id: SnapshotId) -> apex_core::error::Result<()> {
                Ok(())
            }
            fn language(&self) -> Language {
                Language::Python
            }
        }

        struct OneShotSeeder {
            done: std::sync::Mutex<bool>,
        }
        #[async_trait::async_trait]
        impl Strategy for OneShotSeeder {
            fn name(&self) -> &str {
                "timeout-seeder"
            }
            async fn suggest_inputs(
                &self,
                _ctx: &ExplorationContext,
            ) -> apex_core::error::Result<Vec<InputSeed>> {
                let mut done = self.done.lock().unwrap();
                if *done {
                    Ok(Vec::new())
                } else {
                    *done = true;
                    Ok(vec![InputSeed::new(
                        b"hang".to_vec(),
                        apex_core::types::SeedOrigin::Fuzzer,
                    )])
                }
            }
            async fn observe(&self, _result: &ExecutionResult) -> apex_core::error::Result<()> {
                Ok(())
            }
        }

        let oracle = Arc::new(CoverageOracle::new());
        let b = apex_core::types::BranchId::new(60, 1, 0, 0);
        oracle.register_branches([b]);

        let cluster = AgentCluster::new(oracle, Arc::new(TimeoutSandbox), test_target())
            .with_strategy(Box::new(OneShotSeeder {
                done: std::sync::Mutex::new(false),
            }))
            .with_config(OrchestratorConfig {
                coverage_target: 1.0,
                deadline_secs: Some(5),
                stall_threshold: 2,
            });

        cluster.run().await.unwrap();
        let summary = cluster.bug_summary();
        assert!(summary.total > 0, "expected timeout bug to be recorded");
    }

    // ------------------------------------------------------------------
    // Bug recording, new coverage logging, and observe notification tests
    // ------------------------------------------------------------------

    /// Sandbox that always returns Crash status (no new branches).
    struct CrashSandbox;

    #[async_trait::async_trait]
    impl Sandbox for CrashSandbox {
        async fn run(&self, input: &InputSeed) -> apex_core::error::Result<ExecutionResult> {
            Ok(ExecutionResult {
                seed_id: input.id,
                status: ExecutionStatus::Crash,
                new_branches: Vec::new(),
                trace: None,
                duration_ms: 1,
                stdout: String::new(),
                stderr: "segfault".into(),
                input: None,
            })
        }
        async fn snapshot(&self) -> apex_core::error::Result<SnapshotId> {
            Ok(SnapshotId::new())
        }
        async fn restore(&self, _id: SnapshotId) -> apex_core::error::Result<()> {
            Ok(())
        }
        fn language(&self) -> Language {
            Language::Python
        }
    }

    #[tokio::test]
    async fn run_records_bug_from_result() {
        // Strategy returns a seed; CrashSandbox returns Crash status.
        // After run(), bug_summary() should have count > 0.
        struct OneShotStrategy {
            fired: std::sync::Mutex<bool>,
        }
        #[async_trait::async_trait]
        impl Strategy for OneShotStrategy {
            fn name(&self) -> &str {
                "one-shot"
            }
            async fn suggest_inputs(
                &self,
                _ctx: &ExplorationContext,
            ) -> apex_core::error::Result<Vec<InputSeed>> {
                let mut fired = self.fired.lock().unwrap();
                if *fired {
                    Ok(Vec::new())
                } else {
                    *fired = true;
                    Ok(vec![InputSeed::new(
                        b"crash-me".to_vec(),
                        apex_core::types::SeedOrigin::Fuzzer,
                    )])
                }
            }
            async fn observe(&self, _result: &ExecutionResult) -> apex_core::error::Result<()> {
                Ok(())
            }
        }

        let oracle = Arc::new(CoverageOracle::new());
        let b = apex_core::types::BranchId::new(1, 1, 0, 0);
        oracle.register_branches([b]);
        // Use deadline to bound the loop after the first iteration produces no suggestions.
        let cluster = AgentCluster::new(oracle, Arc::new(CrashSandbox), test_target())
            .with_strategy(Box::new(OneShotStrategy {
                fired: std::sync::Mutex::new(false),
            }))
            .with_config(OrchestratorConfig {
                coverage_target: 1.0,
                deadline_secs: Some(5),
                stall_threshold: 2,
            });

        cluster.run().await.unwrap();
        let summary = cluster.bug_summary();
        assert!(
            summary.total > 0,
            "expected at least one bug recorded, got {}",
            summary.total
        );
    }

    #[tokio::test]
    async fn run_logs_new_coverage_from_results() {
        // Strategy suggests seeds; CoveringSandbox returns results with new_branches.
        // After run(), the oracle should report 100% coverage.
        let oracle = Arc::new(CoverageOracle::new());
        let b1 = apex_core::types::BranchId::new(2, 1, 0, 0);
        let b2 = apex_core::types::BranchId::new(2, 2, 0, 0);
        oracle.register_branches([b1.clone(), b2.clone()]);

        let sandbox: Arc<dyn Sandbox> =
            Arc::new(CoveringSandbox::new(vec![b1.clone(), b2.clone()]));

        struct ConstantStrategy;
        #[async_trait::async_trait]
        impl Strategy for ConstantStrategy {
            fn name(&self) -> &str {
                "constant"
            }
            async fn suggest_inputs(
                &self,
                _ctx: &ExplorationContext,
            ) -> apex_core::error::Result<Vec<InputSeed>> {
                Ok(vec![InputSeed::new(
                    b"input".to_vec(),
                    apex_core::types::SeedOrigin::Fuzzer,
                )])
            }
            async fn observe(&self, _result: &ExecutionResult) -> apex_core::error::Result<()> {
                Ok(())
            }
        }

        let cluster = AgentCluster::new(oracle.clone(), sandbox, test_target())
            .with_strategy(Box::new(ConstantStrategy))
            .with_config(OrchestratorConfig {
                coverage_target: 0.9,
                deadline_secs: Some(5),
                stall_threshold: 100,
            });

        cluster.run().await.unwrap();
        // Both branches should have been merged via oracle.
        assert_eq!(oracle.coverage_percent(), 100.0);
        assert_eq!(oracle.covered_count(), 2);
    }

    #[tokio::test]
    async fn run_with_observe_notifies_strategies() {
        // Strategy tracks how many times observe() was called.
        use std::sync::atomic::{AtomicUsize, Ordering};

        struct CountingStrategy {
            observe_count: Arc<AtomicUsize>,
        }
        #[async_trait::async_trait]
        impl Strategy for CountingStrategy {
            fn name(&self) -> &str {
                "counting"
            }
            async fn suggest_inputs(
                &self,
                _ctx: &ExplorationContext,
            ) -> apex_core::error::Result<Vec<InputSeed>> {
                Ok(vec![InputSeed::new(
                    b"obs".to_vec(),
                    apex_core::types::SeedOrigin::Fuzzer,
                )])
            }
            async fn observe(&self, _result: &ExecutionResult) -> apex_core::error::Result<()> {
                self.observe_count.fetch_add(1, Ordering::SeqCst);
                Ok(())
            }
        }

        let observe_count = Arc::new(AtomicUsize::new(0));

        let oracle = Arc::new(CoverageOracle::new());
        let b = apex_core::types::BranchId::new(3, 1, 0, 0);
        oracle.register_branches([b.clone()]);

        // CoveringSandbox covers the branch on first call; after that sandbox returns empty.
        let sandbox: Arc<dyn Sandbox> = Arc::new(CoveringSandbox::new(vec![b]));

        let cluster = AgentCluster::new(oracle.clone(), sandbox, test_target())
            .with_strategy(Box::new(CountingStrategy {
                observe_count: observe_count.clone(),
            }))
            .with_config(OrchestratorConfig {
                coverage_target: 0.9,
                deadline_secs: Some(5),
                stall_threshold: 100,
            });

        cluster.run().await.unwrap();
        // observe() must have been called at least once (once per sandbox result per strategy).
        assert!(
            observe_count.load(Ordering::SeqCst) > 0,
            "expected observe() to be called at least once"
        );
    }

    #[tokio::test]
    async fn run_stops_when_all_branches_covered() {
        // Register branches then pre-cover them all — run() should exit immediately.
        let oracle = Arc::new(CoverageOracle::new());
        let b = apex_core::types::BranchId::new(1, 1, 0, 0);
        oracle.register_branches([b.clone()]);
        oracle.mark_covered(&b, apex_core::types::SeedId::new());
        assert_eq!(oracle.uncovered_branches().len(), 0);

        let sandbox: Arc<dyn Sandbox> = Arc::new(StubSandbox);
        let cluster = AgentCluster::new(oracle, sandbox, test_target());
        // Should return immediately with Ok
        cluster.run().await.unwrap();
    }

    #[tokio::test]
    async fn run_stops_on_deadline() {
        // Register uncovered branches but set deadline to 0 so it fires immediately.
        let oracle = Arc::new(CoverageOracle::new());
        let b = apex_core::types::BranchId::new(1, 1, 0, 0);
        oracle.register_branches([b]);

        struct NeverStrategy;
        #[async_trait::async_trait]
        impl Strategy for NeverStrategy {
            fn name(&self) -> &str {
                "never"
            }
            async fn suggest_inputs(
                &self,
                _ctx: &ExplorationContext,
            ) -> apex_core::error::Result<Vec<InputSeed>> {
                Ok(vec![])
            }
            async fn observe(&self, _result: &ExecutionResult) -> apex_core::error::Result<()> {
                Ok(())
            }
        }

        let sandbox: Arc<dyn Sandbox> = Arc::new(StubSandbox);
        let cluster = AgentCluster::new(oracle, sandbox, test_target())
            .with_strategy(Box::new(NeverStrategy))
            .with_config(OrchestratorConfig {
                coverage_target: 1.0,
                deadline_secs: Some(0), // immediate deadline
                stall_threshold: 100,
            });
        cluster.run().await.unwrap();
    }

    #[tokio::test]
    async fn run_stops_on_stall() {
        // Strategy always returns empty → stall_count increments → stops at threshold.
        let oracle = Arc::new(CoverageOracle::new());
        let b = apex_core::types::BranchId::new(1, 1, 0, 0);
        oracle.register_branches([b]);

        struct EmptyStrategy;
        #[async_trait::async_trait]
        impl Strategy for EmptyStrategy {
            fn name(&self) -> &str {
                "empty"
            }
            async fn suggest_inputs(
                &self,
                _ctx: &ExplorationContext,
            ) -> apex_core::error::Result<Vec<InputSeed>> {
                Ok(vec![])
            }
            async fn observe(&self, _result: &ExecutionResult) -> apex_core::error::Result<()> {
                Ok(())
            }
        }

        let sandbox: Arc<dyn Sandbox> = Arc::new(StubSandbox);
        let cluster = AgentCluster::new(oracle, sandbox, test_target())
            .with_strategy(Box::new(EmptyStrategy))
            .with_config(OrchestratorConfig {
                coverage_target: 1.0,
                deadline_secs: Some(10),
                stall_threshold: 3,
            });
        cluster.run().await.unwrap();
    }

    #[test]
    fn monitor_action_accessible() {
        let oracle = Arc::new(CoverageOracle::new());
        let sandbox: Arc<dyn Sandbox> = Arc::new(StubSandbox);
        let cluster = AgentCluster::new(oracle, sandbox, test_target());
        let action = cluster.monitor_action();
        // Fresh monitor should say Continue
        assert_eq!(action, MonitorAction::Normal);
    }

    #[test]
    fn bug_summary_empty_on_new_cluster() {
        let oracle = Arc::new(CoverageOracle::new());
        let sandbox: Arc<dyn Sandbox> = Arc::new(StubSandbox);
        let cluster = AgentCluster::new(oracle, sandbox, test_target());
        let summary = cluster.bug_summary();
        assert_eq!(summary.total, 0);
    }

    // ------------------------------------------------------------------
    // Additional branch-coverage tests
    // ------------------------------------------------------------------

    /// `monitor_action()` returns SwitchStrategy after small number of stalls.
    #[test]
    fn monitor_action_switch_strategy_after_stalls() {
        let oracle = Arc::new(CoverageOracle::new());
        let sandbox: Arc<dyn Sandbox> = Arc::new(StubSandbox);
        let cluster = AgentCluster::new(oracle, sandbox, test_target());

        // Inject stalls directly into the monitor (window_size=10 → 2*10=20 for AgentCycle).
        let mut monitor = cluster.monitor.lock().unwrap();
        monitor.record(0, 100);
        // 3 stalls → SwitchStrategy (3 < 2*10=20)
        for i in 1..=3 {
            monitor.record(i, 100);
        }
        drop(monitor);
        assert_eq!(
            cluster.monitor_action(),
            crate::monitor::MonitorAction::SwitchStrategy
        );
    }

    /// `monitor_action()` returns AgentCycle after many stalls.
    #[test]
    fn monitor_action_agent_cycle_after_many_stalls() {
        let oracle = Arc::new(CoverageOracle::new());
        let sandbox: Arc<dyn Sandbox> = Arc::new(StubSandbox);
        let cluster = AgentCluster::new(oracle, sandbox, test_target());

        let mut monitor = cluster.monitor.lock().unwrap();
        monitor.record(0, 50);
        // 20 stalls → stall_count = 20 = 2*10 → AgentCycle
        for i in 1..=20 {
            monitor.record(i, 50);
        }
        drop(monitor);
        assert_eq!(
            cluster.monitor_action(),
            crate::monitor::MonitorAction::AgentCycle
        );
    }

    /// `monitor_action()` returns Stop after very many stalls.
    #[test]
    fn monitor_action_stop_after_extreme_stalls() {
        let oracle = Arc::new(CoverageOracle::new());
        let sandbox: Arc<dyn Sandbox> = Arc::new(StubSandbox);
        let cluster = AgentCluster::new(oracle, sandbox, test_target());

        let mut monitor = cluster.monitor.lock().unwrap();
        monitor.record(0, 10);
        // 40 stalls → stall_count = 40 >= 4*10=40 → Stop
        for i in 1..=40 {
            monitor.record(i, 10);
        }
        drop(monitor);
        assert_eq!(
            cluster.monitor_action(),
            crate::monitor::MonitorAction::Stop
        );
    }

    /// `bug_summary()` reports correct total after bugs are recorded.
    #[tokio::test]
    async fn bug_summary_after_crashes_recorded() {
        let oracle = Arc::new(CoverageOracle::new());
        let b = apex_core::types::BranchId::new(99, 1, 0, 0);
        oracle.register_branches([b]);

        struct OneShotCrash {
            fired: std::sync::Mutex<bool>,
        }
        #[async_trait::async_trait]
        impl Strategy for OneShotCrash {
            fn name(&self) -> &str {
                "crash-strategy"
            }
            async fn suggest_inputs(
                &self,
                _ctx: &ExplorationContext,
            ) -> apex_core::error::Result<Vec<InputSeed>> {
                let mut f = self.fired.lock().unwrap();
                if *f {
                    Ok(Vec::new())
                } else {
                    *f = true;
                    Ok(vec![InputSeed::new(
                        b"c".to_vec(),
                        apex_core::types::SeedOrigin::Fuzzer,
                    )])
                }
            }
            async fn observe(&self, _result: &ExecutionResult) -> apex_core::error::Result<()> {
                Ok(())
            }
        }

        let cluster = AgentCluster::new(oracle, Arc::new(CrashSandbox), test_target())
            .with_strategy(Box::new(OneShotCrash {
                fired: std::sync::Mutex::new(false),
            }))
            .with_config(OrchestratorConfig {
                coverage_target: 1.0,
                deadline_secs: Some(5),
                stall_threshold: 2,
            });

        cluster.run().await.unwrap();
        let summary = cluster.bug_summary();
        assert!(summary.total > 0);
        assert!(summary.by_class.contains_key("crash"));
    }

    /// `with_strategy()` then `strategy_count()` chains correctly.
    #[test]
    fn with_strategy_chain_three_strategies() {
        struct Noop(&'static str);
        #[async_trait::async_trait]
        impl Strategy for Noop {
            fn name(&self) -> &str {
                self.0
            }
            async fn suggest_inputs(
                &self,
                _: &ExplorationContext,
            ) -> apex_core::error::Result<Vec<InputSeed>> {
                Ok(vec![])
            }
            async fn observe(&self, _: &ExecutionResult) -> apex_core::error::Result<()> {
                Ok(())
            }
        }

        let oracle = Arc::new(CoverageOracle::new());
        let cluster = AgentCluster::new(oracle, Arc::new(StubSandbox), test_target())
            .with_strategy(Box::new(Noop("a")))
            .with_strategy(Box::new(Noop("b")))
            .with_strategy(Box::new(Noop("c")));
        assert_eq!(cluster.strategy_count(), 3);
    }

    /// Target with empty test_command can be constructed.
    #[test]
    fn cluster_empty_test_command() {
        let oracle = Arc::new(CoverageOracle::new());
        let target = Target {
            root: PathBuf::from("/tmp"),
            language: Language::Rust,
            test_command: vec![],
        };
        let cluster = AgentCluster::new(oracle, Arc::new(StubSandbox), target);
        assert!(cluster.target.test_command.is_empty());
    }

    /// OrchestratorConfig::default() stall_threshold equals the module constant.
    #[test]
    fn orchestrator_config_stall_threshold_matches_constant() {
        let cfg = OrchestratorConfig::default();
        assert_eq!(cfg.stall_threshold, STALL_THRESHOLD);
    }

    /// `with_config()` preserves `None` deadline.
    #[test]
    fn with_config_none_deadline_preserved() {
        let cfg = OrchestratorConfig {
            coverage_target: 0.8,
            deadline_secs: None,
            stall_threshold: 5,
        };
        let oracle = Arc::new(CoverageOracle::new());
        let cluster =
            AgentCluster::new(oracle, Arc::new(StubSandbox), test_target()).with_config(cfg);
        assert!(cluster.config.deadline_secs.is_none());
    }

    /// `run()` with an oracle that has pre-covered branches exits via `uncovered.is_empty()`.
    #[tokio::test]
    async fn run_pre_covered_branches_exits_immediately() {
        let oracle = Arc::new(CoverageOracle::new());
        let b1 = apex_core::types::BranchId::new(5, 1, 0, 0);
        let b2 = apex_core::types::BranchId::new(5, 2, 0, 0);
        oracle.register_branches([b1.clone(), b2.clone()]);
        oracle.mark_covered(&b1, apex_core::types::SeedId::new());
        oracle.mark_covered(&b2, apex_core::types::SeedId::new());

        let cluster = AgentCluster::new(oracle.clone(), Arc::new(StubSandbox), test_target());
        cluster.run().await.unwrap();
        assert_eq!(oracle.covered_count(), 2);
    }

    /// `bug_summary()` contains the reports vec.
    #[test]
    fn bug_summary_reports_field_populated() {
        use apex_core::types::{BugClass, BugReport, SeedId};
        let oracle = Arc::new(CoverageOracle::new());
        let cluster = AgentCluster::new(oracle, Arc::new(StubSandbox), test_target());
        let report = BugReport::new(BugClass::Crash, SeedId::new(), "oops".into());
        cluster.ledger.record(report);
        let summary = cluster.bug_summary();
        assert_eq!(summary.total, 1);
        assert_eq!(summary.reports.len(), 1);
    }

    // ------------------------------------------------------------------
    // Additional branch-coverage tests: run() loop paths
    // ------------------------------------------------------------------

    /// Coverage target of 0.0 — exits immediately via the coverage_target check
    /// on the very first iteration (0/0 = 100% >= 0.0).
    #[tokio::test]
    async fn run_zero_coverage_target_exits_immediately() {
        let oracle = Arc::new(CoverageOracle::new());
        let b = apex_core::types::BranchId::new(100, 1, 0, 0);
        oracle.register_branches([b]);
        // 0% coverage but target is 0.0 → 0.0 >= 0.0 → exit
        let cluster = AgentCluster::new(oracle, Arc::new(StubSandbox), test_target()).with_config(
            OrchestratorConfig {
                coverage_target: 0.0,
                deadline_secs: None,
                stall_threshold: 100,
            },
        );
        cluster.run().await.unwrap();
    }

    /// Coverage target of 0.5 with 1 of 2 branches pre-covered → 50% >= 50% → exit.
    #[tokio::test]
    async fn run_coverage_target_boundary_exactly_met() {
        let oracle = Arc::new(CoverageOracle::new());
        let b1 = apex_core::types::BranchId::new(101, 1, 0, 0);
        let b2 = apex_core::types::BranchId::new(101, 2, 0, 0);
        oracle.register_branches([b1.clone(), b2.clone()]);
        oracle.mark_covered(&b1, apex_core::types::SeedId::new());
        // 50% coverage, target 0.5 → exit immediately
        let cluster = AgentCluster::new(oracle.clone(), Arc::new(StubSandbox), test_target())
            .with_config(OrchestratorConfig {
                coverage_target: 0.5,
                deadline_secs: None,
                stall_threshold: 100,
            });
        cluster.run().await.unwrap();
        assert_eq!(oracle.covered_count(), 1);
    }

    /// Multiple strategies: one fails, one succeeds — tests filter_map(|r| r.ok())
    /// on strategies producing a mix of Ok and Err results.
    #[tokio::test]
    async fn run_mixed_strategy_results_filtered() {
        struct OkStrategy;
        #[async_trait::async_trait]
        impl Strategy for OkStrategy {
            fn name(&self) -> &str {
                "ok"
            }
            async fn suggest_inputs(
                &self,
                _ctx: &ExplorationContext,
            ) -> apex_core::error::Result<Vec<InputSeed>> {
                Ok(vec![InputSeed::new(
                    b"ok".to_vec(),
                    apex_core::types::SeedOrigin::Fuzzer,
                )])
            }
            async fn observe(&self, _result: &ExecutionResult) -> apex_core::error::Result<()> {
                Ok(())
            }
        }

        struct ErrStrategy;
        #[async_trait::async_trait]
        impl Strategy for ErrStrategy {
            fn name(&self) -> &str {
                "err"
            }
            async fn suggest_inputs(
                &self,
                _ctx: &ExplorationContext,
            ) -> apex_core::error::Result<Vec<InputSeed>> {
                Err(apex_core::error::ApexError::Other("fail".into()))
            }
            async fn observe(&self, _result: &ExecutionResult) -> apex_core::error::Result<()> {
                Ok(())
            }
        }

        let oracle = Arc::new(CoverageOracle::new());
        let b = apex_core::types::BranchId::new(102, 1, 0, 0);
        oracle.register_branches([b.clone()]);
        let sandbox: Arc<dyn Sandbox> = Arc::new(CoveringSandbox::new(vec![b]));

        let cluster = AgentCluster::new(oracle.clone(), sandbox, test_target())
            .with_strategy(Box::new(ErrStrategy))
            .with_strategy(Box::new(OkStrategy))
            .with_config(OrchestratorConfig {
                coverage_target: 0.9,
                deadline_secs: Some(5),
                stall_threshold: 100,
            });
        cluster.run().await.unwrap();
        assert_eq!(oracle.coverage_percent(), 100.0);
    }

    /// Multiple seeds per strategy — exercises the flatten() path where
    /// a single strategy returns multiple InputSeeds.
    #[tokio::test]
    async fn run_strategy_returns_multiple_seeds() {
        struct MultiSeedStrategy;
        #[async_trait::async_trait]
        impl Strategy for MultiSeedStrategy {
            fn name(&self) -> &str {
                "multi"
            }
            async fn suggest_inputs(
                &self,
                _ctx: &ExplorationContext,
            ) -> apex_core::error::Result<Vec<InputSeed>> {
                Ok(vec![
                    InputSeed::new(b"seed1".to_vec(), apex_core::types::SeedOrigin::Fuzzer),
                    InputSeed::new(b"seed2".to_vec(), apex_core::types::SeedOrigin::Fuzzer),
                    InputSeed::new(b"seed3".to_vec(), apex_core::types::SeedOrigin::Fuzzer),
                ])
            }
            async fn observe(&self, _result: &ExecutionResult) -> apex_core::error::Result<()> {
                Ok(())
            }
        }

        let oracle = Arc::new(CoverageOracle::new());
        let b = apex_core::types::BranchId::new(103, 1, 0, 0);
        oracle.register_branches([b.clone()]);
        let sandbox: Arc<dyn Sandbox> = Arc::new(CoveringSandbox::new(vec![b]));

        let cluster = AgentCluster::new(oracle.clone(), sandbox, test_target())
            .with_strategy(Box::new(MultiSeedStrategy))
            .with_config(OrchestratorConfig {
                coverage_target: 0.9,
                deadline_secs: Some(5),
                stall_threshold: 100,
            });
        cluster.run().await.unwrap();
        assert_eq!(oracle.coverage_percent(), 100.0);
    }

    /// Sandbox returns a mix of Ok and Err results — exercises filter_map on sandbox results.
    #[tokio::test]
    async fn run_sandbox_mix_ok_and_err_results() {
        use std::sync::atomic::{AtomicUsize, Ordering};

        struct AlternatingErrorSandbox {
            call_count: AtomicUsize,
        }

        #[async_trait::async_trait]
        impl Sandbox for AlternatingErrorSandbox {
            async fn run(&self, input: &InputSeed) -> apex_core::error::Result<ExecutionResult> {
                let n = self.call_count.fetch_add(1, Ordering::SeqCst);
                if n % 2 == 0 {
                    Ok(ExecutionResult {
                        seed_id: input.id,
                        status: ExecutionStatus::Pass,
                        new_branches: Vec::new(),
                        trace: None,
                        duration_ms: 1,
                        stdout: String::new(),
                        stderr: String::new(),
                        input: None,
                    })
                } else {
                    Err(apex_core::error::ApexError::Other("sandbox err".into()))
                }
            }
            async fn snapshot(&self) -> apex_core::error::Result<SnapshotId> {
                Ok(SnapshotId::new())
            }
            async fn restore(&self, _id: SnapshotId) -> apex_core::error::Result<()> {
                Ok(())
            }
            fn language(&self) -> Language {
                Language::Python
            }
        }

        struct TwoSeedStrategy;
        #[async_trait::async_trait]
        impl Strategy for TwoSeedStrategy {
            fn name(&self) -> &str {
                "two"
            }
            async fn suggest_inputs(
                &self,
                _ctx: &ExplorationContext,
            ) -> apex_core::error::Result<Vec<InputSeed>> {
                Ok(vec![
                    InputSeed::new(b"a".to_vec(), apex_core::types::SeedOrigin::Fuzzer),
                    InputSeed::new(b"b".to_vec(), apex_core::types::SeedOrigin::Fuzzer),
                ])
            }
            async fn observe(&self, _result: &ExecutionResult) -> apex_core::error::Result<()> {
                Ok(())
            }
        }

        let oracle = Arc::new(CoverageOracle::new());
        let b = apex_core::types::BranchId::new(104, 1, 0, 0);
        oracle.register_branches([b]);

        let sandbox: Arc<dyn Sandbox> = Arc::new(AlternatingErrorSandbox {
            call_count: AtomicUsize::new(0),
        });
        let cluster = AgentCluster::new(oracle, sandbox, test_target())
            .with_strategy(Box::new(TwoSeedStrategy))
            .with_config(OrchestratorConfig {
                coverage_target: 1.0,
                deadline_secs: Some(5),
                stall_threshold: 3,
            });
        cluster.run().await.unwrap();
    }

    /// Sandbox returns results with Fail status (AssertionFailure bug class) —
    /// exercises the `BugClass::from_status` → `map_or` path with AssertionFailure.
    #[tokio::test]
    async fn run_records_assertion_failure_bug() {
        struct FailSandbox;
        #[async_trait::async_trait]
        impl Sandbox for FailSandbox {
            async fn run(&self, input: &InputSeed) -> apex_core::error::Result<ExecutionResult> {
                Ok(ExecutionResult {
                    seed_id: input.id,
                    status: ExecutionStatus::Fail,
                    new_branches: Vec::new(),
                    trace: None,
                    duration_ms: 10,
                    stdout: String::new(),
                    stderr: "assertion failed at src/lib.rs:42".into(),
                    input: None,
                })
            }
            async fn snapshot(&self) -> apex_core::error::Result<SnapshotId> {
                Ok(SnapshotId::new())
            }
            async fn restore(&self, _id: SnapshotId) -> apex_core::error::Result<()> {
                Ok(())
            }
            fn language(&self) -> Language {
                Language::Python
            }
        }

        struct OnceSeeder {
            done: std::sync::Mutex<bool>,
        }
        #[async_trait::async_trait]
        impl Strategy for OnceSeeder {
            fn name(&self) -> &str {
                "once-fail"
            }
            async fn suggest_inputs(
                &self,
                _ctx: &ExplorationContext,
            ) -> apex_core::error::Result<Vec<InputSeed>> {
                let mut d = self.done.lock().unwrap();
                if *d {
                    Ok(Vec::new())
                } else {
                    *d = true;
                    Ok(vec![InputSeed::new(
                        b"fail".to_vec(),
                        apex_core::types::SeedOrigin::Fuzzer,
                    )])
                }
            }
            async fn observe(&self, _result: &ExecutionResult) -> apex_core::error::Result<()> {
                Ok(())
            }
        }

        let oracle = Arc::new(CoverageOracle::new());
        let b = apex_core::types::BranchId::new(105, 1, 0, 0);
        oracle.register_branches([b]);

        let cluster = AgentCluster::new(oracle, Arc::new(FailSandbox), test_target())
            .with_strategy(Box::new(OnceSeeder {
                done: std::sync::Mutex::new(false),
            }))
            .with_config(OrchestratorConfig {
                coverage_target: 1.0,
                deadline_secs: Some(5),
                stall_threshold: 2,
            });
        cluster.run().await.unwrap();
        let summary = cluster.bug_summary();
        assert!(summary.total > 0);
        assert!(summary.by_class.contains_key("assertion_failure"));
    }

    /// Sandbox returns OomKill status — exercises the OomKill bug class branch.
    #[tokio::test]
    async fn run_records_oom_bug() {
        struct OomSandbox;
        #[async_trait::async_trait]
        impl Sandbox for OomSandbox {
            async fn run(&self, input: &InputSeed) -> apex_core::error::Result<ExecutionResult> {
                Ok(ExecutionResult {
                    seed_id: input.id,
                    status: ExecutionStatus::OomKill,
                    new_branches: Vec::new(),
                    trace: None,
                    duration_ms: 10,
                    stdout: String::new(),
                    stderr: "killed by oom at src/alloc.rs:99".into(),
                    input: None,
                })
            }
            async fn snapshot(&self) -> apex_core::error::Result<SnapshotId> {
                Ok(SnapshotId::new())
            }
            async fn restore(&self, _id: SnapshotId) -> apex_core::error::Result<()> {
                Ok(())
            }
            fn language(&self) -> Language {
                Language::Python
            }
        }

        struct OnceOomSeeder {
            done: std::sync::Mutex<bool>,
        }
        #[async_trait::async_trait]
        impl Strategy for OnceOomSeeder {
            fn name(&self) -> &str {
                "once-oom"
            }
            async fn suggest_inputs(
                &self,
                _ctx: &ExplorationContext,
            ) -> apex_core::error::Result<Vec<InputSeed>> {
                let mut d = self.done.lock().unwrap();
                if *d {
                    Ok(Vec::new())
                } else {
                    *d = true;
                    Ok(vec![InputSeed::new(
                        b"oom".to_vec(),
                        apex_core::types::SeedOrigin::Fuzzer,
                    )])
                }
            }
            async fn observe(&self, _result: &ExecutionResult) -> apex_core::error::Result<()> {
                Ok(())
            }
        }

        let oracle = Arc::new(CoverageOracle::new());
        let b = apex_core::types::BranchId::new(106, 1, 0, 0);
        oracle.register_branches([b]);

        let cluster = AgentCluster::new(oracle, Arc::new(OomSandbox), test_target())
            .with_strategy(Box::new(OnceOomSeeder {
                done: std::sync::Mutex::new(false),
            }))
            .with_config(OrchestratorConfig {
                coverage_target: 1.0,
                deadline_secs: Some(5),
                stall_threshold: 2,
            });
        cluster.run().await.unwrap();
        let summary = cluster.bug_summary();
        assert!(summary.total > 0);
        assert!(summary.by_class.contains_key("oom_kill"));
    }

    /// Sandbox returns Pass status — no bug recorded, exercises the
    /// `record_from_result` returning false branch.
    #[tokio::test]
    async fn run_pass_status_no_bug_recorded() {
        struct PassOnlySeeder;
        #[async_trait::async_trait]
        impl Strategy for PassOnlySeeder {
            fn name(&self) -> &str {
                "pass-only"
            }
            async fn suggest_inputs(
                &self,
                _ctx: &ExplorationContext,
            ) -> apex_core::error::Result<Vec<InputSeed>> {
                Ok(vec![InputSeed::new(
                    b"pass".to_vec(),
                    apex_core::types::SeedOrigin::Fuzzer,
                )])
            }
            async fn observe(&self, _result: &ExecutionResult) -> apex_core::error::Result<()> {
                Ok(())
            }
        }

        let oracle = Arc::new(CoverageOracle::new());
        let b = apex_core::types::BranchId::new(107, 1, 0, 0);
        oracle.register_branches([b]);

        // StubSandbox returns Pass → no bug
        let cluster = AgentCluster::new(oracle, Arc::new(StubSandbox), test_target())
            .with_strategy(Box::new(PassOnlySeeder))
            .with_config(OrchestratorConfig {
                coverage_target: 1.0,
                deadline_secs: None,
                stall_threshold: 2,
            });
        cluster.run().await.unwrap();
        assert_eq!(cluster.bug_summary().total, 0);
    }

    /// Run with tracing subscriber installed so log branches are actually exercised.
    #[tokio::test]
    async fn run_with_tracing_exercises_info_branches() {
        let subscriber = tracing_subscriber::fmt()
            .with_max_level(tracing::Level::TRACE)
            .with_test_writer()
            .finish();
        let _guard = tracing::subscriber::set_default(subscriber);

        let oracle = Arc::new(CoverageOracle::new());
        let b1 = apex_core::types::BranchId::new(108, 1, 0, 0);
        let b2 = apex_core::types::BranchId::new(108, 2, 0, 0);
        oracle.register_branches([b1.clone(), b2.clone()]);

        let sandbox: Arc<dyn Sandbox> = Arc::new(CoveringSandbox::new(vec![b1, b2]));

        struct ConstSeeder;
        #[async_trait::async_trait]
        impl Strategy for ConstSeeder {
            fn name(&self) -> &str {
                "const-trace"
            }
            async fn suggest_inputs(
                &self,
                _ctx: &ExplorationContext,
            ) -> apex_core::error::Result<Vec<InputSeed>> {
                Ok(vec![InputSeed::new(
                    b"t".to_vec(),
                    apex_core::types::SeedOrigin::Fuzzer,
                )])
            }
            async fn observe(&self, _result: &ExecutionResult) -> apex_core::error::Result<()> {
                Ok(())
            }
        }

        let cluster = AgentCluster::new(oracle.clone(), sandbox, test_target())
            .with_strategy(Box::new(ConstSeeder))
            .with_config(OrchestratorConfig {
                coverage_target: 0.9,
                deadline_secs: Some(5),
                stall_threshold: 100,
            });
        cluster.run().await.unwrap();
        assert_eq!(oracle.coverage_percent(), 100.0);
    }

    /// Run with tracing: exercises the "coverage target reached" info! branch.
    #[tokio::test]
    async fn run_with_tracing_coverage_target_reached_log() {
        let subscriber = tracing_subscriber::fmt()
            .with_max_level(tracing::Level::TRACE)
            .with_test_writer()
            .finish();
        let _guard = tracing::subscriber::set_default(subscriber);

        let oracle = Arc::new(CoverageOracle::new());
        // No branches → 100% coverage → "coverage target reached" log
        let cluster = AgentCluster::new(oracle, Arc::new(StubSandbox), test_target());
        cluster.run().await.unwrap();
    }

    /// Run with tracing: exercises the "all branches covered" info! branch.
    #[tokio::test]
    async fn run_with_tracing_all_branches_covered_log() {
        let subscriber = tracing_subscriber::fmt()
            .with_max_level(tracing::Level::TRACE)
            .with_test_writer()
            .finish();
        let _guard = tracing::subscriber::set_default(subscriber);

        let oracle = Arc::new(CoverageOracle::new());
        let b = apex_core::types::BranchId::new(109, 1, 0, 0);
        oracle.register_branches([b.clone()]);
        oracle.mark_covered(&b, apex_core::types::SeedId::new());
        // 100% coverage but coverage_target is 1.0 → passes first check
        // uncovered.is_empty() → "all branches covered"
        let cluster = AgentCluster::new(oracle, Arc::new(StubSandbox), test_target()).with_config(
            OrchestratorConfig {
                coverage_target: 1.1, // above 100% to skip coverage_target check
                deadline_secs: None,
                stall_threshold: 100,
            },
        );
        cluster.run().await.unwrap();
    }

    /// Run with tracing: exercises the "deadline reached" warn! branch.
    #[tokio::test]
    async fn run_with_tracing_deadline_reached_log() {
        let subscriber = tracing_subscriber::fmt()
            .with_max_level(tracing::Level::TRACE)
            .with_test_writer()
            .finish();
        let _guard = tracing::subscriber::set_default(subscriber);

        let oracle = Arc::new(CoverageOracle::new());
        let b = apex_core::types::BranchId::new(110, 1, 0, 0);
        oracle.register_branches([b]);

        let cluster = AgentCluster::new(oracle, Arc::new(StubSandbox), test_target()).with_config(
            OrchestratorConfig {
                coverage_target: 1.0,
                deadline_secs: Some(0),
                stall_threshold: 100,
            },
        );
        cluster.run().await.unwrap();
    }

    /// Run with tracing: exercises the "coverage stalled" warn! branch.
    #[tokio::test]
    async fn run_with_tracing_stall_log() {
        let subscriber = tracing_subscriber::fmt()
            .with_max_level(tracing::Level::TRACE)
            .with_test_writer()
            .finish();
        let _guard = tracing::subscriber::set_default(subscriber);

        let oracle = Arc::new(CoverageOracle::new());
        let b = apex_core::types::BranchId::new(111, 1, 0, 0);
        oracle.register_branches([b]);

        let cluster = AgentCluster::new(oracle, Arc::new(StubSandbox), test_target()).with_config(
            OrchestratorConfig {
                coverage_target: 1.0,
                deadline_secs: None,
                stall_threshold: 1,
            },
        );
        cluster.run().await.unwrap();
    }

    /// Run with tracing: exercises the "new coverage" info! branch.
    #[tokio::test]
    async fn run_with_tracing_new_coverage_log() {
        let subscriber = tracing_subscriber::fmt()
            .with_max_level(tracing::Level::TRACE)
            .with_test_writer()
            .finish();
        let _guard = tracing::subscriber::set_default(subscriber);

        let oracle = Arc::new(CoverageOracle::new());
        let b = apex_core::types::BranchId::new(112, 1, 0, 0);
        oracle.register_branches([b.clone()]);

        let sandbox: Arc<dyn Sandbox> = Arc::new(CoveringSandbox::new(vec![b]));

        struct LogSeeder;
        #[async_trait::async_trait]
        impl Strategy for LogSeeder {
            fn name(&self) -> &str {
                "log-seeder"
            }
            async fn suggest_inputs(
                &self,
                _ctx: &ExplorationContext,
            ) -> apex_core::error::Result<Vec<InputSeed>> {
                Ok(vec![InputSeed::new(
                    b"log".to_vec(),
                    apex_core::types::SeedOrigin::Fuzzer,
                )])
            }
            async fn observe(&self, _result: &ExecutionResult) -> apex_core::error::Result<()> {
                Ok(())
            }
        }

        let cluster = AgentCluster::new(oracle.clone(), sandbox, test_target())
            .with_strategy(Box::new(LogSeeder))
            .with_config(OrchestratorConfig {
                coverage_target: 0.9,
                deadline_secs: Some(5),
                stall_threshold: 100,
            });
        cluster.run().await.unwrap();
    }

    /// Run with tracing: exercises the "bug found" info! branch with a Crash.
    #[tokio::test]
    async fn run_with_tracing_bug_found_crash_log() {
        let subscriber = tracing_subscriber::fmt()
            .with_max_level(tracing::Level::TRACE)
            .with_test_writer()
            .finish();
        let _guard = tracing::subscriber::set_default(subscriber);

        struct CrashOnceSeeder {
            done: std::sync::Mutex<bool>,
        }
        #[async_trait::async_trait]
        impl Strategy for CrashOnceSeeder {
            fn name(&self) -> &str {
                "crash-once-log"
            }
            async fn suggest_inputs(
                &self,
                _ctx: &ExplorationContext,
            ) -> apex_core::error::Result<Vec<InputSeed>> {
                let mut d = self.done.lock().unwrap();
                if *d {
                    Ok(Vec::new())
                } else {
                    *d = true;
                    Ok(vec![InputSeed::new(
                        b"crash".to_vec(),
                        apex_core::types::SeedOrigin::Fuzzer,
                    )])
                }
            }
            async fn observe(&self, _result: &ExecutionResult) -> apex_core::error::Result<()> {
                Ok(())
            }
        }

        let oracle = Arc::new(CoverageOracle::new());
        let b = apex_core::types::BranchId::new(113, 1, 0, 0);
        oracle.register_branches([b]);

        let cluster = AgentCluster::new(oracle, Arc::new(CrashSandbox), test_target())
            .with_strategy(Box::new(CrashOnceSeeder {
                done: std::sync::Mutex::new(false),
            }))
            .with_config(OrchestratorConfig {
                coverage_target: 1.0,
                deadline_secs: Some(5),
                stall_threshold: 2,
            });
        cluster.run().await.unwrap();
    }

    /// Run with tracing: exercises the "bug found" info! with Timeout class.
    #[tokio::test]
    async fn run_with_tracing_bug_found_timeout_log() {
        let subscriber = tracing_subscriber::fmt()
            .with_max_level(tracing::Level::TRACE)
            .with_test_writer()
            .finish();
        let _guard = tracing::subscriber::set_default(subscriber);

        struct TimeoutSandboxLog;
        #[async_trait::async_trait]
        impl Sandbox for TimeoutSandboxLog {
            async fn run(&self, input: &InputSeed) -> apex_core::error::Result<ExecutionResult> {
                Ok(ExecutionResult {
                    seed_id: input.id,
                    status: ExecutionStatus::Timeout,
                    new_branches: Vec::new(),
                    trace: None,
                    duration_ms: 5000,
                    stdout: String::new(),
                    stderr: String::new(),
                    input: None,
                })
            }
            async fn snapshot(&self) -> apex_core::error::Result<SnapshotId> {
                Ok(SnapshotId::new())
            }
            async fn restore(&self, _id: SnapshotId) -> apex_core::error::Result<()> {
                Ok(())
            }
            fn language(&self) -> Language {
                Language::Python
            }
        }

        struct OnceTimeoutSeeder {
            done: std::sync::Mutex<bool>,
        }
        #[async_trait::async_trait]
        impl Strategy for OnceTimeoutSeeder {
            fn name(&self) -> &str {
                "once-timeout-log"
            }
            async fn suggest_inputs(
                &self,
                _ctx: &ExplorationContext,
            ) -> apex_core::error::Result<Vec<InputSeed>> {
                let mut d = self.done.lock().unwrap();
                if *d {
                    Ok(Vec::new())
                } else {
                    *d = true;
                    Ok(vec![InputSeed::new(
                        b"to".to_vec(),
                        apex_core::types::SeedOrigin::Fuzzer,
                    )])
                }
            }
            async fn observe(&self, _result: &ExecutionResult) -> apex_core::error::Result<()> {
                Ok(())
            }
        }

        let oracle = Arc::new(CoverageOracle::new());
        let b = apex_core::types::BranchId::new(114, 1, 0, 0);
        oracle.register_branches([b]);

        let cluster = AgentCluster::new(oracle, Arc::new(TimeoutSandboxLog), test_target())
            .with_strategy(Box::new(OnceTimeoutSeeder {
                done: std::sync::Mutex::new(false),
            }))
            .with_config(OrchestratorConfig {
                coverage_target: 1.0,
                deadline_secs: Some(5),
                stall_threshold: 2,
            });
        cluster.run().await.unwrap();
    }

    /// Run with tracing: exercises "exploration complete" info! log with non-zero
    /// iteration count, coverage, and bug count.
    #[tokio::test]
    async fn run_with_tracing_exploration_complete_with_bugs() {
        let subscriber = tracing_subscriber::fmt()
            .with_max_level(tracing::Level::TRACE)
            .with_test_writer()
            .finish();
        let _guard = tracing::subscriber::set_default(subscriber);

        struct CrashOnce2 {
            done: std::sync::Mutex<bool>,
        }
        #[async_trait::async_trait]
        impl Strategy for CrashOnce2 {
            fn name(&self) -> &str {
                "crash-once2"
            }
            async fn suggest_inputs(
                &self,
                _ctx: &ExplorationContext,
            ) -> apex_core::error::Result<Vec<InputSeed>> {
                let mut d = self.done.lock().unwrap();
                if *d {
                    Ok(Vec::new())
                } else {
                    *d = true;
                    Ok(vec![InputSeed::new(
                        b"c".to_vec(),
                        apex_core::types::SeedOrigin::Fuzzer,
                    )])
                }
            }
            async fn observe(&self, _result: &ExecutionResult) -> apex_core::error::Result<()> {
                Ok(())
            }
        }

        let oracle = Arc::new(CoverageOracle::new());
        let b = apex_core::types::BranchId::new(115, 1, 0, 0);
        oracle.register_branches([b]);

        let cluster = AgentCluster::new(oracle, Arc::new(CrashSandbox), test_target())
            .with_strategy(Box::new(CrashOnce2 {
                done: std::sync::Mutex::new(false),
            }))
            .with_config(OrchestratorConfig {
                coverage_target: 1.0,
                deadline_secs: Some(5),
                stall_threshold: 2,
            });
        cluster.run().await.unwrap();
        assert!(cluster.bug_summary().total > 0);
    }

    /// Stall counter transitions: coverage found (reset to 0) then stalls again.
    /// Exercises the `stall_count = if new_coverage { 0 } else { stall_count + 1 }`
    /// branch in both directions within the same run.
    #[tokio::test]
    async fn run_stall_count_reset_then_increment_again() {
        use std::sync::atomic::{AtomicUsize, Ordering};

        /// Strategy that produces seeds on every call.
        struct AlwaysSeed;
        #[async_trait::async_trait]
        impl Strategy for AlwaysSeed {
            fn name(&self) -> &str {
                "always-seed"
            }
            async fn suggest_inputs(
                &self,
                _ctx: &ExplorationContext,
            ) -> apex_core::error::Result<Vec<InputSeed>> {
                Ok(vec![InputSeed::new(
                    b"s".to_vec(),
                    apex_core::types::SeedOrigin::Fuzzer,
                )])
            }
            async fn observe(&self, _result: &ExecutionResult) -> apex_core::error::Result<()> {
                Ok(())
            }
        }

        /// Sandbox that covers b1 on 1st call and b2 on 4th call, nothing otherwise.
        struct StagedCoveringSandbox {
            call_count: AtomicUsize,
            b1: apex_core::types::BranchId,
            b2: apex_core::types::BranchId,
        }
        #[async_trait::async_trait]
        impl Sandbox for StagedCoveringSandbox {
            async fn run(&self, input: &InputSeed) -> apex_core::error::Result<ExecutionResult> {
                let n = self.call_count.fetch_add(1, Ordering::SeqCst);
                let new_branches = match n {
                    0 => vec![self.b1.clone()],
                    3 => vec![self.b2.clone()],
                    _ => vec![],
                };
                Ok(ExecutionResult {
                    seed_id: input.id,
                    status: ExecutionStatus::Pass,
                    new_branches,
                    trace: None,
                    duration_ms: 1,
                    stdout: String::new(),
                    stderr: String::new(),
                    input: None,
                })
            }
            async fn snapshot(&self) -> apex_core::error::Result<SnapshotId> {
                Ok(SnapshotId::new())
            }
            async fn restore(&self, _id: SnapshotId) -> apex_core::error::Result<()> {
                Ok(())
            }
            fn language(&self) -> Language {
                Language::Python
            }
        }

        let oracle = Arc::new(CoverageOracle::new());
        let b1 = apex_core::types::BranchId::new(116, 1, 0, 0);
        let b2 = apex_core::types::BranchId::new(116, 2, 0, 0);
        oracle.register_branches([b1.clone(), b2.clone()]);

        let sandbox: Arc<dyn Sandbox> = Arc::new(StagedCoveringSandbox {
            call_count: AtomicUsize::new(0),
            b1,
            b2,
        });

        let cluster = AgentCluster::new(oracle.clone(), sandbox, test_target())
            .with_strategy(Box::new(AlwaysSeed))
            .with_config(OrchestratorConfig {
                coverage_target: 0.9,
                deadline_secs: Some(10),
                stall_threshold: 100,
            });
        cluster.run().await.unwrap();
        assert_eq!(oracle.covered_count(), 2);
    }

    /// Sandbox returns a result with new_branches covering a branch — exercises
    /// the `!delta.newly_covered.is_empty()` → true branch within the for loop,
    /// while another result in the same iteration has no new branches (false branch).
    #[tokio::test]
    async fn run_mixed_coverage_in_single_iteration() {
        use std::sync::atomic::{AtomicUsize, Ordering};

        struct TwoSeedStrat;
        #[async_trait::async_trait]
        impl Strategy for TwoSeedStrat {
            fn name(&self) -> &str {
                "two-seed-mix"
            }
            async fn suggest_inputs(
                &self,
                _ctx: &ExplorationContext,
            ) -> apex_core::error::Result<Vec<InputSeed>> {
                Ok(vec![
                    InputSeed::new(b"a".to_vec(), apex_core::types::SeedOrigin::Fuzzer),
                    InputSeed::new(b"b".to_vec(), apex_core::types::SeedOrigin::Fuzzer),
                ])
            }
            async fn observe(&self, _result: &ExecutionResult) -> apex_core::error::Result<()> {
                Ok(())
            }
        }

        /// First call returns branches, second returns nothing.
        struct FirstOnlySandbox {
            call_count: AtomicUsize,
            branch: apex_core::types::BranchId,
        }
        #[async_trait::async_trait]
        impl Sandbox for FirstOnlySandbox {
            async fn run(&self, input: &InputSeed) -> apex_core::error::Result<ExecutionResult> {
                let n = self.call_count.fetch_add(1, Ordering::SeqCst);
                let new_branches = if n == 0 {
                    vec![self.branch.clone()]
                } else {
                    vec![]
                };
                Ok(ExecutionResult {
                    seed_id: input.id,
                    status: ExecutionStatus::Pass,
                    new_branches,
                    trace: None,
                    duration_ms: 1,
                    stdout: String::new(),
                    stderr: String::new(),
                    input: None,
                })
            }
            async fn snapshot(&self) -> apex_core::error::Result<SnapshotId> {
                Ok(SnapshotId::new())
            }
            async fn restore(&self, _id: SnapshotId) -> apex_core::error::Result<()> {
                Ok(())
            }
            fn language(&self) -> Language {
                Language::Python
            }
        }

        let oracle = Arc::new(CoverageOracle::new());
        let b = apex_core::types::BranchId::new(117, 1, 0, 0);
        oracle.register_branches([b.clone()]);

        let sandbox: Arc<dyn Sandbox> = Arc::new(FirstOnlySandbox {
            call_count: AtomicUsize::new(0),
            branch: b,
        });

        let cluster = AgentCluster::new(oracle.clone(), sandbox, test_target())
            .with_strategy(Box::new(TwoSeedStrat))
            .with_config(OrchestratorConfig {
                coverage_target: 0.9,
                deadline_secs: Some(5),
                stall_threshold: 100,
            });
        cluster.run().await.unwrap();
        assert_eq!(oracle.covered_count(), 1);
    }

    /// Multiple observe calls per iteration — multiple strategies each get
    /// observe() called for each result.
    #[tokio::test]
    async fn run_multiple_strategies_each_observe() {
        use std::sync::atomic::{AtomicUsize, Ordering};

        struct CountObserver {
            name: &'static str,
            count: Arc<AtomicUsize>,
        }
        #[async_trait::async_trait]
        impl Strategy for CountObserver {
            fn name(&self) -> &str {
                self.name
            }
            async fn suggest_inputs(
                &self,
                _ctx: &ExplorationContext,
            ) -> apex_core::error::Result<Vec<InputSeed>> {
                Ok(vec![InputSeed::new(
                    b"m".to_vec(),
                    apex_core::types::SeedOrigin::Fuzzer,
                )])
            }
            async fn observe(&self, _result: &ExecutionResult) -> apex_core::error::Result<()> {
                self.count.fetch_add(1, Ordering::SeqCst);
                Ok(())
            }
        }

        let c1 = Arc::new(AtomicUsize::new(0));
        let c2 = Arc::new(AtomicUsize::new(0));

        let oracle = Arc::new(CoverageOracle::new());
        let b = apex_core::types::BranchId::new(118, 1, 0, 0);
        oracle.register_branches([b.clone()]);
        let sandbox: Arc<dyn Sandbox> = Arc::new(CoveringSandbox::new(vec![b]));

        let cluster = AgentCluster::new(oracle.clone(), sandbox, test_target())
            .with_strategy(Box::new(CountObserver {
                name: "obs1",
                count: c1.clone(),
            }))
            .with_strategy(Box::new(CountObserver {
                name: "obs2",
                count: c2.clone(),
            }))
            .with_config(OrchestratorConfig {
                coverage_target: 0.9,
                deadline_secs: Some(5),
                stall_threshold: 100,
            });
        cluster.run().await.unwrap();
        // Both strategies should have been called at least once.
        // With 2 strategies, each suggests a seed → 2 seeds run → 2 results.
        // Each result triggers observe() on both strategies → at least 2 calls each.
        assert!(c1.load(Ordering::SeqCst) > 0);
        assert!(c2.load(Ordering::SeqCst) > 0);
    }

    /// Crash sandbox with new_branches — exercises both the `newly_covered` and
    /// `record_from_result` branches in the same result.
    #[tokio::test]
    async fn run_crash_with_new_coverage_both_branches() {
        struct CrashWithCoverageSandbox {
            branches: std::sync::Mutex<Vec<apex_core::types::BranchId>>,
        }
        #[async_trait::async_trait]
        impl Sandbox for CrashWithCoverageSandbox {
            async fn run(&self, input: &InputSeed) -> apex_core::error::Result<ExecutionResult> {
                let new_branches = {
                    let mut guard = self.branches.lock().unwrap();
                    std::mem::take(&mut *guard)
                };
                Ok(ExecutionResult {
                    seed_id: input.id,
                    status: ExecutionStatus::Crash,
                    new_branches,
                    trace: None,
                    duration_ms: 1,
                    stdout: String::new(),
                    stderr: "crash at src/x.rs:1".into(),
                    input: None,
                })
            }
            async fn snapshot(&self) -> apex_core::error::Result<SnapshotId> {
                Ok(SnapshotId::new())
            }
            async fn restore(&self, _id: SnapshotId) -> apex_core::error::Result<()> {
                Ok(())
            }
            fn language(&self) -> Language {
                Language::Python
            }
        }

        struct OneShotSeed3 {
            done: std::sync::Mutex<bool>,
        }
        #[async_trait::async_trait]
        impl Strategy for OneShotSeed3 {
            fn name(&self) -> &str {
                "one-shot3"
            }
            async fn suggest_inputs(
                &self,
                _ctx: &ExplorationContext,
            ) -> apex_core::error::Result<Vec<InputSeed>> {
                let mut d = self.done.lock().unwrap();
                if *d {
                    Ok(Vec::new())
                } else {
                    *d = true;
                    Ok(vec![InputSeed::new(
                        b"cc".to_vec(),
                        apex_core::types::SeedOrigin::Fuzzer,
                    )])
                }
            }
            async fn observe(&self, _result: &ExecutionResult) -> apex_core::error::Result<()> {
                Ok(())
            }
        }

        let oracle = Arc::new(CoverageOracle::new());
        let b = apex_core::types::BranchId::new(119, 1, 0, 0);
        oracle.register_branches([b.clone()]);

        let sandbox: Arc<dyn Sandbox> = Arc::new(CrashWithCoverageSandbox {
            branches: std::sync::Mutex::new(vec![b]),
        });

        let cluster = AgentCluster::new(oracle.clone(), sandbox, test_target())
            .with_strategy(Box::new(OneShotSeed3 {
                done: std::sync::Mutex::new(false),
            }))
            .with_config(OrchestratorConfig {
                coverage_target: 0.9,
                deadline_secs: Some(5),
                stall_threshold: 3,
            });
        cluster.run().await.unwrap();
        assert_eq!(oracle.covered_count(), 1);
        assert!(cluster.bug_summary().total > 0);
    }

    /// Stall threshold of 1 with no strategies — immediately stalls and exits.
    #[tokio::test]
    async fn run_stall_threshold_one_no_strategies() {
        let oracle = Arc::new(CoverageOracle::new());
        let b = apex_core::types::BranchId::new(120, 1, 0, 0);
        oracle.register_branches([b]);
        let cluster = AgentCluster::new(oracle, Arc::new(StubSandbox), test_target()).with_config(
            OrchestratorConfig {
                coverage_target: 1.0,
                deadline_secs: None,
                stall_threshold: 1,
            },
        );
        cluster.run().await.unwrap();
    }

    /// Many branches, partial coverage target — exercises iteration > 0.
    #[tokio::test]
    async fn run_multiple_iterations_before_target() {
        use std::sync::atomic::{AtomicUsize, Ordering};

        /// Sandbox that covers one branch per call.
        struct OnePerCallSandbox {
            branches: Vec<apex_core::types::BranchId>,
            idx: AtomicUsize,
        }
        #[async_trait::async_trait]
        impl Sandbox for OnePerCallSandbox {
            async fn run(&self, input: &InputSeed) -> apex_core::error::Result<ExecutionResult> {
                let i = self.idx.fetch_add(1, Ordering::SeqCst);
                let new_branches = if i < self.branches.len() {
                    vec![self.branches[i].clone()]
                } else {
                    vec![]
                };
                Ok(ExecutionResult {
                    seed_id: input.id,
                    status: ExecutionStatus::Pass,
                    new_branches,
                    trace: None,
                    duration_ms: 1,
                    stdout: String::new(),
                    stderr: String::new(),
                    input: None,
                })
            }
            async fn snapshot(&self) -> apex_core::error::Result<SnapshotId> {
                Ok(SnapshotId::new())
            }
            async fn restore(&self, _id: SnapshotId) -> apex_core::error::Result<()> {
                Ok(())
            }
            fn language(&self) -> Language {
                Language::Python
            }
        }

        struct AlwaysOneSeed;
        #[async_trait::async_trait]
        impl Strategy for AlwaysOneSeed {
            fn name(&self) -> &str {
                "always-one"
            }
            async fn suggest_inputs(
                &self,
                _ctx: &ExplorationContext,
            ) -> apex_core::error::Result<Vec<InputSeed>> {
                Ok(vec![InputSeed::new(
                    b"i".to_vec(),
                    apex_core::types::SeedOrigin::Fuzzer,
                )])
            }
            async fn observe(&self, _result: &ExecutionResult) -> apex_core::error::Result<()> {
                Ok(())
            }
        }

        let oracle = Arc::new(CoverageOracle::new());
        let branches: Vec<_> = (0..5)
            .map(|i| apex_core::types::BranchId::new(121, i, 0, 0))
            .collect();
        oracle.register_branches(branches.clone());

        let sandbox: Arc<dyn Sandbox> = Arc::new(OnePerCallSandbox {
            branches: branches.clone(),
            idx: AtomicUsize::new(0),
        });

        let cluster = AgentCluster::new(oracle.clone(), sandbox, test_target())
            .with_strategy(Box::new(AlwaysOneSeed))
            .with_config(OrchestratorConfig {
                coverage_target: 0.6, // 3 of 5 = 60%
                deadline_secs: Some(10),
                stall_threshold: 100,
            });
        cluster.run().await.unwrap();
        assert!(oracle.covered_count() >= 3);
    }

    /// Run with all strategies returning Err — all filtered out → suggestions empty
    /// → stall path.
    #[tokio::test]
    async fn run_all_strategies_fail_stalls() {
        struct AlwaysErr;
        #[async_trait::async_trait]
        impl Strategy for AlwaysErr {
            fn name(&self) -> &str {
                "always-err"
            }
            async fn suggest_inputs(
                &self,
                _ctx: &ExplorationContext,
            ) -> apex_core::error::Result<Vec<InputSeed>> {
                Err(apex_core::error::ApexError::Other("boom".into()))
            }
            async fn observe(&self, _result: &ExecutionResult) -> apex_core::error::Result<()> {
                Ok(())
            }
        }

        let oracle = Arc::new(CoverageOracle::new());
        let b = apex_core::types::BranchId::new(122, 1, 0, 0);
        oracle.register_branches([b]);

        let cluster = AgentCluster::new(oracle, Arc::new(StubSandbox), test_target())
            .with_strategy(Box::new(AlwaysErr))
            .with_strategy(Box::new(AlwaysErr))
            .with_config(OrchestratorConfig {
                coverage_target: 1.0,
                deadline_secs: None,
                stall_threshold: 2,
            });
        cluster.run().await.unwrap();
    }

    /// All strategies return Ok(Vec::new()) — empty seeds → stall path.
    #[tokio::test]
    async fn run_all_strategies_empty_seeds_stalls() {
        struct EmptyOk;
        #[async_trait::async_trait]
        impl Strategy for EmptyOk {
            fn name(&self) -> &str {
                "empty-ok"
            }
            async fn suggest_inputs(
                &self,
                _ctx: &ExplorationContext,
            ) -> apex_core::error::Result<Vec<InputSeed>> {
                Ok(Vec::new())
            }
            async fn observe(&self, _result: &ExecutionResult) -> apex_core::error::Result<()> {
                Ok(())
            }
        }

        let oracle = Arc::new(CoverageOracle::new());
        let b = apex_core::types::BranchId::new(123, 1, 0, 0);
        oracle.register_branches([b]);

        let cluster = AgentCluster::new(oracle, Arc::new(StubSandbox), test_target())
            .with_strategy(Box::new(EmptyOk))
            .with_config(OrchestratorConfig {
                coverage_target: 1.0,
                deadline_secs: None,
                stall_threshold: 2,
            });
        cluster.run().await.unwrap();
    }

    /// Exercises iteration incrementing: with strategy that checks iteration.
    #[tokio::test]
    async fn run_iteration_increments_across_loops() {
        use std::sync::atomic::{AtomicU64, Ordering};

        struct IterTracker {
            max_iteration: Arc<AtomicU64>,
        }
        #[async_trait::async_trait]
        impl Strategy for IterTracker {
            fn name(&self) -> &str {
                "iter-tracker"
            }
            async fn suggest_inputs(
                &self,
                ctx: &ExplorationContext,
            ) -> apex_core::error::Result<Vec<InputSeed>> {
                self.max_iteration
                    .fetch_max(ctx.iteration, Ordering::SeqCst);
                Ok(vec![InputSeed::new(
                    b"it".to_vec(),
                    apex_core::types::SeedOrigin::Fuzzer,
                )])
            }
            async fn observe(&self, _result: &ExecutionResult) -> apex_core::error::Result<()> {
                Ok(())
            }
        }

        let max_iter = Arc::new(AtomicU64::new(0));
        let oracle = Arc::new(CoverageOracle::new());
        let b = apex_core::types::BranchId::new(124, 1, 0, 0);
        oracle.register_branches([b]);

        let cluster = AgentCluster::new(oracle, Arc::new(StubSandbox), test_target())
            .with_strategy(Box::new(IterTracker {
                max_iteration: max_iter.clone(),
            }))
            .with_config(OrchestratorConfig {
                coverage_target: 1.0,
                deadline_secs: None,
                stall_threshold: 5,
            });
        cluster.run().await.unwrap();
        // With stall_threshold=5, sandbox produces no coverage, so stall increments
        // each iteration. We should see iteration go up to at least 4.
        assert!(max_iter.load(Ordering::SeqCst) >= 4);
    }

    /// Exercises the `for result in &results` loop with empty results vec
    /// (all sandbox calls fail → results is empty → for loop body never entered,
    /// new_coverage stays false → stall_count increments).
    #[tokio::test]
    async fn run_all_sandbox_results_fail_empty_results_vec() {
        struct SeedProvider;
        #[async_trait::async_trait]
        impl Strategy for SeedProvider {
            fn name(&self) -> &str {
                "seed-provider"
            }
            async fn suggest_inputs(
                &self,
                _ctx: &ExplorationContext,
            ) -> apex_core::error::Result<Vec<InputSeed>> {
                Ok(vec![
                    InputSeed::new(b"x".to_vec(), apex_core::types::SeedOrigin::Fuzzer),
                    InputSeed::new(b"y".to_vec(), apex_core::types::SeedOrigin::Fuzzer),
                ])
            }
            async fn observe(&self, _result: &ExecutionResult) -> apex_core::error::Result<()> {
                Ok(())
            }
        }

        let oracle = Arc::new(CoverageOracle::new());
        let b = apex_core::types::BranchId::new(125, 1, 0, 0);
        oracle.register_branches([b]);

        // ErrorSandbox always returns Err → filter_map drops all → results is empty
        let cluster = AgentCluster::new(oracle, Arc::new(ErrorSandbox), test_target())
            .with_strategy(Box::new(SeedProvider))
            .with_config(OrchestratorConfig {
                coverage_target: 1.0,
                deadline_secs: None,
                stall_threshold: 2,
            });
        cluster.run().await.unwrap();
    }

    /// Exercises `with_file_paths` with an empty map.
    #[test]
    fn with_file_paths_empty_map() {
        let oracle = Arc::new(CoverageOracle::new());
        let cluster = AgentCluster::new(oracle, Arc::new(StubSandbox), test_target())
            .with_file_paths(HashMap::new());
        assert!(cluster.file_paths.is_empty());
    }

    /// Exercises `with_file_paths` with a single entry.
    #[test]
    fn with_file_paths_single_entry() {
        let oracle = Arc::new(CoverageOracle::new());
        let mut paths = HashMap::new();
        paths.insert(1u64, PathBuf::from("single.py"));
        let cluster =
            AgentCluster::new(oracle, Arc::new(StubSandbox), test_target()).with_file_paths(paths);
        assert_eq!(cluster.file_paths.len(), 1);
        assert_eq!(
            cluster.file_paths.get(&1),
            Some(&PathBuf::from("single.py"))
        );
    }

    /// Ledger count is accessible through Arc after run.
    #[tokio::test]
    async fn run_ledger_count_accessible() {
        let oracle = Arc::new(CoverageOracle::new());
        let cluster = AgentCluster::new(oracle, Arc::new(StubSandbox), test_target());
        cluster.run().await.unwrap();
        assert_eq!(cluster.ledger.count(), 0);
    }

    /// Multiple bugs of same class at different locations are both recorded.
    #[tokio::test]
    async fn run_multiple_distinct_crashes_recorded() {
        use std::sync::atomic::{AtomicUsize, Ordering};

        struct VariedCrashSandbox {
            call_count: AtomicUsize,
        }
        #[async_trait::async_trait]
        impl Sandbox for VariedCrashSandbox {
            async fn run(&self, input: &InputSeed) -> apex_core::error::Result<ExecutionResult> {
                let n = self.call_count.fetch_add(1, Ordering::SeqCst);
                Ok(ExecutionResult {
                    seed_id: input.id,
                    status: ExecutionStatus::Crash,
                    new_branches: Vec::new(),
                    trace: None,
                    duration_ms: 1,
                    stdout: String::new(),
                    stderr: format!("crash at src/f{}.rs:{}", n, n + 1),
                    input: None,
                })
            }
            async fn snapshot(&self) -> apex_core::error::Result<SnapshotId> {
                Ok(SnapshotId::new())
            }
            async fn restore(&self, _id: SnapshotId) -> apex_core::error::Result<()> {
                Ok(())
            }
            fn language(&self) -> Language {
                Language::Python
            }
        }

        struct AlwaysSeedBugs;
        #[async_trait::async_trait]
        impl Strategy for AlwaysSeedBugs {
            fn name(&self) -> &str {
                "always-seed-bugs"
            }
            async fn suggest_inputs(
                &self,
                _ctx: &ExplorationContext,
            ) -> apex_core::error::Result<Vec<InputSeed>> {
                Ok(vec![InputSeed::new(
                    b"bug".to_vec(),
                    apex_core::types::SeedOrigin::Fuzzer,
                )])
            }
            async fn observe(&self, _result: &ExecutionResult) -> apex_core::error::Result<()> {
                Ok(())
            }
        }

        let oracle = Arc::new(CoverageOracle::new());
        let b = apex_core::types::BranchId::new(126, 1, 0, 0);
        oracle.register_branches([b]);

        let sandbox: Arc<dyn Sandbox> = Arc::new(VariedCrashSandbox {
            call_count: AtomicUsize::new(0),
        });
        let cluster = AgentCluster::new(oracle, sandbox, test_target())
            .with_strategy(Box::new(AlwaysSeedBugs))
            .with_config(OrchestratorConfig {
                coverage_target: 1.0,
                deadline_secs: None,
                stall_threshold: 3,
            });
        cluster.run().await.unwrap();
        // Multiple distinct crashes should each be recorded (different locations).
        assert!(cluster.bug_summary().total >= 2);
    }

    /// Duplicate bug in same iteration — exercises `record_from_result` returning
    /// false (bug already seen) vs true (new bug).
    #[tokio::test]
    async fn run_duplicate_bug_not_double_counted() {
        struct DupCrashSandbox;
        #[async_trait::async_trait]
        impl Sandbox for DupCrashSandbox {
            async fn run(&self, input: &InputSeed) -> apex_core::error::Result<ExecutionResult> {
                Ok(ExecutionResult {
                    seed_id: input.id,
                    status: ExecutionStatus::Crash,
                    new_branches: Vec::new(),
                    trace: None,
                    duration_ms: 1,
                    stdout: String::new(),
                    stderr: "crash at src/same.rs:1".into(),
                    input: None,
                })
            }
            async fn snapshot(&self) -> apex_core::error::Result<SnapshotId> {
                Ok(SnapshotId::new())
            }
            async fn restore(&self, _id: SnapshotId) -> apex_core::error::Result<()> {
                Ok(())
            }
            fn language(&self) -> Language {
                Language::Python
            }
        }

        struct MultiSeedDup;
        #[async_trait::async_trait]
        impl Strategy for MultiSeedDup {
            fn name(&self) -> &str {
                "multi-dup"
            }
            async fn suggest_inputs(
                &self,
                _ctx: &ExplorationContext,
            ) -> apex_core::error::Result<Vec<InputSeed>> {
                Ok(vec![
                    InputSeed::new(b"d1".to_vec(), apex_core::types::SeedOrigin::Fuzzer),
                    InputSeed::new(b"d2".to_vec(), apex_core::types::SeedOrigin::Fuzzer),
                ])
            }
            async fn observe(&self, _result: &ExecutionResult) -> apex_core::error::Result<()> {
                Ok(())
            }
        }

        let oracle = Arc::new(CoverageOracle::new());
        let b = apex_core::types::BranchId::new(127, 1, 0, 0);
        oracle.register_branches([b]);

        let cluster = AgentCluster::new(oracle, Arc::new(DupCrashSandbox), test_target())
            .with_strategy(Box::new(MultiSeedDup))
            .with_config(OrchestratorConfig {
                coverage_target: 1.0,
                deadline_secs: None,
                stall_threshold: 2,
            });
        cluster.run().await.unwrap();
        // Same crash location → deduped to 1 bug even with multiple results.
        assert_eq!(cluster.bug_summary().total, 1);
    }

    /// Run with Rust language target.
    #[tokio::test]
    async fn run_with_rust_target() {
        let oracle = Arc::new(CoverageOracle::new());
        let target = Target {
            root: PathBuf::from("/tmp/rust-proj"),
            language: Language::Rust,
            test_command: vec!["cargo".into(), "test".into()],
        };
        let cluster = AgentCluster::new(oracle, Arc::new(StubSandbox), target);
        cluster.run().await.unwrap();
    }

    /// Exercises the `monitor_action` method on a cluster that has just been
    /// constructed with a custom config.
    #[test]
    fn monitor_action_on_cluster_with_custom_config() {
        let oracle = Arc::new(CoverageOracle::new());
        let cluster = AgentCluster::new(oracle, Arc::new(StubSandbox), test_target()).with_config(
            OrchestratorConfig {
                coverage_target: 0.5,
                deadline_secs: Some(60),
                stall_threshold: 5,
            },
        );
        assert_eq!(cluster.monitor_action(), MonitorAction::Normal);
    }

    /// OrchestratorConfig with very large stall_threshold.
    #[test]
    fn config_large_stall_threshold() {
        let cfg = OrchestratorConfig {
            coverage_target: 1.0,
            deadline_secs: None,
            stall_threshold: u64::MAX,
        };
        assert_eq!(cfg.stall_threshold, u64::MAX);
    }

    /// OrchestratorConfig with very large deadline.
    #[test]
    fn config_large_deadline() {
        let cfg = OrchestratorConfig {
            coverage_target: 1.0,
            deadline_secs: Some(u64::MAX),
            stall_threshold: 10,
        };
        assert_eq!(cfg.deadline_secs, Some(u64::MAX));
    }

    /// Run with tracing and combined new-coverage + bug in same result.
    #[tokio::test]
    async fn run_with_tracing_coverage_and_bug_same_result() {
        let subscriber = tracing_subscriber::fmt()
            .with_max_level(tracing::Level::TRACE)
            .with_test_writer()
            .finish();
        let _guard = tracing::subscriber::set_default(subscriber);

        struct CrashCoverSandbox {
            branches: std::sync::Mutex<Vec<apex_core::types::BranchId>>,
        }
        #[async_trait::async_trait]
        impl Sandbox for CrashCoverSandbox {
            async fn run(&self, input: &InputSeed) -> apex_core::error::Result<ExecutionResult> {
                let new_branches = {
                    let mut g = self.branches.lock().unwrap();
                    std::mem::take(&mut *g)
                };
                Ok(ExecutionResult {
                    seed_id: input.id,
                    status: ExecutionStatus::Crash,
                    new_branches,
                    trace: None,
                    duration_ms: 1,
                    stdout: String::new(),
                    stderr: "crash at src/combined.rs:10".into(),
                    input: None,
                })
            }
            async fn snapshot(&self) -> apex_core::error::Result<SnapshotId> {
                Ok(SnapshotId::new())
            }
            async fn restore(&self, _id: SnapshotId) -> apex_core::error::Result<()> {
                Ok(())
            }
            fn language(&self) -> Language {
                Language::Python
            }
        }

        struct OnceSeed4 {
            done: std::sync::Mutex<bool>,
        }
        #[async_trait::async_trait]
        impl Strategy for OnceSeed4 {
            fn name(&self) -> &str {
                "once4"
            }
            async fn suggest_inputs(
                &self,
                _ctx: &ExplorationContext,
            ) -> apex_core::error::Result<Vec<InputSeed>> {
                let mut d = self.done.lock().unwrap();
                if *d {
                    Ok(Vec::new())
                } else {
                    *d = true;
                    Ok(vec![InputSeed::new(
                        b"cb".to_vec(),
                        apex_core::types::SeedOrigin::Fuzzer,
                    )])
                }
            }
            async fn observe(&self, _result: &ExecutionResult) -> apex_core::error::Result<()> {
                Ok(())
            }
        }

        let oracle = Arc::new(CoverageOracle::new());
        let b = apex_core::types::BranchId::new(128, 1, 0, 0);
        oracle.register_branches([b.clone()]);

        let sandbox: Arc<dyn Sandbox> = Arc::new(CrashCoverSandbox {
            branches: std::sync::Mutex::new(vec![b]),
        });

        let cluster = AgentCluster::new(oracle.clone(), sandbox, test_target())
            .with_strategy(Box::new(OnceSeed4 {
                done: std::sync::Mutex::new(false),
            }))
            .with_config(OrchestratorConfig {
                coverage_target: 0.9,
                deadline_secs: Some(5),
                stall_threshold: 3,
            });
        cluster.run().await.unwrap();
        assert_eq!(oracle.covered_count(), 1);
        assert!(cluster.bug_summary().total > 0);
    }

    // ------------------------------------------------------------------
    // DrillerEscalation integration
    // ------------------------------------------------------------------

    #[test]
    fn orchestrator_without_driller_escalation() {
        let oracle = Arc::new(CoverageOracle::new());
        let sandbox: Arc<dyn Sandbox> = Arc::new(StubSandbox);
        let cluster = AgentCluster::new(oracle, sandbox, test_target());
        assert!(cluster.driller_escalation.is_none());
    }

    #[test]
    fn orchestrator_with_driller_escalation() {
        use crate::driller::{DrillerEscalation, DrillerStrategy, StuckDetector};
        use apex_symbolic::traits::Solver;

        struct NoopSolver;
        impl Solver for NoopSolver {
            fn solve(
                &self,
                _constraints: &[String],
                _negate_last: bool,
            ) -> apex_core::error::Result<Option<InputSeed>> {
                Ok(None)
            }
            fn set_logic(&mut self, _logic: apex_symbolic::traits::SolverLogic) {}
            fn name(&self) -> &str {
                "noop"
            }
        }

        let solver = Arc::new(std::sync::Mutex::new(NoopSolver));
        let strategy = Arc::new(std::sync::Mutex::new(DrillerStrategy::new(solver, 10)));
        let escalation = DrillerEscalation::new(strategy, 5, 0);

        let oracle = Arc::new(CoverageOracle::new());
        let sandbox: Arc<dyn Sandbox> = Arc::new(StubSandbox);
        let cluster = AgentCluster::new(oracle, sandbox, test_target())
            .with_driller_escalation(escalation);
        assert!(cluster.driller_escalation.is_some());
    }
}
