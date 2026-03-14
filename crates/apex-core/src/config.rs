//! Layered configuration system.
//!
//! `ApexConfig` is the top-level struct deserialized from `apex.toml`.
//! Each section maps to a domain-specific sub-struct with serde defaults.
//! The precedence chain is: CLI args > config file > struct defaults.

use serde::Deserialize;
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
        toml::from_str(s).map_err(|e| crate::ApexError::Config(format!("invalid TOML: {e}")))
    }

    /// Try to discover and load `apex.toml` from the given directory (or parents).
    /// Returns the default config if no file is found.
    pub fn discover(start_dir: &Path) -> Self {
        let mut dir = start_dir;
        loop {
            let candidate = dir.join("apex.toml");
            if candidate.is_file() {
                match Self::from_file(&candidate) {
                    Ok(cfg) => {
                        tracing::info!(path = %candidate.display(), "loaded apex.toml");
                        return cfg;
                    }
                    Err(e) => {
                        tracing::warn!(error = %e, "failed to parse apex.toml; using defaults");
                        return Self::default();
                    }
                }
            }
            match dir.parent() {
                Some(parent) => dir = parent,
                None => break,
            }
        }
        Self::default()
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
}

impl Default for CoverageConfig {
    fn default() -> Self {
        CoverageConfig {
            target: 1.0,
            min_ratchet: 0.8,
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
}

impl Default for AgentConfig {
    fn default() -> Self {
        AgentConfig {
            stall_threshold: 10,
            max_rounds: 3,
            max_refinement_rounds: 3,
            source_context_lines: 15,
            max_files_per_round: 3,
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
}

impl Default for InstrumentConfig {
    fn default() -> Self {
        InstrumentConfig {
            bitmap_size: 65_536,
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
}

impl Default for DetectConfig {
    fn default() -> Self {
        DetectConfig {
            enabled: Vec::new(),
            severity_threshold: "low".into(),
            per_detector_timeout_secs: None,
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
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_has_expected_values() {
        let cfg = ApexConfig::default();
        assert_eq!(cfg.coverage.target, 1.0);
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

[fuzz.scheduler]
floor = 0.02
alpha = 0.2

[concolic]
max_rounds = 10

[agent]
stall_threshold = 20
max_rounds = 5
max_refinement_rounds = 5
source_context_lines = 25
max_files_per_round = 5

[sandbox]
process_timeout_ms = 5000
python_timeout_ms = 60000
command_timeout_ms = 15000

[symbolic]
max_depth = 128

[instrument]
bitmap_size = 131072

[logging]
level = "debug"
format = "json"
"#;
        let cfg = ApexConfig::parse_toml(toml).unwrap();
        assert_eq!(cfg.coverage.target, 0.95);
        assert_eq!(cfg.fuzz.corpus_max, 20000);
        assert_eq!(cfg.fuzz.scheduler.floor, 0.02);
        assert_eq!(cfg.concolic.max_rounds, 10);
        assert_eq!(cfg.agent.stall_threshold, 20);
        assert_eq!(cfg.sandbox.process_timeout_ms, 5000);
        assert_eq!(cfg.symbolic.max_depth, 128);
        assert_eq!(cfg.instrument.bitmap_size, 131072);
        assert_eq!(cfg.logging.level, "debug");
        assert_eq!(cfg.logging.format, "json");
    }

    #[test]
    fn invalid_toml_returns_error() {
        assert!(ApexConfig::parse_toml("not valid toml [[[").is_err());
    }

    #[test]
    fn discover_returns_default_when_no_file() {
        let cfg = ApexConfig::discover(Path::new("/nonexistent/path"));
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
        let cfg = ApexConfig::discover(&dir);
        assert_eq!(cfg.coverage.target, 0.42);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn discover_with_invalid_file_returns_defaults() {
        let dir = std::env::temp_dir().join("apex_test_discover_invalid");
        let _ = std::fs::create_dir_all(&dir);
        std::fs::write(dir.join("apex.toml"), "this is not valid toml [[[").unwrap();
        let cfg = ApexConfig::discover(&dir);
        // Should return defaults when parse fails
        assert_eq!(cfg.fuzz.corpus_max, 10_000);
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
        let cfg = ApexConfig::discover(&child);
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
}
