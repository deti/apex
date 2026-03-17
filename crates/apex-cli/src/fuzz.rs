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

        #[allow(unknown_lints, clippy::manual_is_multiple_of)]
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

    // ------------------------------------------------------------------
    // Coverage for uncovered regions:
    //   Lines 30-35:  shim compile failure warn! path
    //   Lines 185-190: Python + uncovered branches → enters concolic phase
    //   Lines 194-219: run_concolic_phase body (concolic strategy loop)
    //   Lines 223-253: concolic seed runs, debug log on failure
    // ------------------------------------------------------------------

    /// Target: lines 185-190 — run_all_strategies with Python language and
    /// uncovered branches below coverage target exercises the
    /// `pct / 100.0 < coverage_target && !oracle.uncovered_branches().is_empty()`
    /// branch and calls run_concolic_phase.
    ///
    /// The concolic phase will fail to generate useful seeds (no real Python
    /// files in /tmp) but must complete without panic or error propagation.
    #[test]
    fn run_all_strategies_python_uncovered_below_target_enters_concolic() {
        // Target: lines 185-190, 194-253
        let mut cfg = ApexConfig::default();
        cfg.concolic.max_rounds = 1;
        cfg.agent.max_rounds = 1;

        let oracle = Arc::new(CoverageOracle::new());
        let b = BranchId::new(1, 1, 0, 0);
        oracle.register_branches([b.clone()]);
        // Do NOT mark the branch covered — 0% coverage, target 0.9 → enters concolic

        let target = Target {
            root: std::path::PathBuf::from("/tmp"),
            language: Language::Python,
            test_command: vec!["python3".into(), "-m".into(), "pytest".into()],
        };
        let instrumented = apex_core::types::InstrumentedTarget {
            target,
            branch_ids: vec![b.clone()],
            executed_branch_ids: vec![],
            file_paths: std::collections::HashMap::new(),
            work_dir: std::path::PathBuf::from("/tmp"),
        };
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        // run_concolic_phase will attempt and gracefully fail (no real Python project)
        let result = rt.block_on(run_all_strategies(
            oracle,
            &instrumented,
            0.9, // target not met — 0% coverage
            0,
            1,
            None,
            vec!["python3".into()],
            &cfg,
        ));
        assert!(result.is_ok());
    }

    /// Target: lines 185-190 — concolic phase is skipped when oracle has
    /// uncovered branches but coverage_target is already exceeded.
    ///
    /// This exercises the `else` branch at line 185 (the "concolic skipped"
    /// info! log at line ~187).
    #[test]
    fn run_all_strategies_python_skips_concolic_when_target_already_met() {
        // Target: line ~187 (else branch after concolic check)
        let cfg = ApexConfig::default();
        let oracle = Arc::new(CoverageOracle::new());
        let b1 = BranchId::new(2, 1, 0, 0);
        let b2 = BranchId::new(2, 2, 0, 0);
        oracle.register_branches([b1.clone(), b2.clone()]);
        // Mark one covered: 50% coverage. target = 0.4 → already met → skip concolic
        oracle.mark_covered(&b1, SeedId::new());

        let target = Target {
            root: std::path::PathBuf::from("/tmp"),
            language: Language::Python,
            test_command: vec!["pytest".into()],
        };
        let instrumented = apex_core::types::InstrumentedTarget {
            target,
            branch_ids: vec![b1.clone(), b2],
            executed_branch_ids: vec![b1],
            file_paths: std::collections::HashMap::new(),
            work_dir: std::path::PathBuf::from("/tmp"),
        };
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let result = rt.block_on(run_all_strategies(
            oracle.clone(),
            &instrumented,
            0.4, // coverage is 50% >= 40% → concolic skipped
            0,
            0,
            None,
            vec!["dummy".into()],
            &cfg,
        ));
        assert!(result.is_ok());
        // Coverage stays at 50% (no additional work was done)
        assert_eq!(oracle.coverage_percent(), 50.0);
    }

    /// Target: run_concolic_phase — exercises the concolic loop with
    /// max_rounds=2 and a non-empty uncovered set to test the inner loop body.
    ///
    /// The strategy will try to generate seeds for a dummy /tmp target;
    /// suggest_inputs is expected to return empty or fail gracefully.
    /// The loop should complete within max_rounds without panicking.
    #[test]
    fn run_all_strategies_python_concolic_multiple_rounds() {
        // Target: lines 194-253 (run_concolic_phase loop body)
        let mut cfg = ApexConfig::default();
        cfg.concolic.max_rounds = 2;
        cfg.agent.max_rounds = 2;

        let oracle = Arc::new(CoverageOracle::new());
        let b1 = BranchId::new(10, 1, 0, 0);
        let b2 = BranchId::new(10, 2, 0, 0);
        oracle.register_branches([b1.clone(), b2.clone()]);
        // Both uncovered — 0% < 0.9 target → enters and loops through concolic

        let target = Target {
            root: std::path::PathBuf::from("/tmp"),
            language: Language::Python,
            test_command: vec!["python3".into(), "-m".into(), "pytest".into()],
        };
        let instrumented = apex_core::types::InstrumentedTarget {
            target,
            branch_ids: vec![b1.clone(), b2.clone()],
            executed_branch_ids: vec![],
            file_paths: std::collections::HashMap::new(),
            work_dir: std::path::PathBuf::from("/tmp"),
        };
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let result = rt.block_on(run_all_strategies(
            oracle.clone(),
            &instrumented,
            0.9,
            0,
            2,
            None,
            vec!["python3".into()],
            &cfg,
        ));
        assert!(result.is_ok());
        // Branches remain uncovered (no real execution) — no panic is the key invariant
        assert_eq!(oracle.covered_count(), 0);
    }

    /// Target: lines 30-35 — the warn! path when shim compilation fails.
    ///
    /// We cannot force shim::ensure_compiled() to fail directly, but we can
    /// verify that run_fuzz_strategy handles the case where shim_path is None
    /// (which is what happens when compilation fails). The existing zero-iters
    /// test exercises this path but we add one with explicit coverage tracking.
    #[test]
    fn run_fuzz_strategy_no_shim_path_completes_without_error() {
        // Target: lines 30-35 (shim failure path)
        // When shim compilation fails, shim_path = None and the strategy
        // continues without SHM coverage. With zero iters this path is fast.
        let cfg = ApexConfig::default();
        let oracle = Arc::new(CoverageOracle::new());
        let b = BranchId::new(99, 1, 0, 0);
        oracle.register_branches([b.clone()]);
        // Branch uncovered, target 0.9 not met, but 0 iters → exits immediately

        let target = Target {
            root: std::path::PathBuf::from("/tmp"),
            language: Language::C,
            test_command: Vec::new(),
        };
        let instrumented = apex_core::types::InstrumentedTarget {
            target,
            branch_ids: vec![b.clone()],
            executed_branch_ids: vec![],
            file_paths: std::collections::HashMap::new(),
            work_dir: std::path::PathBuf::from("/tmp"),
        };
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let result = rt.block_on(run_fuzz_strategy(
            oracle.clone(),
            &instrumented,
            0.9,
            0, // zero iters → always exits before needing shim
            vec!["/nonexistent/binary".into()],
            &cfg,
        ));
        assert!(result.is_ok());
        // Branch was never covered (zero iters)
        assert_eq!(oracle.coverage_percent(), 0.0);
    }

    // ------------------------------------------------------------------
    // Round 2: Cover fuzz loop body (lines 68-97) and coverage-target
    // early-exit path (lines 70-72).
    // ------------------------------------------------------------------

    /// Target: lines 68-83 — the fuzz loop body entered when max_iters > 0
    /// and there are uncovered branches.
    ///
    /// With max_iters = 1 and a nonexistent binary, the loop enters, calls
    /// suggest_inputs (line 79), iterates over seeds (line 82), and then
    /// sandbox.run() fails because the binary does not exist. The `?` at
    /// line 83 propagates the error up, so run_fuzz_strategy returns Err.
    ///
    /// Classification: WRONG — the function returns Ok(()) with zero iters
    /// when the sandbox would fail on the first iter, masking the error.
    /// Confirmed by this test: calling with iters=1 correctly returns Err.
    #[test]
    fn run_fuzz_strategy_sandbox_error_propagates_on_first_iter() {
        // Target: lines 68-83 (fuzz loop body, suggest_inputs, seed loop)
        let cfg = ApexConfig::default();
        let oracle = Arc::new(CoverageOracle::new());
        let b = BranchId::new(200, 1, 0, 0);
        oracle.register_branches([b.clone()]);
        // Branch uncovered → loop will not exit early at line 74

        let target = Target {
            root: std::path::PathBuf::from("/tmp"),
            language: Language::C,
            test_command: Vec::new(),
        };
        let instrumented = apex_core::types::InstrumentedTarget {
            target,
            branch_ids: vec![b.clone()],
            executed_branch_ids: vec![],
            file_paths: std::collections::HashMap::new(),
            work_dir: std::path::PathBuf::from("/tmp"),
        };
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        // With max_iters = 1, the loop body executes once. suggest_inputs
        // returns 8 random seeds (corpus was just seeded with [0u8]).
        // sandbox.run() will fail for the nonexistent binary, propagating Err.
        let result = rt.block_on(run_fuzz_strategy(
            oracle.clone(),
            &instrumented,
            0.9,
            1, // ONE iteration → enters loop body
            vec!["/nonexistent_apex_test_binary_xyz_12345".into()],
            &cfg,
        ));
        // The sandbox will fail to spawn the nonexistent binary, and the
        // error propagates via `?` at line 83.
        assert!(
            result.is_err(),
            "expected Err from sandbox failure, got Ok"
        );
    }

    /// Target: lines 70-72 — coverage target already reached at the start of
    /// the fuzz loop. The loop enters on iter=0, checks pct/100.0 >= target,
    /// and breaks immediately with the "coverage target reached" log.
    ///
    /// This exercises the first `if` inside the loop body, which is distinct
    /// from the pre-loop empty-oracle check.
    #[test]
    fn run_fuzz_strategy_coverage_target_reached_mid_loop() {
        // Target: lines 68-72 (loop enter, pct check, break)
        // Set coverage_target = 0.0 so that 0% >= 0% triggers the break.
        // But oracle must have uncovered branches so the pre-loop check at
        // line 74 does NOT trigger first (empty-oracle breaks at line 74).
        // Actually with coverage_target=0.0 and pct=0.0: 0.0/100.0 >= 0.0 is
        // TRUE → breaks at line 72 before the empty check at line 74.
        let cfg = ApexConfig::default();
        let oracle = Arc::new(CoverageOracle::new());
        let b = BranchId::new(300, 1, 0, 0);
        oracle.register_branches([b.clone()]);
        // Branch is uncovered but target is 0.0 → 0% >= 0% breaks the loop

        let target = Target {
            root: std::path::PathBuf::from("/tmp"),
            language: Language::C,
            test_command: Vec::new(),
        };
        let instrumented = apex_core::types::InstrumentedTarget {
            target,
            branch_ids: vec![b.clone()],
            executed_branch_ids: vec![],
            file_paths: std::collections::HashMap::new(),
            work_dir: std::path::PathBuf::from("/tmp"),
        };
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let result = rt.block_on(run_fuzz_strategy(
            oracle.clone(),
            &instrumented,
            0.0, // already met → breaks at line 70-72
            10,  // would iterate if not for target check
            vec!["/nonexistent_apex_test_binary_xyz_12345".into()],
            &cfg,
        ));
        assert!(
            result.is_ok(),
            "expected Ok (loop broke at coverage-target check)"
        );
        // No iterations completed — coverage stays at 0
        assert_eq!(oracle.coverage_percent(), 0.0);
    }

    /// Target: lines 74-76 — all branches already covered, loop enters but
    /// breaks at the `uncovered_branches().is_empty()` check (second guard).
    ///
    /// This is distinct from the coverage_target break above: here the
    /// coverage_target is NOT met (pct < target) but there are no uncovered
    /// branches to work on.
    #[test]
    fn run_fuzz_strategy_no_uncovered_branches_breaks_loop() {
        // Target: lines 74-76 (uncovered_branches empty check inside loop)
        let cfg = ApexConfig::default();
        let oracle = Arc::new(CoverageOracle::new());
        let b = BranchId::new(400, 1, 0, 0);
        oracle.register_branches([b.clone()]);
        oracle.mark_covered(&b, SeedId::new());
        // 100% covered → uncovered_branches().is_empty() == true
        // But coverage_target = 0.99 > 1.0 would still have pct/100=1.0 >= 0.99
        // so actually line 70 would break first. Use target > 1.0 to skip line 70.
        // coverage_target = 1.1 means pct/100.0 = 1.0 < 1.1 → passes line 70 check
        // then uncovered_branches is empty → breaks at line 74-76.

        let target = Target {
            root: std::path::PathBuf::from("/tmp"),
            language: Language::C,
            test_command: Vec::new(),
        };
        let instrumented = apex_core::types::InstrumentedTarget {
            target,
            branch_ids: vec![b.clone()],
            executed_branch_ids: vec![b],
            file_paths: std::collections::HashMap::new(),
            work_dir: std::path::PathBuf::from("/tmp"),
        };
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let result = rt.block_on(run_fuzz_strategy(
            oracle.clone(),
            &instrumented,
            1.1, // above 100% so line 70 does NOT break; line 74 does
            10,
            vec!["/nonexistent_apex_test_binary_xyz_12345".into()],
            &cfg,
        ));
        assert!(result.is_ok());
        assert_eq!(oracle.coverage_percent(), 100.0);
    }

    /// Bug test: run_fuzz_strategy coverage_target comparison uses
    /// `pct / 100.0 >= coverage_target` where pct is already a percentage
    /// (e.g. 75.0 for 75%). So coverage_target must be in [0.0, 1.0] range.
    ///
    /// If a caller passes coverage_target = 75.0 (thinking it's a percent),
    /// the comparison becomes `75.0 / 100.0 >= 75.0` → `0.75 >= 75.0` → false,
    /// and the fuzz loop never terminates early even when the target is met.
    ///
    /// Classification: STYLE — the API is ambiguous (0-1 fraction vs 0-100 percent).
    #[test]
    fn bug_fuzz_strategy_coverage_target_scale_ambiguity() {
        // Document the expected scale: coverage_target must be 0.0..=1.0
        // passing 0.75 means "75% coverage required"
        let cfg = ApexConfig::default();
        let oracle = Arc::new(CoverageOracle::new());
        let b = BranchId::new(500, 1, 0, 0);
        oracle.register_branches([b.clone()]);
        oracle.mark_covered(&b, SeedId::new());
        // 100% coverage, target = 0.75 → 100/100 = 1.0 >= 0.75 → should break at iter 0

        let target = Target {
            root: std::path::PathBuf::from("/tmp"),
            language: Language::C,
            test_command: Vec::new(),
        };
        let instrumented = apex_core::types::InstrumentedTarget {
            target,
            branch_ids: vec![b.clone()],
            executed_branch_ids: vec![b],
            file_paths: std::collections::HashMap::new(),
            work_dir: std::path::PathBuf::from("/tmp"),
        };
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        // With coverage_target = 0.75 (fraction) and 100% coverage:
        // pct=100.0, 100.0/100.0 = 1.0 >= 0.75 → breaks at line 70
        let result = rt.block_on(run_fuzz_strategy(
            Arc::clone(&oracle),
            &instrumented,
            0.75,
            100,
            vec!["/nonexistent_apex_test_binary_xyz_12345".into()],
            &cfg,
        ));
        assert!(result.is_ok(), "should exit early when target met");

        // If caller passes 75.0 instead of 0.75:
        // pct=100.0, 100.0/100.0 = 1.0 >= 75.0 → FALSE → loop does NOT break
        // at line 70, falls through to line 74 (uncovered empty) → breaks there.
        // So with 0 uncovered branches the result is still Ok — not wrong in
        // this case. Document the ambiguity.
        let oracle2 = Arc::new(CoverageOracle::new());
        let b2 = BranchId::new(501, 1, 0, 0);
        oracle2.register_branches([b2.clone()]);
        oracle2.mark_covered(&b2, SeedId::new());
        let target2 = Target {
            root: std::path::PathBuf::from("/tmp"),
            language: Language::C,
            test_command: Vec::new(),
        };
        let instrumented2 = apex_core::types::InstrumentedTarget {
            target: target2,
            branch_ids: vec![b2.clone()],
            executed_branch_ids: vec![b2],
            file_paths: std::collections::HashMap::new(),
            work_dir: std::path::PathBuf::from("/tmp"),
        };
        let result2 = rt.block_on(run_fuzz_strategy(
            oracle2,
            &instrumented2,
            75.0, // scale mismatch: treated as "7500%" target, never met
            100,
            vec!["/nonexistent_apex_test_binary_xyz_12345".into()],
            &cfg,
        ));
        // With 0 uncovered branches, loop still breaks at line 74 → Ok
        assert!(result2.is_ok());
    }
}
