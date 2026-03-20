//! SARIF 2.1.0 output format for APEX findings.
//!
//! Converts [`Finding`] results into the SARIF (Static Analysis Results Interchange Format)
//! for integration with GitHub Code Scanning, VS Code SARIF Viewer, and other tools.

use serde::Serialize;

use crate::finding::{Finding, FindingCategory, Severity};

const SARIF_SCHEMA: &str =
    "https://raw.githubusercontent.com/oasis-tcs/sarif-spec/main/sarif-2.1/schema/sarif-schema-2.1.0.json";
const SARIF_VERSION: &str = "2.1.0";

// ---------------------------------------------------------------------------
// SARIF structs
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SarifReport {
    #[serde(rename = "$schema")]
    pub schema: String,
    pub version: String,
    pub runs: Vec<SarifRun>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SarifRun {
    pub tool: SarifTool,
    pub results: Vec<SarifResult>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SarifTool {
    pub driver: SarifDriver,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SarifDriver {
    pub name: String,
    pub version: String,
    pub rules: Vec<SarifRule>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SarifRule {
    pub id: String,
    pub short_description: SarifMessage,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub properties: Option<SarifRuleProperties>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SarifRuleProperties {
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SarifResult {
    pub rule_id: String,
    pub level: String,
    pub message: SarifMessage,
    pub locations: Vec<SarifLocation>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SarifMessage {
    pub text: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SarifLocation {
    pub physical_location: SarifPhysicalLocation,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SarifPhysicalLocation {
    pub artifact_location: SarifArtifactLocation,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub region: Option<SarifRegion>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SarifArtifactLocation {
    pub uri: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SarifRegion {
    pub start_line: u32,
}

// ---------------------------------------------------------------------------
// Conversion helpers
// ---------------------------------------------------------------------------

fn severity_to_level(severity: &Severity) -> &'static str {
    match severity {
        Severity::Critical | Severity::High => "error",
        Severity::Medium => "warning",
        Severity::Low | Severity::Info => "note",
    }
}

/// Map a `FindingCategory` to its most representative CWE IDs.
fn category_to_cwes(category: &FindingCategory) -> Vec<String> {
    match category {
        FindingCategory::MemorySafety => vec!["CWE-119".into()],
        FindingCategory::UndefinedBehavior => vec!["CWE-758".into()],
        FindingCategory::Injection => vec!["CWE-78".into()],
        FindingCategory::PanicPath => vec!["CWE-248".into()],
        FindingCategory::UnsafeCode => vec!["CWE-676".into()],
        FindingCategory::DependencyVuln => vec!["CWE-1104".into()],
        FindingCategory::LogicBug => vec!["CWE-670".into()],
        FindingCategory::SecuritySmell => vec!["CWE-710".into()],
        FindingCategory::PathTraversal => vec!["CWE-22".into()],
        FindingCategory::InsecureConfig => vec!["CWE-16".into()],
        FindingCategory::HardcodedSecret => vec!["CWE-798".into()],
        FindingCategory::LicenseViolation => vec!["CWE-1357".into()],
        FindingCategory::FeatureFlagHygiene => vec!["CWE-1127".into()],
        FindingCategory::ApiBreakingChange => vec!["CWE-1105".into()],
        FindingCategory::ApiSpecCoverage => vec!["CWE-1059".into()],
        FindingCategory::ServiceDependency => vec!["CWE-1127".into()],
        FindingCategory::SchemaMigrationRisk => vec!["CWE-1066".into()],
        FindingCategory::TestDataQuality => vec!["CWE-1007".into()],
    }
}

/// Build a deterministic rule ID from the detector name and category.
fn rule_id_for(finding: &Finding) -> String {
    format!(
        "{}/{}",
        finding.detector,
        serde_json::to_value(finding.category)
            .ok()
            .and_then(|v| v.as_str().map(String::from))
            .unwrap_or_else(|| "unknown".into())
    )
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Convert a slice of [`Finding`]s into a [`SarifReport`].
pub fn findings_to_sarif(findings: &[Finding], tool_version: &str) -> SarifReport {
    // Collect unique rules (deduplicate by rule_id).
    let mut rules: Vec<SarifRule> = Vec::new();
    let mut seen_rules: std::collections::HashSet<String> = std::collections::HashSet::new();

    let mut results: Vec<SarifResult> = Vec::with_capacity(findings.len());

    for f in findings {
        let rid = rule_id_for(f);

        if seen_rules.insert(rid.clone()) {
            let cwes = category_to_cwes(&f.category);
            rules.push(SarifRule {
                id: rid.clone(),
                short_description: SarifMessage {
                    text: f.title.clone(),
                },
                properties: if cwes.is_empty() {
                    None
                } else {
                    Some(SarifRuleProperties { tags: cwes })
                },
            });
        }

        let mut region = None;
        if let Some(line) = f.line {
            region = Some(SarifRegion { start_line: line });
        }

        results.push(SarifResult {
            rule_id: rid,
            level: severity_to_level(&f.severity).to_string(),
            message: SarifMessage {
                text: f.description.clone(),
            },
            locations: vec![SarifLocation {
                physical_location: SarifPhysicalLocation {
                    artifact_location: SarifArtifactLocation {
                        uri: f.file.display().to_string(),
                    },
                    region,
                },
            }],
        });
    }

    SarifReport {
        schema: SARIF_SCHEMA.into(),
        version: SARIF_VERSION.into(),
        runs: vec![SarifRun {
            tool: SarifTool {
                driver: SarifDriver {
                    name: "APEX".into(),
                    version: tool_version.into(),
                    rules,
                },
            },
            results,
        }],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::finding::{Finding, FindingCategory, Severity};
    use std::path::PathBuf;

    fn make_finding(severity: Severity, category: FindingCategory) -> Finding {
        Finding {
            id: uuid::Uuid::nil(),
            detector: "test-detector".into(),
            severity,
            category,
            file: PathBuf::from("src/main.rs"),
            line: Some(42),
            title: "test title".into(),
            description: "test description".into(),
            evidence: vec![],
            covered: false,
            suggestion: "fix it".into(),
            explanation: None,
            fix: None,
            cwe_ids: vec![],
                    noisy: false,
        }
    }

    #[test]
    fn sarif_output_valid_structure() {
        let findings = vec![make_finding(Severity::High, FindingCategory::Injection)];
        let report = findings_to_sarif(&findings, "0.1.0");

        assert_eq!(report.schema, SARIF_SCHEMA);
        assert_eq!(report.version, "2.1.0");
        assert_eq!(report.runs.len(), 1);
        assert_eq!(report.runs[0].tool.driver.name, "APEX");
        assert_eq!(report.runs[0].tool.driver.version, "0.1.0");

        // Verify it serializes to valid JSON.
        let json = serde_json::to_string_pretty(&report).unwrap();
        assert!(json.contains("\"$schema\""));
        assert!(json.contains("\"version\": \"2.1.0\""));
        assert!(json.contains("\"runs\""));
    }

    #[test]
    fn sarif_maps_severity_to_level() {
        let cases = vec![
            (Severity::Critical, "error"),
            (Severity::High, "error"),
            (Severity::Medium, "warning"),
            (Severity::Low, "note"),
            (Severity::Info, "note"),
        ];

        for (sev, expected_level) in cases {
            let findings = vec![make_finding(sev, FindingCategory::PanicPath)];
            let report = findings_to_sarif(&findings, "0.1.0");
            assert_eq!(
                report.runs[0].results[0].level, expected_level,
                "severity {:?} should map to level \"{}\"",
                sev, expected_level
            );
        }
    }

    #[test]
    fn sarif_includes_cwe_in_tags() {
        let findings = vec![
            make_finding(Severity::High, FindingCategory::Injection),
            make_finding(Severity::Medium, FindingCategory::PathTraversal),
        ];
        let report = findings_to_sarif(&findings, "0.1.0");

        let rules = &report.runs[0].tool.driver.rules;
        assert_eq!(rules.len(), 2);

        // Injection maps to CWE-78.
        let injection_rule = rules.iter().find(|r| r.id.contains("injection")).unwrap();
        let tags = &injection_rule.properties.as_ref().unwrap().tags;
        assert!(tags.contains(&"CWE-78".to_string()));

        // PathTraversal maps to CWE-22.
        let path_rule = rules
            .iter()
            .find(|r| r.id.contains("path_traversal"))
            .unwrap();
        let tags = &path_rule.properties.as_ref().unwrap().tags;
        assert!(tags.contains(&"CWE-22".to_string()));
    }

    #[test]
    fn sarif_location_from_finding() {
        let findings = vec![make_finding(Severity::Low, FindingCategory::LogicBug)];
        let report = findings_to_sarif(&findings, "0.1.0");

        let loc = &report.runs[0].results[0].locations[0].physical_location;
        assert_eq!(loc.artifact_location.uri, "src/main.rs");
        assert_eq!(loc.region.as_ref().unwrap().start_line, 42);
    }

    #[test]
    fn sarif_location_without_line() {
        let mut f = make_finding(Severity::Info, FindingCategory::SecuritySmell);
        f.line = None;
        let report = findings_to_sarif(&[f], "0.1.0");

        let loc = &report.runs[0].results[0].locations[0].physical_location;
        assert_eq!(loc.artifact_location.uri, "src/main.rs");
        assert!(loc.region.is_none());
    }

    #[test]
    fn sarif_empty_findings() {
        let report = findings_to_sarif(&[], "0.1.0");

        assert_eq!(report.version, "2.1.0");
        assert_eq!(report.runs.len(), 1);
        assert!(report.runs[0].results.is_empty());
        assert!(report.runs[0].tool.driver.rules.is_empty());

        // Must still serialize to valid JSON.
        let json = serde_json::to_string(&report).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert!(parsed.get("$schema").is_some());
        assert!(parsed.get("version").is_some());
    }

    #[test]
    fn sarif_all_category_cwes_covered() {
        let categories = vec![
            FindingCategory::MemorySafety,
            FindingCategory::UndefinedBehavior,
            FindingCategory::UnsafeCode,
            FindingCategory::DependencyVuln,
            FindingCategory::InsecureConfig,
            FindingCategory::HardcodedSecret,
            FindingCategory::LicenseViolation,
        ];
        for cat in categories {
            let findings = vec![make_finding(Severity::Medium, cat)];
            let report = findings_to_sarif(&findings, "0.1.0");
            let rules = &report.runs[0].tool.driver.rules;
            assert_eq!(rules.len(), 1);
            assert!(rules[0].properties.as_ref().unwrap().tags[0].starts_with("CWE-"));
        }
    }

    #[test]
    fn sarif_deduplicates_rules() {
        let findings = vec![
            make_finding(Severity::High, FindingCategory::Injection),
            make_finding(Severity::Medium, FindingCategory::Injection),
        ];
        let report = findings_to_sarif(&findings, "0.1.0");
        assert_eq!(report.runs[0].tool.driver.rules.len(), 1);
        assert_eq!(report.runs[0].results.len(), 2);
    }
}
