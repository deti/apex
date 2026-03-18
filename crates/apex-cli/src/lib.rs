//! APEX CLI — Autonomous Path EXploration.
//!
//! Drives any repository toward 100% branch coverage through instrumentation,
//! fuzzing, concolic execution, symbolic solving, and AI-guided test synthesis.
//!
//! This library crate exposes [`Cli`], [`Commands`], and [`run_cli`] so that
//! integration tests can exercise CLI logic without spawning a subprocess.

pub mod doctor;
pub mod fuzz;
pub mod integrate;
pub mod mcp;

use apex_agent::{AgentCluster, OrchestratorConfig};
use apex_core::{
    config::ApexConfig,
    traits::{Instrumentor, LanguageRunner},
    types::{Language, SeedId, Target},
};
use apex_coverage::CoverageOracle;
use apex_fuzz::FuzzStrategy;
use apex_instrument::{
    CCoverageInstrumentor, JavaInstrumentor, JavaScriptInstrumentor, LlvmInstrumentor,
    PythonInstrumentor, RustCovInstrumentor, WasmInstrumentor,
};
use apex_lang::{CRunner, JavaRunner, JavaScriptRunner, KotlinRunner, PythonRunner, WasmRunner};
use apex_sandbox::{ProcessSandbox, PythonTestSandbox};
use clap::{Parser, Subcommand, ValueEnum};
use color_eyre::Result;
use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
    sync::Arc,
};
use tracing::{debug, info, warn};

// ---------------------------------------------------------------------------
// CLI definition
// ---------------------------------------------------------------------------

#[derive(Parser)]
#[command(
    name = "apex",
    about = "Autonomous Path EXploration — drives any repository to 100% branch coverage"
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,

    /// Path to apex.toml config file. Auto-discovered from CWD if not set.
    #[arg(long, global = true)]
    pub config: Option<PathBuf>,

    #[arg(long, global = true)]
    pub log_level: Option<String>,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Run APEX against a target repository.
    Run(RunArgs),
    /// Ratchet: fail if coverage drops below a threshold (CI gate).
    Ratchet(RatchetArgs),
    /// Check that all required external tools are installed.
    Doctor,
    /// Run security and bug detection analysis.
    Audit(AuditArgs),
    /// Build per-test branch index for intelligence commands.
    Index(IndexArgs),
    /// Find minimal test subset that maintains current coverage.
    TestOptimize(TestOptimizeArgs),
    /// Order tests by relevance to changed files.
    TestPrioritize(TestPrioritizeArgs),
    /// Detect semantically dead code via branch analysis.
    DeadCode(DeadCodeArgs),
    /// Runtime-prioritized lint findings.
    Lint(LintArgs),
    /// Behavioral diff between current and base branch.
    Diff(DiffArgs),
    /// Detect flaky tests via execution path divergence.
    FlakyDetect(FlakyDetectArgs),
    /// Exercised vs static complexity per function.
    Complexity(ComplexityArgs),
    /// Generate behavioral documentation from execution traces.
    Docs(DocsArgs),
    /// Map attack surface from entry-point reachability.
    AttackSurface(AttackSurfaceArgs),
    /// Verify all entry-point paths pass through auth checks.
    VerifyBoundaries(VerifyBoundariesArgs),
    /// CI gate: fail on unexpected behavioral changes vs base branch.
    RegressionCheck(RegressionCheckArgs),
    /// Assess change risk from branch coverage data.
    Risk(RiskArgs),
    /// Rank branches by execution frequency (hot paths).
    Hotpaths(HotpathsArgs),
    /// Discover invariants from branch execution patterns.
    Contracts(ContractsArgs),
    /// Aggregate deployment confidence score (0-100).
    DeployScore(DeployScoreArgs),
    /// Show per-language feature support matrix.
    Features(FeaturesArgs),
    /// Reverse-path reachability: find entry points that reach a given file:line.
    Reach(ReachArgs),
    /// Scan source code for leaked secrets (API keys, tokens, passwords).
    SecretScan(SecretScanArgs),
    /// Scan dependencies for license compliance violations.
    LicenseScan(LicenseScanArgs),
    /// Detect stale, always-on, or dead feature flags.
    FlagHygiene(FlagHygieneArgs),
    /// Detect breaking changes between two OpenAPI spec versions.
    ApiDiff(ApiDiffArgs),
    /// Trace data flow from input sources to output sinks.
    DataFlow(DataFlowArgs),
    /// Calculate change blast radius from branch index data.
    BlastRadius(BlastRadiusArgs),
    /// Export compliance evidence packages (ASVS, SSDF, STRIDE).
    ComplianceExport(ComplianceExportArgs),
    /// Compare OpenAPI spec against code to find undocumented or unimplemented endpoints.
    ApiCoverage(ApiCoverageArgs),
    /// Discover runtime service dependencies (HTTP, gRPC, MQ, DB) from code.
    ServiceMap(ServiceMapArgs),
    /// Analyze SQL migration scripts for unsafe operations.
    SchemaCheck(SchemaCheckArgs),
    /// Generate realistic test data from SQL schema files.
    TestData(TestDataArgs),
    /// Start MCP STDIO server for AI tool integration.
    Mcp,
    /// Write MCP server config for Claude Code, Cursor, or Windsurf.
    Integrate(integrate::IntegrateArgs),
}

#[derive(Parser, Clone)]
pub struct RunArgs {
    /// Path to the target repository.
    #[arg(long, short)]
    pub target: PathBuf,

    /// Programming language of the target.
    #[arg(long, short, value_enum)]
    pub lang: LangArg,

    /// Coverage target (0.0-1.0). Overrides config `coverage.target`.
    #[arg(long)]
    pub coverage_target: Option<f64>,

    /// Exploration strategy: baseline | fuzz | concolic | all
    #[arg(long, default_value = "baseline")]
    pub strategy: String,

    /// Output directory for generated tests.
    #[arg(long)]
    pub output: Option<PathBuf>,

    /// Output format: text (human-readable) or json (machine-readable).
    #[arg(long)]
    pub output_format: Option<OutputFormat>,

    /// Skip dependency installation.
    #[arg(long)]
    pub no_install: bool,

    /// Maximum concolic rounds. Overrides config `concolic.max_rounds`.
    #[arg(long)]
    pub rounds: Option<usize>,

    /// Maximum fuzzer iterations. Overrides config `fuzz.stall_iterations * mutations_per_input`.
    #[arg(long)]
    pub fuzz_iters: Option<usize>,

    /// Command to run the compiled binary for fuzzing (fuzz / all strategy).
    /// Example: `--fuzz-cmd ./target_instrumented`
    #[arg(long, value_delimiter = ' ')]
    pub fuzz_cmd: Vec<String>,
}

#[derive(Parser)]
pub struct RatchetArgs {
    #[arg(long, short)]
    pub target: PathBuf,

    #[arg(long, short, value_enum)]
    pub lang: LangArg,

    /// Minimum coverage threshold. Overrides config `coverage.min_ratchet`.
    #[arg(long)]
    pub min_coverage: Option<f64>,
}

#[derive(Parser)]
pub struct AuditArgs {
    /// Path to the target repository.
    #[arg(long, short)]
    pub target: PathBuf,

    /// Programming language of the target.
    #[arg(long, short, value_enum)]
    pub lang: LangArg,

    /// Comma-separated list of detectors to run.
    #[arg(long, value_delimiter = ',')]
    pub detectors: Option<Vec<String>>,

    /// Minimum severity to report.
    #[arg(long, default_value = "low")]
    pub severity_threshold: String,

    /// Output format: text (human-readable) or json (machine-readable).
    #[arg(long, default_value = "text")]
    pub output_format: OutputFormat,

    /// Write output to a file instead of stdout.
    #[arg(long, short)]
    pub output: Option<PathBuf>,
}

#[derive(Parser)]
pub struct IndexArgs {
    /// Path to the target repository.
    #[arg(long, short)]
    pub target: PathBuf,

    /// Programming language of the target.
    #[arg(long, short, value_enum)]
    pub lang: LangArg,

    /// Number of parallel test runners.
    #[arg(long, default_value = "4")]
    pub parallel: usize,
}

#[derive(Parser)]
pub struct TestOptimizeArgs {
    /// Path to the target repository (to find .apex/index.json).
    #[arg(long, short)]
    pub target: PathBuf,

    /// Output format.
    #[arg(long, default_value = "text")]
    pub output_format: OutputFormat,
}

#[derive(Parser)]
pub struct TestPrioritizeArgs {
    /// Path to the target repository.
    #[arg(long, short)]
    pub target: PathBuf,

    /// Comma-separated list of changed files (relative to target root).
    #[arg(long, value_delimiter = ',')]
    pub changed_files: Vec<String>,
}

#[derive(Parser)]
pub struct DeadCodeArgs {
    /// Path to the target repository.
    #[arg(long, short)]
    pub target: PathBuf,

    /// Output format.
    #[arg(long, default_value = "text")]
    pub output_format: OutputFormat,
}

#[derive(Parser)]
pub struct LintArgs {
    /// Path to the target repository.
    #[arg(long, short)]
    pub target: PathBuf,

    /// Programming language of the target.
    #[arg(long, short, value_enum)]
    pub lang: LangArg,

    /// Comma-separated list of detectors to run (default: all enabled detectors).
    #[arg(long, value_delimiter = ',')]
    pub detectors: Option<Vec<String>>,

    /// Output format.
    #[arg(long, default_value = "text")]
    pub output_format: OutputFormat,
}

#[derive(Parser)]
pub struct DiffArgs {
    /// Path to the target repository.
    #[arg(long, short)]
    pub target: PathBuf,

    /// Programming language of the target.
    #[arg(long, short, value_enum)]
    pub lang: LangArg,

    /// Git ref to compare against (e.g., main, HEAD~1).
    #[arg(long)]
    pub base: String,

    /// Exit with code 1 on unexpected behavioral changes.
    #[arg(long)]
    pub strict: bool,
}

#[derive(Parser)]
pub struct FlakyDetectArgs {
    /// Path to the target repository.
    #[arg(long, short)]
    pub target: PathBuf,

    /// Programming language of the target.
    #[arg(long, short, value_enum)]
    pub lang: LangArg,

    /// Number of repetitions per test (default 5).
    #[arg(long, default_value = "5")]
    pub runs: usize,

    /// Number of parallel test runners.
    #[arg(long, default_value = "4")]
    pub parallel: usize,

    /// Output format.
    #[arg(long, default_value = "text")]
    pub output_format: OutputFormat,
}

#[derive(Parser)]
pub struct ComplexityArgs {
    /// Path to the target repository.
    #[arg(long, short)]
    pub target: PathBuf,

    /// Output format.
    #[arg(long, default_value = "text")]
    pub output_format: OutputFormat,
}

#[derive(Parser)]
pub struct DocsArgs {
    /// Path to the target repository.
    #[arg(long, short)]
    pub target: PathBuf,

    /// Write output to a file instead of stdout.
    #[arg(long, short)]
    pub output: Option<PathBuf>,

    /// Output format: text (markdown) or json.
    #[arg(long, default_value = "text")]
    pub output_format: OutputFormat,
}

#[derive(Parser)]
pub struct AttackSurfaceArgs {
    /// Path to the target repository.
    #[arg(long, short)]
    pub target: PathBuf,

    /// Programming language of the target.
    #[arg(long, short, value_enum)]
    pub lang: LangArg,

    /// Pattern matching entry-point tests (e.g., "test_api", "test_http").
    #[arg(long)]
    pub entry_pattern: String,

    /// Output format.
    #[arg(long, default_value = "text")]
    pub output_format: OutputFormat,
}

#[derive(Parser)]
pub struct VerifyBoundariesArgs {
    /// Path to the target repository.
    #[arg(long, short)]
    pub target: PathBuf,

    /// Programming language of the target.
    #[arg(long, short, value_enum)]
    pub lang: LangArg,

    /// Pattern matching entry-point tests (e.g., "test_api").
    #[arg(long)]
    pub entry_pattern: String,

    /// Substring to match auth-check lines in source (e.g., "check_auth", "@login_required").
    #[arg(long)]
    pub auth_checks: String,

    /// Exit with code 1 if any unprotected paths are found.
    #[arg(long)]
    pub strict: bool,

    /// Output format.
    #[arg(long, default_value = "text")]
    pub output_format: OutputFormat,
}

#[derive(Parser)]
pub struct RegressionCheckArgs {
    /// Path to the target repository.
    #[arg(long, short)]
    pub target: PathBuf,

    /// Programming language of the target.
    #[arg(long, short, value_enum)]
    pub lang: LangArg,

    /// Git ref to compare against (e.g., main, HEAD~1).
    #[arg(long)]
    pub base: String,

    /// Comma-separated list of test patterns to ignore (e.g., flaky tests).
    #[arg(long, value_delimiter = ',')]
    pub allow: Vec<String>,

    /// Output format.
    #[arg(long, default_value = "text")]
    pub output_format: OutputFormat,
}

#[derive(Parser)]
pub struct RiskArgs {
    /// Path to the target repository.
    #[arg(long, short)]
    pub target: PathBuf,

    /// Comma-separated list of changed files (relative to target root).
    #[arg(long, value_delimiter = ',')]
    pub changed_files: Vec<String>,

    /// Output format.
    #[arg(long, default_value = "text")]
    pub output_format: OutputFormat,
}

#[derive(Parser)]
pub struct HotpathsArgs {
    /// Path to the target repository.
    #[arg(long, short)]
    pub target: PathBuf,

    /// Number of top hot paths to show.
    #[arg(long, default_value = "20")]
    pub top: usize,

    /// Output format.
    #[arg(long, default_value = "text")]
    pub output_format: OutputFormat,
}

#[derive(Parser)]
pub struct ContractsArgs {
    /// Path to the target repository.
    #[arg(long, short)]
    pub target: PathBuf,

    /// Output format.
    #[arg(long, default_value = "text")]
    pub output_format: OutputFormat,
}

#[derive(Parser)]
pub struct DeployScoreArgs {
    /// Path to the target repository.
    #[arg(long, short)]
    pub target: PathBuf,

    /// Number of detector findings (from `apex audit`).
    #[arg(long, default_value = "0")]
    pub detector_findings: usize,

    /// Number of critical findings (from `apex audit`).
    #[arg(long, default_value = "0")]
    pub critical_findings: usize,

    /// Output format.
    #[arg(long, default_value = "text")]
    pub output_format: OutputFormat,
}

#[derive(Parser)]
pub struct FeaturesArgs {
    /// Show features for a specific language (omit to show all).
    #[arg(long, short, value_enum)]
    pub lang: Option<LangArg>,

    /// Output format: text (ASCII table) or json.
    #[arg(long, default_value = "text")]
    pub output_format: OutputFormat,
}

#[derive(Parser)]
pub struct ReachArgs {
    /// Target in "file:line" format.
    #[arg(long)]
    pub target: String,

    /// Programming language of the target.
    #[arg(long, short, value_enum)]
    pub lang: LangArg,

    /// Granularity: function, block, or line.
    #[arg(long, default_value = "function")]
    pub granularity: String,

    /// Filter to entry point kind: test, http, main, api, cli.
    #[arg(long)]
    pub entry_kind: Option<String>,
}

#[derive(Parser)]
pub struct SecretScanArgs {
    #[arg(long, short)]
    pub target: PathBuf,
    #[arg(long, short, value_enum)]
    pub lang: LangArg,
    #[arg(long, default_value = "4.5")]
    pub entropy_threshold: f64,
    #[arg(long, default_value = "text")]
    pub output_format: OutputFormat,
}

#[derive(Parser)]
pub struct LicenseScanArgs {
    #[arg(long, short)]
    pub target: PathBuf,
    #[arg(long, short, value_enum)]
    pub lang: LangArg,
    #[arg(long, default_value = "enterprise")]
    pub policy: String,
    #[arg(long, default_value = "text")]
    pub output_format: OutputFormat,
}

#[derive(Parser)]
pub struct FlagHygieneArgs {
    #[arg(long, short)]
    pub target: PathBuf,
    #[arg(long, short, value_enum)]
    pub lang: LangArg,
    #[arg(long, default_value = "90")]
    pub max_age: u64,
    #[arg(long, default_value = "text")]
    pub output_format: OutputFormat,
}

#[derive(Parser)]
pub struct ApiDiffArgs {
    /// Path to the old/baseline OpenAPI spec (JSON).
    #[arg(long)]
    pub old: PathBuf,
    /// Path to the new/current OpenAPI spec (JSON).
    #[arg(long)]
    pub new: PathBuf,
    #[arg(long, default_value = "text")]
    pub output_format: OutputFormat,
}

#[derive(Parser)]
pub struct DataFlowArgs {
    #[arg(long, short)]
    pub target: PathBuf,
    #[arg(long, short, value_enum)]
    pub lang: LangArg,
    #[arg(long, default_value = "10")]
    pub max_depth: usize,
    #[arg(long, default_value = "text")]
    pub output_format: OutputFormat,
}

#[derive(Parser)]
pub struct BlastRadiusArgs {
    #[arg(long, short)]
    pub target: PathBuf,
    #[arg(long, short, value_enum)]
    pub lang: LangArg,
    #[arg(long, value_delimiter = ',')]
    pub changed_files: Vec<String>,
    #[arg(long, default_value = "text")]
    pub output_format: OutputFormat,
}

#[derive(Parser)]
pub struct ComplianceExportArgs {
    #[arg(long, short)]
    pub target: PathBuf,
    #[arg(long, short, value_enum)]
    pub lang: LangArg,
    #[arg(long, default_value = "all")]
    pub framework: String,
    #[arg(long, default_value = "L1")]
    pub level: String,
    #[arg(long, default_value = "text")]
    pub output_format: OutputFormat,
    #[arg(long, short)]
    pub output: Option<PathBuf>,
}

#[derive(Parser)]
pub struct ApiCoverageArgs {
    #[arg(long)]
    pub spec: PathBuf,
    #[arg(long, short)]
    pub target: PathBuf,
    #[arg(long, short, value_enum)]
    pub lang: LangArg,
    #[arg(long, default_value = "text")]
    pub output_format: OutputFormat,
}

#[derive(Parser)]
pub struct ServiceMapArgs {
    #[arg(long, short)]
    pub target: PathBuf,
    #[arg(long, short, value_enum)]
    pub lang: LangArg,
    #[arg(long, default_value = "text")]
    pub output_format: OutputFormat,
}

#[derive(Parser)]
pub struct SchemaCheckArgs {
    #[arg(long, short)]
    pub migration: PathBuf,
    #[arg(long, default_value = "text")]
    pub output_format: OutputFormat,
}

#[derive(Parser)]
pub struct TestDataArgs {
    #[arg(long, short)]
    pub schema: PathBuf,
    #[arg(long, default_value = "100")]
    pub rows: usize,
    #[arg(long, default_value = "text")]
    pub output_format: OutputFormat,
}

#[derive(Clone, Copy, ValueEnum)]
pub enum OutputFormat {
    Text,
    Json,
}

#[derive(Clone, Copy, ValueEnum)]
pub enum LangArg {
    Python,
    Js,
    Java,
    C,
    Rust,
    Wasm,
    Ruby,
    Kotlin,
    Go,
    Cpp,
    Swift,
    CSharp,
}

impl From<LangArg> for Language {
    fn from(l: LangArg) -> Self {
        match l {
            LangArg::Python => Language::Python,
            LangArg::Js => Language::JavaScript,
            LangArg::Java => Language::Java,
            LangArg::C => Language::C,
            LangArg::Rust => Language::Rust,
            LangArg::Wasm => Language::Wasm,
            LangArg::Ruby => Language::Ruby,
            LangArg::Kotlin => Language::Kotlin,
            LangArg::Go => Language::Go,
            LangArg::Cpp => Language::Cpp,
            LangArg::Swift => Language::Swift,
            LangArg::CSharp => Language::CSharp,
        }
    }
}

// ---------------------------------------------------------------------------
// Public entry point — dispatches CLI commands
// ---------------------------------------------------------------------------

pub async fn run_cli(cli: Cli, cfg: &ApexConfig) -> Result<()> {
    match cli.command {
        Commands::Run(args) => run(args, cfg).await,
        Commands::Ratchet(args) => ratchet(args, cfg).await,
        Commands::Doctor => doctor::run_doctor().await,
        Commands::Audit(args) => run_audit(args, cfg).await,
        Commands::Index(args) => run_index(args).await,
        Commands::TestOptimize(args) => run_test_optimize(args).await,
        Commands::TestPrioritize(args) => run_test_prioritize(args).await,
        Commands::DeadCode(args) => run_dead_code(args).await,
        Commands::Lint(args) => run_lint(args, cfg).await,
        Commands::Diff(args) => run_diff(args).await,
        Commands::FlakyDetect(args) => run_flaky_detect(args).await,
        Commands::Complexity(args) => run_complexity(args).await,
        Commands::Docs(args) => run_docs(args).await,
        Commands::AttackSurface(args) => run_attack_surface(args).await,
        Commands::VerifyBoundaries(args) => run_verify_boundaries(args).await,
        Commands::RegressionCheck(args) => run_regression_check(args).await,
        Commands::Risk(args) => run_risk(args).await,
        Commands::Hotpaths(args) => run_hotpaths(args).await,
        Commands::Contracts(args) => run_contracts(args).await,
        Commands::DeployScore(args) => run_deploy_score(args).await,
        Commands::Features(args) => run_features(args),
        Commands::Reach(args) => run_reach(args).await,
        Commands::SecretScan(args) => run_secret_scan(args).await,
        Commands::LicenseScan(args) => run_license_scan(args).await,
        Commands::FlagHygiene(args) => run_flag_hygiene(args).await,
        Commands::ApiDiff(args) => run_api_diff(args).await,
        Commands::DataFlow(args) => run_data_flow(args).await,
        Commands::BlastRadius(args) => run_blast_radius(args).await,
        Commands::ComplianceExport(args) => run_compliance_export(args, cfg).await,
        Commands::ApiCoverage(args) => run_api_coverage(args).await,
        Commands::ServiceMap(args) => run_service_map(args).await,
        Commands::SchemaCheck(args) => run_schema_check(args).await,
        Commands::TestData(args) => run_test_data(args).await,
        Commands::Mcp => mcp::run_mcp().await,
        Commands::Integrate(args) => integrate::run_integrate(args).await,
    }
}

// ---------------------------------------------------------------------------
// `apex run`
// ---------------------------------------------------------------------------

async fn run(args: RunArgs, cfg: &ApexConfig) -> Result<()> {
    let lang: Language = args.lang.into();
    let target_path = args.target.canonicalize()?;

    // Resolve effective values: CLI > config > defaults
    let coverage_target = args.coverage_target.unwrap_or(cfg.coverage.target);
    let rounds = args.rounds.unwrap_or(cfg.concolic.max_rounds);
    let fuzz_iters = args.fuzz_iters.unwrap_or(10_000);
    let output_format = args
        .output_format
        .unwrap_or(match cfg.logging.format.as_str() {
            "json" => OutputFormat::Json,
            _ => OutputFormat::Text,
        });

    info!(target = %target_path.display(), lang = %lang, strategy = %args.strategy, "starting APEX");

    // 0. Preflight check — review project structure before doing anything
    let preflight = preflight_check(lang, &target_path);
    if let Ok(ref info) = preflight {
        if !info.missing_tools.is_empty() {
            for tool in &info.missing_tools {
                eprintln!("  \x1b[33m⚠\x1b[0m Missing tool: {tool}");
            }
        }
        for (tool, ver) in &info.available_tools {
            info!(tool, ver, "tool available");
        }
        for warning in &info.warnings {
            eprintln!("  \x1b[33m⚠\x1b[0m {warning}");
        }
        if let Some(ref bs) = info.build_system {
            info!(build_system = bs, "detected build system");
        }
        if let Some(ref tf) = info.test_framework {
            info!(test_framework = tf, "detected test framework");
        }
        // Log summary
        let summary = info.summary();
        if !summary.is_empty() {
            info!(summary, "preflight check complete");
        }
    }

    // 1. Install deps
    if !args.no_install {
        install_deps(lang, &target_path).await?;
    }

    // 2. Instrument -> populate oracle
    let oracle = Arc::new(CoverageOracle::new());
    let instrumented = instrument(lang, &target_path, &oracle).await?;

    // 2b. For fuzz/driller/all/agent strategies on Rust targets, compile binary
    //     with SanitizerCoverage so the SHM feedback loop works.
    let needs_sancov = matches!(args.strategy.as_str(), "fuzz" | "driller" | "all" | "agent");
    if needs_sancov && lang == Language::Rust && args.fuzz_cmd.is_empty() {
        info!("compiling Rust binary with SanitizerCoverage for fuzz feedback");
        let shim = apex_sandbox::shim::ensure_compiled().ok();
        apex_lang::rust_lang::build_with_sancov(&target_path, Some("apex_target"), shim.as_deref())
            .await?;
    }

    // 3. Strategy dispatch
    match args.strategy.as_str() {
        "fuzz" => {
            let cmd = fuzz_command(&args, &target_path);
            fuzz::run_fuzz_strategy(
                Arc::clone(&oracle),
                &instrumented,
                coverage_target,
                fuzz_iters,
                cmd,
                cfg,
            )
            .await?;
        }
        "concolic" => {
            if instrumented.target.language != Language::Python {
                warn!("concolic strategy is currently Python-only; switch to --lang python");
            } else {
                run_concolic_strategy(
                    Arc::clone(&oracle),
                    &instrumented,
                    coverage_target,
                    rounds,
                    args.output,
                )
                .await?;
            }
        }
        "driller" => {
            // Driller runs through the combined strategy path (fuzz + concolic + driller).
            info!("driller strategy: running SMT-driven path exploration");
            let cmd = fuzz_command(&args, &target_path);
            fuzz::run_all_strategies(
                Arc::clone(&oracle),
                &instrumented,
                coverage_target,
                fuzz_iters,
                rounds,
                args.output.clone(),
                cmd,
                cfg,
            )
            .await?;
        }
        // "all", "agent", and any unknown strategy use the AgentCluster orchestrator.
        _ => {
            run_agent_cluster(
                Arc::clone(&oracle),
                &instrumented,
                coverage_target,
                fuzz_iters,
                &args,
                cfg,
            )
            .await?;
        }
    }

    // 4. Run detection pipeline for agent/all strategies (all output formats)
    let uses_agent = matches!(args.strategy.as_str(), "agent" | "all")
        || !matches!(args.strategy.as_str(), "fuzz" | "concolic" | "driller");
    let analysis = if uses_agent {
        let detect_cfg = apex_detect::DetectConfig::default();
        let file_source_cache = build_source_cache(&target_path, lang);

        // Build CPG for Python projects (other languages: TODO)
        let cpg = if lang == Language::Python {
            let mut combined_cpg = apex_cpg::Cpg::new();
            for (path, source) in &file_source_cache {
                let file_cpg =
                    apex_cpg::builder::build_python_cpg(source, &path.display().to_string());
                combined_cpg.merge(file_cpg);
            }
            if combined_cpg.node_count() > 0 {
                Some(Arc::new(combined_cpg))
            } else {
                None
            }
        } else {
            None
        };

        // Build call graph for reverse path analysis
        let reach_graph = apex_reach::extractors::build_call_graph(&file_source_cache, lang);
        let reverse_path_engine = if reach_graph.node_count() > 0 {
            Some(Arc::new(apex_reach::ReversePathEngine::new(reach_graph)))
        } else {
            None
        };

        let detect_ctx = apex_detect::AnalysisContext {
            target_root: target_path.clone(),
            language: lang,
            oracle: Arc::clone(&oracle),
            file_paths: instrumented.file_paths.clone(),
            known_bugs: vec![],
            source_cache: file_source_cache,
            fuzz_corpus: None,
            config: detect_cfg.clone(),
            runner: Arc::new(apex_core::command::RealCommandRunner),
            cpg,
            threat_model: cfg.threat_model.clone(),
            reverse_path_engine,
        };

        let pipeline = apex_detect::DetectorPipeline::from_config(&detect_cfg, lang);
        let detection_report = pipeline.run_all(&detect_ctx).await;

        // Compound analysis: discover artifacts and run applicable analyzers
        let compound = if cfg.analyze.enabled {
            use apex_detect::analyzer_registry;

            let artifacts = analyzer_registry::discover_artifacts(&target_path);
            let mut analyzers = analyzer_registry::applicable_analyzers(&artifacts, lang);

            // Filter out skipped analyzers
            if !cfg.analyze.skip.is_empty() {
                analyzers.retain(|a| !cfg.analyze.skip.contains(&a.name.to_string()));
            }

            info!(count = analyzers.len(), "running compound analyzers");

            let analyzer_results = analyzer_registry::run_applicable_analyzers(
                &target_path,
                lang,
                &detect_ctx.source_cache,
                &artifacts,
                &analyzers,
            )
            .await;

            apex_detect::compound_report::CompoundReport::new(
                detection_report,
                analyzer_results,
                artifacts,
            )
        } else {
            apex_detect::compound_report::CompoundReport::new(
                detection_report,
                vec![],
                apex_detect::analyzer_registry::Artifacts::default(),
            )
        };

        Some(compound)
    } else {
        None
    };

    // 5. Output gap report
    match output_format {
        OutputFormat::Json if uses_agent => {
            let mut report = build_agent_report(&oracle, &instrumented.file_paths, &target_path);

            if let Some(ref compound) = analysis {
                report.findings =
                    Some(serde_json::to_value(&compound.detection.findings).unwrap_or_default());
                report.security_summary = Some(
                    serde_json::to_value(compound.detection.security_summary()).unwrap_or_default(),
                );
                // Include compound analysis data
                report.compound_analysis = Some(serde_json::to_value(compound).unwrap_or_default());
            }

            match serde_json::to_string_pretty(&report) {
                Ok(json) => println!("{json}"),
                Err(e) => eprintln!("{{\"error\": \"failed to serialize report: {e}\"}}"),
            }
        }
        OutputFormat::Json => {
            print_json_gap_report(&oracle, &instrumented.file_paths, &target_path);
        }
        OutputFormat::Text => {
            print_gap_report(&oracle, &instrumented.file_paths, &target_path);
            if let Some(ref compound) = analysis {
                if !compound.detection.findings.is_empty() {
                    println!("\nFindings ({}):", compound.detection.findings.len());
                    for finding in &compound.detection.findings {
                        println!("  - [{:?}] {}", finding.severity, finding.title);
                    }
                }

                // Print analyzer summary
                if !compound.analyzers.is_empty() {
                    println!("\nAnalyzers ({}):", compound.analyzers.len());
                    for result in &compound.analyzers {
                        let status_label = match &result.status {
                            apex_detect::analyzer_registry::AnalyzerStatus::Ok => "OK".to_string(),
                            apex_detect::analyzer_registry::AnalyzerStatus::Skipped(reason) => {
                                format!("SKIP ({})", reason)
                            }
                            apex_detect::analyzer_registry::AnalyzerStatus::Failed(err) => {
                                format!("FAIL ({})", err)
                            }
                        };
                        println!(
                            "  {:>4}  {} \u{2014} {} ({}ms)",
                            status_label, result.name, result.description, result.duration_ms
                        );
                    }
                }
            }
        }
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Agent cluster orchestration
// ---------------------------------------------------------------------------

async fn run_agent_cluster(
    oracle: Arc<CoverageOracle>,
    instrumented: &apex_core::types::InstrumentedTarget,
    coverage_target: f64,
    _fuzz_iters: usize,
    args: &RunArgs,
    cfg: &ApexConfig,
) -> Result<()> {
    let lang = instrumented.target.language;
    let target_root = &instrumented.target.root;

    // Build sandbox appropriate for the language.
    let sandbox: Arc<dyn apex_core::traits::Sandbox> = match lang {
        Language::Python => {
            let file_paths = Arc::new(instrumented.file_paths.clone());
            Arc::new(
                PythonTestSandbox::new(Arc::clone(&oracle), file_paths, target_root.clone())
                    .with_timeout(cfg.sandbox.process_timeout_ms),
            )
        }
        _ => {
            let cmd = fuzz_command(args, target_root);
            let branch_index: Vec<apex_core::types::BranchId> = oracle
                .uncovered_branches()
                .into_iter()
                .chain(instrumented.executed_branch_ids.iter().cloned())
                .collect();

            let mut sandbox = ProcessSandbox::new(lang, target_root.clone(), cmd);
            sandbox = sandbox.with_timeout(cfg.sandbox.process_timeout_ms);
            sandbox = sandbox.with_coverage(Arc::clone(&oracle), branch_index);

            if let Ok(shim_path) = apex_sandbox::shim::ensure_compiled() {
                sandbox = sandbox.with_shim(shim_path);
            }
            Arc::new(sandbox)
        }
    };

    // Create fuzz strategy and seed it.
    let fuzz_strategy = FuzzStrategy::new(Arc::clone(&oracle));
    let _ = fuzz_strategy.seed_corpus([vec![0u8]]);

    // Build the cluster.
    let mut cluster = AgentCluster::new(Arc::clone(&oracle), sandbox, instrumented.target.clone());
    cluster = cluster.with_strategy(Box::new(fuzz_strategy));

    // For Python, also add the concolic strategy.
    if lang == Language::Python {
        use apex_concolic::PythonConcolicStrategy;
        let file_paths = Arc::new(instrumented.file_paths.clone());
        let concolic = PythonConcolicStrategy::new(
            Arc::clone(&oracle),
            file_paths,
            target_root.clone(),
            instrumented.target.test_command.clone(),
        );
        cluster = cluster.with_strategy(Box::new(concolic));
    }

    cluster = cluster.with_file_paths(instrumented.file_paths.clone());

    // Configure from ApexConfig.
    let orch_config = OrchestratorConfig {
        coverage_target,
        // Use the explicitly configured deadline when set; otherwise fall back to a
        // 30-minute cap (1800 s).  The old per-iteration formula
        //   process_timeout_ms * fuzz_iters / 1000
        // produces nonsensical values (e.g. 10_000 ms × 10_000 iters = 100_000 s ≈ 28 h)
        // for typical defaults and was never the user's intent.
        deadline_secs: cfg.agent.deadline_secs.or(Some(1800)),
        stall_threshold: cfg.fuzz.stall_iterations as u64,
    };
    cluster = cluster.with_config(orch_config);

    info!(
        strategies = cluster.strategy_count(),
        lang = %lang,
        "starting agent cluster"
    );
    cluster.run().await?;

    let summary = cluster.bug_summary();
    if summary.total > 0 {
        info!(bugs = summary.total, "bugs found during exploration");
    }

    Ok(())
}

/// Determine fuzzer command: explicit `--fuzz-cmd` or default binary path.
fn fuzz_command(args: &RunArgs, target_path: &std::path::Path) -> Vec<String> {
    if !args.fuzz_cmd.is_empty() {
        return args.fuzz_cmd.clone();
    }
    // Default: look for an `apex_target` binary in target/debug/ first (cargo build output),
    // then fall back to repo root.
    let cargo_bin = target_path.join("target/debug/apex_target");
    if cargo_bin.exists() {
        return vec![cargo_bin.to_string_lossy().to_string()];
    }
    vec![target_path
        .join("apex_target")
        .to_string_lossy()
        .to_string()]
}

// ---------------------------------------------------------------------------
// Helpers shared between `run` and `ratchet`
// ---------------------------------------------------------------------------

fn preflight_check(
    lang: Language,
    target: &std::path::Path,
) -> std::result::Result<apex_core::traits::PreflightInfo, apex_core::error::ApexError> {
    use apex_core::traits::LanguageRunner;
    let runner_check = |r: &dyn apex_core::traits::LanguageRunner| r.preflight_check(target);
    match lang {
        Language::Python => runner_check(&PythonRunner::new()),
        Language::JavaScript => runner_check(&JavaScriptRunner::new()),
        Language::Java => runner_check(&JavaRunner::new()),
        Language::Kotlin => runner_check(&KotlinRunner::new()),
        Language::Go => runner_check(&apex_lang::go::GoRunner::new()),
        Language::Swift => runner_check(&apex_lang::swift::SwiftRunner::new()),
        Language::CSharp => runner_check(&apex_lang::csharp::CSharpRunner::new()),
        Language::Ruby => runner_check(&apex_lang::ruby::RubyRunner::new()),
        Language::C => runner_check(&CRunner::new()),
        Language::Cpp => runner_check(&apex_lang::cpp::CppRunner::new()),
        Language::Rust => runner_check(&apex_lang::rust_lang::RustRunner::new()),
        Language::Wasm => Ok(apex_core::traits::PreflightInfo::default()),
    }
}

async fn install_deps(lang: Language, target: &std::path::Path) -> Result<()> {
    match lang {
        Language::Python => {
            let runner = PythonRunner::new();
            if !runner.detect(target) {
                warn!("no Python project files detected; proceeding anyway");
            }
            runner.install_deps(target).await?;
        }
        Language::JavaScript => {
            let runner = JavaScriptRunner::new();
            if !runner.detect(target) {
                warn!("no package.json found; proceeding anyway");
            }
            runner.install_deps(target).await?;
        }
        Language::Java => {
            let runner = JavaRunner::new();
            if !runner.detect(target) {
                warn!("no pom.xml / build.gradle found; proceeding anyway");
            }
            runner.install_deps(target).await?;
        }
        Language::C => {
            let runner = CRunner::new();
            runner.install_deps(target).await?;
        }
        Language::Rust => {
            // Nothing to install for Rust.
        }
        Language::Wasm => {
            let runner = WasmRunner::new();
            runner.install_deps(target).await?;
        }
        Language::Ruby => {
            let runner = apex_lang::ruby::RubyRunner::new();
            runner.install_deps(target).await?;
        }
        Language::Kotlin => {
            let runner = KotlinRunner::new();
            if !runner.detect(target) {
                warn!("no build.gradle.kts or .kt files found; proceeding anyway");
            }
            runner.install_deps(target).await?;
        }
        Language::Go => {
            let runner = apex_lang::go::GoRunner::new();
            runner.install_deps(target).await?;
        }
        Language::Cpp => {
            let runner = apex_lang::cpp::CppRunner::new();
            runner.install_deps(target).await?;
        }
        Language::Swift => {
            let runner = apex_lang::swift::SwiftRunner::new();
            runner.install_deps(target).await?;
        }
        Language::CSharp => {
            let runner = apex_lang::csharp::CSharpRunner::new();
            runner.install_deps(target).await?;
        }
    }
    Ok(())
}

async fn instrument(
    lang: Language,
    target_path: &std::path::Path,
    oracle: &Arc<CoverageOracle>,
) -> Result<apex_core::types::InstrumentedTarget> {
    let target = Target {
        root: target_path.to_path_buf(),
        language: lang,
        test_command: Vec::new(),
    };

    let instrumented = match lang {
        Language::Python => PythonInstrumentor::new().instrument(&target).await?,
        Language::JavaScript => JavaScriptInstrumentor::new().instrument(&target).await?,
        Language::Java => JavaInstrumentor::new().instrument(&target).await?,
        Language::Rust => {
            // Use cargo-llvm-cov for Rust targets (no binary required).
            RustCovInstrumentor::new().instrument(&target).await?
        }
        Language::C => {
            // Try LLVM SanitizerCoverage first (best precision, needs feature flag).
            let result = LlvmInstrumentor::new().instrument(&target).await?;
            if !result.branch_ids.is_empty() {
                result
            } else {
                // Fall back to gcov: compile with --coverage, run, parse .gcov files.
                info!(
                    "LLVM instrumentation returned no branches; \
                     falling back to gcov coverage"
                );
                CCoverageInstrumentor::new().instrument(&target).await?
            }
        }
        Language::Wasm => WasmInstrumentor::new().instrument(&target).await?,
        Language::Ruby => {
            apex_instrument::ruby::RubyInstrumentor::new()
                .instrument(&target)
                .await?
        }
        Language::Kotlin => {
            // Kotlin reuses Java instrumentor (JaCoCo)
            JavaInstrumentor::new().instrument(&target).await?
        }
        Language::Go => {
            apex_instrument::go::GoInstrumentor::new()
                .instrument(&target)
                .await?
        }
        Language::Cpp => {
            apex_instrument::c_coverage::CCoverageInstrumentor::new()
                .instrument(&target)
                .await?
        }
        Language::Swift => {
            apex_instrument::swift::SwiftInstrumentor::new()
                .instrument(&target)
                .await?
        }
        Language::CSharp => {
            apex_instrument::csharp::CSharpInstrumentor::new()
                .instrument(&target)
                .await?
        }
    };

    oracle.register_branches(instrumented.branch_ids.iter().cloned());

    let baseline_seed = SeedId::new();
    for branch in &instrumented.executed_branch_ids {
        oracle.mark_covered(branch, baseline_seed);
    }

    info!(
        total = oracle.total_count(),
        covered = oracle.covered_count(),
        coverage = %format!("{:.1}%", oracle.coverage_percent()),
        "baseline coverage after instrumentation"
    );

    Ok(instrumented)
}

// ---------------------------------------------------------------------------
// Gap report printer (Phase 1 spec: includes source line)
// ---------------------------------------------------------------------------

fn print_gap_report(
    oracle: &CoverageOracle,
    file_paths: &HashMap<u64, PathBuf>,
    target_root: &std::path::Path,
) {
    let covered = oracle.covered_count();
    let total = oracle.total_count();
    let pct = oracle.coverage_percent();

    println!("\nCoverage: {covered}/{total} branches ({pct:.1}%)");

    let uncovered = oracle.uncovered_branches();
    if uncovered.is_empty() {
        println!("(none — full branch coverage!)");
        return;
    }

    // Cache file contents for source-line lookup.
    let mut file_cache: HashMap<u64, Vec<String>> = HashMap::new();

    println!("\nUncovered branches ({}):", uncovered.len());
    for branch in &uncovered {
        let dir = if branch.direction == 0 {
            "true-branch"
        } else {
            "false-branch"
        };
        let rel_path = file_paths
            .get(&branch.file_id)
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| format!("<{:016x}>", branch.file_id));

        // Look up the source line.
        let src_line = file_paths.get(&branch.file_id).and_then(|rel| {
            let lines = file_cache.entry(branch.file_id).or_insert_with(|| {
                std::fs::read_to_string(target_root.join(rel))
                    .map(|s| s.lines().map(String::from).collect())
                    .unwrap_or_default()
            });
            let idx = branch.line.saturating_sub(1) as usize;
            lines.get(idx).cloned()
        });

        if let Some(line) = src_line {
            println!("  {}:{}  {}  [{}]", rel_path, branch.line, line.trim(), dir);
        } else {
            println!("  {}:{}  [{}]", rel_path, branch.line, dir);
        }
    }
    println!();
}

// ---------------------------------------------------------------------------
// JSON gap report (consumed by Claude Code / CI tooling)
// ---------------------------------------------------------------------------

fn print_json_gap_report(
    oracle: &CoverageOracle,
    file_paths: &HashMap<u64, PathBuf>,
    target_root: &std::path::Path,
) {
    let uncovered = oracle.uncovered_branches();
    let mut file_cache: HashMap<u64, Vec<String>> = HashMap::new();

    let branches: Vec<serde_json::Value> = uncovered
        .iter()
        .map(|b| {
            let rel = file_paths
                .get(&b.file_id)
                .map(|p| p.display().to_string())
                .unwrap_or_else(|| format!("{:016x}", b.file_id));

            let src = file_paths.get(&b.file_id).and_then(|rel_path| {
                let lines = file_cache.entry(b.file_id).or_insert_with(|| {
                    std::fs::read_to_string(target_root.join(rel_path))
                        .map(|s| s.lines().map(String::from).collect())
                        .unwrap_or_default()
                });
                lines
                    .get(b.line.saturating_sub(1) as usize)
                    .map(|l| l.trim().to_string())
            });

            let mut obj = serde_json::json!({
                "file": rel,
                "line": b.line,
                "direction": if b.direction == 0 { "true" } else { "false" },
            });
            if let Some(s) = src {
                obj["source"] = serde_json::Value::String(s);
            }
            obj
        })
        .collect();

    let report = serde_json::json!({
        "covered": oracle.covered_count(),
        "total": oracle.total_count(),
        "coverage_percent": oracle.coverage_percent(),
        "uncovered": branches,
    });

    println!(
        "{}",
        serde_json::to_string_pretty(&report).unwrap_or_default()
    );
}

/// Build the rich agent-format gap report (without printing).
fn build_agent_report(
    oracle: &CoverageOracle,
    file_paths: &HashMap<u64, PathBuf>,
    target_path: &std::path::Path,
) -> apex_core::agent_report::AgentGapReport {
    use apex_core::agent_report::build_agent_gap_report;

    let total = oracle.total_count();
    let covered = oracle.covered_count();
    let uncovered = oracle.uncovered_branches();

    let mut source_cache: HashMap<(u64, u32), String> = HashMap::new();
    for branch in &uncovered {
        if source_cache.contains_key(&(branch.file_id, branch.line)) {
            continue;
        }
        if let Some(path) = file_paths.get(&branch.file_id) {
            let full_path = target_path.join(path);
            if let Ok(content) = std::fs::read_to_string(&full_path) {
                let lines: Vec<&str> = content.lines().collect();
                let start = (branch.line as usize).saturating_sub(6);
                let end = (branch.line as usize + 5).min(lines.len());
                for (i, line) in lines[start..end].iter().enumerate() {
                    let line_num = (start + i + 1) as u32;
                    source_cache
                        .entry((branch.file_id, line_num))
                        .or_insert_with(|| line.to_string());
                }
            }
        }
    }

    build_agent_gap_report(total, covered, &uncovered, file_paths, &source_cache)
}

/// Print rich agent-format JSON gap report for external agent consumption.
#[allow(dead_code)]
fn print_agent_json_report(
    oracle: &CoverageOracle,
    file_paths: &HashMap<u64, PathBuf>,
    target_path: &std::path::Path,
) {
    let report = build_agent_report(oracle, file_paths, target_path);
    match serde_json::to_string_pretty(&report) {
        Ok(json) => println!("{json}"),
        Err(e) => eprintln!("{{\"error\": \"failed to serialize report: {e}\"}}"),
    }
}

// ---------------------------------------------------------------------------
// Bug report output
// ---------------------------------------------------------------------------

#[allow(dead_code)]
fn print_bug_report(summary: &apex_core::types::BugSummary) {
    if summary.total == 0 {
        println!("\nBugs found: 0");
        return;
    }
    println!("\nBugs found: {}", summary.total);
    for (class, count) in &summary.by_class {
        println!("  {class}: {count}");
    }
    println!();
    for (i, report) in summary.reports.iter().enumerate() {
        let loc = report.location.as_deref().unwrap_or("unknown");
        let msg_preview: String = report
            .message
            .lines()
            .next()
            .unwrap_or("")
            .chars()
            .take(120)
            .collect();
        println!(
            "  [{}] {} at {} — {}",
            i + 1,
            report.class,
            loc,
            msg_preview
        );
    }
    println!();
}

#[allow(dead_code)]
fn print_json_bug_report(summary: &apex_core::types::BugSummary) {
    let bugs: Vec<serde_json::Value> = summary
        .reports
        .iter()
        .map(|r| {
            serde_json::json!({
                "id": r.id.to_string(),
                "class": r.class.to_string(),
                "location": r.location,
                "message": r.message.lines().next().unwrap_or(""),
                "iteration": r.discovered_at_iteration,
            })
        })
        .collect();

    let report = serde_json::json!({
        "bugs_total": summary.total,
        "by_class": summary.by_class,
        "bugs": bugs,
    });
    println!(
        "{}",
        serde_json::to_string_pretty(&report).unwrap_or_default()
    );
}

// ---------------------------------------------------------------------------
// `apex run --strategy concolic`
// ---------------------------------------------------------------------------

async fn run_concolic_strategy(
    oracle: Arc<CoverageOracle>,
    instrumented: &apex_core::types::InstrumentedTarget,
    coverage_target: f64,
    rounds: usize,
    _output: Option<PathBuf>,
) -> Result<()> {
    use apex_concolic::PythonConcolicStrategy;
    use apex_core::traits::{Sandbox, Strategy};
    use apex_sandbox::PythonTestSandbox;

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

    for round in 1..=rounds {
        let uncovered = oracle.uncovered_branches();
        if uncovered.is_empty() {
            info!("all branches covered");
            break;
        }
        let pct = oracle.coverage_percent();
        if pct / 100.0 >= coverage_target {
            info!(coverage = %format!("{pct:.1}%"), "target reached");
            break;
        }
        info!(round, uncovered = uncovered.len(), coverage = %format!("{pct:.1}%"), "concolic round");

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
            info!("no concolic seeds generated — coverage stalled");
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
                Err(e) => debug!(error = %e, "concolic seed failed to run"),
            }
        }
    }

    let covered = oracle.covered_count();
    let total = oracle.total_count();
    println!(
        "\nFinal coverage: {covered}/{total} ({:.1}%)",
        oracle.coverage_percent()
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// `apex ratchet`
// ---------------------------------------------------------------------------

async fn ratchet(args: RatchetArgs, cfg: &ApexConfig) -> Result<()> {
    let lang: Language = args.lang.into();
    let target_path = args.target.canonicalize()?;
    let min_coverage = args.min_coverage.unwrap_or(cfg.coverage.min_ratchet);

    info!(target = %target_path.display(), "running ratchet check");

    let oracle = Arc::new(CoverageOracle::new());
    let _instrumented = instrument(lang, &target_path, &oracle).await?;

    let pct = oracle.coverage_percent() / 100.0;
    println!(
        "Coverage: {:.1}%  (min required: {:.1}%)",
        pct * 100.0,
        min_coverage * 100.0
    );

    if pct < min_coverage {
        return Err(color_eyre::eyre::eyre!(
            "FAIL: coverage {:.1}% is below minimum {:.1}%",
            pct * 100.0,
            min_coverage * 100.0
        ));
    }

    println!("PASS");
    Ok(())
}

// ---------------------------------------------------------------------------
// `apex audit`
// ---------------------------------------------------------------------------

async fn run_audit(args: AuditArgs, cfg: &ApexConfig) -> Result<()> {
    use apex_detect::{AnalysisContext, DetectConfig, DetectorPipeline, Severity};
    use std::sync::Arc;

    let lang: Language = args.lang.into();
    let target_path = args.target.canonicalize()?;

    // Build detect config from apex.toml + CLI overrides
    let mut detect_cfg = DetectConfig::default();
    if !cfg.detect.enabled.is_empty() {
        detect_cfg.enabled = cfg.detect.enabled.clone();
    }
    detect_cfg.severity_threshold = cfg.detect.severity_threshold.clone();
    detect_cfg.per_detector_timeout_secs = cfg.detect.per_detector_timeout_secs;
    // CLI --detectors overrides config file
    if let Some(detectors) = args.detectors {
        detect_cfg.enabled = detectors;
    }

    // Build source cache
    let source_cache = build_source_cache(&target_path, lang);

    // Build CPG for Python projects (other languages: TODO)
    let cpg = if lang == Language::Python {
        let mut combined_cpg = apex_cpg::Cpg::new();
        for (path, source) in &source_cache {
            let file_cpg = apex_cpg::builder::build_python_cpg(source, &path.display().to_string());
            combined_cpg.merge(file_cpg);
        }
        if combined_cpg.node_count() > 0 {
            Some(Arc::new(combined_cpg))
        } else {
            None
        }
    } else {
        None
    };

    let ctx = AnalysisContext {
        target_root: target_path.clone(),
        language: lang,
        oracle: Arc::new(CoverageOracle::new()),
        file_paths: std::collections::HashMap::new(),
        known_bugs: vec![],
        source_cache,
        fuzz_corpus: None,
        config: detect_cfg.clone(),
        runner: Arc::new(apex_core::command::RealCommandRunner),
        cpg,
        threat_model: cfg.threat_model.clone(),
        reverse_path_engine: None,
    };

    let pipeline = DetectorPipeline::from_config(&detect_cfg, lang);
    let report = pipeline.run_all(&ctx).await;

    let min_severity = match args.severity_threshold.as_str() {
        "critical" => Severity::Critical,
        "high" => Severity::High,
        "medium" => Severity::Medium,
        "low" => Severity::Low,
        _ => Severity::Info,
    };

    let output_text = match args.output_format {
        OutputFormat::Json => serde_json::to_string_pretty(&report)?,
        OutputFormat::Text => {
            let summary = report.security_summary();
            let mut buf = String::new();
            use std::fmt::Write;
            writeln!(buf, "\nAPEX Security Audit — {}\n", target_path.display()).ok();
            writeln!(
                buf,
                "  CRITICAL  {}      HIGH  {}      MEDIUM  {}      LOW  {}\n",
                summary.critical, summary.high, summary.medium, summary.low
            )
            .ok();

            for f in &report.findings {
                if f.severity.rank() > min_severity.rank() {
                    continue;
                }
                let sev = format!("{:?}", f.severity).to_uppercase();
                writeln!(
                    buf,
                    "{:<9} {}:{} — {}",
                    sev,
                    f.file.display(),
                    f.line.map(|l| l.to_string()).unwrap_or_default(),
                    f.title
                )
                .ok();
                writeln!(buf, "          [{}] {}", f.detector, f.description).ok();
                writeln!(buf, "          Suggestion: {}\n", f.suggestion).ok();
            }

            let status_line: String = report
                .detector_status
                .iter()
                .map(|(name, ok)| {
                    if *ok {
                        format!("{name} OK")
                    } else {
                        format!("{name} FAIL")
                    }
                })
                .collect::<Vec<_>>()
                .join("  ");
            writeln!(buf, "Detectors: {status_line}").ok();
            buf
        }
    };

    if let Some(path) = args.output {
        let path = validate_output_path(&path)?;
        std::fs::write(&path, &output_text)?;
        eprintln!("Wrote audit report to {}", path.display());
    } else {
        print!("{output_text}");
    }

    Ok(())
}

fn build_source_cache(
    target: &std::path::Path,
    lang: Language,
) -> std::collections::HashMap<PathBuf, String> {
    let extensions: &[&str] = match lang {
        Language::Rust => &["rs"],
        Language::Python => &["py"],
        Language::JavaScript => &["js", "ts", "jsx", "tsx", "mjs", "cjs"],
        Language::Java => &["java"],
        Language::C => &["c", "h", "cpp", "cc", "cxx", "hpp", "hh"],
        Language::Wasm => &["rs", "wat"],
        Language::Ruby => &["rb"],
        Language::Kotlin => &["kt", "kts"],
        Language::Go => &["go"],
        Language::Cpp => &["cpp", "cxx", "cc", "hpp", "hxx", "h"],
        Language::Swift => &["swift"],
        Language::CSharp => &["cs"],
    };

    let mut cache = std::collections::HashMap::new();

    if let Ok(entries) = walkdir(target, extensions) {
        for path in entries {
            // Skip files larger than 1 MB to avoid loading generated or binary-adjacent files.
            if path
                .metadata()
                .map(|m| m.len() > MAX_SOURCE_FILE_BYTES)
                .unwrap_or(false)
            {
                continue;
            }
            if let Ok(content) = std::fs::read_to_string(&path) {
                let rel = path.strip_prefix(target).unwrap_or(&path).to_path_buf();
                cache.insert(rel, content);
            }
        }
    }

    cache
}

/// Maximum number of source files to collect during directory walking.
/// Prevents APEX from being overwhelmed by massive repos (e.g. Linux kernel, 75k+ files).
const MAX_SOURCE_FILES: usize = 10_000;

/// Maximum file size (in bytes) to load into the source cache.
/// Files larger than this are skipped to avoid reading multi-MB generated files.
const MAX_SOURCE_FILE_BYTES: u64 = 1_024 * 1_024; // 1 MB

/// Build the appropriate [`apex_core::traits::TestSynthesizer`] for the given language.
///
/// Returns a `Box<dyn TestSynthesizer>` pointing at `output_dir`.  The caller is
/// responsible for writing synthesized tests to disk via `synthesize()`.
pub fn make_synthesizer(
    lang: Language,
    output_dir: impl Into<std::path::PathBuf>,
) -> Box<dyn apex_core::traits::TestSynthesizer> {
    use apex_synth::{
        CargoTestSynthesizer, CSharpTestSynthesizer, CTestSynthesizer, CppTestSynthesizer,
        GoTestSynthesizer, JUnitSynthesizer, JestSynthesizer, KotlinTestSynthesizer,
        PytestSynthesizer, RubyTestSynthesizer, SwiftTestSynthesizer, WasmTestSynthesizer,
    };
    let dir = output_dir.into();
    match lang {
        Language::Python => Box::new(PytestSynthesizer::new(&dir)),
        Language::JavaScript => Box::new(JestSynthesizer::new(&dir)),
        Language::Java => Box::new(JUnitSynthesizer::new(&dir)),
        Language::Rust => Box::new(CargoTestSynthesizer::new(&dir)),
        Language::Go => Box::new(GoTestSynthesizer::new(&dir)),
        Language::Cpp => Box::new(CppTestSynthesizer::new(&dir)),
        Language::C => Box::new(CTestSynthesizer::new(&dir)),
        Language::CSharp => Box::new(CSharpTestSynthesizer::new(&dir)),
        Language::Swift => Box::new(SwiftTestSynthesizer::new(&dir)),
        Language::Kotlin => Box::new(KotlinTestSynthesizer::new(&dir)),
        Language::Ruby => Box::new(RubyTestSynthesizer::new(&dir)),
        Language::Wasm => Box::new(WasmTestSynthesizer::new(&dir)),
    }
}

fn walkdir(root: &std::path::Path, extensions: &[&str]) -> std::io::Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    walk_recursive(root, extensions, &mut files)?;
    Ok(files)
}

fn walk_recursive(
    dir: &std::path::Path,
    extensions: &[&str],
    files: &mut Vec<PathBuf>,
) -> std::io::Result<()> {
    if files.len() >= MAX_SOURCE_FILES {
        return Ok(());
    }
    if dir.is_dir() {
        for entry in std::fs::read_dir(dir)? {
            if files.len() >= MAX_SOURCE_FILES {
                break;
            }
            let entry = entry?;
            let path = entry.path();
            if path.is_dir() {
                let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
                if name.starts_with('.')
                    || matches!(
                        name,
                        "target"
                            | "node_modules"
                            | "venv"
                            | ".venv"
                            | "__pycache__"
                            | "dist"
                            | "build"
                            | ".git"
                            | "vendor"
                            | "third_party"
                            | "testdata"
                            | "fixtures"
                    )
                {
                    continue;
                }
                walk_recursive(&path, extensions, files)?;
            } else if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
                let ext_lower = ext.to_ascii_lowercase();
                if extensions
                    .iter()
                    .any(|e| e.eq_ignore_ascii_case(&ext_lower))
                {
                    files.push(path);
                }
            }
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// SDLC Intelligence Commands
// ---------------------------------------------------------------------------

/// Load (or auto-detect) the branch index from .apex/index.json.
fn load_index(target: &std::path::Path) -> Result<apex_index::BranchIndex> {
    let index_path = target.join(".apex").join("index.json");
    if !index_path.exists() {
        return Err(color_eyre::eyre::eyre!(
            "No branch index found at {}. Run `apex index` first.",
            index_path.display()
        ));
    }
    let index = apex_index::BranchIndex::load(&index_path)
        .map_err(|e| color_eyre::eyre::eyre!("failed to load index: {e}"))?;
    Ok(index)
}

// ---------------------------------------------------------------------------
// `apex index`
// ---------------------------------------------------------------------------

async fn run_index(args: IndexArgs) -> Result<()> {
    let lang: Language = args.lang.into();
    let target_path = args.target.canonicalize()?;

    let index = match lang {
        Language::Python => apex_index::python::build_python_index(&target_path, args.parallel)
            .await
            .map_err(|e| color_eyre::eyre::eyre!("index build failed: {e}"))?,
        Language::Rust => apex_index::rust::build_rust_index(&target_path, args.parallel)
            .await
            .map_err(|e| color_eyre::eyre::eyre!("index build failed: {e}"))?,
        other => {
            return Err(color_eyre::eyre::eyre!(
                "indexing not yet supported for {other}"
            ));
        }
    };

    let out_path = target_path.join(".apex").join("index.json");
    index
        .save(&out_path)
        .map_err(|e| color_eyre::eyre::eyre!("save index: {e}"))?;

    println!("Branch index built:");
    println!("  Tests:    {}", index.traces.len());
    println!(
        "  Branches: {} total, {} covered",
        index.total_branches, index.covered_branches
    );
    println!("  Coverage: {:.1}%", index.coverage_percent());
    println!("  Saved:    {}", out_path.display());

    Ok(())
}

// ---------------------------------------------------------------------------
// `apex test-optimize`
// ---------------------------------------------------------------------------

async fn run_test_optimize(args: TestOptimizeArgs) -> Result<()> {
    let target_path = args.target.canonicalize()?;
    let index = load_index(&target_path)?;

    if index.traces.is_empty() {
        println!("No tests in index.");
        return Ok(());
    }

    // Greedy weighted set cover
    let mut uncovered: std::collections::HashSet<String> = index.profiles.keys().cloned().collect();
    let mut selected: Vec<(String, usize)> = Vec::new(); // (test_name, unique_branches_covered)
    let mut remaining: Vec<&apex_index::TestTrace> = index.traces.iter().collect();

    while !uncovered.is_empty() && !remaining.is_empty() {
        // Pick test covering most uncovered branches (tie-break: shortest duration)
        remaining.sort_by(|a, b| {
            let a_score = a
                .branches
                .iter()
                .filter(|br| uncovered.contains(&apex_index::types::branch_key(br)))
                .count();
            let b_score = b
                .branches
                .iter()
                .filter(|br| uncovered.contains(&apex_index::types::branch_key(br)))
                .count();
            b_score
                .cmp(&a_score)
                .then(a.duration_ms.cmp(&b.duration_ms))
        });

        let best = remaining[0];
        let newly: usize = best
            .branches
            .iter()
            .filter(|br| uncovered.contains(&apex_index::types::branch_key(br)))
            .count();

        if newly == 0 {
            break;
        }

        for br in &best.branches {
            uncovered.remove(&apex_index::types::branch_key(br));
        }
        selected.push((best.test_name.clone(), newly));
        remaining.remove(0);
    }

    let total_tests = index.traces.len();
    let selected_count = selected.len();
    let total_duration: u64 = index.traces.iter().map(|t| t.duration_ms).sum();
    let selected_duration: u64 = index
        .traces
        .iter()
        .filter(|t| selected.iter().any(|(name, _)| name == &t.test_name))
        .map(|t| t.duration_ms)
        .sum();

    let speedup = if selected_duration > 0 {
        total_duration as f64 / selected_duration as f64
    } else {
        1.0
    };

    match args.output_format {
        OutputFormat::Json => {
            let report = serde_json::json!({
                "total_tests": total_tests,
                "selected_tests": selected_count,
                "redundant_tests": total_tests - selected_count,
                "speedup": format!("{:.1}x", speedup),
                "total_duration_ms": total_duration,
                "selected_duration_ms": selected_duration,
                "selected": selected.iter().map(|(name, unique)| {
                    serde_json::json!({"test": name, "unique_branches": unique})
                }).collect::<Vec<_>>(),
            });
            println!("{}", serde_json::to_string_pretty(&report)?);
        }
        OutputFormat::Text => {
            println!("Test Suite Optimization:");
            println!("  Minimal covering set: {selected_count} / {total_tests} tests");
            println!("  Redundant tests:     {}", total_tests - selected_count);
            println!("  Estimated speedup:   {speedup:.1}x");
            println!("  Duration:            {total_duration}ms → {selected_duration}ms");
            println!();

            // Show essential tests (cover unique branches)
            let essential: Vec<_> = selected.iter().filter(|(_, u)| *u > 0).collect();
            if !essential.is_empty() {
                println!(
                    "Essential tests ({} cover unique branches):",
                    essential.len()
                );
                for (name, unique) in essential.iter().take(20) {
                    println!("  {name} — {unique} unique branches");
                }
                if essential.len() > 20 {
                    println!("  ... and {} more", essential.len() - 20);
                }
            }
        }
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// `apex test-prioritize`
// ---------------------------------------------------------------------------

async fn run_test_prioritize(args: TestPrioritizeArgs) -> Result<()> {
    let target_path = args.target.canonicalize()?;
    let index = load_index(&target_path)?;

    if args.changed_files.is_empty() {
        return Err(color_eyre::eyre::eyre!(
            "--changed-files is required (comma-separated list of relative paths)"
        ));
    }

    // Build set of file_ids for changed files
    let changed_file_ids: std::collections::HashSet<u64> = index
        .file_paths
        .iter()
        .filter(|(_, path)| {
            let path_str = path.to_string_lossy();
            args.changed_files
                .iter()
                .any(|cf| path_str.contains(cf.as_str()))
        })
        .map(|(id, _)| *id)
        .collect();

    if changed_file_ids.is_empty() {
        eprintln!("Warning: none of the changed files match indexed files. Outputting all tests.");
        for t in &index.traces {
            println!("{}", t.test_name);
        }
        return Ok(());
    }

    // Score each test by overlap with changed file branches
    let mut scored: Vec<(&str, usize)> = index
        .traces
        .iter()
        .map(|t| {
            let score = t
                .branches
                .iter()
                .filter(|b| changed_file_ids.contains(&b.file_id))
                .count();
            (t.test_name.as_str(), score)
        })
        .collect();

    scored.sort_by(|a, b| b.1.cmp(&a.1));

    let relevant = scored.iter().filter(|(_, s)| *s > 0).count();
    eprintln!(
        "{} tests cover changed files ({} total)",
        relevant,
        scored.len()
    );

    for (name, _score) in &scored {
        println!("{name}");
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// `apex dead-code`
// ---------------------------------------------------------------------------

async fn run_dead_code(args: DeadCodeArgs) -> Result<()> {
    let target_path = args.target.canonicalize()?;
    let index = load_index(&target_path)?;

    // Compute per-file coverage from profiles.
    let mut file_stats: HashMap<PathBuf, (usize, usize)> = HashMap::new(); // (covered, total_in_profiles)

    // Group profiles by file
    for profile in index.profiles.values() {
        let file_path = index
            .file_paths
            .get(&profile.branch.file_id)
            .cloned()
            .unwrap_or_else(|| PathBuf::from(format!("<{:016x}>", profile.branch.file_id)));
        let entry = file_stats.entry(file_path).or_insert((0, 0));
        entry.1 += 1;
        if profile.hit_count > 0 {
            entry.0 += 1;
        }
    }

    // Report uncovered = total_branches - covered_branches
    let dead_count = index.total_branches.saturating_sub(index.covered_branches);

    match args.output_format {
        OutputFormat::Json => {
            let report = serde_json::json!({
                "total_branches": index.total_branches,
                "covered_branches": index.covered_branches,
                "dead_branches": dead_count,
                "coverage_percent": index.coverage_percent(),
                "per_file": file_stats.iter().map(|(path, (covered, total))| {
                    serde_json::json!({
                        "file": path.display().to_string(),
                        "covered": covered,
                        "total": total,
                    })
                }).collect::<Vec<_>>(),
            });
            println!("{}", serde_json::to_string_pretty(&report)?);
        }
        OutputFormat::Text => {
            println!("Dead Code Analysis:");
            println!(
                "  {} / {} branches never hit by any test ({} dead)",
                dead_count, index.total_branches, dead_count
            );
            println!("  Coverage: {:.1}%\n", index.coverage_percent());

            // Sort files by most dead branches
            let mut files: Vec<_> = file_stats.iter().collect();
            files.sort_by(|a, b| {
                let a_dead = a.1 .1 - a.1 .0;
                let b_dead = b.1 .1 - b.1 .0;
                b_dead.cmp(&a_dead)
            });

            println!("Files with untested branches:");
            for (path, (covered, total)) in files.iter().take(30) {
                let dead = total - covered;
                if dead > 0 {
                    let pct = if *total > 0 {
                        (*covered as f64 / *total as f64) * 100.0
                    } else {
                        100.0
                    };
                    println!(
                        "  {} — {}/{} covered ({:.0}%), {} dead",
                        path.display(),
                        covered,
                        total,
                        pct,
                        dead
                    );
                }
            }
        }
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// `apex lint`
// ---------------------------------------------------------------------------

async fn run_lint(args: LintArgs, _cfg: &ApexConfig) -> Result<()> {
    use apex_detect::{AnalysisContext, DetectConfig, DetectorPipeline, Severity};

    let lang: Language = args.lang.into();
    let target_path = args.target.canonicalize()?;

    // Load the branch index for runtime prioritization
    let index = load_index(&target_path).ok();

    // Run detectors
    let mut detect_cfg = DetectConfig::default();
    if let Some(ref detectors) = args.detectors {
        detect_cfg.enabled = detectors.clone();
    }
    let source_cache = build_source_cache(&target_path, lang);

    // Build CPG for Python projects (other languages: TODO)
    let cpg = if lang == Language::Python {
        let mut combined_cpg = apex_cpg::Cpg::new();
        for (path, source) in &source_cache {
            let file_cpg = apex_cpg::builder::build_python_cpg(source, &path.display().to_string());
            combined_cpg.merge(file_cpg);
        }
        if combined_cpg.node_count() > 0 {
            Some(Arc::new(combined_cpg))
        } else {
            None
        }
    } else {
        None
    };

    let ctx = AnalysisContext {
        target_root: target_path.clone(),
        language: lang,
        oracle: Arc::new(CoverageOracle::new()),
        file_paths: std::collections::HashMap::new(),
        known_bugs: vec![],
        source_cache,
        fuzz_corpus: None,
        config: detect_cfg.clone(),
        runner: Arc::new(apex_core::command::RealCommandRunner),
        cpg,
        threat_model: apex_core::config::ThreatModelConfig::default(),
        reverse_path_engine: None,
    };

    let pipeline = DetectorPipeline::from_config(&detect_cfg, lang);
    let report = pipeline.run_all(&ctx).await;

    // Enrich findings with runtime frequency data
    let mut enriched: Vec<(String, &apex_detect::Finding)> = report
        .findings
        .iter()
        .map(|f| {
            let priority = if let Some(ref idx) = index {
                // Look up branch frequency at this file:line
                let freq: u64 = idx
                    .profiles
                    .values()
                    .filter(|p| {
                        let matches_file = idx
                            .file_paths
                            .get(&p.branch.file_id)
                            .map(|fp| {
                                f.file
                                    .to_string_lossy()
                                    .contains(&fp.to_string_lossy().to_string())
                            })
                            .unwrap_or(false);
                        let matches_line = f.line.map(|l| p.branch.line == l).unwrap_or(false);
                        matches_file && matches_line
                    })
                    .map(|p| p.hit_count)
                    .sum();

                if freq > 1000 {
                    "CRITICAL (hot path)"
                } else if freq > 0 {
                    "HIGH (exercised)"
                } else {
                    "LOW (dead path)"
                }
            } else {
                match f.severity {
                    Severity::Critical => "CRITICAL",
                    Severity::High => "HIGH",
                    Severity::Medium => "MEDIUM",
                    Severity::Low => "LOW",
                    Severity::Info => "INFO",
                }
            };
            (priority.to_string(), f)
        })
        .collect();

    // Sort: CRITICAL first, then HIGH, etc.
    enriched.sort_by(|a, b| {
        let rank = |s: &str| -> u8 {
            if s.starts_with("CRITICAL") {
                0
            } else if s.starts_with("HIGH") {
                1
            } else if s.starts_with("MEDIUM") {
                2
            } else if s.starts_with("LOW") {
                3
            } else {
                4
            }
        };
        rank(&a.0).cmp(&rank(&b.0))
    });

    match args.output_format {
        OutputFormat::Json => {
            let findings: Vec<serde_json::Value> = enriched
                .iter()
                .map(|(priority, f)| {
                    serde_json::json!({
                        "priority": priority,
                        "severity": format!("{:?}", f.severity),
                        "file": f.file.display().to_string(),
                        "line": f.line,
                        "title": f.title,
                        "description": f.description,
                        "suggestion": f.suggestion,
                        "detector": f.detector,
                    })
                })
                .collect();
            println!("{}", serde_json::to_string_pretty(&findings)?);
        }
        OutputFormat::Text => {
            let has_index = index.is_some();
            if has_index {
                println!("Runtime-prioritized lint findings:\n");
            } else {
                println!(
                    "Lint findings (no index — run `apex index` for runtime prioritization):\n"
                );
            }
            for (priority, f) in &enriched {
                println!(
                    "{:<25} {}:{} — {}",
                    priority,
                    f.file.display(),
                    f.line.map(|l| l.to_string()).unwrap_or_default(),
                    f.title
                );
                println!("                          {}", f.description);
                println!("                          Suggestion: {}\n", f.suggestion);
            }
            println!("Total: {} findings", enriched.len());
        }
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// `apex diff --base`
// ---------------------------------------------------------------------------

async fn run_diff(args: DiffArgs) -> Result<()> {
    let lang: Language = args.lang.into();
    let target_path = args.target.canonicalize()?;

    // Validate user-supplied ref before passing it to git (CWE-88).
    validate_git_ref(&args.base)?;

    // Load current index (HEAD)
    let head_index = load_index(&target_path)?;

    // Build index for base ref using git worktree
    eprintln!("Building index for base ref '{}'...", args.base);

    let worktree_dir = target_path.join(format!(".apex-diff-{}", args.base.replace('/', "-")));
    let worktree_result = tokio::process::Command::new("git")
        .args([
            "worktree",
            "add",
            "--",
            &worktree_dir.to_string_lossy(),
            &args.base,
        ])
        .current_dir(&target_path)
        .output()
        .await;

    let _cleanup_worktree = scopeguard_worktree(&target_path, &worktree_dir);

    match worktree_result {
        Ok(output) if output.status.success() => {}
        Ok(output) => {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(color_eyre::eyre::eyre!("git worktree add failed: {stderr}"));
        }
        Err(e) => {
            return Err(color_eyre::eyre::eyre!("git worktree add: {e}"));
        }
    }

    let base_index = match lang {
        Language::Python => apex_index::python::build_python_index(&worktree_dir, 4)
            .await
            .map_err(|e| color_eyre::eyre::eyre!("base index: {e}"))?,
        other => {
            return Err(color_eyre::eyre::eyre!(
                "diff not yet supported for {other}"
            ));
        }
    };

    // Compare per-test branch sets
    let base_tests: HashMap<&str, &apex_index::TestTrace> = base_index
        .traces
        .iter()
        .map(|t| (t.test_name.as_str(), t))
        .collect();
    let head_tests: HashMap<&str, &apex_index::TestTrace> = head_index
        .traces
        .iter()
        .map(|t| (t.test_name.as_str(), t))
        .collect();

    let mut changed_tests = Vec::new();
    let mut new_tests = Vec::new();
    let mut removed_tests = Vec::new();

    for (name, head_trace) in &head_tests {
        if let Some(base_trace) = base_tests.get(name) {
            let head_keys: std::collections::HashSet<String> = head_trace
                .branches
                .iter()
                .map(apex_index::types::branch_key)
                .collect();
            let base_keys: std::collections::HashSet<String> = base_trace
                .branches
                .iter()
                .map(apex_index::types::branch_key)
                .collect();

            let added: Vec<_> = head_keys.difference(&base_keys).collect();
            let removed: Vec<_> = base_keys.difference(&head_keys).collect();

            if !added.is_empty() || !removed.is_empty() {
                changed_tests.push((*name, added.len(), removed.len()));
            }
        } else {
            new_tests.push(*name);
        }
    }

    for name in base_tests.keys() {
        if !head_tests.contains_key(name) {
            removed_tests.push(*name);
        }
    }

    // Report
    println!("Behavioral Diff: HEAD vs {}\n", args.base);

    if changed_tests.is_empty() && new_tests.is_empty() && removed_tests.is_empty() {
        println!("No behavioral changes detected.");
    } else {
        if !changed_tests.is_empty() {
            println!("Changed tests ({}):", changed_tests.len());
            for (name, added, removed) in &changed_tests {
                println!("  {name} — +{added} branches, -{removed} branches");
            }
            println!();
        }

        if !new_tests.is_empty() {
            println!("New tests ({}):", new_tests.len());
            for name in &new_tests {
                println!("  {name}");
            }
            println!();
        }

        if !removed_tests.is_empty() {
            println!("Removed tests ({}):", removed_tests.len());
            for name in &removed_tests {
                println!("  {name}");
            }
            println!();
        }

        let new_covered = head_index.covered_branches;
        let base_covered = base_index.covered_branches;
        println!(
            "Coverage: {:.1}% → {:.1}% ({:+} branches)",
            base_index.coverage_percent(),
            head_index.coverage_percent(),
            new_covered as i64 - base_covered as i64
        );
    }

    if args.strict && !changed_tests.is_empty() {
        return Err(color_eyre::eyre::eyre!(
            "FAIL: {} tests show behavioral changes",
            changed_tests.len()
        ));
    }

    Ok(())
}

/// RAII guard to clean up git worktree on drop.
fn scopeguard_worktree(repo_root: &std::path::Path, worktree_dir: &std::path::Path) -> impl Drop {
    let repo = repo_root.to_path_buf();
    let wt = worktree_dir.to_path_buf();
    scopeguard::guard((), move |_| {
        let _ = std::process::Command::new("git")
            .args(["worktree", "remove", "--force", &wt.to_string_lossy()])
            .current_dir(&repo)
            .output();
    })
}

// ---------------------------------------------------------------------------
// `apex flaky-detect`
// ---------------------------------------------------------------------------

async fn run_flaky_detect(args: FlakyDetectArgs) -> Result<()> {
    let lang: Language = args.lang.into();
    let target_path = args.target.canonicalize()?;

    if !matches!(lang, Language::Python) {
        return Err(color_eyre::eyre::eyre!(
            "flaky-detect currently supports Python only"
        ));
    }

    eprintln!(
        "Running each test {} times to detect flakiness...",
        args.runs
    );

    // Enumerate tests
    let test_names = apex_index::python::enumerate_python_tests(&target_path)
        .await
        .map_err(|e| color_eyre::eyre::eyre!("enumerate tests: {e}"))?;

    eprintln!(
        "Found {} tests, running {} repetitions each",
        test_names.len(),
        args.runs
    );

    // Run N times
    let mut all_runs = Vec::with_capacity(args.runs);
    for run_idx in 0..args.runs {
        eprintln!("  Run {}/{}...", run_idx + 1, args.runs);
        let traces = apex_index::python::run_python_per_test(
            &target_path,
            &test_names,
            args.parallel,
            run_idx * test_names.len(),
        )
        .await
        .map_err(|e| color_eyre::eyre::eyre!("run {}: {e}", run_idx + 1))?;
        all_runs.push(traces);
    }

    // Load file_paths from index or build inline
    let file_paths = if let Ok(index) = load_index(&target_path) {
        index.file_paths
    } else {
        HashMap::new()
    };

    let flaky = apex_index::analysis::detect_flaky_tests(&all_runs, &file_paths);

    match args.output_format {
        OutputFormat::Json => {
            println!("{}", serde_json::to_string_pretty(&flaky)?);
        }
        OutputFormat::Text => {
            if flaky.is_empty() {
                println!("No flaky tests detected across {} runs.", args.runs);
            } else {
                println!(
                    "Flaky tests detected ({} of {}):\n",
                    flaky.len(),
                    test_names.len()
                );
                for f in &flaky {
                    println!(
                        "  {} — {} divergent branches",
                        f.test_name,
                        f.divergent_branches.len()
                    );
                    for db in &f.divergent_branches {
                        let path = db
                            .file_path
                            .as_ref()
                            .map(|p| p.display().to_string())
                            .unwrap_or_else(|| format!("<{:016x}>", db.branch.file_id));
                        let dir = if db.branch.direction == 0 {
                            "true"
                        } else {
                            "false"
                        };
                        println!(
                            "    {}:{} [{}] — hit {}/{} runs",
                            path, db.branch.line, dir, db.hit_ratio, f.total_runs
                        );
                    }
                    println!();
                }
            }
        }
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// `apex complexity`
// ---------------------------------------------------------------------------

async fn run_complexity(args: ComplexityArgs) -> Result<()> {
    let target_path = args.target.canonicalize()?;
    let index = load_index(&target_path)?;

    let results = apex_index::analysis::analyze_complexity(&index, &target_path);

    match args.output_format {
        OutputFormat::Json => {
            println!("{}", serde_json::to_string_pretty(&results)?);
        }
        OutputFormat::Text => {
            println!("Exercised vs Static Complexity:\n");
            println!(
                "{:<40} {:>8} {:>8} {:>7} Classification",
                "Function", "Static", "Exerc.", "Ratio"
            );
            println!("{}", "-".repeat(85));
            for r in &results {
                println!(
                    "{:<40} {:>8} {:>8} {:>6.0}% {}",
                    format!("{}:{} {}", r.file_path.display(), r.line, r.function_name),
                    r.static_complexity,
                    r.exercised_complexity,
                    r.exercise_ratio * 100.0,
                    r.classification
                );
            }
            println!("\nTotal: {} functions analyzed", results.len());

            let under_tested: Vec<_> = results.iter().filter(|r| r.exercise_ratio < 0.5).collect();
            if !under_tested.is_empty() {
                println!(
                    "\n{} functions are under-tested (<50% of branches exercised)",
                    under_tested.len()
                );
            }
        }
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// `apex docs`
// ---------------------------------------------------------------------------

async fn run_docs(args: DocsArgs) -> Result<()> {
    let target_path = args.target.canonicalize()?;
    let index = load_index(&target_path)?;

    let docs = apex_index::analysis::generate_docs(&index, &target_path);

    let output = match args.output_format {
        OutputFormat::Json => serde_json::to_string_pretty(&docs)?,
        OutputFormat::Text => {
            let mut buf = String::new();
            use std::fmt::Write;
            writeln!(buf, "# Behavioral Documentation\n").ok();
            writeln!(
                buf,
                "Generated from {} tests covering {} branches ({:.1}% coverage)\n",
                index.traces.len(),
                index.covered_branches,
                index.coverage_percent()
            )
            .ok();

            for doc in &docs {
                writeln!(
                    buf,
                    "## `{}` ({} line {})\n",
                    doc.function_name,
                    doc.file_path.display(),
                    doc.line
                )
                .ok();
                writeln!(
                    buf,
                    "Tested by {} tests, {} distinct execution paths:\n",
                    doc.total_tests,
                    doc.paths.len()
                )
                .ok();

                for (i, path) in doc.paths.iter().enumerate() {
                    writeln!(
                        buf,
                        "- **Path {}** ({:.0}% of tests, {} branches): `{}`",
                        i + 1,
                        path.frequency_pct,
                        path.branch_count,
                        path.representative_test
                    )
                    .ok();
                }
                writeln!(buf).ok();
            }

            buf
        }
    };

    if let Some(path) = args.output {
        let path = validate_output_path(&path)?;
        std::fs::write(&path, &output)?;
        eprintln!("Docs written to {}", path.display());
    } else {
        print!("{output}");
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// `apex verify-boundaries`
// ---------------------------------------------------------------------------

async fn run_verify_boundaries(args: VerifyBoundariesArgs) -> Result<()> {
    let target_path = args.target.canonicalize()?;
    let index = load_index(&target_path)?;

    let report = apex_index::analysis::verify_boundaries(
        &index,
        &target_path,
        &args.entry_pattern,
        &args.auth_checks,
    );

    match args.output_format {
        OutputFormat::Json => {
            println!("{}", serde_json::to_string_pretty(&report)?);
        }
        OutputFormat::Text => {
            println!("Boundary Verification\n");
            println!("  Entry pattern:     \"{}\"", report.entry_pattern);
            println!("  Auth check:        \"{}\"", report.auth_pattern);
            println!("  Entry tests:       {}", report.total_entry_tests);
            println!(
                "  Protected:         {} (pass through auth)",
                report.passing_tests
            );
            println!(
                "  Unprotected:       {} (NO auth branch hit)\n",
                report.failing_tests
            );

            if report.total_entry_tests == 0 {
                println!("No tests match the entry pattern. Try a broader pattern.");
                return Ok(());
            }

            if !report.unprotected_paths.is_empty() {
                println!("Unprotected paths:");
                for path in &report.unprotected_paths {
                    println!(
                        "  {} — {} branches, reaches {} files",
                        path.test_name,
                        path.branches_traversed,
                        path.files_reached.len()
                    );
                    for f in &path.files_reached {
                        println!("    {}", f.display());
                    }
                }
            } else {
                println!("All entry-point test paths pass through auth checks.");
            }
        }
    }

    if args.strict && report.failing_tests > 0 {
        return Err(color_eyre::eyre::eyre!(
            "FAIL: {} boundary tests failing",
            report.failing_tests
        ));
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// `apex regression-check`
// ---------------------------------------------------------------------------

async fn run_regression_check(args: RegressionCheckArgs) -> Result<()> {
    let target_path = args.target.canonicalize()?;
    let lang: Language = args.lang.into();

    // Build current index
    info!("building index for HEAD");
    let head_index = load_index(&target_path)?;

    // Build base index via worktree
    let worktree_dir = target_path.join(format!(".apex-regression-{}", std::process::id()));
    let status = std::process::Command::new("git")
        .args(["worktree", "add", "--quiet"])
        .arg(&worktree_dir)
        .arg(&args.base)
        .current_dir(&target_path)
        .status()?;

    if !status.success() {
        return Err(color_eyre::eyre::eyre!(
            "git worktree add failed for base ref '{}'",
            args.base
        ));
    }

    let _guard = scopeguard::guard((), |_| {
        let _ = std::process::Command::new("git")
            .args(["worktree", "remove", "--force"])
            .arg(&worktree_dir)
            .current_dir(&target_path)
            .status();
    });

    info!("building index for base ref '{}'", args.base);
    let base_index_path = worktree_dir.join(".apex/index.json");

    // Run indexing on base worktree
    let base_index = if base_index_path.exists() {
        apex_index::BranchIndex::load(&base_index_path)?
    } else {
        // Build index in base worktree
        match lang {
            Language::Python => apex_index::python::build_python_index(&worktree_dir, 4)
                .await
                .map_err(|e| color_eyre::eyre::eyre!("{e}"))?,
            _ => {
                return Err(color_eyre::eyre::eyre!(
                    "regression-check currently supports Python only"
                ));
            }
        }
    };

    // Compare per-test branch sets
    let mut regressions = Vec::new();
    for head_trace in &head_index.traces {
        if args
            .allow
            .iter()
            .any(|pat| head_trace.test_name.contains(pat))
        {
            continue;
        }

        let base_trace = base_index
            .traces
            .iter()
            .find(|t| t.test_name == head_trace.test_name);

        if let Some(base) = base_trace {
            let head_set: HashSet<String> = head_trace
                .branches
                .iter()
                .map(apex_index::types::branch_key)
                .collect();
            let base_set: HashSet<String> = base
                .branches
                .iter()
                .map(apex_index::types::branch_key)
                .collect();

            let gained: Vec<_> = head_set.difference(&base_set).cloned().collect();
            let lost: Vec<_> = base_set.difference(&head_set).cloned().collect();

            if !gained.is_empty() || !lost.is_empty() {
                regressions.push(serde_json::json!({
                    "test": head_trace.test_name,
                    "gained_branches": gained.len(),
                    "lost_branches": lost.len(),
                }));
            }
        }
    }

    let exit_code = if regressions.is_empty() { 0 } else { 1 };

    match args.output_format {
        OutputFormat::Json => {
            println!(
                "{}",
                serde_json::to_string_pretty(&serde_json::json!({
                    "base": args.base,
                    "regressions": regressions,
                    "pass": regressions.is_empty(),
                }))?
            );
        }
        OutputFormat::Text => {
            if regressions.is_empty() {
                println!(
                    "Regression check PASSED — no behavioral changes detected vs {}",
                    args.base
                );
            } else {
                println!(
                    "Regression check FAILED — {} tests show behavioral changes vs {}\n",
                    regressions.len(),
                    args.base
                );
                for r in &regressions {
                    println!(
                        "  {} — gained {} branches, lost {} branches",
                        r["test"], r["gained_branches"], r["lost_branches"]
                    );
                }
            }
        }
    }

    if exit_code != 0 {
        return Err(color_eyre::eyre::eyre!(
            "regression check failed with exit code {}",
            exit_code
        ));
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// `apex risk`
// ---------------------------------------------------------------------------

async fn run_risk(args: RiskArgs) -> Result<()> {
    let target_path = args.target.canonicalize()?;
    let index = load_index(&target_path)?;

    let assessment = apex_index::analysis::assess_risk(&index, &args.changed_files);

    match args.output_format {
        OutputFormat::Json => {
            println!("{}", serde_json::to_string_pretty(&assessment)?);
        }
        OutputFormat::Text => {
            println!("Risk Assessment: {}\n", assessment.level);
            println!("  Score:                  {}/100", assessment.score);
            println!("  Changed branches:       {}", assessment.changed_branches);
            println!(
                "  Covered:                {} ({:.1}%)",
                assessment.covered_changed, assessment.coverage_of_changed
            );
            println!("  Uncovered:              {}", assessment.uncovered_changed);
            println!("  Affected tests:         {}", assessment.affected_tests);

            if !assessment.reasons.is_empty() {
                println!("\nReasons:");
                for r in &assessment.reasons {
                    println!("  - {r}");
                }
            }
        }
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// `apex hotpaths`
// ---------------------------------------------------------------------------

async fn run_hotpaths(args: HotpathsArgs) -> Result<()> {
    let target_path = args.target.canonicalize()?;
    let index = load_index(&target_path)?;

    let hot = apex_index::analysis::analyze_hotpaths(&index, args.top);

    match args.output_format {
        OutputFormat::Json => {
            println!("{}", serde_json::to_string_pretty(&hot)?);
        }
        OutputFormat::Text => {
            println!(
                "Top {} Hot Paths ({} total branches)\n",
                hot.len(),
                index.profiles.len()
            );
            for (i, h) in hot.iter().enumerate() {
                println!(
                    "  {:>3}. {}:{}  dir={}  hits={}  share={:.1}%  tests={}",
                    i + 1,
                    h.file_path.display(),
                    h.line,
                    h.direction,
                    h.hit_count,
                    h.hit_share_pct,
                    h.test_count
                );
            }
        }
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// `apex contracts`
// ---------------------------------------------------------------------------

async fn run_contracts(args: ContractsArgs) -> Result<()> {
    let target_path = args.target.canonicalize()?;
    let index = load_index(&target_path)?;

    let invariants = apex_index::analysis::discover_contracts(&index, &target_path);

    match args.output_format {
        OutputFormat::Json => {
            println!("{}", serde_json::to_string_pretty(&invariants)?);
        }
        OutputFormat::Text => {
            if invariants.is_empty() {
                println!("No invariants discovered (need 3+ tests per function for detection).");
                return Ok(());
            }

            println!("Discovered Invariants ({} found)\n", invariants.len());
            for inv in &invariants {
                println!(
                    "  [{}] {}:{} in {}()",
                    inv.kind,
                    inv.file_path.display(),
                    inv.line,
                    inv.function_name
                );
                println!("    {}", inv.description);
                println!(
                    "    confidence={:.0}%  evidence={} tests\n",
                    inv.confidence * 100.0,
                    inv.evidence_tests
                );
            }
        }
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// `apex deploy-score`
// ---------------------------------------------------------------------------

async fn run_deploy_score(args: DeployScoreArgs) -> Result<()> {
    let target_path = args.target.canonicalize()?;
    let index = load_index(&target_path)?;

    let score = apex_index::analysis::compute_deploy_score(
        &index,
        args.detector_findings,
        args.critical_findings,
    );

    match args.output_format {
        OutputFormat::Json => {
            println!("{}", serde_json::to_string_pretty(&score)?);
        }
        OutputFormat::Text => {
            println!("Deploy Score: {}/100\n", score.total_score);
            println!("  {}\n", score.recommendation);
            println!("Breakdown:");
            for line in &score.breakdown {
                println!("  {line}");
            }
        }
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// `apex attack-surface`
// ---------------------------------------------------------------------------

async fn run_attack_surface(args: AttackSurfaceArgs) -> Result<()> {
    let target_path = args.target.canonicalize()?;
    let index = load_index(&target_path)?;

    let report = apex_index::analysis::analyze_attack_surface(&index, &args.entry_pattern);

    match args.output_format {
        OutputFormat::Json => {
            println!("{}", serde_json::to_string_pretty(&report)?);
        }
        OutputFormat::Text => {
            println!("Attack Surface Analysis\n");
            println!("  Entry pattern:       \"{}\"", report.entry_pattern);
            println!("  Matching tests:      {}", report.entry_tests);
            println!(
                "  Reachable branches:  {} / {} ({:.1}%)",
                report.reachable_branches, report.total_branches, report.attack_surface_pct
            );
            println!("  Reachable files:     {}\n", report.reachable_files);

            if report.entry_tests == 0 {
                println!("No tests match the entry pattern. Try a broader pattern.");
                return Ok(());
            }

            println!("Reachable files (by branch count):");
            for f in &report.reachable_file_details {
                println!(
                    "  {} — {} / {} branches reachable ({:.0}%)",
                    f.file_path.display(),
                    f.reachable_branches,
                    f.total_branches_in_file,
                    f.coverage_pct
                );
            }
        }
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// `apex features`
// ---------------------------------------------------------------------------

fn run_features(args: FeaturesArgs) -> Result<()> {
    let languages: Vec<Language> = if let Some(lang_arg) = args.lang {
        vec![lang_arg.into()]
    } else {
        vec![
            Language::Python,
            Language::JavaScript,
            Language::Java,
            Language::Rust,
            Language::C,
            Language::Wasm,
            Language::Ruby,
            Language::Kotlin,
        ]
    };

    match args.output_format {
        OutputFormat::Json => {
            let mut map = serde_json::Map::new();
            for lang in &languages {
                let features = lang.supported_features();
                map.insert(lang.to_string(), serde_json::to_value(&features)?);
            }
            println!("{}", serde_json::to_string_pretty(&map)?);
        }
        OutputFormat::Text => {
            let feature_names: Vec<String> = Language::Python
                .supported_features()
                .iter()
                .map(|f| f.name.clone())
                .collect();

            // Header row
            print!("{:<20}", "Feature");
            for lang in &languages {
                print!(" {:<15}", lang.to_string());
            }
            println!();

            // Separator
            print!("{}", "-".repeat(20));
            for _ in &languages {
                print!(" {}", "-".repeat(15));
            }
            println!();

            // Data rows
            for feat_name in &feature_names {
                print!("{feat_name:<20}");
                for lang in &languages {
                    let features = lang.supported_features();
                    if let Some(f) = features.iter().find(|f| f.name == *feat_name) {
                        let cell = if f.tool.is_empty() {
                            f.status.to_string()
                        } else {
                            format!("{} ({})", f.status, f.tool)
                        };
                        print!(" {cell:<15}");
                    } else {
                        print!(" {:<15}", "-");
                    }
                }
                println!();
            }
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// `apex reach`
// ---------------------------------------------------------------------------

async fn run_reach(args: ReachArgs) -> Result<()> {
    let lang: Language = args.lang.into();

    // Parse target "file:line"
    let parts: Vec<&str> = args.target.splitn(2, ':').collect();
    let file = PathBuf::from(parts[0]);
    let line: u32 = parts.get(1).and_then(|s| s.parse().ok()).unwrap_or(1);

    // Build source cache
    let target_dir = file.parent().unwrap_or(std::path::Path::new("."));
    let source_cache = build_source_cache(target_dir, lang);

    // Build call graph
    let graph = apex_reach::extractors::build_call_graph(&source_cache, lang);
    info!(
        nodes = graph.node_count(),
        edges = graph.edge_count(),
        "Built call graph"
    );

    let engine = apex_reach::ReversePathEngine::new(graph);

    // Parse granularity
    let granularity = match args.granularity.as_str() {
        "block" => apex_reach::Granularity::Block,
        "line" => apex_reach::Granularity::Line,
        _ => apex_reach::Granularity::Function,
    };

    // Parse entry kind filter
    let entry_kind_filter = args.entry_kind.as_deref().and_then(|k| match k {
        "test" => Some(apex_reach::EntryPointKind::Test),
        "http" => Some(apex_reach::EntryPointKind::HttpHandler),
        "main" => Some(apex_reach::EntryPointKind::Main),
        "api" => Some(apex_reach::EntryPointKind::PublicApi),
        "cli" => Some(apex_reach::EntryPointKind::CliEntry),
        _ => None,
    });

    // Query
    let target = apex_reach::TargetRegion::FileLine(file, line);
    let paths = if let Some(kind) = entry_kind_filter {
        engine.paths_to_entry_kind(&target, kind, granularity)
    } else {
        engine.paths_to_entry(&target, granularity)
    };

    // Output
    if paths.is_empty() {
        println!("No paths found to entry points.");
        return Ok(());
    }

    println!("Found {} paths to entry points:\n", paths.len());
    for path in &paths {
        if let Some(entry_node) = engine.graph().node(path.entry_point) {
            println!("  {} ({})", entry_node.name, path.entry_kind);
            for (fn_id, line) in &path.chain {
                if let Some(node) = engine.graph().node(*fn_id) {
                    println!(
                        "    \u{2192} {} ({}:{})",
                        node.name,
                        node.file.display(),
                        line
                    );
                }
            }
            println!();
        }
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Shared detector output helper
// ---------------------------------------------------------------------------

fn print_detector_findings(
    findings: &[apex_detect::Finding],
    format: &OutputFormat,
    target: &std::path::Path,
) {
    if findings.is_empty() {
        println!("No findings.");
        return;
    }
    match format {
        OutputFormat::Json => {
            println!(
                "{}",
                serde_json::to_string_pretty(findings).unwrap_or_default()
            );
        }
        OutputFormat::Text => {
            println!("\n{} finding(s) in {}\n", findings.len(), target.display());
            for f in findings {
                let sev = format!("{:?}", f.severity).to_uppercase();
                let file_loc = match f.line {
                    Some(l) => format!("{}:{}", f.file.display(), l),
                    None => f.file.display().to_string(),
                };
                println!("[{sev}] {file_loc}");
                println!("  {}", f.title);
                if !f.suggestion.is_empty() {
                    println!("  \u{2192} {}", f.suggestion);
                }
                println!();
            }
        }
    }
}

// ---------------------------------------------------------------------------
// `apex secret-scan`
// ---------------------------------------------------------------------------

async fn run_secret_scan(args: SecretScanArgs) -> Result<()> {
    use apex_detect::detectors::secret_scan::SecretScanDetector;
    use apex_detect::{AnalysisContext, DetectConfig, Detector};
    use std::sync::Arc;

    let lang: Language = args.lang.into();
    let target_path = args.target.canonicalize()?;
    let source_cache = build_source_cache(&target_path, lang);

    let detector = SecretScanDetector::new();
    let ctx = AnalysisContext {
        target_root: target_path.clone(),
        language: lang,
        oracle: Arc::new(CoverageOracle::new()),
        file_paths: std::collections::HashMap::new(),
        known_bugs: vec![],
        source_cache,
        fuzz_corpus: None,
        config: DetectConfig::default(),
        runner: Arc::new(apex_core::command::RealCommandRunner),
        cpg: None,
        threat_model: Default::default(),
        reverse_path_engine: None,
    };

    let findings = detector.analyze(&ctx).await?;
    print_detector_findings(&findings, &args.output_format, &target_path);
    Ok(())
}

// ---------------------------------------------------------------------------
// `apex license-scan`
// ---------------------------------------------------------------------------

async fn run_license_scan(args: LicenseScanArgs) -> Result<()> {
    use apex_detect::detectors::license_scan::LicenseScanDetector;
    use apex_detect::{AnalysisContext, DetectConfig, Detector};
    use std::sync::Arc;

    let lang: Language = args.lang.into();
    let target_path = args.target.canonicalize()?;
    let source_cache = build_source_cache(&target_path, lang);

    let detector = match args.policy.as_str() {
        "permissive" => LicenseScanDetector::permissive(),
        _ => LicenseScanDetector::enterprise(),
    };
    let ctx = AnalysisContext {
        target_root: target_path.clone(),
        language: lang,
        oracle: Arc::new(CoverageOracle::new()),
        file_paths: std::collections::HashMap::new(),
        known_bugs: vec![],
        source_cache,
        fuzz_corpus: None,
        config: DetectConfig::default(),
        runner: Arc::new(apex_core::command::RealCommandRunner),
        cpg: None,
        threat_model: Default::default(),
        reverse_path_engine: None,
    };

    let findings = detector.analyze(&ctx).await?;
    print_detector_findings(&findings, &args.output_format, &target_path);
    Ok(())
}

// ---------------------------------------------------------------------------
// `apex flag-hygiene`
// ---------------------------------------------------------------------------

async fn run_flag_hygiene(args: FlagHygieneArgs) -> Result<()> {
    use apex_detect::detectors::flag_hygiene::FlagHygieneDetector;
    use apex_detect::{AnalysisContext, DetectConfig, Detector};
    use std::sync::Arc;

    let lang: Language = args.lang.into();
    let target_path = args.target.canonicalize()?;
    let source_cache = build_source_cache(&target_path, lang);

    let detector = FlagHygieneDetector::new(args.max_age);
    let ctx = AnalysisContext {
        target_root: target_path.clone(),
        language: lang,
        oracle: Arc::new(CoverageOracle::new()),
        file_paths: std::collections::HashMap::new(),
        known_bugs: vec![],
        source_cache,
        fuzz_corpus: None,
        config: DetectConfig::default(),
        runner: Arc::new(apex_core::command::RealCommandRunner),
        cpg: None,
        threat_model: Default::default(),
        reverse_path_engine: None,
    };

    let findings = detector.analyze(&ctx).await?;
    print_detector_findings(&findings, &args.output_format, &target_path);
    Ok(())
}

// ---------------------------------------------------------------------------
// `apex api-diff`
// ---------------------------------------------------------------------------

async fn run_api_diff(args: ApiDiffArgs) -> Result<()> {
    use apex_detect::api_diff::{ApiDiffer, ChangeKind};

    let old_spec = std::fs::read_to_string(&args.old)?;
    let new_spec = std::fs::read_to_string(&args.new)?;

    let report = ApiDiffer::diff(&old_spec, &new_spec)?;

    match args.output_format {
        OutputFormat::Json => {
            println!("{}", serde_json::to_string_pretty(&report)?);
        }
        OutputFormat::Text => {
            println!(
                "API Diff: {} \u{2192} {}\n",
                args.old.display(),
                args.new.display()
            );
            println!("Breaking changes:     {}", report.breaking_count);
            println!("Non-breaking changes: {}", report.non_breaking_count);
            println!("Deprecations:         {}", report.deprecation_count);
            if report.breaking_count > 0 {
                println!("\n--- Breaking Changes ---");
                for change in &report.changes {
                    if matches!(change.kind, ChangeKind::Breaking) {
                        println!(
                            "  \u{2717} {} {} \u{2014} {}",
                            change.method, change.path, change.description
                        );
                    }
                }
            }
        }
    }

    if report.breaking_count > 0 {
        return Err(color_eyre::eyre::eyre!(
            "FAIL: {} breaking API changes detected",
            report.breaking_count
        ));
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// `apex data-flow`
// ---------------------------------------------------------------------------

async fn run_data_flow(args: DataFlowArgs) -> Result<()> {
    let lang: Language = args.lang.into();
    let target_path = args.target.canonicalize()?;
    let source_cache = build_source_cache(&target_path, lang);

    if lang != Language::Python {
        eprintln!("Warning: data-flow currently best supports Python. Other languages will have limited results.");
    }

    let mut cpg = apex_cpg::Cpg::new();
    for (path, source) in &source_cache {
        let file_cpg = apex_cpg::builder::build_python_cpg(source, &path.display().to_string());
        cpg.merge(file_cpg);
    }

    if cpg.node_count() == 0 {
        println!("No code found to analyze.");
        return Ok(());
    }

    apex_cpg::reaching_def::add_reaching_def_edges(&mut cpg);
    let flows = apex_cpg::taint::find_taint_flows(&cpg, args.max_depth);

    match args.output_format {
        OutputFormat::Json => {
            let flow_data: Vec<serde_json::Value> = flows
                .iter()
                .map(|f| {
                    serde_json::json!({
                        "source": f.source,
                        "sink": f.sink,
                        "path_length": f.path.len(),
                        "variables": f.variable_chain,
                    })
                })
                .collect();
            println!("{}", serde_json::to_string_pretty(&flow_data)?);
        }
        OutputFormat::Text => {
            if flows.is_empty() {
                println!("No taint flows detected.");
            } else {
                println!(
                    "\n{} taint flow(s) in {}\n",
                    flows.len(),
                    target_path.display()
                );
                for (i, flow) in flows.iter().enumerate() {
                    println!(
                        "  Flow {}: node {} \u{2192} node {}",
                        i + 1,
                        flow.source,
                        flow.sink
                    );
                    if !flow.variable_chain.is_empty() {
                        println!("    Variables: {}", flow.variable_chain.join(" \u{2192} "));
                    }
                    println!("    Path length: {} nodes\n", flow.path.len());
                }
            }
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// `apex blast-radius`
// ---------------------------------------------------------------------------

async fn run_blast_radius(args: BlastRadiusArgs) -> Result<()> {
    let target_path = args.target.canonicalize()?;
    let index_path = target_path.join(".apex").join("index.json");

    if !index_path.exists() {
        return Err(color_eyre::eyre::eyre!(
            "No branch index at {}. Run `apex index` first.",
            index_path.display()
        ));
    }

    let index_data = std::fs::read_to_string(&index_path)?;
    let index: apex_index::BranchIndex = serde_json::from_str(&index_data)?;
    let assessment = apex_index::analysis::assess_risk(&index, &args.changed_files);

    match args.output_format {
        OutputFormat::Json => {
            println!("{}", serde_json::to_string_pretty(&assessment)?);
        }
        OutputFormat::Text => {
            println!("\nBlast Radius: {}\n", target_path.display());
            println!("Risk Level:       {}", assessment.level);
            println!("Risk Score:       {}/100", assessment.score);
            println!("Affected Tests:   {}", assessment.affected_tests);
            println!("Changed Branches: {}", assessment.changed_branches);
            println!("Covered:          {}", assessment.covered_changed);
            println!("Uncovered:        {}", assessment.uncovered_changed);
            println!("Coverage:         {:.1}%", assessment.coverage_of_changed);
            if !assessment.reasons.is_empty() {
                println!("\nReasons:");
                for r in &assessment.reasons {
                    println!("  \u{2022} {r}");
                }
            }
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// `apex compliance-export`
// ---------------------------------------------------------------------------

async fn run_compliance_export(args: ComplianceExportArgs, cfg: &ApexConfig) -> Result<()> {
    use apex_detect::{AnalysisContext, DetectConfig, DetectorPipeline};

    let lang: Language = args.lang.into();
    let target_path = args.target.canonicalize()?;
    let source_cache = build_source_cache(&target_path, lang);

    // Build CPG for Python projects
    let cpg = if lang == Language::Python {
        let mut combined_cpg = apex_cpg::Cpg::new();
        for (path, source) in &source_cache {
            let file_cpg = apex_cpg::builder::build_python_cpg(source, &path.display().to_string());
            combined_cpg.merge(file_cpg);
        }
        if combined_cpg.node_count() > 0 {
            Some(Arc::new(combined_cpg))
        } else {
            None
        }
    } else {
        None
    };

    // Run detector pipeline to collect findings
    let detect_cfg = DetectConfig::default();
    let ctx = AnalysisContext {
        target_root: target_path.clone(),
        language: lang,
        oracle: Arc::new(CoverageOracle::new()),
        file_paths: HashMap::new(),
        known_bugs: vec![],
        source_cache: source_cache.clone(),
        fuzz_corpus: None,
        config: detect_cfg.clone(),
        runner: Arc::new(apex_core::command::RealCommandRunner),
        cpg,
        threat_model: cfg.threat_model.clone(),
        reverse_path_engine: None,
    };

    let pipeline = DetectorPipeline::from_config(&detect_cfg, lang);
    let report = pipeline.run_all(&ctx).await;

    // Collect unique detector IDs
    let detector_ids: Vec<String> = report
        .findings
        .iter()
        .map(|f| f.detector.clone())
        .collect::<HashSet<_>>()
        .into_iter()
        .collect();

    let framework = args.framework.to_lowercase();
    let asvs_level = match args.level.to_uppercase().as_str() {
        "L2" => apex_detect::compliance::asvs::AsvsLevel::L2,
        "L3" => apex_detect::compliance::asvs::AsvsLevel::L3,
        _ => apex_detect::compliance::asvs::AsvsLevel::L1,
    };

    let mut output = String::new();
    use std::fmt::Write;

    let show_asvs = framework == "all" || framework == "asvs";
    let show_ssdf = framework == "all" || framework == "ssdf";
    let show_stride = framework == "all" || framework == "stride";

    if show_asvs {
        let asvs = apex_detect::compliance::asvs::generate_asvs_report(&detector_ids, asvs_level);
        match args.output_format {
            OutputFormat::Json => {
                let val = serde_json::json!({
                    "framework": "ASVS",
                    "level": format!("{:?}", asvs.level),
                    "total": asvs.coverage.total,
                    "automated": asvs.coverage.automated,
                    "verified": asvs.coverage.verified,
                    "failed": asvs.coverage.failed,
                    "manual_required": asvs.coverage.manual_required,
                });
                writeln!(output, "{}", serde_json::to_string_pretty(&val)?).ok();
            }
            OutputFormat::Text => {
                writeln!(output, "\n=== ASVS Compliance ({:?}) ===\n", asvs.level).ok();
                writeln!(output, "  Total requirements: {}", asvs.coverage.total).ok();
                writeln!(output, "  Automated:          {}", asvs.coverage.automated).ok();
                writeln!(output, "  Verified:           {}", asvs.coverage.verified).ok();
                writeln!(output, "  Failed:             {}", asvs.coverage.failed).ok();
                writeln!(
                    output,
                    "  Manual required:    {}",
                    asvs.coverage.manual_required
                )
                .ok();
            }
        }
    }

    if show_ssdf {
        let ssdf = apex_detect::compliance::ssdf::generate_ssdf_report();
        match args.output_format {
            OutputFormat::Json => {
                let tasks: Vec<serde_json::Value> = ssdf
                    .tasks
                    .iter()
                    .map(|t| {
                        serde_json::json!({
                            "id": t.id,
                            "practice": t.practice,
                            "satisfied": t.apex_satisfies,
                            "evidence": t.evidence,
                        })
                    })
                    .collect();
                let val = serde_json::json!({
                    "framework": "SSDF",
                    "satisfied": ssdf.satisfied_count,
                    "total": ssdf.total_count,
                    "tasks": tasks,
                });
                writeln!(output, "{}", serde_json::to_string_pretty(&val)?).ok();
            }
            OutputFormat::Text => {
                writeln!(
                    output,
                    "\n=== SSDF Compliance ({}/{}) ===\n",
                    ssdf.satisfied_count, ssdf.total_count
                )
                .ok();
                for task in &ssdf.tasks {
                    let icon = if task.apex_satisfies {
                        "\u{2713}"
                    } else {
                        "\u{2717}"
                    };
                    writeln!(output, "  {} {} — {}", icon, task.id, task.practice).ok();
                }
            }
        }
    }

    if show_stride {
        // Concatenate all sources for STRIDE analysis
        let all_source: String = source_cache
            .values()
            .cloned()
            .collect::<Vec<_>>()
            .join("\n");
        let stride = apex_detect::threat::stride::analyze_stride(&all_source);
        match args.output_format {
            OutputFormat::Json => {
                let entries: Vec<serde_json::Value> = stride
                    .entries
                    .iter()
                    .map(|e| {
                        serde_json::json!({
                            "category": format!("{}", e.category),
                            "risk_level": format!("{:?}", e.risk_level),
                            "mitigations_found": e.mitigations_found,
                            "mitigations_missing": e.mitigations_missing,
                        })
                    })
                    .collect();
                let val = serde_json::json!({
                    "framework": "STRIDE",
                    "entries": entries,
                });
                writeln!(output, "{}", serde_json::to_string_pretty(&val)?).ok();
            }
            OutputFormat::Text => {
                writeln!(output, "\n=== STRIDE Threat Model ===\n").ok();
                for entry in &stride.entries {
                    writeln!(
                        output,
                        "  {} — Risk: {:?}",
                        entry.category, entry.risk_level
                    )
                    .ok();
                    if !entry.mitigations_found.is_empty() {
                        writeln!(
                            output,
                            "    Found:   {}",
                            entry.mitigations_found.join(", ")
                        )
                        .ok();
                    }
                    if !entry.mitigations_missing.is_empty() {
                        writeln!(
                            output,
                            "    Missing: {}",
                            entry.mitigations_missing.join(", ")
                        )
                        .ok();
                    }
                }
            }
        }
    }

    // Write to file or stdout
    if let Some(out_path) = args.output {
        let out_path = validate_output_path(&out_path)?;
        std::fs::write(&out_path, &output)?;
        println!("Compliance report written to {}", out_path.display());
    } else {
        print!("{output}");
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// `apex api-coverage`
// ---------------------------------------------------------------------------

async fn run_api_coverage(args: ApiCoverageArgs) -> Result<()> {
    let lang: Language = args.lang.into();
    let target_path = args.target.canonicalize()?;
    let spec_json = std::fs::read_to_string(&args.spec)?;
    let source_cache = build_source_cache(&target_path, lang);

    let report = apex_detect::api_coverage::analyze_coverage(&spec_json, &source_cache, lang)?;

    match args.output_format {
        OutputFormat::Json => {
            println!("{}", serde_json::to_string_pretty(&report)?);
        }
        OutputFormat::Text => {
            println!("API Spec Coverage\n");
            println!("Spec endpoints:        {}", report.spec_count);
            println!("Implemented:           {}", report.implemented_count);
            println!("Spec-only (missing):   {}", report.spec_only_count);
            println!("Code-only (undoc):     {}", report.code_only_count);
            if report.spec_only_count > 0 {
                println!("\n--- Spec-only (not implemented) ---");
                for ep in &report.endpoints {
                    if matches!(
                        ep.status,
                        apex_detect::api_coverage::EndpointStatus::SpecOnly
                    ) {
                        println!("  {} {}", ep.method, ep.path);
                    }
                }
            }
            if report.code_only_count > 0 {
                println!("\n--- Code-only (not in spec) ---");
                for ep in &report.endpoints {
                    if matches!(
                        ep.status,
                        apex_detect::api_coverage::EndpointStatus::CodeOnly
                    ) {
                        println!("  {} {}", ep.method, ep.path);
                    }
                }
            }
        }
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// `apex service-map`
// ---------------------------------------------------------------------------

async fn run_service_map(args: ServiceMapArgs) -> Result<()> {
    let lang: Language = args.lang.into();
    let target_path = args.target.canonicalize()?;
    let source_cache = build_source_cache(&target_path, lang);

    let map = apex_detect::service_map::analyze_service_map(&source_cache);

    match args.output_format {
        OutputFormat::Json => {
            println!("{}", serde_json::to_string_pretty(&map)?);
        }
        OutputFormat::Text => {
            println!("Service Dependency Map\n");
            println!("HTTP calls:      {}", map.http_count);
            println!("gRPC calls:      {}", map.grpc_count);
            println!("Message queues:  {}", map.mq_count);
            println!("Databases:       {}", map.db_count);
            println!("Total:           {}", map.dependencies.len());
            if !map.dependencies.is_empty() {
                println!();
                for dep in &map.dependencies {
                    println!(
                        "  [{:?}] {}:{} — {}",
                        dep.kind,
                        dep.file.display(),
                        dep.line,
                        dep.evidence
                    );
                }
            }
        }
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// `apex schema-check`
// ---------------------------------------------------------------------------

async fn run_schema_check(args: SchemaCheckArgs) -> Result<()> {
    use apex_detect::schema_check::MigrationRisk;

    let sql = std::fs::read_to_string(&args.migration)?;
    let report = apex_detect::schema_check::analyze_migration(&sql);

    match args.output_format {
        OutputFormat::Json => {
            println!("{}", serde_json::to_string_pretty(&report)?);
        }
        OutputFormat::Text => {
            println!("Schema Migration Safety: {}\n", args.migration.display());
            println!("Dangerous: {}", report.dangerous_count);
            println!("Caution:   {}", report.caution_count);
            println!("Safe:      {}", report.safe_count);
            if !report.issues.is_empty() {
                println!();
                for issue in &report.issues {
                    let icon = match issue.risk {
                        MigrationRisk::Dangerous => "\u{2717}",
                        MigrationRisk::Caution => "\u{26a0}",
                        MigrationRisk::Safe => "\u{2713}",
                    };
                    println!("  {} L{}: {}", icon, issue.line, issue.description);
                    println!("    Statement:  {}", issue.statement);
                    println!("    Suggestion: {}", issue.suggestion);
                }
            }
        }
    }

    if report.dangerous_count > 0 {
        return Err(color_eyre::eyre::eyre!(
            "FAIL: {} dangerous schema changes detected",
            report.dangerous_count
        ));
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// `apex test-data`
// ---------------------------------------------------------------------------

async fn run_test_data(args: TestDataArgs) -> Result<()> {
    let sql = std::fs::read_to_string(&args.schema)?;
    let tables = apex_detect::test_data::parse_schema(&sql);

    match args.output_format {
        OutputFormat::Json => {
            let output = serde_json::json!({
                "tables": tables,
                "rows_per_table": args.rows,
                "sql": apex_detect::test_data::generate_inserts(&tables, args.rows),
            });
            println!("{}", serde_json::to_string_pretty(&output)?);
        }
        OutputFormat::Text => {
            if tables.is_empty() {
                println!("No CREATE TABLE statements found.");
            } else {
                println!(
                    "-- Generated {} rows per table for {} table(s)\n",
                    args.rows,
                    tables.len()
                );
                print!(
                    "{}",
                    apex_detect::test_data::generate_inserts(&tables, args.rows)
                );
            }
        }
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Input validation helpers
// ---------------------------------------------------------------------------

/// Validate a git ref supplied by the user (e.g. `--base` flag).
///
/// Rejects values that could be mistaken for git flags or trigger path
/// traversal / shell metacharacter injection:
/// - Starts with `-` (flag injection, CWE-88)
/// - Contains `..` (path traversal in git refs)
/// - Contains shell metacharacters: `` ` ~ ! $ & * ( ) [ ] { } | ; < > ? \ ' " ``
fn validate_git_ref(s: &str) -> Result<()> {
    if s.starts_with('-') {
        return Err(color_eyre::eyre::eyre!(
            "invalid git ref {:?}: refs must not start with '-'",
            s
        ));
    }
    if s.contains("..") {
        return Err(color_eyre::eyre::eyre!(
            "invalid git ref {:?}: refs must not contain '..'",
            s
        ));
    }
    // Tilde (~) is a valid git ref character (e.g. HEAD~1) so it is NOT banned.
    const SHELL_META: &[char] = &[
        '`', '!', '$', '&', '*', '(', ')', '[', ']', '{', '}', '|', ';', '<', '>', '?', '\\',
        '\'', '"', ' ', '\t', '\n',
    ];
    if let Some(bad) = s.chars().find(|c| SHELL_META.contains(c)) {
        return Err(color_eyre::eyre::eyre!(
            "invalid git ref {:?}: contains disallowed character '{}'",
            s,
            bad
        ));
    }
    Ok(())
}

/// Validate and canonicalize a user-supplied output file path.
///
/// Canonicalizes the *parent* directory so that the resolved path is
/// absolute and free of `..` components. Returns an error if the parent
/// directory does not exist.
fn validate_output_path(p: &std::path::Path) -> Result<PathBuf> {
    let parent = p.parent().unwrap_or(std::path::Path::new("."));
    let canon_parent = parent.canonicalize().map_err(|e| {
        color_eyre::eyre::eyre!(
            "output path {:?}: parent directory does not exist or is not accessible: {e}",
            p
        )
    })?;
    let file_name = p
        .file_name()
        .ok_or_else(|| color_eyre::eyre::eyre!("output path {:?}: no file name", p))?;
    Ok(canon_parent.join(file_name))
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lang_arg_python() {
        let lang: Language = LangArg::Python.into();
        assert_eq!(lang, Language::Python);
    }

    #[test]
    fn lang_arg_js() {
        let lang: Language = LangArg::Js.into();
        assert_eq!(lang, Language::JavaScript);
    }

    #[test]
    fn lang_arg_java() {
        let lang: Language = LangArg::Java.into();
        assert_eq!(lang, Language::Java);
    }

    #[test]
    fn lang_arg_c() {
        let lang: Language = LangArg::C.into();
        assert_eq!(lang, Language::C);
    }

    #[test]
    fn lang_arg_rust() {
        let lang: Language = LangArg::Rust.into();
        assert_eq!(lang, Language::Rust);
    }

    #[test]
    fn lang_arg_wasm() {
        let lang: Language = LangArg::Wasm.into();
        assert_eq!(lang, Language::Wasm);
    }

    #[test]
    fn fuzz_command_uses_explicit_cmd() {
        let args = RunArgs {
            target: PathBuf::from("/tmp"),
            lang: LangArg::C,
            coverage_target: Some(0.9),
            strategy: "fuzz".into(),
            output: None,
            output_format: Some(OutputFormat::Text),
            no_install: false,
            rounds: Some(5),
            fuzz_iters: Some(10000),
            fuzz_cmd: vec!["./my_binary".into(), "--arg".into()],
        };
        let cmd = fuzz_command(&args, std::path::Path::new("/repo"));
        assert_eq!(cmd, vec!["./my_binary", "--arg"]);
    }

    #[test]
    fn fuzz_command_defaults_to_apex_target() {
        let args = RunArgs {
            target: PathBuf::from("/tmp"),
            lang: LangArg::C,
            coverage_target: Some(0.9),
            strategy: "fuzz".into(),
            output: None,
            output_format: Some(OutputFormat::Text),
            no_install: false,
            rounds: Some(5),
            fuzz_iters: Some(10000),
            fuzz_cmd: vec![],
        };
        let cmd = fuzz_command(&args, std::path::Path::new("/repo"));
        assert_eq!(cmd, vec!["/repo/apex_target"]);
    }

    #[test]
    fn print_gap_report_full_coverage() {
        let oracle = CoverageOracle::new();
        // No branches -> full coverage
        print_gap_report(&oracle, &HashMap::new(), std::path::Path::new("/tmp"));
    }

    #[test]
    fn print_gap_report_with_uncovered() {
        let oracle = CoverageOracle::new();
        let b = apex_core::types::BranchId::new(42, 10, 0, 0);
        oracle.register_branches([b]);
        let mut paths = HashMap::new();
        paths.insert(42u64, PathBuf::from("src/main.py"));
        print_gap_report(&oracle, &paths, std::path::Path::new("/nonexistent"));
    }

    #[test]
    fn print_gap_report_unknown_file_id() {
        let oracle = CoverageOracle::new();
        let b = apex_core::types::BranchId::new(999, 5, 0, 1);
        oracle.register_branches([b]);
        // No file_paths entry -> should show hex ID
        print_gap_report(&oracle, &HashMap::new(), std::path::Path::new("/tmp"));
    }

    #[test]
    fn print_json_gap_report_empty() {
        let oracle = CoverageOracle::new();
        print_json_gap_report(&oracle, &HashMap::new(), std::path::Path::new("/tmp"));
    }

    #[test]
    fn print_json_gap_report_with_branches() {
        let oracle = CoverageOracle::new();
        let b0 = apex_core::types::BranchId::new(1, 10, 0, 0);
        let b1 = apex_core::types::BranchId::new(1, 10, 0, 1);
        oracle.register_branches([b0, b1]);
        let mut paths = HashMap::new();
        paths.insert(1u64, PathBuf::from("test.py"));
        print_json_gap_report(&oracle, &paths, std::path::Path::new("/nonexistent"));
    }

    #[test]
    fn print_gap_report_with_source_lines() {
        // Create a real temporary file so the gap report can read source lines
        let tmp = tempfile::tempdir().unwrap();
        let src_path = tmp.path().join("mod.py");
        std::fs::write(
            &src_path,
            "line1\nline2\nif x > 0:\n    y = 1\nelse:\n    y = 2\n",
        )
        .unwrap();

        let oracle = CoverageOracle::new();
        let file_id = 1u64;
        let b = apex_core::types::BranchId::new(file_id, 3, 0, 0);
        oracle.register_branches([b]);

        let mut paths = HashMap::new();
        paths.insert(file_id, PathBuf::from("mod.py"));
        // Pass the temp dir as target_root so the file can be found
        print_gap_report(&oracle, &paths, tmp.path());
    }

    #[test]
    fn print_json_gap_report_with_source_lines() {
        // Same as above but for JSON output
        let tmp = tempfile::tempdir().unwrap();
        let src_path = tmp.path().join("app.py");
        std::fs::write(&src_path, "first\nsecond\nif True:\n    pass\n").unwrap();

        let oracle = CoverageOracle::new();
        let file_id = 42u64;
        let b = apex_core::types::BranchId::new(file_id, 3, 0, 0);
        oracle.register_branches([b]);

        let mut paths = HashMap::new();
        paths.insert(file_id, PathBuf::from("app.py"));
        print_json_gap_report(&oracle, &paths, tmp.path());
    }

    #[test]
    fn print_gap_report_with_covered_and_uncovered() {
        let oracle = CoverageOracle::new();
        let b0 = apex_core::types::BranchId::new(1, 5, 0, 0);
        let b1 = apex_core::types::BranchId::new(1, 5, 0, 1);
        let b2 = apex_core::types::BranchId::new(1, 10, 0, 0);
        oracle.register_branches([b0.clone(), b1, b2]);
        // Cover one branch
        oracle.mark_covered(&b0, apex_core::types::SeedId::new());

        let mut paths = HashMap::new();
        paths.insert(1u64, PathBuf::from("src/lib.py"));
        print_gap_report(&oracle, &paths, std::path::Path::new("/tmp"));
    }

    #[test]
    fn print_json_gap_report_with_unknown_file_id() {
        let oracle = CoverageOracle::new();
        let b = apex_core::types::BranchId::new(0xDEAD, 42, 0, 0);
        oracle.register_branches([b]);
        // No file_paths entry -> JSON should use hex ID
        print_json_gap_report(&oracle, &HashMap::new(), std::path::Path::new("/tmp"));
    }

    #[test]
    fn print_gap_report_false_branch_direction() {
        let oracle = CoverageOracle::new();
        // direction = 1 -> "false-branch" label
        let b = apex_core::types::BranchId::new(1, 7, 0, 1);
        oracle.register_branches([b]);
        let mut paths = HashMap::new();
        paths.insert(1u64, PathBuf::from("x.py"));
        print_gap_report(&oracle, &paths, std::path::Path::new("/nonexistent"));
    }

    #[test]
    fn print_json_gap_report_direction_values() {
        let oracle = CoverageOracle::new();
        let b0 = apex_core::types::BranchId::new(1, 10, 0, 0); // direction 0 -> "true"
        let b1 = apex_core::types::BranchId::new(1, 10, 0, 1); // direction 1 -> "false"
        oracle.register_branches([b0, b1]);
        let mut paths = HashMap::new();
        paths.insert(1u64, PathBuf::from("code.py"));
        print_json_gap_report(&oracle, &paths, std::path::Path::new("/tmp"));
    }

    #[test]
    fn print_gap_report_line_out_of_range() {
        // When branch line is beyond file length, should print without source line
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("short.py"), "x = 1\n").unwrap();

        let oracle = CoverageOracle::new();
        let b = apex_core::types::BranchId::new(1, 999, 0, 0); // line 999 in 1-line file
        oracle.register_branches([b]);
        let mut paths = HashMap::new();
        paths.insert(1u64, PathBuf::from("short.py"));
        print_gap_report(&oracle, &paths, tmp.path());
    }

    #[test]
    fn print_gap_report_multiple_files() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("a.py"), "if True:\n    pass\n").unwrap();
        std::fs::write(tmp.path().join("b.py"), "if False:\n    pass\n").unwrap();

        let oracle = CoverageOracle::new();
        let b1 = apex_core::types::BranchId::new(1, 1, 0, 0);
        let b2 = apex_core::types::BranchId::new(2, 1, 0, 0);
        oracle.register_branches([b1, b2]);

        let mut paths = HashMap::new();
        paths.insert(1u64, PathBuf::from("a.py"));
        paths.insert(2u64, PathBuf::from("b.py"));
        print_gap_report(&oracle, &paths, tmp.path());
    }

    #[test]
    fn print_gap_report_file_cache_reuse() {
        // Multiple branches in same file should reuse the cached file content
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(
            tmp.path().join("multi.py"),
            "line1\nif x:\n    pass\nif y:\n    pass\n",
        )
        .unwrap();

        let oracle = CoverageOracle::new();
        let b1 = apex_core::types::BranchId::new(1, 2, 0, 0);
        let b2 = apex_core::types::BranchId::new(1, 4, 0, 0);
        oracle.register_branches([b1, b2]);

        let mut paths = HashMap::new();
        paths.insert(1u64, PathBuf::from("multi.py"));
        print_gap_report(&oracle, &paths, tmp.path());
    }

    // -----------------------------------------------------------------------
    // Bug report output tests
    // -----------------------------------------------------------------------

    #[test]
    fn print_bug_report_empty() {
        let summary = apex_core::types::BugSummary::default();
        print_bug_report(&summary);
    }

    #[test]
    fn print_bug_report_with_bugs() {
        let reports = vec![
            apex_core::types::BugReport::new(
                apex_core::types::BugClass::Crash,
                apex_core::types::SeedId::new(),
                "segfault at src/main.rs:42".into(),
            ),
            apex_core::types::BugReport::new(
                apex_core::types::BugClass::Timeout,
                apex_core::types::SeedId::new(),
                "timed out after 30s".into(),
            ),
        ];
        let summary = apex_core::types::BugSummary::new(reports);
        print_bug_report(&summary);
    }

    #[test]
    fn print_json_bug_report_empty() {
        let summary = apex_core::types::BugSummary::default();
        print_json_bug_report(&summary);
    }

    #[test]
    fn print_json_bug_report_with_bugs() {
        let mut report = apex_core::types::BugReport::new(
            apex_core::types::BugClass::AssertionFailure,
            apex_core::types::SeedId::new(),
            "assert failed: x != 0".into(),
        );
        report.location = Some("tests/test_foo.py:10".into());
        report.discovered_at_iteration = 42;
        let summary = apex_core::types::BugSummary::new(vec![report]);
        print_json_bug_report(&summary);
    }

    // -- Bug-exposing tests for walkdir / build_source_cache -----------------

    fn create_test_tree(dirs: &[&str], files: &[&str]) -> tempfile::TempDir {
        let tmp = tempfile::tempdir().unwrap();
        for d in dirs {
            std::fs::create_dir_all(tmp.path().join(d)).unwrap();
        }
        for f in files {
            let p = tmp.path().join(f);
            if let Some(parent) = p.parent() {
                std::fs::create_dir_all(parent).unwrap();
            }
            std::fs::write(&p, "// content").unwrap();
        }
        tmp
    }

    #[test]
    fn bug_walkdir_skips_venv() {
        let tmp = create_test_tree(&["src", "venv/lib"], &["src/main.py", "venv/lib/dep.py"]);
        let files = walkdir(tmp.path(), &["py"]).unwrap();
        let names: Vec<_> = files
            .iter()
            .map(|p| p.file_name().unwrap().to_str().unwrap())
            .collect();
        assert!(names.contains(&"main.py"), "should find src/main.py");
        assert!(!names.contains(&"dep.py"), "should skip venv/lib/dep.py");
    }

    #[test]
    fn bug_walkdir_skips_pycache() {
        let tmp = create_test_tree(
            &["src", "__pycache__"],
            &["src/app.py", "__pycache__/app.cpython-311.pyc"],
        );
        let files = walkdir(tmp.path(), &["py", "pyc"]).unwrap();
        assert_eq!(
            files.len(),
            1,
            "should only find src/app.py, not __pycache__ files"
        );
    }

    #[test]
    fn bug_walkdir_skips_dist_and_build() {
        let tmp = create_test_tree(
            &["src", "dist", "build"],
            &["src/lib.rs", "dist/bundle.js", "build/output.js"],
        );
        let files = walkdir(tmp.path(), &["rs", "js"]).unwrap();
        let names: Vec<_> = files
            .iter()
            .map(|p| p.file_name().unwrap().to_str().unwrap())
            .collect();
        assert!(names.contains(&"lib.rs"));
        assert!(!names.contains(&"bundle.js"), "should skip dist/");
        assert!(!names.contains(&"output.js"), "should skip build/");
    }

    #[test]
    fn bug_source_cache_js_includes_jsx_tsx() {
        let tmp = create_test_tree(
            &["src"],
            &[
                "src/App.jsx",
                "src/Page.tsx",
                "src/index.js",
                "src/util.mjs",
            ],
        );
        let cache = build_source_cache(tmp.path(), Language::JavaScript);
        assert!(
            cache.contains_key(&PathBuf::from("src/App.jsx")),
            "should include .jsx"
        );
        assert!(
            cache.contains_key(&PathBuf::from("src/Page.tsx")),
            "should include .tsx"
        );
        assert!(
            cache.contains_key(&PathBuf::from("src/index.js")),
            "should include .js"
        );
        assert!(
            cache.contains_key(&PathBuf::from("src/util.mjs")),
            "should include .mjs"
        );
    }

    #[test]
    fn bug_source_cache_c_includes_cpp() {
        let tmp = create_test_tree(
            &["src"],
            &["src/main.c", "src/util.cpp", "src/lib.cc", "src/types.hpp"],
        );
        let cache = build_source_cache(tmp.path(), Language::C);
        assert!(
            cache.contains_key(&PathBuf::from("src/main.c")),
            "should include .c"
        );
        assert!(
            cache.contains_key(&PathBuf::from("src/util.cpp")),
            "should include .cpp"
        );
        assert!(
            cache.contains_key(&PathBuf::from("src/lib.cc")),
            "should include .cc"
        );
        assert!(
            cache.contains_key(&PathBuf::from("src/types.hpp")),
            "should include .hpp"
        );
    }

    #[test]
    fn bug_walkdir_case_insensitive_extensions() {
        let tmp = create_test_tree(&["src"], &["src/main.RS", "src/lib.rs", "src/Mod.Rs"]);
        let files = walkdir(tmp.path(), &["rs"]).unwrap();
        assert_eq!(
            files.len(),
            3,
            "should match .RS, .rs, and .Rs case-insensitively"
        );
    }

    // -----------------------------------------------------------------------
    // validate_git_ref tests
    // -----------------------------------------------------------------------

    #[test]
    fn validate_git_ref_rejects_flag_prefix() {
        assert!(
            validate_git_ref("--exec").is_err(),
            "--exec should be rejected"
        );
        assert!(
            validate_git_ref("-c").is_err(),
            "-c should be rejected"
        );
    }

    #[test]
    fn validate_git_ref_rejects_dotdot() {
        assert!(
            validate_git_ref("../hack").is_err(),
            "../hack should be rejected"
        );
        assert!(
            validate_git_ref("main..evil").is_err(),
            "double-dot should be rejected"
        );
    }

    #[test]
    fn validate_git_ref_accepts_valid_refs() {
        assert!(validate_git_ref("main").is_ok());
        assert!(validate_git_ref("HEAD~1").is_ok());
        assert!(validate_git_ref("v0.2.0").is_ok());
        assert!(validate_git_ref("feature/my-branch").is_ok());
        assert!(validate_git_ref("refs/heads/main").is_ok());
    }

    // -----------------------------------------------------------------------
    // validate_output_path tests
    // -----------------------------------------------------------------------

    #[test]
    fn validate_output_path_rejects_nonexistent_parent() {
        let p = std::path::Path::new("/nonexistent-dir-apex-test/out.txt");
        assert!(
            validate_output_path(p).is_err(),
            "nonexistent parent should be rejected"
        );
    }

    #[test]
    fn validate_output_path_accepts_valid_path() {
        let tmp = tempfile::tempdir().unwrap();
        let out = tmp.path().join("report.txt");
        let result = validate_output_path(&out);
        assert!(result.is_ok(), "valid path should be accepted");
        // Returned path should be absolute
        assert!(result.unwrap().is_absolute());
    }
}
