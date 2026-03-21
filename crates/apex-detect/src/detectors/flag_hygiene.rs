//! Feature Flag Hygiene detector.
//!
//! Scans source code for feature flag patterns and identifies stale, always-on,
//! or dead flags. Uses git blame for flag age and optionally branch index data
//! for always-on/off detection.

use async_trait::async_trait;
use regex::Regex;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;
use uuid::Uuid;

use apex_core::error::Result;

use crate::context::AnalysisContext;
use crate::finding::{Finding, FindingCategory, Severity};
use crate::Detector;

/// Status of a feature flag.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FlagStatus {
    Active,
    Stale,
    AlwaysOn,
    AlwaysOff,
    Dead,
}

/// A detected feature flag reference.
#[derive(Debug, Clone)]
pub struct FlagReference {
    pub name: String,
    pub file: PathBuf,
    pub line: u32,
    pub pattern_type: String,
}

pub struct FlagHygieneDetector {
    #[allow(dead_code)]
    max_age_days: u64,
}

impl FlagHygieneDetector {
    pub fn new(max_age_days: u64) -> Self {
        Self { max_age_days }
    }

    pub fn default_max_age() -> Self {
        Self { max_age_days: 90 }
    }
}

/// Compiled patterns for detecting feature flags in source code.
static FLAG_PATTERNS: LazyLock<Vec<(Regex, &'static str)>> = LazyLock::new(|| {
    vec![
        // Python
        (
            Regex::new(r#"feature_flag\(\s*["']([^"']+)["']\s*\)"#).unwrap(),
            "python_feature_flag",
        ),
        (
            Regex::new(r#"is_enabled\(\s*["']([^"']+)["']\s*\)"#).unwrap(),
            "python_is_enabled",
        ),
        (
            Regex::new(r#"flags\.get\(\s*["']([^"']+)["']\s*\)"#).unwrap(),
            "python_flags_get",
        ),
        (
            Regex::new(r#"FLAGS\[["']([^"']+)["']\]"#).unwrap(),
            "python_FLAGS",
        ),
        // JavaScript
        (
            Regex::new(r#"featureFlag\(\s*["']([^"']+)["']\s*\)"#).unwrap(),
            "js_featureFlag",
        ),
        (
            Regex::new(r#"isEnabled\(\s*["']([^"']+)["']\s*\)"#).unwrap(),
            "js_isEnabled",
        ),
        (
            Regex::new(r#"getFlag\(\s*["']([^"']+)["']\s*\)"#).unwrap(),
            "js_getFlag",
        ),
        // Rust
        (
            Regex::new(r#"feature_flag!\(\s*["']([^"']+)["']\s*\)"#).unwrap(),
            "rust_feature_flag_macro",
        ),
        (
            Regex::new(r#"cfg!\(feature\s*=\s*"([^"]+)"\)"#).unwrap(),
            "rust_cfg_feature",
        ),
    ]
});

/// Extract flag references from source code.
fn extract_flag_refs(content: &str, file: &Path) -> Vec<FlagReference> {
    let mut refs = Vec::new();

    for (line_num, line) in content.lines().enumerate() {
        for (regex, pattern_type) in FLAG_PATTERNS.iter() {
            for cap in regex.captures_iter(line) {
                if let Some(name_match) = cap.get(1) {
                    refs.push(FlagReference {
                        name: name_match.as_str().to_string(),
                        file: file.to_path_buf(),
                        line: (line_num + 1) as u32,
                        pattern_type: pattern_type.to_string(),
                    });
                }
            }
        }
    }

    refs
}

/// Extract all flag references from the source cache.
pub fn extract_flags(source_cache: &HashMap<PathBuf, String>) -> Vec<FlagReference> {
    let mut all_refs = Vec::new();

    for (file, content) in source_cache {
        all_refs.extend(extract_flag_refs(content, file));
    }

    all_refs
}

/// Build a flag inventory: map flag_name → list of references.
pub fn build_inventory(refs: &[FlagReference]) -> HashMap<String, Vec<&FlagReference>> {
    let mut inventory: HashMap<String, Vec<&FlagReference>> = HashMap::new();
    for r in refs {
        inventory.entry(r.name.clone()).or_default().push(r);
    }
    inventory
}

#[async_trait]
impl Detector for FlagHygieneDetector {
    fn name(&self) -> &str {
        "flag-hygiene"
    }

    async fn analyze(&self, ctx: &AnalysisContext) -> Result<Vec<Finding>> {
        let refs = extract_flags(&ctx.source_cache);
        let inventory = build_inventory(&refs);

        let mut findings = Vec::new();

        for (flag_name, references) in &inventory {
            // Use the first reference for the finding location
            let first_ref = &references[0];

            // Report each unique flag as a finding for inventory tracking.
            // In a full implementation, we'd check age via git blame and
            // always-on/off via branch index. For now, report as Info.
            findings.push(Finding {
                id: Uuid::new_v4(),
                detector: "flag-hygiene".into(),
                severity: Severity::Info,
                category: FindingCategory::SecuritySmell,
                file: first_ref.file.clone(),
                line: Some(first_ref.line),
                title: format!("Feature flag: {flag_name}"),
                description: format!(
                    "Flag '{}' found in {} location(s) via {} pattern",
                    flag_name,
                    references.len(),
                    first_ref.pattern_type
                ),
                evidence: vec![],
                covered: true,
                suggestion: format!(
                    "Review flag '{flag_name}' — ensure it is still needed and not stale"
                ),
                explanation: None,
                fix: None,
                cwe_ids: vec![],
                    noisy: false, base_severity: None, coverage_confidence: None,
            });
        }

        // Sort by flag name for deterministic output
        findings.sort_by(|a, b| a.title.cmp(&b.title));

        Ok(findings)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_python_feature_flag() {
        let code = r#"if feature_flag("dark_mode"):
    enable_dark()
"#;
        let refs = extract_flag_refs(code, Path::new("app.py"));
        assert_eq!(refs.len(), 1);
        assert_eq!(refs[0].name, "dark_mode");
        assert_eq!(refs[0].line, 1);
        assert_eq!(refs[0].pattern_type, "python_feature_flag");
    }

    #[test]
    fn extract_python_is_enabled() {
        let code = r#"result = is_enabled("new_checkout")
"#;
        let refs = extract_flag_refs(code, Path::new("checkout.py"));
        assert_eq!(refs.len(), 1);
        assert_eq!(refs[0].name, "new_checkout");
    }

    #[test]
    fn extract_python_flags_dict() {
        let code = r#"if flags.get("beta_feature"):
    do_beta()
x = FLAGS["legacy_mode"]
"#;
        let refs = extract_flag_refs(code, Path::new("config.py"));
        assert_eq!(refs.len(), 2);
        assert_eq!(refs[0].name, "beta_feature");
        assert_eq!(refs[1].name, "legacy_mode");
    }

    #[test]
    fn extract_js_feature_flag() {
        let code = r#"if (featureFlag("new_ui")) {
  renderNew();
}
const enabled = isEnabled("dark_mode");
const flag = getFlag("experiment_x");
"#;
        let refs = extract_flag_refs(code, Path::new("app.js"));
        assert_eq!(refs.len(), 3);
        assert_eq!(refs[0].name, "new_ui");
        assert_eq!(refs[1].name, "dark_mode");
        assert_eq!(refs[2].name, "experiment_x");
    }

    #[test]
    fn extract_rust_feature_flag() {
        let code = r#"if feature_flag!("experimental") {
    run_experiment();
}
if cfg!(feature = "nightly") {
    use_nightly();
}
"#;
        let refs = extract_flag_refs(code, Path::new("lib.rs"));
        assert_eq!(refs.len(), 2);
        assert_eq!(refs[0].name, "experimental");
        assert_eq!(refs[1].name, "nightly");
    }

    #[test]
    fn extract_no_flags() {
        let code = "fn main() {\n    println!(\"hello\");\n}\n";
        let refs = extract_flag_refs(code, Path::new("main.rs"));
        assert!(refs.is_empty());
    }

    #[test]
    fn extract_flags_from_source_cache() {
        let mut cache = HashMap::new();
        cache.insert(
            PathBuf::from("app.py"),
            r#"if feature_flag("dark_mode"): pass"#.to_string(),
        );
        cache.insert(
            PathBuf::from("config.js"),
            r#"const x = featureFlag("beta")"#.to_string(),
        );
        cache.insert(
            PathBuf::from("utils.rs"),
            "fn helper() {}".to_string(), // no flags
        );

        let refs = extract_flags(&cache);
        assert_eq!(refs.len(), 2);
    }

    #[test]
    fn build_inventory_groups_by_name() {
        let refs = vec![
            FlagReference {
                name: "dark_mode".into(),
                file: PathBuf::from("a.py"),
                line: 1,
                pattern_type: "python_feature_flag".into(),
            },
            FlagReference {
                name: "dark_mode".into(),
                file: PathBuf::from("b.py"),
                line: 5,
                pattern_type: "python_is_enabled".into(),
            },
            FlagReference {
                name: "beta".into(),
                file: PathBuf::from("c.py"),
                line: 10,
                pattern_type: "python_feature_flag".into(),
            },
        ];

        let inventory = build_inventory(&refs);
        assert_eq!(inventory.len(), 2);
        assert_eq!(inventory["dark_mode"].len(), 2);
        assert_eq!(inventory["beta"].len(), 1);
    }

    #[test]
    fn extract_multiple_flags_same_line() {
        let code = r#"x = feature_flag("a") and feature_flag("b")"#;
        let refs = extract_flag_refs(code, Path::new("test.py"));
        assert_eq!(refs.len(), 2);
        assert_eq!(refs[0].name, "a");
        assert_eq!(refs[1].name, "b");
    }

    #[test]
    fn extract_single_quotes_python() {
        let code = "feature_flag('single_quote_flag')";
        let refs = extract_flag_refs(code, Path::new("test.py"));
        assert_eq!(refs.len(), 1);
        assert_eq!(refs[0].name, "single_quote_flag");
    }

    #[tokio::test]
    async fn detector_produces_findings() {
        use crate::config::DetectConfig;
        use apex_core::command::RealCommandRunner;
        use apex_coverage::CoverageOracle;
        use std::sync::Arc;

        let mut source_cache = HashMap::new();
        source_cache.insert(
            PathBuf::from("app.py"),
            r#"if feature_flag("dark_mode"): pass
if feature_flag("beta"): pass
"#
            .to_string(),
        );

        let ctx = crate::context::AnalysisContext {
            language: apex_core::types::Language::Python,
            source_cache,
            ..crate::context::AnalysisContext::test_default()
        };

        let detector = FlagHygieneDetector::default_max_age();
        let findings = detector.analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 2);
        assert!(findings.iter().any(|f| f.title.contains("dark_mode")));
        assert!(findings.iter().any(|f| f.title.contains("beta")));
    }
}
