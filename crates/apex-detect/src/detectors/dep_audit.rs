use apex_core::command::CommandSpec;
use apex_core::error::{ApexError, Result};
use async_trait::async_trait;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::PathBuf;
use uuid::Uuid;

use crate::context::AnalysisContext;
use crate::finding::{Finding, FindingCategory, Fix, Severity};
use crate::Detector;

/// Produce a pseudo line number from an advisory ID so each advisory
/// gets a unique dedup key (file, line, category) in the pipeline.
fn advisory_line(id: &str) -> u32 {
    let mut h = DefaultHasher::new();
    id.hash(&mut h);
    (h.finish() % 1_000_000) as u32 + 1
}

pub struct DependencyAuditDetector;

#[async_trait]
impl Detector for DependencyAuditDetector {
    fn name(&self) -> &str {
        "dependency-audit"
    }

    fn uses_cargo_subprocess(&self) -> bool {
        true
    }

    async fn analyze(&self, ctx: &AnalysisContext) -> Result<Vec<Finding>> {
        let spec = CommandSpec::new("cargo", &ctx.target_root)
            .args(["audit", "--json"]);

        let output = ctx.runner.run_command(&spec).await
            .map_err(|e| ApexError::Detect(format!("cargo-audit: {e}")))?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        parse_cargo_audit_output(&stdout)
    }
}

pub fn parse_cargo_audit_output(raw: &str) -> Result<Vec<Finding>> {
    // cargo-audit may append warnings after the JSON object on stdout.
    // Use streaming deserializer to parse only the first JSON value.
    let mut de = serde_json::Deserializer::from_str(raw).into_iter::<serde_json::Value>();
    let parsed = de
        .next()
        .ok_or_else(|| ApexError::Detect("cargo-audit: empty output".into()))?
        .map_err(|e| ApexError::Detect(format!("cargo-audit JSON parse: {e}")))?;

    let mut findings = Vec::new();

    let vulns = parsed
        .get("vulnerabilities")
        .and_then(|v| v.get("list"))
        .and_then(|v| v.as_array());

    if let Some(vuln_list) = vulns {
        for vuln in vuln_list {
            let advisory = &vuln["advisory"];
            let id = advisory["id"].as_str().unwrap_or("unknown");
            let title = advisory["title"]
                .as_str()
                .unwrap_or("unknown vulnerability");
            let sev_str = advisory["severity"].as_str().unwrap_or("medium");
            let pkg_name = vuln["package"]["name"].as_str().unwrap_or("unknown");
            let pkg_version = vuln["package"]["version"].as_str().unwrap_or("?");
            let patched = vuln["versions"]["patched"]
                .as_array()
                .and_then(|a| a.first())
                .and_then(|v| v.as_str())
                .unwrap_or("");

            let severity = match sev_str {
                "critical" => Severity::Critical,
                "high" => Severity::High,
                "medium" => Severity::Medium,
                "low" => Severity::Low,
                _ => Severity::Medium,
            };

            let fix = if !patched.is_empty() {
                Some(Fix::DependencyUpgrade {
                    package: pkg_name.into(),
                    to: patched.into(),
                })
            } else {
                None
            };

            findings.push(Finding {
                id: Uuid::new_v4(),
                detector: "dependency-audit".into(),
                severity,
                category: FindingCategory::DependencyVuln,
                file: PathBuf::from("Cargo.toml"),
                line: Some(advisory_line(id)),
                title: format!("{pkg_name} {pkg_version} ({id})"),
                description: title.to_string(),
                evidence: vec![],
                covered: true,
                suggestion: if !patched.is_empty() {
                    format!("Upgrade {pkg_name} to {patched}")
                } else {
                    "No patched version available — consider alternative crate".into()
                },
                explanation: None,
                fix,
            });
        }
    }

    // Parse warnings (unmaintained crates, yanked versions, informational advisories)
    let warnings = parsed.get("warnings").and_then(|v| v.as_object());
    if let Some(warning_map) = warnings {
        for (kind, entries) in warning_map {
            let list = match entries.as_array() {
                Some(a) => a,
                None => continue,
            };
            for entry in list {
                let advisory = &entry["advisory"];
                let id = advisory["id"].as_str().unwrap_or("unknown");
                let title = advisory["title"].as_str().unwrap_or("advisory warning");
                let pkg_name = entry["package"]["name"].as_str().unwrap_or("unknown");
                let pkg_version = entry["package"]["version"].as_str().unwrap_or("?");

                findings.push(Finding {
                    id: Uuid::new_v4(),
                    detector: "dependency-audit".into(),
                    severity: Severity::Info,
                    category: FindingCategory::DependencyVuln,
                    file: PathBuf::from("Cargo.toml"),
                    line: Some(advisory_line(id)),
                    title: format!("{pkg_name} {pkg_version} — {kind} ({id})"),
                    description: title.to_string(),
                    evidence: vec![],
                    covered: true,
                    suggestion: format!("Review {kind} advisory for {pkg_name}: {id}"),
                    explanation: None,
                    fix: None,
                });
            }
        }
    }

    Ok(findings)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::DetectConfig;
    use crate::context::AnalysisContext;
    use crate::finding::FindingCategory;
    use apex_core::command::CommandOutput;
    use apex_core::fixture_runner::FixtureRunner;
    use apex_coverage::CoverageOracle;
    use std::collections::HashMap;
    use std::sync::Arc;

    fn make_ctx_with_runner(runner: FixtureRunner) -> AnalysisContext {
        AnalysisContext {
            target_root: PathBuf::from("/tmp/test"),
            language: apex_core::types::Language::Rust,
            oracle: Arc::new(CoverageOracle::new()),
            file_paths: HashMap::new(),
            known_bugs: vec![],
            source_cache: HashMap::new(),
            fuzz_corpus: None,
            config: DetectConfig::default(),
            runner: Arc::new(runner),
        }
    }

    #[tokio::test]
    async fn analyze_with_vulns() {
        let audit_json = r#"{
            "vulnerabilities": {
                "found": 1,
                "list": [{
                    "advisory": {
                        "id": "RUSTSEC-2023-0044",
                        "title": "openssl: X.509 bypass",
                        "severity": "high"
                    },
                    "package": {"name": "openssl", "version": "0.10.38"},
                    "versions": {"patched": [">=0.10.55"]}
                }]
            }
        }"#;
        let runner = FixtureRunner::new()
            .on("cargo", CommandOutput::success(audit_json.as_bytes().to_vec()));
        let ctx = make_ctx_with_runner(runner);
        let findings = DependencyAuditDetector.analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 1);
        assert!(findings[0].title.contains("RUSTSEC-2023-0044"));
    }

    #[tokio::test]
    async fn analyze_no_vulns() {
        let audit_json = r#"{"vulnerabilities": {"found": 0, "list": []}}"#;
        let runner = FixtureRunner::new()
            .on("cargo", CommandOutput::success(audit_json.as_bytes().to_vec()));
        let ctx = make_ctx_with_runner(runner);
        let findings = DependencyAuditDetector.analyze(&ctx).await.unwrap();
        assert!(findings.is_empty());
    }

    #[test]
    fn parse_cargo_audit_json_with_vulns() {
        let json = r#"{
            "vulnerabilities": {
                "found": 1,
                "list": [{
                    "advisory": {
                        "id": "RUSTSEC-2023-0044",
                        "title": "openssl: X.509 bypass",
                        "severity": "high",
                        "url": "https://rustsec.org/advisories/RUSTSEC-2023-0044",
                        "description": "desc"
                    },
                    "package": {
                        "name": "openssl",
                        "version": "0.10.38"
                    },
                    "versions": {
                        "patched": [">=0.10.55"]
                    }
                }]
            }
        }"#;
        let findings = parse_cargo_audit_output(json).unwrap();
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].category, FindingCategory::DependencyVuln);
        assert!(findings[0].title.contains("RUSTSEC-2023-0044"));
        assert_eq!(findings[0].severity, Severity::High);
    }

    #[test]
    fn parse_cargo_audit_json_no_vulns() {
        let json = r#"{"vulnerabilities": {"found": 0, "list": []}}"#;
        let findings = parse_cargo_audit_output(json).unwrap();
        assert!(findings.is_empty());
    }

    #[test]
    fn parse_cargo_audit_invalid_json() {
        let result = parse_cargo_audit_output("not json");
        assert!(result.is_err());
    }

    #[test]
    fn parse_cargo_audit_json_with_warnings() {
        let json = r#"{
            "vulnerabilities": {"found": 0, "list": []},
            "warnings": {
                "unmaintained": [{
                    "advisory": {
                        "id": "RUSTSEC-2024-0370",
                        "title": "proc-macro-error is unmaintained"
                    },
                    "package": {
                        "name": "proc-macro-error",
                        "version": "1.0.4"
                    },
                    "versions": {"patched": []}
                }]
            }
        }"#;
        let findings = parse_cargo_audit_output(json).unwrap();
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, Severity::Info);
        assert!(findings[0].title.contains("unmaintained"));
        assert!(findings[0].title.contains("proc-macro-error"));
    }

    #[test]
    fn parse_cargo_audit_json_with_trailing_warnings() {
        // cargo-audit appends warnings to stdout after the JSON object
        let json = r#"{"vulnerabilities": {"found": 0, "list": []}}
warning: 2 allowed advisories were not found in the advisory database:
  RUSTSEC-2020-0159, RUSTSEC-2024-0370
"#;
        let findings = parse_cargo_audit_output(json).unwrap();
        assert!(findings.is_empty());
    }

    #[test]
    fn uses_cargo_subprocess_returns_true() {
        let d = DependencyAuditDetector;
        assert!(d.uses_cargo_subprocess());
    }

    #[test]
    fn parse_cargo_audit_empty_output() {
        let result = parse_cargo_audit_output("");
        assert!(result.is_err());
    }

    #[test]
    fn parse_cargo_audit_critical_severity() {
        let json = r#"{
            "vulnerabilities": {
                "found": 1,
                "list": [{
                    "advisory": {
                        "id": "RUSTSEC-2024-0001",
                        "title": "critical vuln",
                        "severity": "critical"
                    },
                    "package": {"name": "badlib", "version": "0.1.0"},
                    "versions": {"patched": [">=0.2.0"]}
                }]
            }
        }"#;
        let findings = parse_cargo_audit_output(json).unwrap();
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, Severity::Critical);
        assert!(findings[0].suggestion.contains("Upgrade"));
    }

    #[test]
    fn parse_cargo_audit_low_severity() {
        let json = r#"{
            "vulnerabilities": {
                "found": 1,
                "list": [{
                    "advisory": {
                        "id": "RUSTSEC-2024-0002",
                        "title": "low vuln",
                        "severity": "low"
                    },
                    "package": {"name": "minorlib", "version": "1.0.0"},
                    "versions": {"patched": []}
                }]
            }
        }"#;
        let findings = parse_cargo_audit_output(json).unwrap();
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, Severity::Low);
        assert!(findings[0].fix.is_none()); // no patched version
        assert!(findings[0].suggestion.contains("No patched version"));
    }

    #[test]
    fn parse_cargo_audit_unknown_severity() {
        let json = r#"{
            "vulnerabilities": {
                "found": 1,
                "list": [{
                    "advisory": {
                        "id": "RUSTSEC-2024-0003",
                        "title": "vuln",
                        "severity": "unknown"
                    },
                    "package": {"name": "lib", "version": "1.0.0"},
                    "versions": {"patched": []}
                }]
            }
        }"#;
        let findings = parse_cargo_audit_output(json).unwrap();
        assert_eq!(findings[0].severity, Severity::Medium); // _ fallback
    }

    #[test]
    fn parse_cargo_audit_medium_severity() {
        let json = r#"{
            "vulnerabilities": {
                "found": 1,
                "list": [{
                    "advisory": {
                        "id": "RUSTSEC-2024-0004",
                        "title": "medium vuln",
                        "severity": "medium"
                    },
                    "package": {"name": "medlib", "version": "1.0.0"},
                    "versions": {"patched": [">=1.1.0"]}
                }]
            }
        }"#;
        let findings = parse_cargo_audit_output(json).unwrap();
        assert_eq!(findings[0].severity, Severity::Medium);
        assert!(findings[0].fix.is_some());
    }

    #[test]
    fn parse_cargo_audit_warnings_non_array_skipped() {
        let json = r#"{
            "vulnerabilities": {"found": 0, "list": []},
            "warnings": {
                "unmaintained": "not an array"
            }
        }"#;
        let findings = parse_cargo_audit_output(json).unwrap();
        assert!(findings.is_empty());
    }

    #[test]
    fn advisory_line_is_deterministic() {
        let l1 = advisory_line("RUSTSEC-2023-0001");
        let l2 = advisory_line("RUSTSEC-2023-0001");
        assert_eq!(l1, l2);
        assert!(l1 >= 1);
    }

    #[test]
    fn advisory_line_differs_for_different_ids() {
        let l1 = advisory_line("RUSTSEC-2023-0001");
        let l2 = advisory_line("RUSTSEC-2024-0999");
        // Different IDs should almost certainly produce different lines
        assert_ne!(l1, l2);
    }
}
