use serde::{Deserialize, Serialize};

fn default_enabled() -> Vec<String> {
    vec![
        "unsafe".into(),
        "deps".into(),
        "panic".into(),
        "static".into(),
        "security".into(),
        "secrets".into(),
        "path-normalize".into(),
        "timeout".into(),
        "session-security".into(),
        "secret-scan".into(),
        "license-scan".into(),
        "flag-hygiene".into(),
        "discarded-async-result".into(),
        "mixed-bool-ops".into(),
        "partial-cmp-unwrap".into(),
        "substring-security".into(),
        "vecdeque-partial".into(),
        "process-exit-in-lib".into(),
        "unsafe-send-sync".into(),
        "duplicated-fn".into(),
        "js-sql-injection".into(),
        "js-command-injection".into(),
        "js-ssrf".into(),
        "js-crypto-failure".into(),
        "js-timeout".into(),
        "js-insecure-deser".into(),
        "js-path-traversal".into(),
        // Multi-language security detectors
        "multi-command-injection".into(),
        "multi-sql-injection".into(),
        "multi-crypto-failure".into(),
        "multi-insecure-deser".into(),
        "multi-ssrf".into(),
        "multi-path-traversal".into(),
        // Dig 2 high-confidence detectors
        "blocking-io-in-async".into(),
        "swallowed-errors".into(),
        "broad-exception-catching".into(),
        "error-context-loss".into(),
        "string-concat-in-loop".into(),
        "regex-in-loop".into(),
        // P1 concurrency detectors
        "mutex-across-await".into(),
        "open-without-with".into(),
        "unbounded-queue".into(),
        "ffi-panic".into(),
        // P2 detectors
        "missing-async-timeout".into(),
        "zombie-subprocess".into(),
        "relaxed-atomics".into(),
        "hardcoded-env-values".into(),
        "wall-clock-misuse".into(),
        // P3 detectors
        "missing-shutdown-handler".into(),
        "connection-in-loop".into(),
        "poisoned-mutex-recovery".into(),
    ]
}

/// Detectors that produce high-volume code-quality findings. All enabled by
/// default but findings are tagged `noisy: true` so consumers can filter them
/// in summaries while still showing them in full reports.
pub const NOISY_DETECTORS: &[&str] = &[
    "panic",
    "panic-pattern",
    "mixed-bool-ops",
    "static",
    "duplicated-fn",
    "process-exit-in-lib",
    "string-concat-in-loop",
    "regex-in-loop",
    "hardcoded-env-values",
    "wall-clock-misuse",
    "error-context-loss",
    "poisoned-mutex-recovery",
];

/// Returns true if the detector name is marked as noisy.
pub fn is_noisy_detector(name: &str) -> bool {
    NOISY_DETECTORS.contains(&name)
}

/// Returns the default enabled detector set for audit mode.
/// All detectors are enabled — noisy ones are tagged, not excluded.
pub fn default_audit_enabled() -> Vec<String> {
    default_enabled()
}

fn default_severity() -> String {
    "low".into()
}

fn default_replay_top_percent() -> u8 {
    1
}

fn default_sanitizers() -> Vec<String> {
    vec!["address".into(), "undefined".into()]
}

/// Tag a detector belongs to, used for `--tag` filtering.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DetectorTag {
    /// Detectors that find security vulnerabilities (injection, secrets, etc.)
    Security,
    /// Detectors that find code quality issues (duplicates, bool ops, etc.)
    Quality,
    /// Detectors that audit software dependencies for vulnerabilities
    Dependency,
}

/// Map from detector name to its tag. Used for `--tag` filtering.
pub fn detector_tag(name: &str) -> DetectorTag {
    match name {
        "panic"
        | "mixed-bool-ops"
        | "duplicated-fn"
        | "process-exit-in-lib"
        | "partial-cmp-unwrap"
        | "substring-security"
        | "vecdeque-partial"
        | "unsafe-send-sync"
        | "discarded-async-result"
        | "static" => DetectorTag::Quality,
        "deps" | "license-scan" => DetectorTag::Dependency,
        _ => DetectorTag::Security,
    }
}

/// Controls which detectors run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum DetectMode {
    /// Run all detectors including subprocess-based ones.
    #[default]
    Full,
    /// Run only lightweight pattern-matching detectors (skip cargo subprocess).
    Fast,
}

fn default_entropy_threshold() -> f64 {
    5.0
}

fn default_max_subprocess_concurrency() -> usize {
    4
}

fn default_context_window() -> usize {
    3
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct DetectConfig {
    #[serde(default = "default_enabled")]
    pub enabled: Vec<String>,
    #[serde(default = "default_severity")]
    pub severity_threshold: String,
    #[serde(default)]
    pub per_detector_timeout_secs: Option<u64>,
    #[serde(default)]
    pub sanitizer: SanitizerConfig,
    #[serde(default, rename = "static")]
    pub static_analysis: StaticAnalysisConfig,
    #[serde(default)]
    pub llm: LlmConfig,
    #[serde(default)]
    pub diff: DiffConfig,
    #[serde(default)]
    pub properties: Vec<PropertyConfig>,
    #[serde(default)]
    pub detect_mode: DetectMode,
    /// Optional tag filter — if set, only detectors with this tag are enabled.
    #[serde(default)]
    pub tag_filter: Option<DetectorTag>,
    /// Shannon entropy threshold for secret-scan high-entropy string detection. Default: 5.0.
    #[serde(default = "default_entropy_threshold")]
    pub entropy_threshold: f64,
    /// Max concurrent subprocess-based detectors in the pipeline. Default: 4.
    #[serde(default = "default_max_subprocess_concurrency")]
    pub max_subprocess_concurrency: usize,
    /// Lines of source context examined around each potential finding. Default: 3.
    #[serde(default = "default_context_window")]
    pub context_window: usize,
}

impl Default for DetectConfig {
    fn default() -> Self {
        DetectConfig {
            enabled: default_enabled(),
            severity_threshold: default_severity(),
            per_detector_timeout_secs: None,
            sanitizer: SanitizerConfig::default(),
            static_analysis: StaticAnalysisConfig::default(),
            llm: LlmConfig::default(),
            diff: DiffConfig::default(),
            properties: Vec::new(),
            detect_mode: DetectMode::default(),
            tag_filter: None,
            entropy_threshold: default_entropy_threshold(),
            max_subprocess_concurrency: default_max_subprocess_concurrency(),
            context_window: default_context_window(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct SanitizerConfig {
    #[serde(default = "default_replay_top_percent")]
    pub replay_top_percent: u8,
    #[serde(default = "default_sanitizers")]
    pub sanitizers: Vec<String>,
}

impl Default for SanitizerConfig {
    fn default() -> Self {
        SanitizerConfig {
            replay_top_percent: default_replay_top_percent(),
            sanitizers: default_sanitizers(),
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct StaticAnalysisConfig {
    pub clippy_extra_args: Vec<String>,
    pub sarif_paths: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct LlmConfig {
    pub enabled: bool,
    pub batch_size: usize,
    pub model: String,
}

impl Default for LlmConfig {
    fn default() -> Self {
        LlmConfig {
            enabled: false,
            batch_size: 10,
            model: "claude-sonnet-4-6".into(),
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct DiffConfig {
    pub base_ref: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PropertyConfig {
    pub name: String,
    pub check: String,
    pub target: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_has_tier1_detectors() {
        let cfg = DetectConfig::default();
        assert!(cfg.enabled.contains(&"panic".to_string()));
        assert!(cfg.enabled.contains(&"deps".to_string()));
        assert!(cfg.enabled.contains(&"unsafe".to_string()));
        assert!(cfg.enabled.contains(&"static".to_string()));
        assert!(cfg.enabled.contains(&"security".to_string()));
        assert!(cfg.enabled.contains(&"secrets".to_string()));
    }

    #[test]
    fn default_config_has_expansion_tier1_detectors() {
        let cfg = DetectConfig::default();
        assert!(cfg.enabled.contains(&"secret-scan".to_string()));
        assert!(cfg.enabled.contains(&"license-scan".to_string()));
        assert!(cfg.enabled.contains(&"flag-hygiene".to_string()));
    }

    #[test]
    fn default_timeout_is_none() {
        let cfg = DetectConfig::default();
        assert!(cfg.per_detector_timeout_secs.is_none());
    }

    #[test]
    fn deserialize_from_toml() {
        let toml_str = r#"
enabled = ["panic", "deps"]
severity_threshold = "high"
per_detector_timeout_secs = 60

[sanitizer]
replay_top_percent = 5
sanitizers = ["address"]

[static]
clippy_extra_args = ["-W", "clippy::pedantic"]
"#;
        let cfg: DetectConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(cfg.enabled, vec!["panic", "deps"]);
        assert_eq!(cfg.severity_threshold, "high");
        assert_eq!(cfg.per_detector_timeout_secs, Some(60));
        assert_eq!(cfg.sanitizer.replay_top_percent, 5);
        assert_eq!(
            cfg.static_analysis.clippy_extra_args,
            vec!["-W".to_string(), "clippy::pedantic".to_string()]
        );
    }

    #[test]
    fn empty_toml_gives_defaults() {
        let cfg: DetectConfig = toml::from_str("").unwrap();
        assert_eq!(cfg.enabled.len(), 51);
        assert_eq!(cfg.severity_threshold, "low");
    }

    #[test]
    fn sanitizer_config_defaults() {
        let sc = SanitizerConfig::default();
        assert_eq!(sc.replay_top_percent, 1);
        assert_eq!(sc.sanitizers, vec!["address", "undefined"]);
    }

    #[test]
    fn llm_config_defaults() {
        let lc = LlmConfig::default();
        assert!(!lc.enabled);
        assert_eq!(lc.batch_size, 10);
        assert_eq!(lc.model, "claude-sonnet-4-6");
    }

    #[test]
    fn diff_config_defaults() {
        let dc = DiffConfig::default();
        assert_eq!(dc.base_ref, "");
    }

    #[test]
    fn static_analysis_config_defaults() {
        let sac = StaticAnalysisConfig::default();
        assert!(sac.clippy_extra_args.is_empty());
        assert!(sac.sarif_paths.is_empty());
    }

    #[test]
    fn property_config_serializes() {
        let pc = PropertyConfig {
            name: "no-panic".into(),
            check: "assert!(true)".into(),
            target: "src/lib.rs".into(),
        };
        let json = serde_json::to_string(&pc).unwrap();
        assert!(json.contains("no-panic"));
    }

    #[test]
    fn detect_config_with_properties() {
        let toml_str = r#"
enabled = ["panic"]

[[properties]]
name = "no-panic"
check = "assert_no_panic()"
target = "src/lib.rs"

[[properties]]
name = "idempotent"
check = "assert_idempotent()"
target = "src/api.rs"
"#;
        let cfg: DetectConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(cfg.properties.len(), 2);
        assert_eq!(cfg.properties[0].name, "no-panic");
        assert_eq!(cfg.properties[1].target, "src/api.rs");
    }

    #[test]
    fn detect_config_with_llm_and_diff() {
        let toml_str = r#"
[llm]
enabled = true
batch_size = 20
model = "gpt-4"

[diff]
base_ref = "main"
"#;
        let cfg: DetectConfig = toml::from_str(toml_str).unwrap();
        assert!(cfg.llm.enabled);
        assert_eq!(cfg.llm.batch_size, 20);
        assert_eq!(cfg.llm.model, "gpt-4");
        assert_eq!(cfg.diff.base_ref, "main");
    }

    #[test]
    fn detect_mode_defaults_to_full() {
        let cfg = DetectConfig::default();
        assert_eq!(cfg.detect_mode, DetectMode::Full);
    }

    #[test]
    fn detect_mode_fast_from_toml() {
        let toml_str = r#"
detect_mode = "Fast"
"#;
        let cfg: DetectConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(cfg.detect_mode, DetectMode::Fast);
    }

    #[test]
    fn detect_config_serializes_roundtrip() {
        let cfg = DetectConfig::default();
        let json = serde_json::to_string(&cfg).unwrap();
        let cfg2: DetectConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(cfg2.enabled.len(), 51);
        assert_eq!(cfg2.severity_threshold, "low");
    }

    // -----------------------------------------------------------------------
    // Bug 3: default_audit_enabled() excludes noisy detectors
    // -----------------------------------------------------------------------

    #[test]
    fn audit_enabled_includes_all_detectors() {
        let audit = default_audit_enabled();
        let default = default_enabled();
        // Audit mode now includes all detectors — noisy ones are tagged, not excluded
        assert_eq!(audit.len(), default.len());
        assert!(audit.contains(&"panic".to_string()));
        assert!(audit.contains(&"mixed-bool-ops".to_string()));
        assert!(audit.contains(&"static".to_string()));
    }

    #[test]
    fn noisy_detectors_are_identified() {
        assert!(is_noisy_detector("panic"));
        assert!(is_noisy_detector("string-concat-in-loop"));
        assert!(!is_noisy_detector("security"));
        assert!(!is_noisy_detector("ffi-panic"));
    }

    #[test]
    fn audit_enabled_keeps_security_detectors() {
        let audit = default_audit_enabled();
        assert!(audit.contains(&"security".to_string()));
        assert!(audit.contains(&"secret-scan".to_string()));
        assert!(audit.contains(&"deps".to_string()));
    }

    #[test]
    fn audit_enabled_is_subset_of_default() {
        let default = default_enabled();
        let audit = default_audit_enabled();
        for name in &audit {
            assert!(
                default.contains(name),
                "audit-enabled detector '{}' is not in default set",
                name
            );
        }
    }

    // -----------------------------------------------------------------------
    // Bug 19: DetectorTag and tag_filter
    // -----------------------------------------------------------------------

    #[test]
    fn detector_tag_security() {
        assert_eq!(
            super::detector_tag("security"),
            super::DetectorTag::Security
        );
        assert_eq!(
            super::detector_tag("secret-scan"),
            super::DetectorTag::Security
        );
    }

    #[test]
    fn detector_tag_quality() {
        assert_eq!(super::detector_tag("panic"), super::DetectorTag::Quality);
        assert_eq!(
            super::detector_tag("mixed-bool-ops"),
            super::DetectorTag::Quality
        );
        assert_eq!(
            super::detector_tag("duplicated-fn"),
            super::DetectorTag::Quality
        );
    }

    #[test]
    fn detector_tag_dependency() {
        assert_eq!(super::detector_tag("deps"), super::DetectorTag::Dependency);
        assert_eq!(
            super::detector_tag("license-scan"),
            super::DetectorTag::Dependency
        );
    }

    #[test]
    fn tag_filter_defaults_to_none() {
        let cfg = DetectConfig::default();
        assert!(cfg.tag_filter.is_none());
    }

    // -----------------------------------------------------------------------
    // New wired config fields
    // -----------------------------------------------------------------------

    #[test]
    fn default_entropy_threshold_is_5_0() {
        let cfg = DetectConfig::default();
        assert!(
            (cfg.entropy_threshold - 5.0).abs() < f64::EPSILON,
            "default entropy_threshold should be 5.0, got {}",
            cfg.entropy_threshold
        );
    }

    #[test]
    fn default_max_subprocess_concurrency_is_4() {
        let cfg = DetectConfig::default();
        assert_eq!(cfg.max_subprocess_concurrency, 4);
    }

    #[test]
    fn default_context_window_is_3() {
        let cfg = DetectConfig::default();
        assert_eq!(cfg.context_window, 3);
    }

    #[test]
    fn new_fields_deserialize_from_toml() {
        let toml_str = r#"
entropy_threshold = 4.5
max_subprocess_concurrency = 8
context_window = 5
"#;
        let cfg: DetectConfig = toml::from_str(toml_str).unwrap();
        assert!((cfg.entropy_threshold - 4.5).abs() < f64::EPSILON);
        assert_eq!(cfg.max_subprocess_concurrency, 8);
        assert_eq!(cfg.context_window, 5);
    }
}
