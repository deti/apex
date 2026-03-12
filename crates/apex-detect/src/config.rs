use serde::{Deserialize, Serialize};

fn default_enabled() -> Vec<String> {
    vec![
        "unsafe".into(),
        "deps".into(),
        "panic".into(),
        "static".into(),
        "security".into(),
        "secrets".into(),
    ]
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
        assert_eq!(cfg.enabled.len(), 6);
        assert_eq!(cfg.severity_threshold, "low");
    }
}
