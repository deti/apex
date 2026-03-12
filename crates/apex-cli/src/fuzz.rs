use apex_core::{
    config::ApexConfig,
    traits::{Sandbox, Strategy},
    types::{BranchId, InstrumentedTarget, Language},
};
use apex_coverage::CoverageOracle;
use apex_fuzz::FuzzStrategy;
use apex_sandbox::{shim, ProcessSandbox, PythonTestSandbox};
use std::{path::PathBuf, sync::Arc};
use tracing::{debug, info, warn};

// ---------------------------------------------------------------------------
// Fuzz strategy entry point (C and Rust targets)
// ---------------------------------------------------------------------------

pub async fn run_fuzz_strategy(
    oracle: Arc<CoverageOracle>,
    instrumented: &InstrumentedTarget,
    coverage_target: f64,
    max_iters: usize,
    command: Vec<String>,
    cfg: &ApexConfig,
) -> color_eyre::Result<()> {
    let target_root = &instrumented.target.root;
    let lang = instrumented.target.language;

    // Try to compile the coverage shim once.
    let shim_path = match shim::ensure_compiled() {
        Ok(p) => {
            info!(path = %p.display(), "coverage shim ready");
            Some(p)
        }
        Err(e) => {
            warn!(error = %e, "could not compile coverage shim; running without SHM coverage");
            None
        }
    };

    // Build ProcessSandbox, wiring coverage if shim is available.
    let branch_index: Vec<BranchId> = oracle
        .uncovered_branches()
        .into_iter()
        .chain(instrumented.executed_branch_ids.iter().cloned())
        .collect();

    let mut sandbox = ProcessSandbox::new(lang, target_root.clone(), command);
    sandbox = sandbox.with_timeout(cfg.sandbox.process_timeout_ms);
    sandbox = sandbox.with_coverage(Arc::clone(&oracle), branch_index);
    if let Some(ref p) = shim_path {
        sandbox = sandbox.with_shim(p.clone());
    }
    let sandbox = Arc::new(sandbox);

    let fuzz = Arc::new(FuzzStrategy::new(Arc::clone(&oracle)));

    // Seed corpus with a minimal empty input.
    let _ = fuzz.seed_corpus([vec![0u8]]);

    info!(lang = %lang, max_iters, "starting fuzz loop");
    let mut stall = 0usize;

    let ctx = apex_core::types::ExplorationContext {
        target: instrumented.target.clone(),
        uncovered_branches: oracle.uncovered_branches(),
        iteration: 0,
    };

    for iter in 0..max_iters {
        let pct = oracle.coverage_percent();
        if pct / 100.0 >= coverage_target {
            info!(coverage = %format!("{pct:.1}%"), "fuzz: coverage target reached");
            break;
        }
        if oracle.uncovered_branches().is_empty() {
            info!("fuzz: all branches covered");
            break;
        }

        let inputs = fuzz.suggest_inputs(&ctx).await?;
        let mut any_new = false;

        for seed in &inputs {
            let result = sandbox.run(seed).await?;
            let delta = oracle.merge_from_result(&result);
            if !delta.newly_covered.is_empty() {
                any_new = true;
                info!(
                    iter,
                    newly_covered = delta.newly_covered.len(),
                    coverage = %format!("{:.1}%", oracle.coverage_percent()),
                    "fuzz: new coverage"
                );
                // Add winning input to corpus.
                let _ = fuzz.seed_corpus([seed.data.to_vec()]);
            }
            fuzz.observe(&result).await?;
        }

        if !any_new {
            stall += 1;
        } else {
            stall = 0;
        }

        if stall >= cfg.fuzz.stall_iterations {
            info!(iter, "fuzz: stalled; stopping early");
            break;
        }

        if iter % 100 == 0 {
            info!(
                iter,
                coverage = %format!("{:.1}%", oracle.coverage_percent()),
                stall,
                "fuzz progress"
            );
        }
    }

    let covered = oracle.covered_count();
    let total = oracle.total_count();
    let pct = oracle.coverage_percent();
    println!("\nFuzz complete: {covered}/{total} branches ({pct:.1}%)");
    Ok(())
}

// ---------------------------------------------------------------------------
// Combined fuzz + agent (--strategy all)
// ---------------------------------------------------------------------------

#[allow(clippy::too_many_arguments)]
pub async fn run_all_strategies(
    oracle: Arc<CoverageOracle>,
    instrumented: &InstrumentedTarget,
    coverage_target: f64,
    fuzz_iters: usize,
    _agent_rounds: usize,
    _output: Option<PathBuf>,
    command: Vec<String>,
    cfg: &ApexConfig,
) -> color_eyre::Result<()> {
    let lang = instrumented.target.language;

    // ── Phase A: fuzz (C/Rust binaries) or skip (Python/others) ───────────
    match lang {
        Language::C | Language::Rust => {
            info!("all: phase A — fuzzing");
            run_fuzz_strategy(
                Arc::clone(&oracle),
                instrumented,
                coverage_target,
                fuzz_iters,
                command,
                cfg,
            )
            .await?;
        }
        _ => {
            info!(lang = %lang, "all: skipping fuzz phase (not applicable)");
        }
    }

    // ── Phase B: concolic (Python targets only) ────────────────────────────
    if lang == Language::Python {
        let pct = oracle.coverage_percent();
        if pct / 100.0 < coverage_target && !oracle.uncovered_branches().is_empty() {
            info!("all: phase B — concolic");
            run_concolic_phase(Arc::clone(&oracle), instrumented, coverage_target, cfg).await;
        } else {
            info!("all: phase B — concolic skipped (target already reached)");
        }
    }

    // ── Phase C: report remaining gaps ─────────────────────────────────────
    let covered = oracle.covered_count();
    let total = oracle.total_count();
    let pct = oracle.coverage_percent();
    info!(covered, total, coverage = %format!("{pct:.1}%"), "all strategies complete");

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use apex_core::types::{BranchId, SeedId, Target};

    #[test]
    fn default_config_stall_iterations() {
        let cfg = ApexConfig::default();
        assert_eq!(cfg.fuzz.stall_iterations, 50);
    }

    #[test]
    fn run_all_strategies_skips_fuzz_for_python() {
        let cfg = ApexConfig::default();
        let oracle = Arc::new(CoverageOracle::new());
        let target = Target {
            root: PathBuf::from("/tmp"),
            language: Language::Python,
            test_command: Vec::new(),
        };
        let instrumented = apex_core::types::InstrumentedTarget {
            target,
            branch_ids: vec![],
            executed_branch_ids: vec![],
            file_paths: std::collections::HashMap::new(),
            work_dir: PathBuf::from("/tmp"),
        };
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let result = rt.block_on(run_all_strategies(
            oracle,
            &instrumented,
            0.0,
            0,
            0,
            None,
            vec!["dummy".into()],
            &cfg,
        ));
        assert!(result.is_ok());
    }

    #[test]
    fn run_all_strategies_c_language_enters_fuzz_phase() {
        let cfg = ApexConfig::default();
        let oracle = Arc::new(CoverageOracle::new());
        let b = BranchId::new(1, 1, 0, 0);
        oracle.register_branches([b.clone()]);
        oracle.mark_covered(&b, SeedId::new());

        let target = Target {
            root: PathBuf::from("/tmp"),
            language: Language::C,
            test_command: Vec::new(),
        };
        let instrumented = apex_core::types::InstrumentedTarget {
            target,
            branch_ids: vec![b.clone()],
            executed_branch_ids: vec![b],
            file_paths: std::collections::HashMap::new(),
            work_dir: PathBuf::from("/tmp"),
        };
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let result = rt.block_on(run_all_strategies(
            oracle,
            &instrumented,
            0.0,
            0,
            0,
            None,
            vec!["/nonexistent/binary".into()],
            &cfg,
        ));
        assert!(result.is_ok());
    }

    #[test]
    fn run_all_strategies_rust_language_enters_fuzz_phase() {
        let cfg = ApexConfig::default();
        let oracle = Arc::new(CoverageOracle::new());
        let target = Target {
            root: PathBuf::from("/tmp"),
            language: Language::Rust,
            test_command: Vec::new(),
        };
        let instrumented = apex_core::types::InstrumentedTarget {
            target,
            branch_ids: vec![],
            executed_branch_ids: vec![],
            file_paths: std::collections::HashMap::new(),
            work_dir: PathBuf::from("/tmp"),
        };
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let result = rt.block_on(run_all_strategies(
            oracle,
            &instrumented,
            0.0,
            0,
            0,
            None,
            vec!["dummy".into()],
            &cfg,
        ));
        assert!(result.is_ok());
    }

    #[test]
    fn run_all_strategies_js_skips_both_fuzz_and_concolic() {
        let cfg = ApexConfig::default();
        let oracle = Arc::new(CoverageOracle::new());
        let target = Target {
            root: PathBuf::from("/tmp"),
            language: Language::JavaScript,
            test_command: Vec::new(),
        };
        let instrumented = apex_core::types::InstrumentedTarget {
            target,
            branch_ids: vec![],
            executed_branch_ids: vec![],
            file_paths: std::collections::HashMap::new(),
            work_dir: PathBuf::from("/tmp"),
        };
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let result = rt.block_on(run_all_strategies(
            oracle,
            &instrumented,
            0.0,
            0,
            0,
            None,
            vec!["dummy".into()],
            &cfg,
        ));
        assert!(result.is_ok());
    }

    #[test]
    fn run_all_strategies_java_skips_fuzz_and_concolic() {
        let cfg = ApexConfig::default();
        let oracle = Arc::new(CoverageOracle::new());
        let b = BranchId::new(1, 1, 0, 0);
        oracle.register_branches([b.clone()]);
        // Leave branch uncovered — should still skip fuzz+concolic for Java
        let target = Target {
            root: PathBuf::from("/tmp"),
            language: Language::Java,
            test_command: Vec::new(),
        };
        let instrumented = apex_core::types::InstrumentedTarget {
            target,
            branch_ids: vec![b.clone()],
            executed_branch_ids: vec![],
            file_paths: std::collections::HashMap::new(),
            work_dir: PathBuf::from("/tmp"),
        };
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let result = rt.block_on(run_all_strategies(
            oracle.clone(),
            &instrumented,
            0.9,
            100,
            10,
            None,
            vec!["dummy".into()],
            &cfg,
        ));
        assert!(result.is_ok());
        // Coverage should remain 0% since fuzz was skipped
        assert_eq!(oracle.coverage_percent(), 0.0);
    }

    #[test]
    fn run_all_strategies_wasm_skips_fuzz_and_concolic() {
        let cfg = ApexConfig::default();
        let oracle = Arc::new(CoverageOracle::new());
        let target = Target {
            root: PathBuf::from("/tmp"),
            language: Language::Wasm,
            test_command: Vec::new(),
        };
        let instrumented = apex_core::types::InstrumentedTarget {
            target,
            branch_ids: vec![],
            executed_branch_ids: vec![],
            file_paths: std::collections::HashMap::new(),
            work_dir: PathBuf::from("/tmp"),
        };
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let result = rt.block_on(run_all_strategies(
            oracle,
            &instrumented,
            0.0,
            0,
            0,
            None,
            vec!["dummy".into()],
            &cfg,
        ));
        assert!(result.is_ok());
    }

    #[test]
    fn run_all_strategies_ruby_skips_fuzz_and_concolic() {
        let cfg = ApexConfig::default();
        let oracle = Arc::new(CoverageOracle::new());
        let target = Target {
            root: PathBuf::from("/tmp"),
            language: Language::Ruby,
            test_command: Vec::new(),
        };
        let instrumented = apex_core::types::InstrumentedTarget {
            target,
            branch_ids: vec![],
            executed_branch_ids: vec![],
            file_paths: std::collections::HashMap::new(),
            work_dir: PathBuf::from("/tmp"),
        };
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let result = rt.block_on(run_all_strategies(
            oracle,
            &instrumented,
            0.0,
            0,
            0,
            None,
            vec!["dummy".into()],
            &cfg,
        ));
        assert!(result.is_ok());
    }

    /// When all branches are already covered, run_fuzz_strategy should exit
    /// immediately without iterating.
    #[test]
    fn run_fuzz_strategy_all_covered_exits_immediately() {
        let cfg = ApexConfig::default();
        let oracle = Arc::new(CoverageOracle::new());
        let b = BranchId::new(1, 1, 0, 0);
        oracle.register_branches([b.clone()]);
        oracle.mark_covered(&b, SeedId::new());

        let target = Target {
            root: PathBuf::from("/tmp"),
            language: Language::C,
            test_command: Vec::new(),
        };
        let instrumented = apex_core::types::InstrumentedTarget {
            target,
            branch_ids: vec![b.clone()],
            executed_branch_ids: vec![b],
            file_paths: std::collections::HashMap::new(),
            work_dir: PathBuf::from("/tmp"),
        };
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let result = rt.block_on(run_fuzz_strategy(
            oracle.clone(),
            &instrumented,
            0.0, // coverage_target = 0 means any coverage is enough
            1000,
            vec!["/nonexistent/binary".into()],
            &cfg,
        ));
        assert!(result.is_ok());
        // Should be 100% since the only branch was pre-covered
        assert_eq!(oracle.coverage_percent(), 100.0);
    }

    /// When oracle has no branches at all, run_fuzz_strategy should exit
    /// immediately (uncovered_branches is empty).
    #[test]
    fn run_fuzz_strategy_empty_oracle_exits_immediately() {
        let cfg = ApexConfig::default();
        let oracle = Arc::new(CoverageOracle::new());

        let target = Target {
            root: PathBuf::from("/tmp"),
            language: Language::C,
            test_command: Vec::new(),
        };
        let instrumented = apex_core::types::InstrumentedTarget {
            target,
            branch_ids: vec![],
            executed_branch_ids: vec![],
            file_paths: std::collections::HashMap::new(),
            work_dir: PathBuf::from("/tmp"),
        };
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let result = rt.block_on(run_fuzz_strategy(
            oracle,
            &instrumented,
            0.0,
            1000,
            vec!["/nonexistent/binary".into()],
            &cfg,
        ));
        assert!(result.is_ok());
    }

    /// run_fuzz_strategy with zero max_iters should not loop at all.
    #[test]
    fn run_fuzz_strategy_zero_iters() {
        let cfg = ApexConfig::default();
        let oracle = Arc::new(CoverageOracle::new());
        let b = BranchId::new(1, 1, 0, 0);
        oracle.register_branches([b.clone()]);

        let target = Target {
            root: PathBuf::from("/tmp"),
            language: Language::C,
            test_command: Vec::new(),
        };
        let instrumented = apex_core::types::InstrumentedTarget {
            target,
            branch_ids: vec![b.clone()],
            executed_branch_ids: vec![],
            file_paths: std::collections::HashMap::new(),
            work_dir: PathBuf::from("/tmp"),
        };
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let result = rt.block_on(run_fuzz_strategy(
            oracle.clone(),
            &instrumented,
            0.9,
            0, // zero iterations
            vec!["/nonexistent/binary".into()],
            &cfg,
        ));
        assert!(result.is_ok());
        // Branch should remain uncovered since we never ran
        assert_eq!(oracle.coverage_percent(), 0.0);
    }

    /// run_all_strategies with Python + empty oracle skips concolic
    /// because uncovered_branches is empty.
    #[test]
    fn run_all_strategies_python_empty_oracle_skips_concolic() {
        let cfg = ApexConfig::default();
        let oracle = Arc::new(CoverageOracle::new());
        let target = Target {
            root: PathBuf::from("/tmp"),
            language: Language::Python,
            test_command: Vec::new(),
        };
        let instrumented = apex_core::types::InstrumentedTarget {
            target,
            branch_ids: vec![],
            executed_branch_ids: vec![],
            file_paths: std::collections::HashMap::new(),
            work_dir: PathBuf::from("/tmp"),
        };
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let result = rt.block_on(run_all_strategies(
            oracle,
            &instrumented,
            0.9,
            0,
            0,
            None,
            vec!["dummy".into()],
            &cfg,
        ));
        assert!(result.is_ok());
    }

    /// run_all_strategies with C + zero fuzz_iters completes without error.
    #[test]
    fn run_all_strategies_c_zero_fuzz_iters() {
        let cfg = ApexConfig::default();
        let oracle = Arc::new(CoverageOracle::new());
        let b = BranchId::new(1, 1, 0, 0);
        oracle.register_branches([b.clone()]);

        let target = Target {
            root: PathBuf::from("/tmp"),
            language: Language::C,
            test_command: Vec::new(),
        };
        let instrumented = apex_core::types::InstrumentedTarget {
            target,
            branch_ids: vec![b.clone()],
            executed_branch_ids: vec![],
            file_paths: std::collections::HashMap::new(),
            work_dir: PathBuf::from("/tmp"),
        };
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let result = rt.block_on(run_all_strategies(
            oracle.clone(),
            &instrumented,
            0.9,
            0, // zero fuzz iterations
            0,
            None,
            vec!["/nonexistent/binary".into()],
            &cfg,
        ));
        assert!(result.is_ok());
        // Branch stays uncovered
        assert_eq!(oracle.coverage_percent(), 0.0);
    }

    #[test]
    fn run_all_strategies_python_skips_concolic_when_target_reached() {
        let cfg = ApexConfig::default();
        let oracle = Arc::new(CoverageOracle::new());
        let b = BranchId::new(1, 1, 0, 0);
        oracle.register_branches([b.clone()]);
        oracle.mark_covered(&b, SeedId::new());

        let target = Target {
            root: PathBuf::from("/tmp"),
            language: Language::Python,
            test_command: Vec::new(),
        };
        let instrumented = apex_core::types::InstrumentedTarget {
            target,
            branch_ids: vec![b.clone()],
            executed_branch_ids: vec![b],
            file_paths: std::collections::HashMap::new(),
            work_dir: PathBuf::from("/tmp"),
        };
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let result = rt.block_on(run_all_strategies(
            oracle,
            &instrumented,
            0.5,
            0,
            0,
            None,
            vec!["dummy".into()],
            &cfg,
        ));
        assert!(result.is_ok());
    }
}

/// Run a limited concolic pass as part of the combined strategy.
async fn run_concolic_phase(
    oracle: Arc<CoverageOracle>,
    instrumented: &InstrumentedTarget,
    coverage_target: f64,
    cfg: &ApexConfig,
) {
    use apex_concolic::PythonConcolicStrategy;
    use apex_core::traits::Strategy;

    let target_root = instrumented.target.root.clone();
    let file_paths = Arc::new(instrumented.file_paths.clone());

    let sandbox = Arc::new(PythonTestSandbox::new(
        Arc::clone(&oracle),
        Arc::clone(&file_paths),
        target_root.clone(),
    ));

    let strategy = PythonConcolicStrategy::new(
        Arc::clone(&oracle),
        Arc::clone(&file_paths),
        target_root.clone(),
        instrumented.target.test_command.clone(),
    );

    let max_rounds = cfg.concolic.max_rounds.min(cfg.agent.max_rounds);
    for round in 1..=max_rounds {
        let uncovered = oracle.uncovered_branches();
        if uncovered.is_empty() || oracle.coverage_percent() / 100.0 >= coverage_target {
            break;
        }

        info!(
            round,
            uncovered = uncovered.len(),
            "concolic round (all strategy)"
        );

        let ctx = apex_core::types::ExplorationContext {
            target: instrumented.target.clone(),
            uncovered_branches: uncovered,
            iteration: round as u64,
        };

        let seeds = match strategy.suggest_inputs(&ctx).await {
            Ok(s) => s,
            Err(e) => {
                warn!(error = %e, "concolic suggest_inputs failed");
                break;
            }
        };

        if seeds.is_empty() {
            info!("concolic: no seeds generated");
            break;
        }

        for seed in &seeds {
            match sandbox.run(seed).await {
                Ok(result) => {
                    let delta = oracle.merge_from_result(&result);
                    if !delta.newly_covered.is_empty() {
                        info!(
                            newly_covered = delta.newly_covered.len(),
                            "concolic seed improved coverage"
                        );
                    }
                }
                Err(e) => debug!(error = %e, "concolic seed run failed"),
            }
        }
    }
}
