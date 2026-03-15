//! Auto-discovery of applicable analyzers based on target artifacts.
//!
//! Scans a target directory for artifact types (Dockerfiles, IaC, env files, etc.)
//! and dispatches to the appropriate analyzer modules.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Instant;

use apex_core::types::Language;
use serde::Serialize;
use tracing::{debug, warn};

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Artifacts discovered in the target directory.
#[derive(Debug, Default, Clone, Serialize)]
pub struct Artifacts {
    pub dockerfiles: Vec<PathBuf>,
    pub iac_files: Vec<PathBuf>,
    pub env_files: Vec<PathBuf>,
    pub openapi_specs: Vec<PathBuf>,
    pub sql_migrations: Vec<PathBuf>,
    pub frontend_files: Vec<PathBuf>,
    pub i18n_files: Vec<PathBuf>,
    pub runbook_files: Vec<PathBuf>,
    pub slo_files: Vec<PathBuf>,
    pub cargo_toml: Option<PathBuf>,
    pub package_json: Option<PathBuf>,
    pub has_apex_index: bool,
}

impl Artifacts {
    /// Total number of discovered artifact files.
    pub fn total_count(&self) -> usize {
        self.dockerfiles.len()
            + self.iac_files.len()
            + self.env_files.len()
            + self.openapi_specs.len()
            + self.sql_migrations.len()
            + self.frontend_files.len()
            + self.i18n_files.len()
            + self.runbook_files.len()
            + self.slo_files.len()
            + self.cargo_toml.iter().count()
            + self.package_json.iter().count()
            + if self.has_apex_index { 1 } else { 0 }
    }
}

/// An analyzer that should run based on discovered artifacts.
#[derive(Debug, Clone)]
pub struct ApplicableAnalyzer {
    pub name: &'static str,
    pub description: &'static str,
    pub artifacts_used: Vec<PathBuf>,
}

/// Result from running a single analyzer.
#[derive(Debug, Clone, Serialize)]
pub struct AnalyzerResult {
    pub name: String,
    pub description: String,
    pub status: AnalyzerStatus,
    pub report: serde_json::Value,
    pub duration_ms: u64,
}

/// Status of a completed analyzer run.
#[derive(Debug, Clone, Serialize)]
pub enum AnalyzerStatus {
    Ok,
    Failed(String),
    Skipped(String),
}

// ---------------------------------------------------------------------------
// Skip dirs
// ---------------------------------------------------------------------------

const SKIP_DIRS: &[&str] = &[
    "node_modules",
    ".git",
    "target",
    "__pycache__",
    ".venv",
    "venv",
    "dist",
    "build",
    ".tox",
    ".mypy_cache",
];

// ---------------------------------------------------------------------------
// Discovery
// ---------------------------------------------------------------------------

/// Walk the target directory (max depth 4) and classify discovered artifacts.
pub fn discover_artifacts(target: &Path) -> Artifacts {
    let mut artifacts = Artifacts::default();

    let walker = walkdir::WalkDir::new(target)
        .max_depth(4)
        .into_iter()
        .filter_entry(|e| {
            if e.file_type().is_dir() {
                let name = e.file_name().to_string_lossy();
                !SKIP_DIRS.contains(&name.as_ref())
            } else {
                true
            }
        });

    for entry in walker.flatten() {
        if !entry.file_type().is_file() {
            continue;
        }

        let path = entry.path().to_path_buf();
        let file_name = entry.file_name().to_string_lossy();

        // Dockerfiles
        if file_name.starts_with("Dockerfile") {
            artifacts.dockerfiles.push(path.clone());
            continue;
        }

        // IaC files (.tf, .hcl)
        if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
            match ext {
                "tf" | "hcl" => {
                    artifacts.iac_files.push(path.clone());
                    continue;
                }
                "jsx" | "tsx" | "vue" | "svelte" => {
                    artifacts.frontend_files.push(path.clone());
                    continue;
                }
                "po" => {
                    artifacts.i18n_files.push(path.clone());
                    continue;
                }
                "sql" => {
                    // Only count files under a migrations/ directory
                    if path
                        .components()
                        .any(|c| c.as_os_str() == "migrations")
                    {
                        artifacts.sql_migrations.push(path.clone());
                    }
                    continue;
                }
                _ => {}
            }
        }

        // Env files (.env, .env.local, etc.) — skip .env.example
        if file_name.starts_with(".env") && !file_name.ends_with(".example") {
            artifacts.env_files.push(path.clone());
            continue;
        }

        // OpenAPI / Swagger specs
        if file_name == "openapi.json"
            || file_name == "swagger.json"
            || file_name == "openapi.yaml"
        {
            artifacts.openapi_specs.push(path.clone());
            continue;
        }

        // Cargo.toml / package.json
        if file_name == "Cargo.toml" && artifacts.cargo_toml.is_none() {
            artifacts.cargo_toml = Some(path.clone());
            continue;
        }
        if file_name == "package.json" && artifacts.package_json.is_none() {
            artifacts.package_json = Some(path.clone());
            continue;
        }

        // Runbook files (runbooks/*.md)
        if file_name.ends_with(".md") {
            if path
                .components()
                .any(|c| c.as_os_str() == "runbooks")
            {
                artifacts.runbook_files.push(path.clone());
                continue;
            }
        }

        // SLO files
        if file_name == "slo.json" || file_name == "slo.yaml" {
            artifacts.slo_files.push(path.clone());
            continue;
        }

        // locales/ directory check for i18n
        if path
            .components()
            .any(|c| c.as_os_str() == "locales")
            && path
                .extension()
                .map_or(false, |e| e == "json" || e == "yaml" || e == "yml")
        {
            artifacts.i18n_files.push(path.clone());
            continue;
        }

        // .apex/index.json
        if file_name == "index.json" {
            if path
                .components()
                .any(|c| c.as_os_str() == ".apex")
            {
                artifacts.has_apex_index = true;
            }
        }
    }

    debug!(
        total = artifacts.total_count(),
        dockerfiles = artifacts.dockerfiles.len(),
        iac = artifacts.iac_files.len(),
        env = artifacts.env_files.len(),
        openapi = artifacts.openapi_specs.len(),
        sql = artifacts.sql_migrations.len(),
        frontend = artifacts.frontend_files.len(),
        i18n = artifacts.i18n_files.len(),
        runbooks = artifacts.runbook_files.len(),
        slo = artifacts.slo_files.len(),
        "discovered artifacts"
    );

    artifacts
}

// ---------------------------------------------------------------------------
// Applicable analyzers
// ---------------------------------------------------------------------------

/// Determine which analyzers are applicable given the discovered artifacts and language.
pub fn applicable_analyzers(artifacts: &Artifacts, lang: Language) -> Vec<ApplicableAnalyzer> {
    let mut analyzers = Vec::new();

    // Always applicable (any language)
    analyzers.push(ApplicableAnalyzer {
        name: "service-map",
        description: "Discover inter-service dependencies",
        artifacts_used: vec![],
    });
    analyzers.push(ApplicableAnalyzer {
        name: "secret-scan",
        description: "Scan for hardcoded secrets and credentials",
        artifacts_used: vec![],
    });
    analyzers.push(ApplicableAnalyzer {
        name: "mem-check",
        description: "Check for memory safety issues",
        artifacts_used: vec![],
    });
    analyzers.push(ApplicableAnalyzer {
        name: "cost-estimate",
        description: "Estimate cloud cost drivers",
        artifacts_used: vec![],
    });

    // Dockerfile → container-scan
    if !artifacts.dockerfiles.is_empty() {
        analyzers.push(ApplicableAnalyzer {
            name: "container-scan",
            description: "Scan container images for misconfigurations",
            artifacts_used: artifacts.dockerfiles.clone(),
        });
    }

    // IaC → iac-scan
    if !artifacts.iac_files.is_empty() {
        analyzers.push(ApplicableAnalyzer {
            name: "iac-scan",
            description: "Scan IaC for misconfigurations",
            artifacts_used: artifacts.iac_files.clone(),
        });
    }

    // 2+ env files → config-drift
    if artifacts.env_files.len() >= 2 {
        analyzers.push(ApplicableAnalyzer {
            name: "config-drift",
            description: "Detect configuration drift between environments",
            artifacts_used: artifacts.env_files.clone(),
        });
    }

    // OpenAPI specs → api-coverage + doc-coverage
    if !artifacts.openapi_specs.is_empty() {
        analyzers.push(ApplicableAnalyzer {
            name: "api-coverage",
            description: "Check API endpoint test coverage",
            artifacts_used: artifacts.openapi_specs.clone(),
        });
        analyzers.push(ApplicableAnalyzer {
            name: "doc-coverage",
            description: "Check API documentation completeness",
            artifacts_used: artifacts.openapi_specs.clone(),
        });
    }

    // SQL migrations → schema-check
    if !artifacts.sql_migrations.is_empty() {
        analyzers.push(ApplicableAnalyzer {
            name: "schema-check",
            description: "Check database migrations for issues",
            artifacts_used: artifacts.sql_migrations.clone(),
        });
    }

    // Frontend files → a11y-scan
    if !artifacts.frontend_files.is_empty() {
        analyzers.push(ApplicableAnalyzer {
            name: "a11y-scan",
            description: "Scan frontend for accessibility issues",
            artifacts_used: artifacts.frontend_files.clone(),
        });
    }

    // i18n files → i18n-check
    if !artifacts.i18n_files.is_empty() {
        analyzers.push(ApplicableAnalyzer {
            name: "i18n-check",
            description: "Check internationalization completeness",
            artifacts_used: artifacts.i18n_files.clone(),
        });
    }

    // Cargo.toml or package.json → dep-graph, license-scan
    if artifacts.cargo_toml.is_some() || artifacts.package_json.is_some() {
        let mut used = Vec::new();
        if let Some(ref p) = artifacts.cargo_toml {
            used.push(p.clone());
        }
        if let Some(ref p) = artifacts.package_json {
            used.push(p.clone());
        }
        analyzers.push(ApplicableAnalyzer {
            name: "dep-graph",
            description: "Analyze dependency graph",
            artifacts_used: used.clone(),
        });
        analyzers.push(ApplicableAnalyzer {
            name: "license-scan",
            description: "Scan dependency licenses",
            artifacts_used: used,
        });
    }

    // Runbooks → runbook-check
    if !artifacts.runbook_files.is_empty() {
        analyzers.push(ApplicableAnalyzer {
            name: "runbook-check",
            description: "Validate runbook completeness",
            artifacts_used: artifacts.runbook_files.clone(),
        });
    }

    // SLO files → slo-check
    if !artifacts.slo_files.is_empty() {
        analyzers.push(ApplicableAnalyzer {
            name: "slo-check",
            description: "Validate SLO definitions",
            artifacts_used: artifacts.slo_files.clone(),
        });
    }

    // .apex/index.json → blast-radius
    if artifacts.has_apex_index {
        analyzers.push(ApplicableAnalyzer {
            name: "blast-radius",
            description: "Compute blast radius from change index",
            artifacts_used: vec![],
        });
    }

    // Python only: data-flow
    if lang == Language::Python {
        analyzers.push(ApplicableAnalyzer {
            name: "data-flow",
            description: "Track data flow through Python code",
            artifacts_used: vec![],
        });
    }

    analyzers
}

// ---------------------------------------------------------------------------
// Runner
// ---------------------------------------------------------------------------

/// Run all applicable analyzers, collecting results. Failures are logged but not fatal.
pub async fn run_applicable_analyzers(
    target: &Path,
    lang: Language,
    source_cache: &HashMap<PathBuf, String>,
    artifacts: &Artifacts,
    analyzers: &[ApplicableAnalyzer],
) -> Vec<AnalyzerResult> {
    let mut results = Vec::with_capacity(analyzers.len());

    for analyzer in analyzers {
        let start = Instant::now();
        let outcome = run_single_analyzer(analyzer.name, target, lang, source_cache, artifacts);
        let duration_ms = start.elapsed().as_millis() as u64;

        let (status, report) = match outcome {
            Ok(value) => (AnalyzerStatus::Ok, value),
            Err(e) => {
                warn!(analyzer = analyzer.name, error = %e, "analyzer failed");
                (AnalyzerStatus::Failed(e.to_string()), serde_json::Value::Null)
            }
        };

        results.push(AnalyzerResult {
            name: analyzer.name.to_string(),
            description: analyzer.description.to_string(),
            status,
            report,
            duration_ms,
        });
    }

    results
}

/// Dispatch to the correct analyzer module.
fn run_single_analyzer(
    name: &str,
    target: &Path,
    lang: Language,
    source_cache: &HashMap<PathBuf, String>,
    artifacts: &Artifacts,
) -> Result<serde_json::Value, Box<dyn std::error::Error + Send + Sync>> {
    match name {
        "service-map" => {
            let report = crate::service_map::analyze_service_map(source_cache);
            Ok(serde_json::to_value(report)?)
        }
        "mem-check" => {
            let report = crate::mem_check::check_memory(source_cache, lang);
            Ok(serde_json::to_value(report)?)
        }
        "cost-estimate" => {
            let report = crate::cost_estimate::estimate_costs(source_cache);
            Ok(serde_json::to_value(report)?)
        }
        "container-scan" => {
            if let Some(path) = artifacts.dockerfiles.first() {
                let content = std::fs::read_to_string(path)?;
                let report = crate::container_scan::scan_dockerfile(&content, path);
                Ok(serde_json::to_value(report)?)
            } else {
                Ok(serde_json::Value::Null)
            }
        }
        "iac-scan" => {
            let report = crate::iac_scan::scan_iac(source_cache);
            Ok(serde_json::to_value(report)?)
        }
        "config-drift" => {
            if artifacts.env_files.len() >= 2 {
                let content_a = std::fs::read_to_string(&artifacts.env_files[0])?;
                let content_b = std::fs::read_to_string(&artifacts.env_files[1])?;
                let env_a = crate::config_drift::parse_env_file(&content_a);
                let env_b = crate::config_drift::parse_env_file(&content_b);
                let report = crate::config_drift::detect_drift(
                    &env_a,
                    &env_b,
                    &artifacts.env_files[0].display().to_string(),
                    &artifacts.env_files[1].display().to_string(),
                );
                Ok(serde_json::to_value(report)?)
            } else {
                Ok(serde_json::Value::Null)
            }
        }
        "api-coverage" => {
            if let Some(spec_path) = artifacts.openapi_specs.first() {
                let spec = std::fs::read_to_string(spec_path)?;
                let report = crate::api_coverage::analyze_coverage(&spec, source_cache, lang)
                    .map_err(|e| format!("api-coverage: {e}"))?;
                Ok(serde_json::to_value(report)?)
            } else {
                Ok(serde_json::Value::Null)
            }
        }
        "doc-coverage" => {
            if let Some(spec_path) = artifacts.openapi_specs.first() {
                let spec = std::fs::read_to_string(spec_path)?;
                let report = crate::doc_coverage::analyze_doc_coverage(&spec)
                    .map_err(|e| format!("doc-coverage: {e}"))?;
                Ok(serde_json::to_value(report)?)
            } else {
                Ok(serde_json::Value::Null)
            }
        }
        "schema-check" => {
            let mut reports = Vec::new();
            for path in &artifacts.sql_migrations {
                if let Ok(sql) = std::fs::read_to_string(path) {
                    reports.push(crate::schema_check::analyze_migration(&sql));
                }
            }
            Ok(serde_json::to_value(reports)?)
        }
        "a11y-scan" => {
            let report = crate::a11y_scan::scan_accessibility(source_cache);
            Ok(serde_json::to_value(report)?)
        }
        "i18n-check" => {
            let report = crate::i18n_check::check_i18n(source_cache);
            Ok(serde_json::to_value(report)?)
        }
        "dep-graph" => {
            let report = crate::dep_graph::analyze_cargo(target);
            Ok(serde_json::to_value(report)?)
        }
        "runbook-check" => {
            let mut reports = Vec::new();
            for path in &artifacts.runbook_files {
                if let Ok(content) = std::fs::read_to_string(path) {
                    reports.push(crate::runbook_check::validate_runbook(&content, path, target));
                }
            }
            Ok(serde_json::to_value(reports)?)
        }
        "slo-check" => {
            if let Some(slo_path) = artifacts.slo_files.first() {
                if let Ok(content) = std::fs::read_to_string(slo_path) {
                    let slos = crate::slo_check::parse_slo_file(&content);
                    let report = crate::slo_check::check_slos(&slos, source_cache);
                    Ok(serde_json::to_value(report)?)
                } else {
                    Ok(serde_json::Value::Null)
                }
            } else {
                Ok(serde_json::Value::Null)
            }
        }
        // Analyzers without a backing module yet return null
        _ => Ok(serde_json::Value::Null),
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn discovers_no_artifacts_in_empty_dir() {
        let dir = std::env::temp_dir().join("apex_test_discover_empty");
        let _ = fs::create_dir_all(&dir);
        let artifacts = discover_artifacts(&dir);
        assert_eq!(artifacts.total_count(), 0);
        assert!(artifacts.dockerfiles.is_empty());
        assert!(artifacts.iac_files.is_empty());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn applicable_always_includes_service_map() {
        let artifacts = Artifacts::default();
        let analyzers = applicable_analyzers(&artifacts, Language::Rust);
        let names: Vec<&str> = analyzers.iter().map(|a| a.name).collect();
        assert!(names.contains(&"service-map"));
        assert!(names.contains(&"mem-check"));
        assert!(names.contains(&"cost-estimate"));
        assert!(names.contains(&"secret-scan"));
    }

    #[test]
    fn dockerfile_triggers_container_scan() {
        let mut artifacts = Artifacts::default();
        artifacts
            .dockerfiles
            .push(PathBuf::from("Dockerfile"));
        let analyzers = applicable_analyzers(&artifacts, Language::Rust);
        let names: Vec<&str> = analyzers.iter().map(|a| a.name).collect();
        assert!(names.contains(&"container-scan"));
    }

    #[test]
    fn no_frontend_files_skips_a11y() {
        let artifacts = Artifacts::default();
        let analyzers = applicable_analyzers(&artifacts, Language::Rust);
        let names: Vec<&str> = analyzers.iter().map(|a| a.name).collect();
        assert!(!names.contains(&"a11y-scan"));
    }

    #[test]
    fn env_files_need_two_for_config_drift() {
        // One env file -> no config-drift
        let mut artifacts = Artifacts::default();
        artifacts.env_files.push(PathBuf::from(".env"));
        let analyzers = applicable_analyzers(&artifacts, Language::Python);
        let names: Vec<&str> = analyzers.iter().map(|a| a.name).collect();
        assert!(!names.contains(&"config-drift"));

        // Two env files -> config-drift
        artifacts.env_files.push(PathBuf::from(".env.production"));
        let analyzers = applicable_analyzers(&artifacts, Language::Python);
        let names: Vec<&str> = analyzers.iter().map(|a| a.name).collect();
        assert!(names.contains(&"config-drift"));
    }

    #[test]
    fn python_gets_data_flow() {
        let artifacts = Artifacts::default();
        let py = applicable_analyzers(&artifacts, Language::Python);
        let rs = applicable_analyzers(&artifacts, Language::Rust);
        let py_names: Vec<&str> = py.iter().map(|a| a.name).collect();
        let rs_names: Vec<&str> = rs.iter().map(|a| a.name).collect();
        assert!(py_names.contains(&"data-flow"));
        assert!(!rs_names.contains(&"data-flow"));
    }

    #[test]
    fn discovers_dockerfile_and_iac() {
        let dir = std::env::temp_dir().join("apex_test_discover_docker_iac");
        let _ = fs::remove_dir_all(&dir);
        let _ = fs::create_dir_all(&dir);
        fs::write(dir.join("Dockerfile"), "FROM alpine").unwrap();
        fs::write(dir.join("main.tf"), "resource {}").unwrap();
        let artifacts = discover_artifacts(&dir);
        assert_eq!(artifacts.dockerfiles.len(), 1);
        assert_eq!(artifacts.iac_files.len(), 1);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn cargo_toml_triggers_dep_graph() {
        let mut artifacts = Artifacts::default();
        artifacts.cargo_toml = Some(PathBuf::from("Cargo.toml"));
        let analyzers = applicable_analyzers(&artifacts, Language::Rust);
        let names: Vec<&str> = analyzers.iter().map(|a| a.name).collect();
        assert!(names.contains(&"dep-graph"));
        assert!(names.contains(&"license-scan"));
    }

    #[test]
    fn openapi_triggers_api_and_doc_coverage() {
        let mut artifacts = Artifacts::default();
        artifacts
            .openapi_specs
            .push(PathBuf::from("openapi.json"));
        let analyzers = applicable_analyzers(&artifacts, Language::Python);
        let names: Vec<&str> = analyzers.iter().map(|a| a.name).collect();
        assert!(names.contains(&"api-coverage"));
        assert!(names.contains(&"doc-coverage"));
    }
}
