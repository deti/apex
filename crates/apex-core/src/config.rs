//! Layered configuration system.
//!
//! `ApexConfig` is the top-level struct deserialized from `apex.toml`.
//! Each section maps to a domain-specific sub-struct with serde defaults.
//! The precedence chain is: CLI args > config file > struct defaults.

use serde::{Deserialize, Serialize};
use std::path::Path;

/// Top-level APEX configuration.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct ApexConfig {
    pub coverage: CoverageConfig,
    pub fuzz: FuzzConfig,
    pub concolic: ConcolicConfig,
    pub agent: AgentConfig,
    pub sandbox: SandboxConfig,
    pub symbolic: SymbolicConfig,
    pub instrument: InstrumentConfig,
    pub logging: LoggingConfig,
    pub detect: DetectConfig,
    pub threat_model: ThreatModelConfig,
    pub analyze: AnalyzeConfig,
    pub index: IndexConfig,
    pub reach: ReachConfig,
    pub cpg: CpgConfig,
    pub synth: SynthConfig,
}

impl ApexConfig {
    /// Load config from a TOML file, falling back to defaults for missing fields.
    pub fn from_file(path: &Path) -> crate::Result<Self> {
        let contents = std::fs::read_to_string(path).map_err(|e| {
            crate::ApexError::Config(format!("cannot read {}: {e}", path.display()))
        })?;
        Self::parse_toml(&contents)
    }

    /// Parse config from a TOML string.
    pub fn parse_toml(s: &str) -> crate::Result<Self> {
        let cfg: Self = toml::from_str(s)
            .map_err(|e| crate::ApexError::Config(format!("invalid TOML: {e}")))?;
        Ok(cfg.validate())
    }

    /// Clamp coverage fields to valid 0.0–1.0 range.
    fn validate(mut self) -> Self {
        self.coverage.target = self.coverage.target.clamp(0.0, 1.0);
        self.coverage.min_ratchet = self.coverage.min_ratchet.clamp(0.0, 1.0);
        self
    }

    /// Try to discover and load `apex.toml` from the given directory (or parents).
    /// Returns the default config if no file is found, or an error if the file
    /// exists but cannot be parsed.
    pub fn discover(start_dir: &Path) -> crate::Result<Self> {
        let mut dir = start_dir;
        loop {
            let candidate = dir.join("apex.toml");
            if candidate.is_file() {
                let cfg = Self::from_file(&candidate)?;
                tracing::info!(path = %candidate.display(), "loaded apex.toml");
                return Ok(cfg);
            }
            match dir.parent() {
                Some(parent) => dir = parent,
                None => break,
            }
        }
        Ok(Self::default())
    }
}

// ---------------------------------------------------------------------------
// Coverage
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct CoverageConfig {
    /// Target coverage ratio (0.0–1.0). Default: 1.0 (100%).
    pub target: f64,
    /// Minimum coverage for `apex ratchet` CI gate. Default: 0.8.
    pub min_ratchet: f64,
    /// Directory name patterns to omit when collecting source files.
    pub omit_patterns: Vec<String>,
}

impl CoverageConfig {
    /// Default directory omit patterns.
    pub fn default_omit_patterns() -> Vec<String> {
        vec![
            "target".into(),
            "node_modules".into(),
            "__pycache__".into(),
            ".venv".into(),
            "venv".into(),
            "dist".into(),
            "build".into(),
        ]
    }
}

impl Default for CoverageConfig {
    fn default() -> Self {
        CoverageConfig {
            target: 0.95,
            min_ratchet: 0.8,
            omit_patterns: Self::default_omit_patterns(),
        }
    }
}

// ---------------------------------------------------------------------------
// Fuzzing
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct FuzzConfig {
    /// Maximum corpus size (LRU eviction). Default: 10_000.
    pub corpus_max: usize,
    /// Mutated inputs per scheduler tick. Default: 8.
    pub mutations_per_input: usize,
    /// Iterations without new coverage before stopping. Default: 50.
    pub stall_iterations: usize,
    /// Minimum random seed length (bootstrap). Default: 1.
    pub seed_len_min: usize,
    /// Maximum random seed length (bootstrap). Default: 64.
    pub seed_len_max: usize,
    /// MOpt scheduler settings.
    pub scheduler: SchedulerConfig,
    /// Particle Swarm Optimization settings.
    pub pso: PsoConfig,
    /// Fox optimizer settings.
    pub fox: FoxConfig,
    /// Semantic scoring weights.
    pub semantic: SemanticConfig,
    /// Thompson sampling beta distribution cap. Default: 50.0.
    pub beta_cap: f64,
    /// CmpLog ring buffer max entries. Default: 256.
    pub cmplog_ring_max: usize,
}

impl Default for FuzzConfig {
    fn default() -> Self {
        FuzzConfig {
            corpus_max: 10_000,
            mutations_per_input: 8,
            stall_iterations: 50,
            seed_len_min: 1,
            seed_len_max: 64,
            scheduler: SchedulerConfig::default(),
            pso: PsoConfig::default(),
            fox: FoxConfig::default(),
            semantic: SemanticConfig::default(),
            beta_cap: 50.0,
            cmplog_ring_max: 256,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct SchedulerConfig {
    /// Minimum EMA yield to prevent zero weights. Default: 0.01.
    pub floor: f64,
    /// EMA smoothing factor. Default: 0.1.
    pub alpha: f64,
}

impl Default for SchedulerConfig {
    fn default() -> Self {
        SchedulerConfig {
            floor: 0.01,
            alpha: 0.1,
        }
    }
}

/// Particle Swarm Optimization tuning.
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct PsoConfig {
    /// Inertia weight. Default: 0.7.
    pub w: f64,
    /// Cognitive coefficient. Default: 1.5.
    pub c1: f64,
    /// Social coefficient. Default: 1.5.
    pub c2: f64,
    /// Minimum mutation probability. Default: 0.01.
    pub prob_min: f64,
}

impl Default for PsoConfig {
    fn default() -> Self {
        PsoConfig {
            w: 0.7,
            c1: 1.5,
            c2: 1.5,
            prob_min: 0.01,
        }
    }
}

/// Fox optimizer tuning.
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct FoxConfig {
    /// Mutation probability. Default: 0.5.
    pub mutation_rate: f64,
    /// Exploration vs exploitation ratio. Default: 0.5.
    pub exploration_rate: f64,
    /// Learning rate. Default: 0.1.
    pub alpha: f64,
}

impl Default for FoxConfig {
    fn default() -> Self {
        FoxConfig {
            mutation_rate: 0.5,
            exploration_rate: 0.5,
            alpha: 0.1,
        }
    }
}

/// Semantic scoring weights for the fuzzer scheduler.
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct SemanticConfig {
    /// Weight applied to branch-coverage score. Default: 1.0.
    pub branch_weight: f64,
    /// Weight applied to semantic-similarity score. Default: 0.5.
    pub semantic_weight: f64,
}

impl Default for SemanticConfig {
    fn default() -> Self {
        SemanticConfig {
            branch_weight: 1.0,
            semantic_weight: 0.5,
        }
    }
}

// ---------------------------------------------------------------------------
// Concolic
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct ConcolicConfig {
    /// Maximum concolic exploration rounds. Default: 5.
    pub max_rounds: usize,
}

impl Default for ConcolicConfig {
    fn default() -> Self {
        ConcolicConfig { max_rounds: 5 }
    }
}

// ---------------------------------------------------------------------------
// Agent / Orchestrator
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct AgentConfig {
    /// Stale iterations before declaring stall. Default: 10.
    pub stall_threshold: u64,
    /// Maximum AI agent rounds. Default: 3.
    pub max_rounds: usize,
    /// Maximum refinement rounds in pipeline. Default: 3.
    pub max_refinement_rounds: usize,
    /// Lines of source context around uncovered branches. Default: 15.
    pub source_context_lines: u32,
    /// Max source files per agent prompt round. Default: 3.
    pub max_files_per_round: usize,
    /// Hard deadline for the agent exploration loop in seconds.
    /// When `Some`, the orchestrator exits after this many seconds.
    /// When `None`, a 30-minute cap (1800s) is used by default to prevent
    /// runaway loops caused by per-iteration timeout * iteration-count math.
    pub deadline_secs: Option<u64>,
    /// Coverage monitor sliding-window size for trend detection. Default: 10.
    pub monitor_window_size: usize,
}

impl Default for AgentConfig {
    fn default() -> Self {
        AgentConfig {
            stall_threshold: 10,
            max_rounds: 3,
            max_refinement_rounds: 3,
            source_context_lines: 15,
            max_files_per_round: 3,
            deadline_secs: None,
            monitor_window_size: 10,
        }
    }
}

// ---------------------------------------------------------------------------
// Sandbox / Timeouts
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct SandboxConfig {
    /// Process sandbox timeout (ms). Default: 10_000.
    pub process_timeout_ms: u64,
    /// Python test sandbox timeout (ms). Default: 30_000.
    pub python_timeout_ms: u64,
    /// General command timeout (ms). Default: 30_000.
    pub command_timeout_ms: u64,
}

impl Default for SandboxConfig {
    fn default() -> Self {
        SandboxConfig {
            process_timeout_ms: 10_000,
            python_timeout_ms: 30_000,
            command_timeout_ms: 30_000,
        }
    }
}

// ---------------------------------------------------------------------------
// Symbolic
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct SymbolicConfig {
    /// Maximum constraint chain depth. Default: 64.
    pub max_depth: usize,
}

impl Default for SymbolicConfig {
    fn default() -> Self {
        SymbolicConfig { max_depth: 64 }
    }
}

// ---------------------------------------------------------------------------
// Instrumentation
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct InstrumentConfig {
    /// Coverage bitmap size (edges). Default: 65_536.
    pub bitmap_size: usize,
    /// Language-specific compile and test timeouts.
    pub timeouts: InstrumentTimeouts,
}

impl Default for InstrumentConfig {
    fn default() -> Self {
        InstrumentConfig {
            bitmap_size: 65_536,
            timeouts: InstrumentTimeouts::default(),
        }
    }
}

/// Per-language instrumentation timeout values (milliseconds).
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct InstrumentTimeouts {
    /// C/C++ compile timeout (ms). Default: 300_000.
    pub c_compile_ms: u64,
    /// C/C++ test run timeout (ms). Default: 120_000.
    pub c_test_ms: u64,
    /// C/C++ gcov collection timeout (ms). Default: 60_000.
    pub c_gcov_ms: u64,
    /// C#/.NET instrumentation timeout (ms). Default: 600_000.
    pub csharp_ms: u64,
    /// C# NuGet restore timeout (ms). Default: 300_000.
    pub csharp_restore_ms: u64,
    /// Swift coverage test timeout (ms). Default: 600_000.
    pub swift_test_ms: u64,
    /// Swift codecov path-resolution timeout (ms). Default: 60_000.
    pub swift_codecov_ms: u64,
    /// Swift package resolve timeout (ms). Default: 300_000.
    pub swift_resolve_ms: u64,
    /// JVM (Java/Kotlin) build timeout (ms). Default: 600_000.
    pub jvm_build_ms: u64,
}

impl Default for InstrumentTimeouts {
    fn default() -> Self {
        InstrumentTimeouts {
            c_compile_ms: 300_000,
            c_test_ms: 120_000,
            c_gcov_ms: 60_000,
            csharp_ms: 600_000,
            csharp_restore_ms: 300_000,
            swift_test_ms: 600_000,
            swift_codecov_ms: 60_000,
            swift_resolve_ms: 300_000,
            jvm_build_ms: 600_000,
        }
    }
}

// ---------------------------------------------------------------------------
// Logging
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct LoggingConfig {
    /// Log level filter. Default: "info".
    pub level: String,
    /// Output format: "text" or "json". Default: "text".
    pub format: String,
}

impl Default for LoggingConfig {
    fn default() -> Self {
        LoggingConfig {
            level: "info".into(),
            format: "text".into(),
        }
    }
}

// ---------------------------------------------------------------------------
// Detection / Analytics
// ---------------------------------------------------------------------------

/// Lightweight detection config stored in apex.toml.
/// The full DetectConfig with sub-structs lives in `apex-detect`.
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct DetectConfig {
    pub enabled: Vec<String>,
    #[serde(default = "default_detect_severity")]
    pub severity_threshold: String,
    pub per_detector_timeout_secs: Option<u64>,
    /// Secret scan Shannon entropy threshold. Default: 5.0.
    pub entropy_threshold: f64,
    /// Max subprocess concurrency for the detector pipeline. Default: 4.
    pub max_subprocess_concurrency: usize,
    /// Lines of source context captured around each finding. Default: 3.
    pub context_window: usize,
}

impl Default for DetectConfig {
    fn default() -> Self {
        DetectConfig {
            enabled: Vec::new(),
            severity_threshold: "low".into(),
            per_detector_timeout_secs: None,
            entropy_threshold: 5.0,
            max_subprocess_concurrency: 4,
            context_window: 3,
        }
    }
}

fn default_detect_severity() -> String {
    "low".into()
}

// ---------------------------------------------------------------------------
// Threat Model
// ---------------------------------------------------------------------------

/// What kind of software is being analyzed.
/// Determines which input sources are considered trusted vs untrusted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ThreatModelType {
    /// CLI tool — argv, env vars, config files are trusted.
    CliTool,
    /// Web service — request data is untrusted, env/config are trusted.
    WebService,
    /// Library — all external input is untrusted.
    Library,
    /// CI pipeline — env vars and argv are trusted, network input is not.
    CiPipeline,
}

/// Threat model configuration from `[threat_model]` in apex.toml.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct ThreatModelConfig {
    /// The type of software being analyzed.
    #[serde(rename = "type")]
    pub model_type: Option<ThreatModelType>,
    /// Additional sources the user considers trusted (beyond the defaults for this type).
    pub trusted_sources: Vec<String>,
    /// Additional sources the user considers untrusted (overrides defaults).
    pub untrusted_sources: Vec<String>,
}

// ---------------------------------------------------------------------------
// Analyze (compound analysis pipeline)
// ---------------------------------------------------------------------------

/// Configuration for the compound analysis pipeline (`[analyze]` in apex.toml).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AnalyzeConfig {
    /// Whether compound analysis is enabled. Default: true.
    pub enabled: bool,
    /// Analyzer names to skip (e.g. `["iac-scan", "slo-check"]`).
    pub skip: Vec<String>,
    /// Per-analyzer timeout in seconds.
    pub timeout_secs: Option<u64>,
}

impl Default for AnalyzeConfig {
    fn default() -> Self {
        AnalyzeConfig {
            enabled: true,
            skip: Vec::new(),
            timeout_secs: None,
        }
    }
}

// ---------------------------------------------------------------------------
// Index
// ---------------------------------------------------------------------------

/// Source file indexing limits (`[index]` in apex.toml).
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct IndexConfig {
    /// Maximum number of source files to scan. Default: 10_000.
    pub max_source_files: usize,
    /// Maximum bytes read per source file. Default: 1_048_576 (1 MB).
    pub max_source_file_bytes: usize,
}

impl Default for IndexConfig {
    fn default() -> Self {
        IndexConfig {
            max_source_files: 10_000,
            max_source_file_bytes: 1_048_576,
        }
    }
}

// ---------------------------------------------------------------------------
// Reach
// ---------------------------------------------------------------------------

/// Call-graph reachability analysis settings (`[reach]` in apex.toml).
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct ReachConfig {
    /// Maximum call graph traversal depth. Default: 20.
    pub max_depth: usize,
}

impl Default for ReachConfig {
    fn default() -> Self {
        ReachConfig { max_depth: 20 }
    }
}

// ---------------------------------------------------------------------------
// CPG
// ---------------------------------------------------------------------------

/// Code Property Graph query limits (`[cpg]` in apex.toml).
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct CpgConfig {
    /// Maximum rows returned by CPG queries. Default: 100_000.
    pub max_query_rows: usize,
}

impl Default for CpgConfig {
    fn default() -> Self {
        CpgConfig {
            max_query_rows: 100_000,
        }
    }
}

// ---------------------------------------------------------------------------
// Synth
// ---------------------------------------------------------------------------

/// Test synthesis LLM prompt settings (`[synth]` in apex.toml).
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct SynthConfig {
    /// Maximum test candidates generated per prompt chunk. Default: 20.
    pub chunk_size: usize,
    /// Maximum uncovered branches included per LLM prompt. Default: 20.
    pub max_branches_in_prompt: usize,
}

impl Default for SynthConfig {
    fn default() -> Self {
        SynthConfig {
            chunk_size: 20,
            max_branches_in_prompt: 20,
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_has_expected_values() {
        let cfg = ApexConfig::default();
        assert!((cfg.coverage.target - 0.95).abs() < f64::EPSILON);
        assert_eq!(cfg.coverage.min_ratchet, 0.8);
        assert_eq!(cfg.fuzz.corpus_max, 10_000);
        assert_eq!(cfg.fuzz.mutations_per_input, 8);
        assert_eq!(cfg.fuzz.stall_iterations, 50);
        assert_eq!(cfg.concolic.max_rounds, 5);
        assert_eq!(cfg.agent.stall_threshold, 10);
        assert_eq!(cfg.sandbox.process_timeout_ms, 10_000);
        assert_eq!(cfg.symbolic.max_depth, 64);
        assert_eq!(cfg.instrument.bitmap_size, 65_536);
        assert_eq!(cfg.logging.level, "info");
        assert_eq!(cfg.detect.severity_threshold, "low");
        assert!(cfg.detect.per_detector_timeout_secs.is_none());

        // New fields — instrument timeouts
        assert_eq!(cfg.instrument.timeouts.c_compile_ms, 300_000);
        assert_eq!(cfg.instrument.timeouts.c_test_ms, 120_000);
        assert_eq!(cfg.instrument.timeouts.c_gcov_ms, 60_000);
        assert_eq!(cfg.instrument.timeouts.csharp_ms, 600_000);
        assert_eq!(cfg.instrument.timeouts.csharp_restore_ms, 300_000);
        assert_eq!(cfg.instrument.timeouts.swift_test_ms, 600_000);
        assert_eq!(cfg.instrument.timeouts.swift_codecov_ms, 60_000);
        assert_eq!(cfg.instrument.timeouts.swift_resolve_ms, 300_000);
        assert_eq!(cfg.instrument.timeouts.jvm_build_ms, 600_000);

        // New fields — index
        assert_eq!(cfg.index.max_source_files, 10_000);
        assert_eq!(cfg.index.max_source_file_bytes, 1_048_576);

        // New fields — detect
        assert!((cfg.detect.entropy_threshold - 5.0).abs() < f64::EPSILON);
        assert_eq!(cfg.detect.max_subprocess_concurrency, 4);
        assert_eq!(cfg.detect.context_window, 3);

        // New fields — fuzz PSO
        assert!((cfg.fuzz.pso.w - 0.7).abs() < f64::EPSILON);
        assert!((cfg.fuzz.pso.c1 - 1.5).abs() < f64::EPSILON);
        assert!((cfg.fuzz.pso.c2 - 1.5).abs() < f64::EPSILON);
        assert!((cfg.fuzz.pso.prob_min - 0.01).abs() < f64::EPSILON);

        // New fields — fuzz Fox
        assert!((cfg.fuzz.fox.mutation_rate - 0.5).abs() < f64::EPSILON);
        assert!((cfg.fuzz.fox.exploration_rate - 0.5).abs() < f64::EPSILON);
        assert!((cfg.fuzz.fox.alpha - 0.1).abs() < f64::EPSILON);

        // New fields — fuzz Semantic
        assert!((cfg.fuzz.semantic.branch_weight - 1.0).abs() < f64::EPSILON);
        assert!((cfg.fuzz.semantic.semantic_weight - 0.5).abs() < f64::EPSILON);

        // New fields — fuzz misc
        assert!((cfg.fuzz.beta_cap - 50.0).abs() < f64::EPSILON);
        assert_eq!(cfg.fuzz.cmplog_ring_max, 256);

        // New fields — agent
        assert_eq!(cfg.agent.monitor_window_size, 10);

        // New fields — reach
        assert_eq!(cfg.reach.max_depth, 20);

        // New fields — cpg
        assert_eq!(cfg.cpg.max_query_rows, 100_000);

        // New fields — synth
        assert_eq!(cfg.synth.chunk_size, 20);
        assert_eq!(cfg.synth.max_branches_in_prompt, 20);
    }

    #[test]
    fn parse_empty_toml_gives_defaults() {
        let cfg = ApexConfig::parse_toml("").unwrap();
        assert_eq!(cfg.fuzz.corpus_max, 10_000);
    }

    #[test]
    fn parse_partial_toml_fills_defaults() {
        let toml = r#"
[coverage]
target = 0.9

[fuzz]
corpus_max = 5000
"#;
        let cfg = ApexConfig::parse_toml(toml).unwrap();
        assert_eq!(cfg.coverage.target, 0.9);
        assert_eq!(cfg.coverage.min_ratchet, 0.8); // default
        assert_eq!(cfg.fuzz.corpus_max, 5000);
        assert_eq!(cfg.fuzz.mutations_per_input, 8); // default
    }

    #[test]
    fn parse_full_toml() {
        let toml = r#"
[coverage]
target = 0.95
min_ratchet = 0.7

[fuzz]
corpus_max = 20000
mutations_per_input = 16
stall_iterations = 100
seed_len_min = 4
seed_len_max = 128
beta_cap = 100.0
cmplog_ring_max = 512

[fuzz.scheduler]
floor = 0.02
alpha = 0.2

[fuzz.pso]
w = 0.8
c1 = 2.0
c2 = 2.0
prob_min = 0.05

[fuzz.fox]
mutation_rate = 0.3
exploration_rate = 0.7
alpha = 0.2

[fuzz.semantic]
branch_weight = 2.0
semantic_weight = 1.0

[concolic]
max_rounds = 10

[agent]
stall_threshold = 20
max_rounds = 5
max_refinement_rounds = 5
source_context_lines = 25
max_files_per_round = 5
monitor_window_size = 20

[sandbox]
process_timeout_ms = 5000
python_timeout_ms = 60000
command_timeout_ms = 15000

[symbolic]
max_depth = 128

[instrument]
bitmap_size = 131072

[instrument.timeouts]
c_compile_ms = 180000
c_test_ms = 90000
c_gcov_ms = 30000
csharp_ms = 300000
csharp_restore_ms = 120000
swift_test_ms = 300000
swift_codecov_ms = 30000
swift_resolve_ms = 120000
jvm_build_ms = 480000

[logging]
level = "debug"
format = "json"

[detect]
entropy_threshold = 4.5
max_subprocess_concurrency = 8
context_window = 5

[index]
max_source_files = 50000
max_source_file_bytes = 2097152

[reach]
max_depth = 30

[cpg]
max_query_rows = 200000

[synth]
chunk_size = 10
max_branches_in_prompt = 15
"#;
        let cfg = ApexConfig::parse_toml(toml).unwrap();
        assert_eq!(cfg.coverage.target, 0.95);
        assert_eq!(cfg.fuzz.corpus_max, 20000);
        assert_eq!(cfg.fuzz.scheduler.floor, 0.02);
        assert!((cfg.fuzz.beta_cap - 100.0).abs() < f64::EPSILON);
        assert_eq!(cfg.fuzz.cmplog_ring_max, 512);
        assert!((cfg.fuzz.pso.w - 0.8).abs() < f64::EPSILON);
        assert!((cfg.fuzz.pso.c1 - 2.0).abs() < f64::EPSILON);
        assert!((cfg.fuzz.pso.prob_min - 0.05).abs() < f64::EPSILON);
        assert!((cfg.fuzz.fox.mutation_rate - 0.3).abs() < f64::EPSILON);
        assert!((cfg.fuzz.fox.exploration_rate - 0.7).abs() < f64::EPSILON);
        assert!((cfg.fuzz.semantic.branch_weight - 2.0).abs() < f64::EPSILON);
        assert!((cfg.fuzz.semantic.semantic_weight - 1.0).abs() < f64::EPSILON);
        assert_eq!(cfg.concolic.max_rounds, 10);
        assert_eq!(cfg.agent.stall_threshold, 20);
        assert_eq!(cfg.agent.monitor_window_size, 20);
        assert_eq!(cfg.sandbox.process_timeout_ms, 5000);
        assert_eq!(cfg.symbolic.max_depth, 128);
        assert_eq!(cfg.instrument.bitmap_size, 131072);
        assert_eq!(cfg.instrument.timeouts.c_compile_ms, 180000);
        assert_eq!(cfg.instrument.timeouts.c_test_ms, 90000);
        assert_eq!(cfg.instrument.timeouts.c_gcov_ms, 30000);
        assert_eq!(cfg.instrument.timeouts.csharp_ms, 300000);
        assert_eq!(cfg.instrument.timeouts.csharp_restore_ms, 120000);
        assert_eq!(cfg.instrument.timeouts.swift_test_ms, 300000);
        assert_eq!(cfg.instrument.timeouts.swift_codecov_ms, 30000);
        assert_eq!(cfg.instrument.timeouts.swift_resolve_ms, 120000);
        assert_eq!(cfg.instrument.timeouts.jvm_build_ms, 480000);
        assert_eq!(cfg.logging.level, "debug");
        assert_eq!(cfg.logging.format, "json");
        assert!((cfg.detect.entropy_threshold - 4.5).abs() < f64::EPSILON);
        assert_eq!(cfg.detect.max_subprocess_concurrency, 8);
        assert_eq!(cfg.detect.context_window, 5);
        assert_eq!(cfg.index.max_source_files, 50000);
        assert_eq!(cfg.index.max_source_file_bytes, 2097152);
        assert_eq!(cfg.reach.max_depth, 30);
        assert_eq!(cfg.cpg.max_query_rows, 200000);
        assert_eq!(cfg.synth.chunk_size, 10);
        assert_eq!(cfg.synth.max_branches_in_prompt, 15);
    }

    #[test]
    fn invalid_toml_returns_error() {
        assert!(ApexConfig::parse_toml("not valid toml [[[").is_err());
    }

    #[test]
    fn discover_returns_default_when_no_file() {
        let cfg = ApexConfig::discover(Path::new("/nonexistent/path")).unwrap();
        assert_eq!(cfg.fuzz.corpus_max, 10_000);
    }

    #[test]
    fn nested_scheduler_config() {
        let toml = r#"
[fuzz.scheduler]
floor = 0.05
alpha = 0.3
"#;
        let cfg = ApexConfig::parse_toml(toml).unwrap();
        assert_eq!(cfg.fuzz.scheduler.floor, 0.05);
        assert_eq!(cfg.fuzz.scheduler.alpha, 0.3);
        // Other fuzz fields should be default
        assert_eq!(cfg.fuzz.corpus_max, 10_000);
    }

    #[test]
    fn from_file_nonexistent_returns_error() {
        let result = ApexConfig::from_file(Path::new("/nonexistent/apex.toml"));
        assert!(result.is_err());
        let err = format!("{}", result.unwrap_err());
        assert!(err.contains("cannot read"), "error was: {err}");
    }

    #[test]
    fn from_file_valid_toml() {
        let dir = std::env::temp_dir().join("apex_test_config_valid");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("apex.toml");
        std::fs::write(&path, "[coverage]\ntarget = 0.5\n").unwrap();
        let cfg = ApexConfig::from_file(&path).unwrap();
        assert_eq!(cfg.coverage.target, 0.5);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn from_file_invalid_toml_returns_error() {
        let dir = std::env::temp_dir().join("apex_test_config_invalid");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("apex.toml");
        std::fs::write(&path, "not valid [[[").unwrap();
        let result = ApexConfig::from_file(&path);
        assert!(result.is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn discover_finds_file_in_directory() {
        let dir = std::env::temp_dir().join("apex_test_discover_find");
        let _ = std::fs::create_dir_all(&dir);
        std::fs::write(dir.join("apex.toml"), "[coverage]\ntarget = 0.42\n").unwrap();
        let cfg = ApexConfig::discover(&dir).unwrap();
        assert_eq!(cfg.coverage.target, 0.42);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn discover_with_invalid_file_returns_error() {
        let dir = std::env::temp_dir().join("apex_test_discover_invalid_err");
        let _ = std::fs::create_dir_all(&dir);
        std::fs::write(dir.join("apex.toml"), "this is not valid toml [[[").unwrap();
        let result = ApexConfig::discover(&dir);
        assert!(result.is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn detect_config_default_severity_threshold() {
        // Exercises the default_detect_severity() serde default function
        let cfg = DetectConfig::default();
        assert_eq!(cfg.severity_threshold, "low");
        assert!(cfg.enabled.is_empty());
        assert!(cfg.per_detector_timeout_secs.is_none());
    }

    #[test]
    fn detect_config_from_toml_uses_default_severity() {
        // When [detect] section exists but severity_threshold is absent,
        // the serde default function default_detect_severity() is called
        let toml = r#"
[detect]
enabled = ["unsafe", "deps"]
"#;
        let cfg = ApexConfig::parse_toml(toml).unwrap();
        assert_eq!(cfg.detect.severity_threshold, "low");
        assert_eq!(cfg.detect.enabled, vec!["unsafe", "deps"]);
    }

    #[test]
    fn detect_config_from_toml_with_explicit_severity() {
        let toml = r#"
[detect]
severity_threshold = "high"
per_detector_timeout_secs = 30
"#;
        let cfg = ApexConfig::parse_toml(toml).unwrap();
        assert_eq!(cfg.detect.severity_threshold, "high");
        assert_eq!(cfg.detect.per_detector_timeout_secs, Some(30));
    }

    #[test]
    fn discover_walks_up_to_parent() {
        // Create a nested dir structure where apex.toml is in the parent
        let base = std::env::temp_dir().join("apex_test_discover_parent");
        let child = base.join("subdir");
        let _ = std::fs::create_dir_all(&child);
        std::fs::write(base.join("apex.toml"), "[coverage]\ntarget = 0.33\n").unwrap();
        let cfg = ApexConfig::discover(&child).unwrap();
        assert!((cfg.coverage.target - 0.33).abs() < f64::EPSILON);
        let _ = std::fs::remove_dir_all(&base);
    }

    // -----------------------------------------------------------------------
    // Threat model config
    // -----------------------------------------------------------------------

    #[test]
    fn parse_threat_model_cli_tool() {
        let toml = r#"
[threat_model]
type = "cli-tool"
trusted_sources = ["config_file"]
"#;
        let cfg = ApexConfig::parse_toml(toml).unwrap();
        assert_eq!(cfg.threat_model.model_type, Some(ThreatModelType::CliTool));
        assert_eq!(cfg.threat_model.trusted_sources, vec!["config_file"]);
    }

    #[test]
    fn parse_threat_model_web_service() {
        let toml = r#"
[threat_model]
type = "web-service"
"#;
        let cfg = ApexConfig::parse_toml(toml).unwrap();
        assert_eq!(
            cfg.threat_model.model_type,
            Some(ThreatModelType::WebService)
        );
    }

    #[test]
    fn parse_threat_model_library() {
        let toml = r#"
[threat_model]
type = "library"
untrusted_sources = ["user_callback"]
"#;
        let cfg = ApexConfig::parse_toml(toml).unwrap();
        assert_eq!(cfg.threat_model.model_type, Some(ThreatModelType::Library));
        assert_eq!(cfg.threat_model.untrusted_sources, vec!["user_callback"]);
    }

    #[test]
    fn parse_threat_model_ci_pipeline() {
        let toml = r#"
[threat_model]
type = "ci-pipeline"
"#;
        let cfg = ApexConfig::parse_toml(toml).unwrap();
        assert_eq!(
            cfg.threat_model.model_type,
            Some(ThreatModelType::CiPipeline)
        );
    }

    #[test]
    fn missing_threat_model_is_none() {
        let cfg = ApexConfig::parse_toml("").unwrap();
        assert!(cfg.threat_model.model_type.is_none());
        assert!(cfg.threat_model.trusted_sources.is_empty());
        assert!(cfg.threat_model.untrusted_sources.is_empty());
    }

    // -----------------------------------------------------------------------
    // Coverage target bound checking (Task 7)
    // -----------------------------------------------------------------------

    #[test]
    fn coverage_target_clamped_to_valid_range() {
        let toml = "[coverage]\ntarget = 2.0\n";
        let cfg = ApexConfig::parse_toml(toml).unwrap();
        assert_eq!(cfg.coverage.target, 1.0);
    }

    #[test]
    fn coverage_target_negative_clamped() {
        let toml = "[coverage]\ntarget = -0.5\n";
        let cfg = ApexConfig::parse_toml(toml).unwrap();
        assert_eq!(cfg.coverage.target, 0.0);
    }

    #[test]
    fn min_ratchet_clamped() {
        let toml = "[coverage]\nmin_ratchet = 1.5\n";
        let cfg = ApexConfig::parse_toml(toml).unwrap();
        assert_eq!(cfg.coverage.min_ratchet, 1.0);
    }

    // -----------------------------------------------------------------------
    // Omit patterns (Task 11)
    // -----------------------------------------------------------------------

    #[test]
    fn parse_omit_patterns() {
        let toml = r#"
[coverage]
omit_patterns = ["vendor", "third_party"]
"#;
        let cfg = ApexConfig::parse_toml(toml).unwrap();
        assert_eq!(cfg.coverage.omit_patterns, vec!["vendor", "third_party"]);
    }

    #[test]
    fn default_omit_patterns() {
        let cfg = ApexConfig::default();
        assert!(cfg.coverage.omit_patterns.contains(&"node_modules".into()));
        assert!(cfg.coverage.omit_patterns.contains(&"__pycache__".into()));
        assert!(cfg.coverage.omit_patterns.contains(&"target".into()));
        assert!(cfg.coverage.omit_patterns.contains(&"dist".into()));
        assert!(cfg.coverage.omit_patterns.contains(&"build".into()));
    }

    // -----------------------------------------------------------------------
    // New section smoke tests
    // -----------------------------------------------------------------------

    #[test]
    fn index_config_defaults() {
        let cfg = IndexConfig::default();
        assert_eq!(cfg.max_source_files, 10_000);
        assert_eq!(cfg.max_source_file_bytes, 1_048_576);
    }

    #[test]
    fn reach_config_defaults() {
        let cfg = ReachConfig::default();
        assert_eq!(cfg.max_depth, 20);
    }

    #[test]
    fn cpg_config_defaults() {
        let cfg = CpgConfig::default();
        assert_eq!(cfg.max_query_rows, 100_000);
    }

    #[test]
    fn synth_config_defaults() {
        let cfg = SynthConfig::default();
        assert_eq!(cfg.chunk_size, 20);
        assert_eq!(cfg.max_branches_in_prompt, 20);
    }

    #[test]
    fn pso_config_defaults() {
        let cfg = PsoConfig::default();
        assert!((cfg.w - 0.7).abs() < f64::EPSILON);
        assert!((cfg.c1 - 1.5).abs() < f64::EPSILON);
        assert!((cfg.c2 - 1.5).abs() < f64::EPSILON);
        assert!((cfg.prob_min - 0.01).abs() < f64::EPSILON);
    }

    #[test]
    fn fox_config_defaults() {
        let cfg = FoxConfig::default();
        assert!((cfg.mutation_rate - 0.5).abs() < f64::EPSILON);
        assert!((cfg.exploration_rate - 0.5).abs() < f64::EPSILON);
        assert!((cfg.alpha - 0.1).abs() < f64::EPSILON);
    }

    #[test]
    fn semantic_config_defaults() {
        let cfg = SemanticConfig::default();
        assert!((cfg.branch_weight - 1.0).abs() < f64::EPSILON);
        assert!((cfg.semantic_weight - 0.5).abs() < f64::EPSILON);
    }

    #[test]
    fn instrument_timeouts_defaults() {
        let cfg = InstrumentTimeouts::default();
        assert_eq!(cfg.c_compile_ms, 300_000);
        assert_eq!(cfg.c_test_ms, 120_000);
        assert_eq!(cfg.c_gcov_ms, 60_000);
        assert_eq!(cfg.csharp_ms, 600_000);
        assert_eq!(cfg.swift_test_ms, 600_000);
        assert_eq!(cfg.swift_codecov_ms, 60_000);
        assert_eq!(cfg.jvm_build_ms, 600_000);
    }
}
