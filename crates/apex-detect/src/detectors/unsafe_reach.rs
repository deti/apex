use apex_core::error::{ApexError, Result};
use async_trait::async_trait;
use std::path::PathBuf;
use uuid::Uuid;

use crate::context::AnalysisContext;
use crate::finding::{Finding, FindingCategory, Severity};
use crate::Detector;

pub struct UnsafeReachabilityDetector;

#[async_trait]
impl Detector for UnsafeReachabilityDetector {
    fn name(&self) -> &str {
        "unsafe-reachability"
    }

    fn uses_cargo_subprocess(&self) -> bool {
        true
    }

    async fn analyze(&self, ctx: &AnalysisContext) -> Result<Vec<Finding>> {
        let output = tokio::process::Command::new("cargo")
            .args(["geiger", "--output-format", "json", "--all-features"])
            .current_dir(&ctx.target_root)
            .output()
            .await
            .map_err(|e| ApexError::Detect(format!("cargo-geiger: {e}")))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            if stderr.contains("no such command") || stderr.contains("not found") {
                tracing::info!("cargo-geiger not installed, skipping unsafe analysis");
                return Ok(vec![]);
            }
            return Err(ApexError::Detect(format!("cargo-geiger failed:\n{stderr}")));
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let pkg_name = ctx
            .target_root
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown");

        parse_geiger_output(&stdout, pkg_name)
    }
}

pub fn parse_geiger_output(json_str: &str, target_pkg: &str) -> Result<Vec<Finding>> {
    let parsed: serde_json::Value = serde_json::from_str(json_str)
        .map_err(|e| ApexError::Detect(format!("geiger JSON parse: {e}")))?;

    let mut findings = Vec::new();

    let packages = parsed.get("packages").and_then(|p| p.as_array());

    if let Some(pkgs) = packages {
        for pkg in pkgs {
            let name = pkg["package"]["name"].as_str().unwrap_or("");
            if !name.eq_ignore_ascii_case(target_pkg) && !target_pkg.is_empty() {
                continue;
            }

            let used = &pkg["unsafety"]["used"];
            let unsafe_fns = used["functions"]["unsafe_"].as_u64().unwrap_or(0);
            let unsafe_exprs = used["exprs"]["unsafe_"].as_u64().unwrap_or(0);

            if unsafe_fns > 0 || unsafe_exprs > 0 {
                findings.push(Finding {
                    id: Uuid::new_v4(),
                    detector: "unsafe-reachability".into(),
                    severity: Severity::Medium,
                    category: FindingCategory::UnsafeCode,
                    file: PathBuf::from("Cargo.toml"),
                    line: None,
                    title: format!(
                        "{name}: {unsafe_fns} unsafe fn(s), {unsafe_exprs} unsafe expr(s)"
                    ),
                    description: format!(
                        "Package {name} uses {unsafe_fns} unsafe functions and {unsafe_exprs} unsafe expressions"
                    ),
                    evidence: vec![],
                    covered: false,
                    suggestion: "Audit unsafe blocks for memory safety, add targeted fuzz tests".into(),
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
    use crate::finding::FindingCategory;

    #[test]
    fn parse_geiger_json_with_unsafe() {
        let json = r#"{
            "packages": [{
                "package": {"name": "mylib", "version": "0.1.0"},
                "unsafety": {
                    "used": {
                        "functions": {"unsafe_": 2},
                        "exprs": {"unsafe_": 5}
                    },
                    "unused": {
                        "functions": {"unsafe_": 0},
                        "exprs": {"unsafe_": 0}
                    }
                }
            }]
        }"#;
        let findings = parse_geiger_output(json, "mylib").unwrap();
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].category, FindingCategory::UnsafeCode);
        assert!(findings[0].title.contains("unsafe"));
    }

    #[test]
    fn parse_geiger_json_no_unsafe() {
        let json = r#"{
            "packages": [{
                "package": {"name": "mylib", "version": "0.1.0"},
                "unsafety": {
                    "used": {"functions": {"unsafe_": 0}, "exprs": {"unsafe_": 0}},
                    "unused": {"functions": {"unsafe_": 0}, "exprs": {"unsafe_": 0}}
                }
            }]
        }"#;
        let findings = parse_geiger_output(json, "mylib").unwrap();
        assert!(findings.is_empty());
    }

    #[test]
    fn uses_cargo_subprocess_returns_true() {
        assert!(UnsafeReachabilityDetector.uses_cargo_subprocess());
    }

    #[test]
    fn parse_geiger_invalid_json() {
        let result = parse_geiger_output("not json", "pkg");
        assert!(result.is_err());
    }

    #[test]
    fn parse_geiger_no_packages_key() {
        let json = r#"{"status": "ok"}"#;
        let findings = parse_geiger_output(json, "pkg").unwrap();
        assert!(findings.is_empty());
    }

    #[test]
    fn parse_geiger_skips_non_matching_package() {
        let json = r#"{
            "packages": [{
                "package": {"name": "other-lib", "version": "1.0.0"},
                "unsafety": {
                    "used": {"functions": {"unsafe_": 5}, "exprs": {"unsafe_": 3}},
                    "unused": {"functions": {"unsafe_": 0}, "exprs": {"unsafe_": 0}}
                }
            }]
        }"#;
        let findings = parse_geiger_output(json, "mylib").unwrap();
        assert!(findings.is_empty());
    }

    #[test]
    fn parse_geiger_empty_target_pkg_includes_all() {
        let json = r#"{
            "packages": [{
                "package": {"name": "any-lib", "version": "1.0.0"},
                "unsafety": {
                    "used": {"functions": {"unsafe_": 1}, "exprs": {"unsafe_": 0}},
                    "unused": {"functions": {"unsafe_": 0}, "exprs": {"unsafe_": 0}}
                }
            }]
        }"#;
        // Empty target_pkg: !name.eq_ignore_ascii_case("") is true, but !target_pkg.is_empty() is false
        // So the skip condition is false → package is included
        let findings = parse_geiger_output(json, "").unwrap();
        assert_eq!(findings.len(), 1);
    }

    #[test]
    fn parse_geiger_only_unsafe_exprs() {
        let json = r#"{
            "packages": [{
                "package": {"name": "mylib", "version": "0.1.0"},
                "unsafety": {
                    "used": {"functions": {"unsafe_": 0}, "exprs": {"unsafe_": 3}},
                    "unused": {"functions": {"unsafe_": 0}, "exprs": {"unsafe_": 0}}
                }
            }]
        }"#;
        let findings = parse_geiger_output(json, "mylib").unwrap();
        assert_eq!(findings.len(), 1);
        assert!(findings[0].title.contains("0 unsafe fn(s)"));
        assert!(findings[0].title.contains("3 unsafe expr(s)"));
    }

    #[test]
    fn parse_geiger_case_insensitive_match() {
        let json = r#"{
            "packages": [{
                "package": {"name": "MyLib", "version": "1.0.0"},
                "unsafety": {
                    "used": {"functions": {"unsafe_": 1}, "exprs": {"unsafe_": 0}},
                    "unused": {"functions": {"unsafe_": 0}, "exprs": {"unsafe_": 0}}
                }
            }]
        }"#;
        let findings = parse_geiger_output(json, "mylib").unwrap();
        assert_eq!(findings.len(), 1);
    }
}
