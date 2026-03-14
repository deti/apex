//! License compliance scanner.
//!
//! Scans project manifests and lockfiles for dependency licenses,
//! then checks them against a configurable policy (Permissive or Enterprise).

use apex_core::error::Result;
use async_trait::async_trait;
use std::path::{Path, PathBuf};
use uuid::Uuid;

use crate::context::AnalysisContext;
use crate::finding::{Finding, FindingCategory, Severity};
use crate::lockfile;
use crate::Detector;

// ---------------------------------------------------------------------------
// Policy
// ---------------------------------------------------------------------------

/// Which licenses are acceptable.
#[derive(Debug, Clone)]
pub enum LicensePolicy {
    /// Only well-known permissive licenses pass; everything else is flagged.
    Permissive,
    /// Explicitly deny strong-copyleft / network-copyleft licenses.
    Enterprise,
    /// Fully custom deny/allow lists.
    Custom {
        deny: Vec<String>,
        allow: Vec<String>,
    },
}

const PERMISSIVE_ALLOW: &[&str] = &[
    "MIT",
    "Apache-2.0",
    "BSD-2-Clause",
    "BSD-3-Clause",
    "ISC",
    "Zlib",
    "Unlicense",
    "CC0-1.0",
];

const ENTERPRISE_DENY: &[&str] = &[
    "GPL-2.0-only",
    "GPL-2.0-or-later",
    "GPL-3.0-only",
    "GPL-3.0-or-later",
    "AGPL-3.0-only",
    "AGPL-3.0-or-later",
    "SSPL-1.0",
    "EUPL-1.2",
];

// ---------------------------------------------------------------------------
// Detector
// ---------------------------------------------------------------------------

pub struct LicenseScanDetector {
    policy: LicensePolicy,
}

impl LicenseScanDetector {
    pub fn new(policy: LicensePolicy) -> Self {
        Self { policy }
    }

    pub fn permissive() -> Self {
        Self::new(LicensePolicy::Permissive)
    }

    pub fn enterprise() -> Self {
        Self::new(LicensePolicy::Enterprise)
    }
}

#[async_trait]
impl Detector for LicenseScanDetector {
    fn name(&self) -> &str {
        "license-scan"
    }

    async fn analyze(&self, ctx: &AnalysisContext) -> Result<Vec<Finding>> {
        let root = &ctx.target_root;
        let mut entries: Vec<LicenseEntry> = Vec::new();

        // 1. Collect licenses from manifest files in the source cache
        for (path, content) in &ctx.source_cache {
            if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                match name {
                    "Cargo.toml" => {
                        entries.extend(parse_cargo_toml_licenses(content, path));
                    }
                    "package.json" => {
                        entries.extend(parse_package_json_licenses(content, path));
                    }
                    "pyproject.toml" => {
                        entries.extend(parse_pyproject_licenses(content, path));
                    }
                    _ => {}
                }
            }
        }

        // 2. Collect licenses from lockfiles on disk
        let cargo_lock = root.join("Cargo.lock");
        if cargo_lock.exists() {
            if let Ok(deps) = lockfile::parse_cargo_lock(&cargo_lock) {
                for dep in deps {
                    if let Some(lic) = dep.license {
                        entries.push(LicenseEntry {
                            package: dep.name,
                            license: lic,
                            file: cargo_lock.clone(),
                        });
                    }
                }
            }
        }

        let pkg_lock = root.join("package-lock.json");
        if pkg_lock.exists() {
            if let Ok(deps) = lockfile::parse_package_lock(&pkg_lock) {
                for dep in deps {
                    if let Some(lic) = dep.license {
                        entries.push(LicenseEntry {
                            package: dep.name,
                            license: lic,
                            file: pkg_lock.clone(),
                        });
                    }
                }
            }
        }

        // 3. Evaluate each entry against the policy
        let mut findings = Vec::new();
        for entry in &entries {
            if let Some(finding) = self.evaluate(entry) {
                findings.push(finding);
            }
        }

        Ok(findings)
    }
}

impl LicenseScanDetector {
    fn evaluate(&self, entry: &LicenseEntry) -> Option<Finding> {
        let verdict = check_policy(&self.policy, &entry.license);
        match verdict {
            PolicyVerdict::Allowed => None,
            PolicyVerdict::Denied { reason } => Some(Finding {
                id: Uuid::new_v4(),
                detector: "license-scan".into(),
                severity: Severity::Critical,
                category: FindingCategory::LicenseViolation,
                file: entry.file.clone(),
                line: None,
                title: format!(
                    "License violation: {} uses {}",
                    entry.package, entry.license
                ),
                description: reason,
                evidence: vec![],
                covered: false,
                suggestion: format!(
                    "Replace '{}' with an alternative under a permissive license, \
                     or obtain a commercial license.",
                    entry.package
                ),
                explanation: None,
                fix: None,
                cwe_ids: vec![],
            }),
            PolicyVerdict::Unknown => Some(Finding {
                id: Uuid::new_v4(),
                detector: "license-scan".into(),
                severity: Severity::Medium,
                category: FindingCategory::LicenseViolation,
                file: entry.file.clone(),
                line: None,
                title: format!(
                    "Unknown license: {} uses '{}'",
                    entry.package, entry.license
                ),
                description: format!(
                    "The license '{}' for package '{}' is not in the approved list. \
                     Manual review required.",
                    entry.license, entry.package
                ),
                evidence: vec![],
                covered: false,
                suggestion: "Review the license terms and add to the allow-list if acceptable."
                    .into(),
                explanation: None,
                fix: None,
                cwe_ids: vec![],
            }),
        }
    }
}

// ---------------------------------------------------------------------------
// Internal types
// ---------------------------------------------------------------------------

struct LicenseEntry {
    package: String,
    license: String,
    file: PathBuf,
}

enum PolicyVerdict {
    Allowed,
    Denied { reason: String },
    Unknown,
}

// ---------------------------------------------------------------------------
// Policy evaluation + simple SPDX expression parsing
// ---------------------------------------------------------------------------

fn check_policy(policy: &LicensePolicy, expr: &str) -> PolicyVerdict {
    match policy {
        LicensePolicy::Permissive => check_permissive(expr),
        LicensePolicy::Enterprise => check_enterprise(expr),
        LicensePolicy::Custom { deny, allow } => check_custom(expr, deny, allow),
    }
}

fn check_permissive(expr: &str) -> PolicyVerdict {
    // For Permissive policy: every component (after SPDX logic) must be in the
    // allow-list, otherwise the expression is Unknown (not Denied).
    if eval_spdx_permissive(expr, PERMISSIVE_ALLOW) {
        PolicyVerdict::Allowed
    } else {
        PolicyVerdict::Unknown
    }
}

fn check_enterprise(expr: &str) -> PolicyVerdict {
    // For Enterprise policy: if any component is explicitly denied, flag it.
    if let Some(bad) = find_denied_in_expr(expr, ENTERPRISE_DENY) {
        PolicyVerdict::Denied {
            reason: format!(
                "License '{bad}' is a copyleft license prohibited under enterprise policy."
            ),
        }
    } else {
        PolicyVerdict::Allowed
    }
}

fn check_custom(expr: &str, deny: &[String], allow: &[String]) -> PolicyVerdict {
    let deny_refs: Vec<&str> = deny.iter().map(|s| s.as_str()).collect();
    let allow_refs: Vec<&str> = allow.iter().map(|s| s.as_str()).collect();

    if let Some(bad) = find_denied_in_expr(expr, &deny_refs) {
        return PolicyVerdict::Denied {
            reason: format!("License '{bad}' is in the custom deny list."),
        };
    }
    if eval_spdx_permissive(expr, &allow_refs) {
        PolicyVerdict::Allowed
    } else {
        PolicyVerdict::Unknown
    }
}

/// Normalize SPDX expression: strip parens, normalize whitespace and operator case.
fn normalize_spdx(expr: &str) -> String {
    let mut s = expr.trim().to_string();
    // Strip parentheses (simple — handles nested too)
    s = s.replace(['(', ')'], "");
    // Normalize whitespace (tabs, multiple spaces → single space)
    s = s.split_whitespace().collect::<Vec<_>>().join(" ");
    // Normalize operator case
    s = s.replace(" or ", " OR ").replace(" and ", " AND ");
    s
}

/// Normalize a single SPDX identifier for matching.
fn normalize_spdx_id(id: &str) -> String {
    let id = id.trim();
    // Strip WITH clauses (exceptions only add permissions)
    let base = if let Some(pos) = id.to_uppercase().find(" WITH ") {
        &id[..pos]
    } else {
        id
    };
    // Map deprecated + suffix: "GPL-2.0+" → "GPL-2.0-or-later"
    if let Some(prefix) = base.strip_suffix('+') {
        format!("{prefix}-or-later")
    } else {
        base.to_string()
    }
}

/// Simple SPDX expression evaluation:
/// - Split on " OR " first (any alternative being allowed suffices)
/// - Split on " AND " (all components must be allowed)
fn eval_spdx_permissive(expr: &str, allow: &[&str]) -> bool {
    let expr = normalize_spdx(expr);
    let or_parts: Vec<&str> = expr.split(" OR ").collect();
    // If any OR-branch is fully allowed, the whole expression is allowed.
    or_parts.iter().any(|or_part| {
        let and_parts: Vec<&str> = or_part.split(" AND ").collect();
        and_parts.iter().all(|id| {
            let normalized = normalize_spdx_id(id);
            allow.iter().any(|a| a.eq_ignore_ascii_case(&normalized))
        })
    })
}

/// Check whether any license identifier in the SPDX expression is denied.
/// For OR: if ALL alternatives are denied, the expression is denied.
/// For AND: if ANY component is denied, the expression is denied.
fn find_denied_in_expr(expr: &str, deny: &[&str]) -> Option<String> {
    let expr = normalize_spdx(expr);
    let or_parts: Vec<&str> = expr.split(" OR ").collect();

    // Collect denied items per OR-branch.
    // An OR expression is denied only if ALL branches are denied.
    let mut all_denied = true;
    let mut first_denied: Option<String> = None;

    for or_part in &or_parts {
        let and_parts: Vec<&str> = or_part.split(" AND ").collect();
        // An AND branch is denied if ANY of its components is denied.
        let branch_denied = and_parts.iter().find_map(|id| {
            let normalized = normalize_spdx_id(id);
            if deny.iter().any(|d| d.eq_ignore_ascii_case(&normalized)) {
                Some(normalized)
            } else {
                None
            }
        });
        if let Some(bad) = branch_denied {
            if first_denied.is_none() {
                first_denied = Some(bad);
            }
        } else {
            all_denied = false;
        }
    }

    if or_parts.len() == 1 {
        // No OR — just check the AND chain directly
        first_denied
    } else if all_denied {
        first_denied
    } else {
        // At least one OR-branch is clean
        None
    }
}

// ---------------------------------------------------------------------------
// Manifest parsers
// ---------------------------------------------------------------------------

fn parse_cargo_toml_licenses(content: &str, path: &Path) -> Vec<LicenseEntry> {
    let mut entries = Vec::new();
    let parsed: toml::Value = match toml::from_str(content) {
        Ok(v) => v,
        Err(_) => return entries,
    };

    // [package] section
    if let Some(pkg) = parsed.get("package").and_then(|v| v.as_table()) {
        let name = pkg
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown")
            .to_string();
        if let Some(lic) = pkg.get("license").and_then(|v| v.as_str()) {
            entries.push(LicenseEntry {
                package: name,
                license: lic.to_string(),
                file: path.to_path_buf(),
            });
        }
    }

    // [dependencies] section — we can't get license from here directly,
    // but we note the manifest path for lockfile cross-ref.

    entries
}

fn parse_package_json_licenses(content: &str, path: &Path) -> Vec<LicenseEntry> {
    let mut entries = Vec::new();
    let parsed: serde_json::Value = match serde_json::from_str(content) {
        Ok(v) => v,
        Err(_) => return entries,
    };

    let name = parsed
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown")
        .to_string();

    if let Some(lic) = parsed.get("license").and_then(|v| v.as_str()) {
        entries.push(LicenseEntry {
            package: name,
            license: lic.to_string(),
            file: path.to_path_buf(),
        });
    }

    entries
}

fn parse_pyproject_licenses(content: &str, path: &Path) -> Vec<LicenseEntry> {
    let mut entries = Vec::new();
    let parsed: toml::Value = match toml::from_str(content) {
        Ok(v) => v,
        Err(_) => return entries,
    };

    let project = match parsed.get("project").and_then(|v| v.as_table()) {
        Some(p) => p,
        None => return entries,
    };

    let name = project
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown")
        .to_string();

    // Direct license field: license = {text = "MIT"} or license = "MIT"
    if let Some(lic_val) = project.get("license") {
        if let Some(text) = lic_val.as_str() {
            entries.push(LicenseEntry {
                package: name.clone(),
                license: text.to_string(),
                file: path.to_path_buf(),
            });
            return entries;
        }
        if let Some(table) = lic_val.as_table() {
            if let Some(text) = table.get("text").and_then(|v| v.as_str()) {
                entries.push(LicenseEntry {
                    package: name.clone(),
                    license: text.to_string(),
                    file: path.to_path_buf(),
                });
                return entries;
            }
        }
    }

    // Classifiers: look for "License :: OSI Approved :: MIT License" etc.
    if let Some(classifiers) = project.get("classifiers").and_then(|v| v.as_array()) {
        for c in classifiers {
            if let Some(s) = c.as_str() {
                if let Some(lic) = extract_license_from_classifier(s) {
                    entries.push(LicenseEntry {
                        package: name.clone(),
                        license: lic,
                        file: path.to_path_buf(),
                    });
                }
            }
        }
    }

    entries
}

/// Map common PyPI license classifiers to SPDX identifiers.
fn extract_license_from_classifier(classifier: &str) -> Option<String> {
    if !classifier.starts_with("License :: ") {
        return None;
    }
    // Common mappings
    let mapping = [
        ("MIT License", "MIT"),
        ("Apache Software License", "Apache-2.0"),
        ("BSD License", "BSD-3-Clause"),
        ("GNU General Public License v3 (GPLv3)", "GPL-3.0-only"),
        ("GNU General Public License v2 (GPLv2)", "GPL-2.0-only"),
        ("GNU Affero General Public License v3", "AGPL-3.0-only"),
        ("ISC License (ISCL)", "ISC"),
        ("The Unlicense (Unlicense)", "Unlicense"),
    ];

    for (fragment, spdx) in &mapping {
        if classifier.contains(fragment) {
            return Some(spdx.to_string());
        }
    }

    // Fallback: return the last segment
    classifier.rsplit(" :: ").next().map(|s| s.to_string())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::AnalysisContext;
    use crate::finding::Severity;
    use std::collections::HashMap;
    use std::sync::Arc;

    fn test_context_with_cache(files: Vec<(&str, &str)>) -> AnalysisContext {
        let mut source_cache = HashMap::new();
        for (path, content) in files {
            source_cache.insert(PathBuf::from(path), content.to_string());
        }
        AnalysisContext {
            target_root: PathBuf::from("/tmp/license-test"),
            source_cache,
            ..AnalysisContext::test_default()
        }
    }

    // -- Policy unit tests --------------------------------------------------

    #[test]
    fn mit_passes_permissive() {
        let result = check_policy(&LicensePolicy::Permissive, "MIT");
        assert!(matches!(result, PolicyVerdict::Allowed));
    }

    #[test]
    fn gpl3_flagged_under_enterprise() {
        let result = check_policy(&LicensePolicy::Enterprise, "GPL-3.0-only");
        assert!(matches!(result, PolicyVerdict::Denied { .. }));
    }

    #[test]
    fn spdx_or_passes_permissive() {
        // "MIT OR Apache-2.0" — at least one branch is allowed
        let result = check_policy(&LicensePolicy::Permissive, "MIT OR Apache-2.0");
        assert!(matches!(result, PolicyVerdict::Allowed));
    }

    #[test]
    fn spdx_and_flagged_under_enterprise() {
        // "GPL-3.0-only AND MIT" — AND means all must be clean, GPL-3.0 is denied
        let result = check_policy(&LicensePolicy::Enterprise, "GPL-3.0-only AND MIT");
        assert!(matches!(result, PolicyVerdict::Denied { .. }));
    }

    #[test]
    fn unknown_license_is_unknown_under_permissive() {
        let result = check_policy(&LicensePolicy::Permissive, "WTFPL");
        assert!(matches!(result, PolicyVerdict::Unknown));
    }

    #[test]
    fn or_with_one_denied_and_one_clean_passes_enterprise() {
        // OR: if at least one branch is clean, it passes
        let result = check_policy(&LicensePolicy::Enterprise, "GPL-3.0-only OR MIT");
        assert!(matches!(result, PolicyVerdict::Allowed));
    }

    #[test]
    fn all_permissive_licenses_pass() {
        for lic in PERMISSIVE_ALLOW {
            let result = check_policy(&LicensePolicy::Permissive, lic);
            assert!(
                matches!(result, PolicyVerdict::Allowed),
                "{lic} should be allowed"
            );
        }
    }

    #[test]
    fn all_enterprise_denied_licenses_flagged() {
        for lic in ENTERPRISE_DENY {
            let result = check_policy(&LicensePolicy::Enterprise, lic);
            assert!(
                matches!(result, PolicyVerdict::Denied { .. }),
                "{lic} should be denied"
            );
        }
    }

    // -- Manifest parsing tests ---------------------------------------------

    #[test]
    fn cargo_toml_license_field() {
        let content = r#"
[package]
name = "my-crate"
version = "0.1.0"
license = "MIT"
"#;
        let entries = parse_cargo_toml_licenses(content, &PathBuf::from("Cargo.toml"));
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].package, "my-crate");
        assert_eq!(entries[0].license, "MIT");
    }

    #[test]
    fn package_json_license_field() {
        let content = r#"{
            "name": "my-pkg",
            "version": "1.0.0",
            "license": "Apache-2.0"
        }"#;
        let entries = parse_package_json_licenses(content, &PathBuf::from("package.json"));
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].package, "my-pkg");
        assert_eq!(entries[0].license, "Apache-2.0");
    }

    #[test]
    fn pyproject_license_text() {
        let content = r#"
[project]
name = "my-py-pkg"
license = {text = "GPL-3.0-only"}
"#;
        let entries = parse_pyproject_licenses(content, &PathBuf::from("pyproject.toml"));
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].license, "GPL-3.0-only");
    }

    #[test]
    fn pyproject_license_classifier() {
        let content = r#"
[project]
name = "my-py-pkg"
classifiers = ["License :: OSI Approved :: MIT License"]
"#;
        let entries = parse_pyproject_licenses(content, &PathBuf::from("pyproject.toml"));
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].license, "MIT");
    }

    // -- Integration via Detector trait -------------------------------------

    #[tokio::test]
    async fn unknown_license_produces_medium_finding() {
        let ctx = test_context_with_cache(vec![(
            "Cargo.toml",
            r#"
[package]
name = "weird-crate"
version = "0.1.0"
license = "WTFPL"
"#,
        )]);
        let det = LicenseScanDetector::permissive();
        let findings = det.analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, Severity::Medium);
        assert_eq!(findings[0].category, FindingCategory::LicenseViolation);
    }

    #[tokio::test]
    async fn gpl_in_cargo_toml_flagged_by_enterprise() {
        let ctx = test_context_with_cache(vec![(
            "Cargo.toml",
            r#"
[package]
name = "copyleft-crate"
version = "0.1.0"
license = "GPL-3.0-only"
"#,
        )]);
        let det = LicenseScanDetector::enterprise();
        let findings = det.analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, Severity::Critical);
    }

    #[tokio::test]
    async fn permissive_license_produces_no_findings() {
        let ctx = test_context_with_cache(vec![(
            "Cargo.toml",
            r#"
[package]
name = "good-crate"
version = "0.1.0"
license = "MIT"
"#,
        )]);
        let det = LicenseScanDetector::permissive();
        let findings = det.analyze(&ctx).await.unwrap();
        assert!(findings.is_empty());
    }

    #[tokio::test]
    async fn spdx_or_expression_passes_permissive() {
        let ctx = test_context_with_cache(vec![(
            "package.json",
            r#"{"name": "dual-pkg", "license": "MIT OR Apache-2.0"}"#,
        )]);
        let det = LicenseScanDetector::permissive();
        let findings = det.analyze(&ctx).await.unwrap();
        assert!(findings.is_empty());
    }

    #[tokio::test]
    async fn spdx_and_with_gpl_flagged_enterprise() {
        let ctx = test_context_with_cache(vec![(
            "package.json",
            r#"{"name": "mixed-pkg", "license": "GPL-3.0-only AND MIT"}"#,
        )]);
        let det = LicenseScanDetector::enterprise();
        let findings = det.analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, Severity::Critical);
    }

    #[test]
    fn detector_name() {
        let det = LicenseScanDetector::permissive();
        assert_eq!(det.name(), "license-scan");
    }

    #[test]
    fn custom_policy_deny() {
        let policy = LicensePolicy::Custom {
            deny: vec!["WTFPL".into()],
            allow: vec!["MIT".into()],
        };
        let result = check_policy(&policy, "WTFPL");
        assert!(matches!(result, PolicyVerdict::Denied { .. }));
    }

    #[test]
    fn custom_policy_allow() {
        let policy = LicensePolicy::Custom {
            deny: vec![],
            allow: vec!["MIT".into()],
        };
        let result = check_policy(&policy, "MIT");
        assert!(matches!(result, PolicyVerdict::Allowed));
    }

    // -- SPDX parsing bug-fix tests -----------------------------------------

    #[test]
    fn bug_spdx_with_exception_allowed() {
        // WITH clause adds exceptions (more permissive) — should match base license
        let result = check_policy(
            &LicensePolicy::Permissive,
            "Apache-2.0 WITH LLVM-exception",
        );
        assert!(matches!(result, PolicyVerdict::Allowed));
    }

    #[test]
    fn bug_spdx_parentheses_stripped() {
        // Parenthesized SPDX expressions should be parsed correctly
        let result = check_policy(&LicensePolicy::Permissive, "(MIT OR Apache-2.0)");
        assert!(matches!(result, PolicyVerdict::Allowed));
    }

    #[test]
    fn bug_spdx_plus_denied() {
        // "GPL-2.0+" is deprecated SPDX for "GPL-2.0-or-later", which is denied
        let result = check_policy(&LicensePolicy::Enterprise, "GPL-2.0+");
        assert!(matches!(result, PolicyVerdict::Denied { .. }));
    }

    #[test]
    fn bug_spdx_lowercase_or() {
        // Lowercase "or" should be treated as SPDX OR operator
        let result = check_policy(&LicensePolicy::Permissive, "MIT or Apache-2.0");
        assert!(matches!(result, PolicyVerdict::Allowed));
    }

    #[test]
    fn bug_spdx_tab_whitespace() {
        // Tab characters in SPDX expressions should not cause false positives
        let result = check_policy(&LicensePolicy::Permissive, "MIT\tOR\tApache-2.0");
        assert!(matches!(result, PolicyVerdict::Allowed));
    }
}
