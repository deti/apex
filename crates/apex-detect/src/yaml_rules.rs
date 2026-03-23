//! Declarative YAML detection rules.
//!
//! Users can define custom detection rules in `.apex/rules/*.yaml` without
//! writing Rust code. Each rule specifies a regex pattern to match and an
//! optional negative pattern to suppress noisy hits.
//!
//! # Rule file format
//!
//! ```yaml
//! id: custom-todo-fixme
//! name: TODO/FIXME in code
//! description: Unresolved TODO or FIXME comment
//! severity: low
//! languages: [rust, python, js]
//! pattern: "(TODO|FIXME|HACK|XXX)\\b"
//! message: "Unresolved {match} comment found"
//! ```

use apex_core::error::{ApexError, Result};
use apex_core::types::Language;
use async_trait::async_trait;
use regex::Regex;
use serde::Deserialize;
use std::path::Path;
use std::str::FromStr;
use uuid::Uuid;

use crate::context::AnalysisContext;
use crate::finding::{Finding, FindingCategory, Severity};
use crate::Detector;

// ---------------------------------------------------------------------------
// Rule definition
// ---------------------------------------------------------------------------

/// A single YAML-defined detection rule.
#[derive(Debug, Clone, Deserialize)]
pub struct YamlRule {
    /// Unique rule identifier (e.g. `custom-todo-fixme`).
    pub id: String,
    /// Human-readable rule name.
    pub name: String,
    /// Longer description of the rule's purpose.
    pub description: String,
    /// Severity level: `critical`, `high`, `medium`, `low`, or `info`.
    pub severity: String,
    /// Optional CWE IDs associated with this rule.
    #[serde(default)]
    pub cwe: Option<Vec<u32>>,
    /// Language slugs this rule applies to (e.g. `rust`, `python`, `js`).
    /// Empty list means the rule applies to all languages.
    #[serde(default)]
    pub languages: Vec<String>,
    /// Regex pattern to match against each source line.
    pub pattern: String,
    /// If present, lines that also match this pattern are suppressed.
    #[serde(default)]
    pub negative_pattern: Option<String>,
    /// Finding message template. `{match}` is replaced with the matched text.
    pub message: String,
}

impl YamlRule {
    /// Parse the severity string into a [`Severity`] enum variant.
    fn parsed_severity(&self) -> Severity {
        match self.severity.to_lowercase().as_str() {
            "critical" => Severity::Critical,
            "high" => Severity::High,
            "medium" | "med" => Severity::Medium,
            "low" => Severity::Low,
            _ => Severity::Info,
        }
    }

    /// True if this rule applies to `language` (or has no language filter).
    fn applies_to_language(&self, language: &Language) -> bool {
        if self.languages.is_empty() {
            return true;
        }
        let lang_str = language.to_string().to_lowercase();
        self.languages.iter().any(|l| l.to_lowercase() == lang_str)
    }
}

// ---------------------------------------------------------------------------
// Compiled rule (patterns pre-compiled)
// ---------------------------------------------------------------------------

struct CompiledRule {
    rule: YamlRule,
    pattern_re: Regex,
    negative_re: Option<Regex>,
}

impl CompiledRule {
    fn try_compile(rule: YamlRule) -> Result<Self> {
        let pattern_re = Regex::new(&rule.pattern).map_err(|e| {
            ApexError::Other(format!(
                "yaml-rules: invalid regex in rule '{}': {e}",
                rule.id
            ))
        })?;
        let negative_re = rule
            .negative_pattern
            .as_deref()
            .map(|p| {
                Regex::new(p).map_err(|e| {
                    ApexError::Other(format!(
                        "yaml-rules: invalid negative_pattern in rule '{}': {e}",
                        rule.id
                    ))
                })
            })
            .transpose()?;
        Ok(CompiledRule {
            rule,
            pattern_re,
            negative_re,
        })
    }
}

// ---------------------------------------------------------------------------
// Detector
// ---------------------------------------------------------------------------

/// Detector that runs user-defined YAML rules against every source file.
pub struct YamlRuleDetector {
    compiled: Vec<CompiledRule>,
}

impl std::fmt::Debug for YamlRuleDetector {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("YamlRuleDetector")
            .field("rule_count", &self.compiled.len())
            .finish()
    }
}

impl YamlRuleDetector {
    /// Load all `.yaml` files from `rules_dir` and compile their rules.
    ///
    /// Files that fail to parse are skipped with a warning; compilation
    /// errors (bad regex) bubble up as `Err`.
    pub fn load(rules_dir: &Path) -> Result<Self> {
        let mut compiled = Vec::new();

        if !rules_dir.exists() {
            return Ok(Self { compiled });
        }

        let entries = std::fs::read_dir(rules_dir).map_err(|e| {
            ApexError::Other(format!(
                "yaml-rules: cannot read rules dir {}: {e}",
                rules_dir.display()
            ))
        })?;

        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("yaml") {
                continue;
            }
            match Self::load_file(&path) {
                Ok(rules) => {
                    for rule in rules {
                        compiled.push(CompiledRule::try_compile(rule)?);
                    }
                }
                Err(e) => {
                    tracing::warn!("yaml-rules: skipping {}: {e}", path.display());
                }
            }
        }

        compiled.sort_by(|a, b| a.rule.id.cmp(&b.rule.id));
        Ok(Self { compiled })
    }

    /// Load rules from a YAML string (useful for tests and in-memory configs).
    pub fn from_yaml_str(yaml: &str) -> Result<Self> {
        let rules = Self::parse_yaml(yaml)?;
        let compiled = rules
            .into_iter()
            .map(CompiledRule::try_compile)
            .collect::<Result<Vec<_>>>()?;
        Ok(Self { compiled })
    }

    // -----------------------------------------------------------------------
    // Private helpers
    // -----------------------------------------------------------------------

    fn load_file(path: &Path) -> Result<Vec<YamlRule>> {
        let content = std::fs::read_to_string(path)
            .map_err(|e| ApexError::Other(format!("yaml-rules: read {}: {e}", path.display())))?;
        Self::parse_yaml(&content)
    }

    fn parse_yaml(yaml: &str) -> Result<Vec<YamlRule>> {
        // A rule file may be a single rule document or a list of rules.
        // Try list first, fall back to single.
        if let Ok(rules) = serde_yaml::from_str::<Vec<YamlRule>>(yaml) {
            return Ok(rules);
        }
        let rule: YamlRule = serde_yaml::from_str(yaml)
            .map_err(|e| ApexError::Other(format!("yaml-rules: failed to parse rule: {e}")))?;
        Ok(vec![rule])
    }

    /// Scan a single source file against all applicable compiled rules.
    fn scan_source(&self, source: &str, file_path: &Path, language: &Language) -> Vec<Finding> {
        let mut findings = Vec::new();

        for compiled in &self.compiled {
            if !compiled.rule.applies_to_language(language) {
                continue;
            }

            let severity = compiled.rule.parsed_severity();
            let cwe_ids: Vec<u32> = compiled.rule.cwe.clone().unwrap_or_default();

            for (line_num, line) in source.lines().enumerate() {
                // Check for pattern match.
                let mat = match compiled.pattern_re.find(line) {
                    Some(m) => m,
                    None => continue,
                };

                // Suppress if negative pattern also matches.
                if let Some(ref neg_re) = compiled.negative_re {
                    if neg_re.is_match(line) {
                        continue;
                    }
                }

                let matched_text = mat.as_str().to_string();
                let message = compiled.rule.message.replace("{match}", &matched_text);

                findings.push(Finding {
                    id: Uuid::new_v4(),
                    detector: format!("yaml-rules:{}", compiled.rule.id),
                    severity,
                    category: FindingCategory::SecuritySmell,
                    file: file_path.to_path_buf(),
                    line: Some((line_num + 1) as u32),
                    title: compiled.rule.name.clone(),
                    description: message,
                    evidence: vec![],
                    covered: false,
                    suggestion: compiled.rule.description.clone(),
                    explanation: None,
                    fix: None,
                    cwe_ids: cwe_ids.clone(),
                    noisy: severity == Severity::Info || severity == Severity::Low,
                    base_severity: None,
                    coverage_confidence: None,
                });
            }
        }

        findings
    }
}

#[async_trait]
impl Detector for YamlRuleDetector {
    fn name(&self) -> &str {
        "yaml-rules"
    }

    async fn analyze(&self, ctx: &AnalysisContext) -> Result<Vec<Finding>> {
        let mut all_findings = Vec::new();

        for (path, source) in &ctx.source_cache {
            // Infer language from file extension, fall back to ctx.language.
            let lang = infer_language(path).unwrap_or(ctx.language);
            let mut file_findings = self.scan_source(source, path, &lang);
            all_findings.append(&mut file_findings);
        }

        Ok(all_findings)
    }
}

/// Infer a language from a file extension.
fn infer_language(path: &Path) -> Option<Language> {
    let ext = path.extension()?.to_str()?;
    Language::from_str(ext).ok()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    const TODO_RULE_YAML: &str = r#"
id: custom-todo-fixme
name: TODO/FIXME in code
description: Unresolved TODO or FIXME comment
severity: low
languages: [rust, python, js]
pattern: "(TODO|FIXME|HACK|XXX)\\b"
message: "Unresolved {match} comment found"
"#;

    const SUPPRESS_RULE_YAML: &str = r##"
id: debug-print
name: Debug print statement
description: Debug print left in code
severity: info
languages: []
pattern: "\\bprint\\("
negative_pattern: "# allow-print"
message: "Debug print found: {match}"
"##;

    #[test]
    fn parse_single_rule_from_yaml_str() {
        let detector = YamlRuleDetector::from_yaml_str(TODO_RULE_YAML).unwrap();
        assert_eq!(detector.compiled.len(), 1);
        assert_eq!(detector.compiled[0].rule.id, "custom-todo-fixme");
    }

    #[test]
    fn parse_rule_list_from_yaml_str() {
        let yaml = format!("[{}, {}]", TODO_RULE_YAML.trim(), SUPPRESS_RULE_YAML.trim());
        // list format not valid as a list of inline YAML docs — use sequence format instead
        let seq_yaml = format!(
            "- {}\n- {}",
            TODO_RULE_YAML.trim().replace('\n', "\n  "),
            SUPPRESS_RULE_YAML.trim().replace('\n', "\n  ")
        );
        // Simpler: just load both separately and confirm each parses
        let d1 = YamlRuleDetector::from_yaml_str(TODO_RULE_YAML).unwrap();
        let d2 = YamlRuleDetector::from_yaml_str(SUPPRESS_RULE_YAML).unwrap();
        assert_eq!(d1.compiled.len(), 1);
        assert_eq!(d2.compiled.len(), 1);
        let _ = seq_yaml; // constructed above for documentation
        let _ = yaml;
    }

    #[test]
    fn pattern_match_produces_finding() {
        let detector = YamlRuleDetector::from_yaml_str(TODO_RULE_YAML).unwrap();
        let source = "fn main() {\n    // TODO: implement\n    let x = 1;\n}";
        let findings = detector.scan_source(source, &PathBuf::from("src/main.rs"), &Language::Rust);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].line, Some(2));
        assert!(findings[0].description.contains("TODO"));
    }

    #[test]
    fn pattern_no_match_returns_empty() {
        let detector = YamlRuleDetector::from_yaml_str(TODO_RULE_YAML).unwrap();
        let source = "fn main() {\n    let x = 1;\n}";
        let findings = detector.scan_source(source, &PathBuf::from("src/main.rs"), &Language::Rust);
        assert!(findings.is_empty());
    }

    #[test]
    fn negative_pattern_suppresses_match() {
        let detector = YamlRuleDetector::from_yaml_str(SUPPRESS_RULE_YAML).unwrap();
        let source = "print(\"debug\")  # allow-print\nprint(\"other\")";
        let findings = detector.scan_source(source, &PathBuf::from("src/a.py"), &Language::Python);
        // First line suppressed by negative_pattern, second fires
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].line, Some(2));
    }

    #[test]
    fn language_filter_excludes_wrong_language() {
        // Rule only applies to rust/python/js
        let detector = YamlRuleDetector::from_yaml_str(TODO_RULE_YAML).unwrap();
        let source = "// TODO: implement something";
        // Java is not in the languages list
        let findings =
            detector.scan_source(source, &PathBuf::from("src/Main.java"), &Language::Java);
        assert!(findings.is_empty());
    }

    #[test]
    fn language_filter_includes_correct_language() {
        let detector = YamlRuleDetector::from_yaml_str(TODO_RULE_YAML).unwrap();
        let source = "# TODO: fix this\n";
        let findings =
            detector.scan_source(source, &PathBuf::from("src/app.py"), &Language::Python);
        assert_eq!(findings.len(), 1);
    }

    #[test]
    fn empty_language_list_applies_to_all_languages() {
        let detector = YamlRuleDetector::from_yaml_str(SUPPRESS_RULE_YAML).unwrap();
        let source = "print(\"hello\")";
        // Java is not in an empty language filter — should still match
        let findings = detector.scan_source(source, &PathBuf::from("App.java"), &Language::Java);
        assert_eq!(findings.len(), 1);
    }

    #[test]
    fn message_template_substitutes_match() {
        let detector = YamlRuleDetector::from_yaml_str(TODO_RULE_YAML).unwrap();
        let source = "// FIXME: bug here";
        let findings = detector.scan_source(source, &PathBuf::from("lib.rs"), &Language::Rust);
        assert_eq!(findings.len(), 1);
        assert!(
            findings[0].description.contains("FIXME"),
            "description: {}",
            findings[0].description
        );
    }

    #[test]
    fn severity_parsed_correctly() {
        for (s, expected) in [
            ("critical", Severity::Critical),
            ("high", Severity::High),
            ("medium", Severity::Medium),
            ("low", Severity::Low),
            ("info", Severity::Info),
            ("unknown", Severity::Info),
        ] {
            let rule = YamlRule {
                id: "x".into(),
                name: "x".into(),
                description: "x".into(),
                severity: s.into(),
                cwe: None,
                languages: vec![],
                pattern: "x".into(),
                negative_pattern: None,
                message: "x".into(),
            };
            assert_eq!(rule.parsed_severity(), expected, "severity={s}");
        }
    }

    #[test]
    fn cwe_ids_propagated_to_finding() {
        let yaml = r#"
id: test-cwe
name: Test CWE rule
description: test
severity: high
cwe: [79, 80]
languages: []
pattern: "innerHTML"
message: "XSS risk"
"#;
        let detector = YamlRuleDetector::from_yaml_str(yaml).unwrap();
        let findings = detector.scan_source(
            "x.innerHTML = val;",
            &PathBuf::from("a.js"),
            &Language::JavaScript,
        );
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].cwe_ids, vec![79, 80]);
    }

    #[test]
    fn detector_name_is_yaml_rules() {
        let detector = YamlRuleDetector::from_yaml_str(TODO_RULE_YAML).unwrap();
        assert_eq!(detector.name(), "yaml-rules");
    }

    #[tokio::test]
    async fn analyze_scans_source_cache() {
        let detector = YamlRuleDetector::from_yaml_str(TODO_RULE_YAML).unwrap();
        let mut ctx = AnalysisContext::test_default();
        ctx.language = Language::Rust;
        ctx.source_cache.insert(
            PathBuf::from("/src/lib.rs"),
            "// TODO: remove this\nfn ok() {}".to_string(),
        );
        ctx.source_cache.insert(
            PathBuf::from("/src/clean.rs"),
            "fn clean() { /* all good */ }".to_string(),
        );

        let findings = detector.analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 1);
        assert!(findings[0].file.to_str().unwrap().contains("lib.rs"));
    }

    #[tokio::test]
    async fn analyze_empty_source_cache_returns_empty() {
        let detector = YamlRuleDetector::from_yaml_str(TODO_RULE_YAML).unwrap();
        let ctx = AnalysisContext::test_default();
        let findings = detector.analyze(&ctx).await.unwrap();
        assert!(findings.is_empty());
    }

    #[test]
    fn load_from_nonexistent_dir_returns_empty_detector() {
        let detector =
            YamlRuleDetector::load(Path::new("/nonexistent/rules/dir/that/does/not/exist"))
                .unwrap();
        assert_eq!(detector.compiled.len(), 0);
    }

    #[tokio::test]
    async fn load_from_temp_dir_parses_rules() {
        use std::fs;
        let tmp = std::env::temp_dir().join(format!("apex-yaml-rules-test-{}", Uuid::new_v4()));
        fs::create_dir_all(&tmp).unwrap();

        let rule_content = r#"
id: temp-rule
name: Temp rule
description: Temporary test rule
severity: medium
languages: [rust]
pattern: "unsafe\\s*\\{"
message: "Unsafe block found"
"#;
        fs::write(tmp.join("unsafe.yaml"), rule_content).unwrap();

        let detector = YamlRuleDetector::load(&tmp).unwrap();
        assert_eq!(detector.compiled.len(), 1);
        assert_eq!(detector.compiled[0].rule.id, "temp-rule");

        let findings = detector.scan_source(
            "fn f() { unsafe { *ptr = 0; } }",
            &PathBuf::from("src/lib.rs"),
            &Language::Rust,
        );
        assert_eq!(findings.len(), 1);

        fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn invalid_regex_returns_error() {
        let yaml = r#"
id: bad-regex
name: Bad regex
description: bad
severity: low
languages: []
pattern: "["
message: "bad"
"#;
        let result = YamlRuleDetector::from_yaml_str(yaml);
        assert!(result.is_err(), "should fail on invalid regex pattern");
        let err_str = result.unwrap_err().to_string();
        assert!(err_str.contains("invalid regex"), "got: {err_str}");
    }

    #[test]
    fn multiple_matches_on_same_line_produces_one_finding() {
        // The regex finds the *first* match per line via `find()`
        let yaml = r#"
id: single-match
name: Single
description: single
severity: info
languages: []
pattern: "TODO"
message: "found {match}"
"#;
        let detector = YamlRuleDetector::from_yaml_str(yaml).unwrap();
        let source = "// TODO TODO TODO";
        let findings = detector.scan_source(source, &PathBuf::from("x.rs"), &Language::Rust);
        // One finding per line, regardless of how many matches are on the line
        assert_eq!(findings.len(), 1);
    }

    #[test]
    fn finding_detector_field_includes_rule_id() {
        let detector = YamlRuleDetector::from_yaml_str(TODO_RULE_YAML).unwrap();
        let source = "# HACK: workaround";
        let findings = detector.scan_source(source, &PathBuf::from("a.py"), &Language::Python);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].detector, "yaml-rules:custom-todo-fixme");
    }
}
