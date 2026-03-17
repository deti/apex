use apex_core::command::CommandSpec;
use apex_core::error::{ApexError, Result};
use apex_core::types::Language;
use async_trait::async_trait;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::PathBuf;
use uuid::Uuid;

use crate::context::AnalysisContext;
use crate::finding::{Finding, FindingCategory, Fix, Severity};
use crate::Detector;

/// Returns true if the error indicates the tool binary was not found on PATH.
///
/// Covers:
/// - `ApexError::Subprocess { exit_code: 127, .. }` — shell convention for "command not found"
/// - `ApexError::Subprocess` whose stderr contains spawn-failure text from
///   `tokio::process::Command::spawn` when the binary does not exist (e.g.
///   "spawn cargo-audit: No such file or directory")
fn is_tool_not_found(err: &ApexError) -> bool {
    match err {
        ApexError::Subprocess { exit_code, stderr } => {
            *exit_code == 127
                || stderr.contains("not found")
                || stderr.contains("No such file")
        }
        _ => false,
    }
}

/// Build a single Info-severity finding indicating the audit tool is not installed.
fn tool_not_installed_finding(tool: &str, file: &str) -> Finding {
    Finding {
        id: Uuid::new_v4(),
        detector: "dependency-audit".into(),
        severity: Severity::Info,
        category: FindingCategory::DependencyVuln,
        file: PathBuf::from(file),
        line: None,
        title: format!("{tool} not installed — dependency audit skipped"),
        description: format!("Install {tool} to enable dependency vulnerability scanning."),
        evidence: vec![],
        covered: true,
        suggestion: format!("Install {tool} and re-run the detector."),
        explanation: None,
        fix: None,
        cwe_ids: vec![],
    }
}

/// Produce a pseudo line number from an advisory ID so each advisory
/// gets a unique dedup key (file, line, category) in the pipeline.
fn advisory_line(id: &str) -> u32 {
    let mut h = DefaultHasher::new();
    id.hash(&mut h);
    (h.finish() % 1_000_000) as u32 + 1
}

pub struct DependencyAuditDetector;

impl DependencyAuditDetector {
    async fn audit_cargo(&self, ctx: &AnalysisContext) -> Result<Vec<Finding>> {
        let spec = CommandSpec::new("cargo", &ctx.target_root).args(["audit", "--json"]);

        let output = match ctx.runner.run_command(&spec).await {
            Ok(o) => o,
            Err(e) if is_tool_not_found(&e) => {
                return Ok(vec![tool_not_installed_finding(
                    "cargo audit",
                    "Cargo.toml",
                )]);
            }
            Err(e) => return Err(ApexError::Detect(format!("cargo-audit: {e}"))),
        };

        let stdout = String::from_utf8_lossy(&output.stdout);
        parse_cargo_audit_output(&stdout)
    }

    async fn audit_pip(&self, ctx: &AnalysisContext) -> Result<Vec<Finding>> {
        let spec = CommandSpec::new("pip", &ctx.target_root)
            .args(["audit", "--format", "json", "--output", "-"]);

        let output = match ctx.runner.run_command(&spec).await {
            Ok(o) => o,
            Err(e) if is_tool_not_found(&e) => {
                return Ok(vec![tool_not_installed_finding(
                    "pip audit",
                    "requirements.txt",
                )]);
            }
            Err(e) => return Err(ApexError::Detect(format!("pip-audit: {e}"))),
        };

        let stdout = String::from_utf8_lossy(&output.stdout);
        parse_pip_audit_output(&stdout)
    }

    async fn audit_npm(&self, ctx: &AnalysisContext) -> Result<Vec<Finding>> {
        let spec = CommandSpec::new("npm", &ctx.target_root).args(["audit", "--json"]);

        let output = match ctx.runner.run_command(&spec).await {
            Ok(o) => o,
            Err(e) if is_tool_not_found(&e) => {
                return Ok(vec![tool_not_installed_finding(
                    "npm audit",
                    "package.json",
                )]);
            }
            Err(e) => return Err(ApexError::Detect(format!("npm-audit: {e}"))),
        };

        let stdout = String::from_utf8_lossy(&output.stdout);
        parse_npm_audit_output(&stdout)
    }

    /// Audit Rust Cargo.lock directly using `cargo audit --json`.
    /// This is an alias for `audit_cargo` but named to reflect the lockfile source.
    async fn audit_cargo_lock(&self, ctx: &AnalysisContext) -> Result<Vec<Finding>> {
        self.audit_cargo(ctx).await
    }

    /// Audit Go modules via `go list -m -json all` + advisory lookup.
    ///
    /// Currently a stub — returns an Info finding if go.sum is detected without
    /// the `govulncheck` tool installed.  Full implementation requires govulncheck.
    async fn audit_go(&self, ctx: &AnalysisContext) -> Result<Vec<Finding>> {
        let spec = CommandSpec::new("govulncheck", &ctx.target_root).args(["./..."]);

        match ctx.runner.run_command(&spec).await {
            Ok(output) => {
                let stdout = String::from_utf8_lossy(&output.stdout);
                // govulncheck outputs one JSON object per vulnerability; count findings.
                let mut findings = Vec::new();
                for line in stdout.lines() {
                    let parse_result: std::result::Result<serde_json::Value, serde_json::Error> =
                        serde_json::from_str(line);
                    if let Ok(v) = parse_result {
                        if v.get("vulnerability").is_some() {
                            let id = v["vulnerability"]["id"].as_str().unwrap_or("unknown");
                            let pkg = v["vulnerability"]["modules"]
                                .as_array()
                                .and_then(|a| a.first())
                                .and_then(|m| m["module"].as_str())
                                .unwrap_or("unknown");
                            findings.push(Finding {
                                id: Uuid::new_v4(),
                                detector: "dependency-audit".into(),
                                severity: Severity::High,
                                category: FindingCategory::DependencyVuln,
                                file: PathBuf::from("go.sum"),
                                line: Some(advisory_line(id)),
                                title: format!("{pkg} ({id})"),
                                description: format!("govulncheck reported {id} in {pkg}"),
                                evidence: vec![],
                                covered: true,
                                suggestion: format!("Review and upgrade {pkg}"),
                                explanation: None,
                                fix: None,
                                cwe_ids: vec![1395],
                            });
                        }
                    }
                }
                Ok(findings)
            }
            Err(e) if is_tool_not_found(&e) => Ok(vec![tool_not_installed_finding(
                "govulncheck",
                "go.sum",
            )]),
            Err(e) => Err(ApexError::Detect(format!("govulncheck: {e}"))),
        }
    }

    /// Audit Maven pom.xml via `mvn dependency-check:check` (OWASP dependency-check).
    ///
    /// Currently a stub — returns an Info finding if the tool is not installed.
    async fn audit_maven(&self, ctx: &AnalysisContext) -> Result<Vec<Finding>> {
        let spec = CommandSpec::new("mvn", &ctx.target_root)
            .args(["dependency-check:check", "-DfailBuildOnCVSS=0", "-q"]);

        match ctx.runner.run_command(&spec).await {
            Ok(_output) => {
                // Minimal stub: in a full implementation we would parse the XML report.
                // For now, return empty (no findings produced by the stub).
                Ok(vec![])
            }
            Err(e) if is_tool_not_found(&e) => Ok(vec![tool_not_installed_finding(
                "mvn dependency-check",
                "pom.xml",
            )]),
            Err(e) => Err(ApexError::Detect(format!("mvn dependency-check: {e}"))),
        }
    }
}

#[async_trait]
impl Detector for DependencyAuditDetector {
    fn name(&self) -> &str {
        "dependency-audit"
    }

    fn uses_cargo_subprocess(&self) -> bool {
        true
    }

    async fn analyze(&self, ctx: &AnalysisContext) -> Result<Vec<Finding>> {
        match ctx.language {
            Language::Rust => self.audit_cargo_lock(ctx).await,
            Language::Python => self.audit_pip(ctx).await,
            Language::JavaScript => self.audit_npm(ctx).await,
            Language::Go => self.audit_go(ctx).await,
            Language::Java | Language::Kotlin => self.audit_maven(ctx).await,
            _ => Ok(vec![]),
        }
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
                cwe_ids: vec![1395],
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
                    cwe_ids: vec![],
                });
            }
        }
    }

    Ok(findings)
}

/// Parse `pip audit --format json --output -` output.
///
/// Format: `[{"name":"pkg","version":"1.0","vulns":[{"id":"PYSEC-XXX","fix_versions":["2.0"],"description":"..."}]}]`
pub fn parse_pip_audit_output(raw: &str) -> Result<Vec<Finding>> {
    if raw.trim().is_empty() {
        return Ok(vec![]);
    }

    let parsed: serde_json::Value = serde_json::from_str(raw)
        .map_err(|e| ApexError::Detect(format!("pip-audit JSON parse: {e}")))?;

    let packages = parsed
        .as_array()
        .ok_or_else(|| ApexError::Detect("pip-audit: expected JSON array".into()))?;

    let mut findings = Vec::new();

    for pkg in packages {
        let pkg_name = pkg["name"].as_str().unwrap_or("unknown");
        let pkg_version = pkg["version"].as_str().unwrap_or("?");

        let vulns = match pkg["vulns"].as_array() {
            Some(v) => v,
            None => continue,
        };

        for vuln in vulns {
            let id = vuln["id"].as_str().unwrap_or("unknown");
            let description = vuln["description"].as_str().unwrap_or("no description");
            let fix_version = vuln["fix_versions"]
                .as_array()
                .and_then(|a| a.first())
                .and_then(|v| v.as_str())
                .unwrap_or("");

            let fix = if !fix_version.is_empty() {
                Some(Fix::DependencyUpgrade {
                    package: pkg_name.into(),
                    to: fix_version.into(),
                })
            } else {
                None
            };

            findings.push(Finding {
                id: Uuid::new_v4(),
                detector: "dependency-audit".into(),
                // pip-audit JSON does not include per-advisory severity; default to High.
                severity: Severity::High,
                category: FindingCategory::DependencyVuln,
                file: PathBuf::from("requirements.txt"),
                line: Some(advisory_line(id)),
                title: format!("{pkg_name} {pkg_version} ({id})"),
                description: description.to_string(),
                evidence: vec![],
                covered: true,
                suggestion: if !fix_version.is_empty() {
                    format!("Upgrade {pkg_name} to {fix_version}")
                } else {
                    "No fixed version available — consider alternative package".into()
                },
                explanation: None,
                fix,
                cwe_ids: vec![1395],
            });
        }
    }

    Ok(findings)
}

/// Parse `npm audit --json` output.
///
/// Format: `{"vulnerabilities":{"lodash":{"severity":"high","via":[{"title":"...","url":"..."}],"range":"<4.17.21"}}}`
pub fn parse_npm_audit_output(raw: &str) -> Result<Vec<Finding>> {
    if raw.trim().is_empty() {
        return Ok(vec![]);
    }

    let parsed: serde_json::Value = serde_json::from_str(raw)
        .map_err(|e| ApexError::Detect(format!("npm-audit JSON parse: {e}")))?;

    let vulnerabilities = match parsed.get("vulnerabilities").and_then(|v| v.as_object()) {
        Some(v) => v,
        None => return Ok(vec![]),
    };

    let mut findings = Vec::new();

    for (pkg_name, vuln_info) in vulnerabilities {
        let sev_str = vuln_info["severity"].as_str().unwrap_or("medium");
        let range = vuln_info["range"].as_str().unwrap_or("unknown range");

        let severity = match sev_str {
            "critical" => Severity::Critical,
            "high" => Severity::High,
            "moderate" | "medium" => Severity::Medium,
            "low" => Severity::Low,
            "info" => Severity::Info,
            _ => Severity::Medium,
        };

        // Collect titles from `via` array (advisory details)
        let via = vuln_info["via"].as_array();
        let title = via
            .and_then(|arr| {
                arr.iter()
                    .find_map(|v| v.get("title").and_then(|t| t.as_str()))
            })
            .unwrap_or("vulnerability");

        let url = via
            .and_then(|arr| {
                arr.iter()
                    .find_map(|v| v.get("url").and_then(|u| u.as_str()))
            })
            .unwrap_or("");

        // Use package name + range as a stable dedup ID
        let advisory_id = format!("{pkg_name}@{range}");

        // npm audit includes fixAvailable as bool or {name, version} object
        let fix = vuln_info
            .get("fixAvailable")
            .and_then(|f| f.as_object())
            .and_then(|obj| obj.get("version").and_then(|v| v.as_str()))
            .map(|version| Fix::DependencyUpgrade {
                package: pkg_name.clone(),
                to: version.to_string(),
            });

        findings.push(Finding {
            id: Uuid::new_v4(),
            detector: "dependency-audit".into(),
            severity,
            category: FindingCategory::DependencyVuln,
            file: PathBuf::from("package.json"),
            line: Some(advisory_line(&advisory_id)),
            title: format!("{pkg_name} ({sev_str}): {title}"),
            description: if url.is_empty() {
                format!("Vulnerable range: {range}")
            } else {
                format!("Vulnerable range: {range} — {url}")
            },
            evidence: vec![],
            covered: true,
            suggestion: format!("Upgrade {pkg_name} to a version outside {range}"),
            explanation: None,
            fix,
            cwe_ids: vec![1395],
        });
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
            runner: Arc::new(runner),
            ..AnalysisContext::test_default()
        }
    }

    fn make_ctx_with_lang(runner: FixtureRunner, lang: Language) -> AnalysisContext {
        AnalysisContext {
            language: lang,
            runner: Arc::new(runner),
            ..AnalysisContext::test_default()
        }
    }

    // ── Cargo audit tests ────────────────────────────────────────────────────

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
        let runner = FixtureRunner::new().on(
            "cargo",
            CommandOutput::success(audit_json.as_bytes().to_vec()),
        );
        let ctx = make_ctx_with_runner(runner);
        let findings = DependencyAuditDetector.analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 1);
        assert!(findings[0].title.contains("RUSTSEC-2023-0044"));
    }

    #[tokio::test]
    async fn analyze_no_vulns() {
        let audit_json = r#"{"vulnerabilities": {"found": 0, "list": []}}"#;
        let runner = FixtureRunner::new().on(
            "cargo",
            CommandOutput::success(audit_json.as_bytes().to_vec()),
        );
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

    // ── pip audit tests ──────────────────────────────────────────────────────

    #[test]
    fn parse_pip_audit_json_with_vulns() {
        let raw = r#"[{"name":"requests","version":"2.25.0","vulns":[{"id":"PYSEC-2023-74","fix_versions":["2.31.0"],"description":"Session fixation"}]}]"#;
        let findings = parse_pip_audit_output(raw).unwrap();
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].category, FindingCategory::DependencyVuln);
        assert!(findings[0].title.contains("requests"));
    }

    #[test]
    fn parse_pip_audit_json_no_vulns() {
        let raw = r#"[{"name":"requests","version":"2.31.0","vulns":[]}]"#;
        let findings = parse_pip_audit_output(raw).unwrap();
        assert!(findings.is_empty());
    }

    #[test]
    fn parse_pip_audit_empty() {
        let findings = parse_pip_audit_output("").unwrap();
        assert!(findings.is_empty());
    }

    #[test]
    fn parse_pip_audit_empty_array() {
        let findings = parse_pip_audit_output("[]").unwrap();
        assert!(findings.is_empty());
    }

    #[test]
    fn parse_pip_audit_fix_version_present() {
        let raw = r#"[{"name":"flask","version":"1.0.0","vulns":[{"id":"PYSEC-2023-10","fix_versions":["2.0.0"],"description":"XSS vuln"}]}]"#;
        let findings = parse_pip_audit_output(raw).unwrap();
        assert_eq!(findings.len(), 1);
        assert!(findings[0].fix.is_some());
        assert!(findings[0].suggestion.contains("Upgrade"));
        assert_eq!(findings[0].file, PathBuf::from("requirements.txt"));
    }

    #[test]
    fn parse_pip_audit_no_fix_version() {
        let raw = r#"[{"name":"oldlib","version":"0.1.0","vulns":[{"id":"PYSEC-2023-99","fix_versions":[],"description":"No fix"}]}]"#;
        let findings = parse_pip_audit_output(raw).unwrap();
        assert_eq!(findings.len(), 1);
        assert!(findings[0].fix.is_none());
        assert!(findings[0].suggestion.contains("No fixed version"));
    }

    #[test]
    fn parse_pip_audit_invalid_json() {
        let result = parse_pip_audit_output("not json");
        assert!(result.is_err());
    }

    // ── npm audit tests ──────────────────────────────────────────────────────

    #[test]
    fn parse_npm_audit_json_with_vulns() {
        let raw = r#"{"vulnerabilities":{"lodash":{"severity":"high","via":[{"title":"Prototype Pollution","url":"https://github.com/advisories/GHSA-1234"}],"range":"<4.17.21"}}}"#;
        let findings = parse_npm_audit_output(raw).unwrap();
        assert_eq!(findings.len(), 1);
        assert!(findings[0].title.contains("lodash"));
    }

    #[test]
    fn parse_npm_audit_json_no_vulns() {
        let raw = r#"{"vulnerabilities":{"safe-pkg":{"severity":"low","via":[],"range":"*"}}}"#;
        let findings = parse_npm_audit_output(raw).unwrap();
        // Entry exists but via is empty — still a finding (severity is set)
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, Severity::Low);
    }

    #[test]
    fn parse_npm_audit_empty() {
        let findings = parse_npm_audit_output("").unwrap();
        assert!(findings.is_empty());
    }

    #[test]
    fn parse_npm_audit_empty_vulnerabilities() {
        let raw = r#"{"vulnerabilities":{}}"#;
        let findings = parse_npm_audit_output(raw).unwrap();
        assert!(findings.is_empty());
    }

    #[test]
    fn parse_npm_audit_severity_mapping() {
        // Test each severity variant individually to avoid HashSet requiring Hash
        let critical_raw =
            r#"{"vulnerabilities":{"a":{"severity":"critical","via":[],"range":"<1.0"}}}"#;
        let findings = parse_npm_audit_output(critical_raw).unwrap();
        assert_eq!(findings[0].severity, Severity::Critical);

        let moderate_raw =
            r#"{"vulnerabilities":{"b":{"severity":"moderate","via":[],"range":"<2.0"}}}"#;
        let findings = parse_npm_audit_output(moderate_raw).unwrap();
        assert_eq!(findings[0].severity, Severity::Medium);

        let info_raw = r#"{"vulnerabilities":{"c":{"severity":"info","via":[],"range":"<3.0"}}}"#;
        let findings = parse_npm_audit_output(info_raw).unwrap();
        assert_eq!(findings[0].severity, Severity::Info);
    }

    #[test]
    fn parse_npm_audit_file_is_package_json() {
        let raw = r#"{"vulnerabilities":{"lodash":{"severity":"high","via":[{"title":"RCE","url":"https://example.com"}],"range":"<4.17.21"}}}"#;
        let findings = parse_npm_audit_output(raw).unwrap();
        assert_eq!(findings[0].file, PathBuf::from("package.json"));
    }

    #[test]
    fn parse_npm_audit_invalid_json() {
        let result = parse_npm_audit_output("not json");
        assert!(result.is_err());
    }

    // ── Language dispatch tests ──────────────────────────────────────────────

    #[tokio::test]
    async fn analyze_python_uses_pip_audit() {
        let runner = FixtureRunner::new().on("pip", CommandOutput::success(b"[]".to_vec()));
        let ctx = make_ctx_with_lang(runner, Language::Python);
        let findings = DependencyAuditDetector.analyze(&ctx).await.unwrap();
        assert!(findings.is_empty());
    }

    #[tokio::test]
    async fn analyze_javascript_uses_npm_audit() {
        let raw = r#"{"vulnerabilities":{}}"#;
        let runner =
            FixtureRunner::new().on("npm", CommandOutput::success(raw.as_bytes().to_vec()));
        let ctx = make_ctx_with_lang(runner, Language::JavaScript);
        let findings = DependencyAuditDetector.analyze(&ctx).await.unwrap();
        assert!(findings.is_empty());
    }

    #[tokio::test]
    async fn analyze_unsupported_language_returns_empty() {
        let runner = FixtureRunner::new();
        let ctx = make_ctx_with_lang(runner, Language::Ruby);
        let findings = DependencyAuditDetector.analyze(&ctx).await.unwrap();
        assert!(findings.is_empty());
    }

    #[tokio::test]
    async fn analyze_python_with_vulns() {
        let raw = r#"[{"name":"requests","version":"2.25.0","vulns":[{"id":"PYSEC-2023-74","fix_versions":["2.31.0"],"description":"Session fixation"}]}]"#;
        let runner =
            FixtureRunner::new().on("pip", CommandOutput::success(raw.as_bytes().to_vec()));
        let ctx = make_ctx_with_lang(runner, Language::Python);
        let findings = DependencyAuditDetector.analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 1);
        assert!(findings[0].title.contains("requests"));
        assert_eq!(findings[0].category, FindingCategory::DependencyVuln);
    }

    #[tokio::test]
    async fn analyze_javascript_with_vulns() {
        let raw = r#"{"vulnerabilities":{"lodash":{"severity":"high","via":[{"title":"Prototype Pollution","url":"https://github.com/advisories/GHSA-1234"}],"range":"<4.17.21"}}}"#;
        let runner =
            FixtureRunner::new().on("npm", CommandOutput::success(raw.as_bytes().to_vec()));
        let ctx = make_ctx_with_lang(runner, Language::JavaScript);
        let findings = DependencyAuditDetector.analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 1);
        assert!(findings[0].title.contains("lodash"));
    }

    // ── Tool-not-installed graceful fallback tests ───────────────────────────

    // Local mock runner so we can inject ApexError::Subprocess (the error
    // RealCommandRunner emits when spawn fails because the binary is absent).
    mockall::mock! {
        pub CmdRunner {}

        #[async_trait]
        impl apex_core::command::CommandRunner for CmdRunner {
            async fn run_command(
                &self,
                spec: &apex_core::command::CommandSpec,
            ) -> apex_core::error::Result<CommandOutput>;
        }
    }

    fn make_ctx_with_mock(mock: MockCmdRunner, lang: Language) -> AnalysisContext {
        AnalysisContext {
            language: lang,
            runner: Arc::new(mock),
            ..AnalysisContext::test_default()
        }
    }

    #[tokio::test]
    async fn cargo_audit_not_installed_returns_info_finding() {
        let mut mock = MockCmdRunner::new();
        mock.expect_run_command().returning(|_| {
            Err(ApexError::Subprocess {
                exit_code: -1,
                stderr: "spawn cargo-audit: No such file or directory".into(),
            })
        });
        let ctx = make_ctx_with_mock(mock, Language::Rust);
        let findings = DependencyAuditDetector.analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, Severity::Info);
        assert!(findings[0].title.contains("not installed"));
        assert!(findings[0].title.contains("cargo audit"));
    }

    #[tokio::test]
    async fn pip_audit_not_installed_returns_info_finding() {
        let mut mock = MockCmdRunner::new();
        mock.expect_run_command().returning(|_| {
            Err(ApexError::Subprocess {
                exit_code: 127,
                stderr: "pip: command not found".into(),
            })
        });
        let ctx = make_ctx_with_mock(mock, Language::Python);
        let findings = DependencyAuditDetector.analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, Severity::Info);
        assert!(findings[0].title.contains("not installed"));
        assert!(findings[0].title.contains("pip audit"));
    }

    #[tokio::test]
    async fn npm_audit_not_installed_returns_info_finding() {
        let mut mock = MockCmdRunner::new();
        mock.expect_run_command().returning(|_| {
            Err(ApexError::Subprocess {
                exit_code: -1,
                stderr: "spawn npm: No such file or directory".into(),
            })
        });
        let ctx = make_ctx_with_mock(mock, Language::JavaScript);
        let findings = DependencyAuditDetector.analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, Severity::Info);
        assert!(findings[0].title.contains("not installed"));
        assert!(findings[0].title.contains("npm audit"));
    }

    #[tokio::test]
    async fn cargo_audit_other_error_propagates() {
        // A non-"not found" error (e.g. network failure) must still propagate.
        let mut mock = MockCmdRunner::new();
        mock.expect_run_command().returning(|_| {
            Err(ApexError::Subprocess {
                exit_code: 1,
                stderr: "error fetching advisory database: network timeout".into(),
            })
        });
        let ctx = make_ctx_with_mock(mock, Language::Rust);
        let result = DependencyAuditDetector.analyze(&ctx).await;
        assert!(result.is_err());
    }

    #[test]
    fn is_tool_not_found_exit_127() {
        let e = ApexError::Subprocess {
            exit_code: 127,
            stderr: "pip: command not found".into(),
        };
        assert!(is_tool_not_found(&e));
    }

    #[test]
    fn is_tool_not_found_no_such_file() {
        let e = ApexError::Subprocess {
            exit_code: -1,
            stderr: "spawn cargo-audit: No such file or directory".into(),
        };
        assert!(is_tool_not_found(&e));
    }

    #[test]
    fn is_tool_not_found_not_found_in_stderr() {
        let e = ApexError::Subprocess {
            exit_code: -1,
            stderr: "spawn npm: not found".into(),
        };
        assert!(is_tool_not_found(&e));
    }

    #[test]
    fn is_tool_not_found_other_subprocess_error() {
        let e = ApexError::Subprocess {
            exit_code: 1,
            stderr: "network timeout".into(),
        };
        assert!(!is_tool_not_found(&e));
    }

    #[test]
    fn is_tool_not_found_detect_error() {
        let e = ApexError::Detect("some detect error".into());
        assert!(!is_tool_not_found(&e));
    }

    // -----------------------------------------------------------------------
    // Bug 20: audit_cargo_lock, audit_go, audit_maven stubs
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn analyze_go_govulncheck_not_installed_returns_info() {
        let mut mock = MockCmdRunner::new();
        mock.expect_run_command().returning(|_| {
            Err(ApexError::Subprocess {
                exit_code: 127,
                stderr: "govulncheck: command not found".into(),
            })
        });
        let ctx = make_ctx_with_mock(mock, Language::Go);
        let findings = DependencyAuditDetector.analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, Severity::Info);
        assert!(findings[0].title.contains("not installed"));
    }

    #[tokio::test]
    async fn analyze_java_maven_not_installed_returns_info() {
        let mut mock = MockCmdRunner::new();
        mock.expect_run_command().returning(|_| {
            Err(ApexError::Subprocess {
                exit_code: 127,
                stderr: "mvn: command not found".into(),
            })
        });
        let ctx = make_ctx_with_mock(mock, Language::Java);
        let findings = DependencyAuditDetector.analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, Severity::Info);
        assert!(findings[0].title.contains("not installed"));
    }

    #[tokio::test]
    async fn analyze_kotlin_maven_not_installed_returns_info() {
        let mut mock = MockCmdRunner::new();
        mock.expect_run_command().returning(|_| {
            Err(ApexError::Subprocess {
                exit_code: 127,
                stderr: "mvn: command not found".into(),
            })
        });
        let ctx = make_ctx_with_mock(mock, Language::Kotlin);
        let findings = DependencyAuditDetector.analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, Severity::Info);
    }

    #[tokio::test]
    async fn analyze_go_govulncheck_success_no_output() {
        // govulncheck ran but found no vulnerabilities (empty output)
        let runner = FixtureRunner::new()
            .on("govulncheck", CommandOutput::success(b"".to_vec()));
        let ctx = make_ctx_with_lang(runner, Language::Go);
        let findings = DependencyAuditDetector.analyze(&ctx).await.unwrap();
        assert!(findings.is_empty());
    }

    #[tokio::test]
    async fn audit_cargo_lock_delegates_to_cargo_audit() {
        // cargo-lock audit delegates to cargo audit JSON output
        let audit_json = r#"{"vulnerabilities": {"found": 0, "list": []}}"#;
        let runner = FixtureRunner::new().on(
            "cargo",
            CommandOutput::success(audit_json.as_bytes().to_vec()),
        );
        let ctx = make_ctx_with_lang(runner, Language::Rust);
        let findings = DependencyAuditDetector.analyze(&ctx).await.unwrap();
        assert!(findings.is_empty());
    }
}
