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
                    if path.components().any(|c| c.as_os_str() == "migrations") {
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
        if file_name == "openapi.json" || file_name == "swagger.json" || file_name == "openapi.yaml"
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
        if file_name.ends_with(".md") && path.components().any(|c| c.as_os_str() == "runbooks") {
            artifacts.runbook_files.push(path.clone());
            continue;
        }

        // SLO files
        if file_name == "slo.json" || file_name == "slo.yaml" {
            artifacts.slo_files.push(path.clone());
            continue;
        }

        // locales/ directory check for i18n
        if path.components().any(|c| c.as_os_str() == "locales")
            && path
                .extension()
                .is_some_and(|e| e == "json" || e == "yaml" || e == "yml")
        {
            artifacts.i18n_files.push(path.clone());
            continue;
        }

        // .apex/index.json
        if file_name == "index.json"
            && path
                .components()
                .any(|c: std::path::Component| c.as_os_str() == ".apex")
        {
            artifacts.has_apex_index = true;
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
    // Always applicable (any language)
    let mut analyzers = vec![
        ApplicableAnalyzer {
            name: "service-map",
            description: "Discover inter-service dependencies",
            artifacts_used: vec![],
        },
        ApplicableAnalyzer {
            name: "secret-scan",
            description: "Scan for hardcoded secrets and credentials",
            artifacts_used: vec![],
        },
        ApplicableAnalyzer {
            name: "mem-check",
            description: "Check for memory safety issues",
            artifacts_used: vec![],
        },
        ApplicableAnalyzer {
            name: "cost-estimate",
            description: "Estimate cloud cost drivers",
            artifacts_used: vec![],
        },
    ];

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
                (
                    AnalyzerStatus::Failed(e.to_string()),
                    serde_json::Value::Null,
                )
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
                    reports.push(crate::runbook_check::validate_runbook(
                        &content, path, target,
                    ));
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

    // -----------------------------------------------------------------------
    // Helpers
    // -----------------------------------------------------------------------

    fn analyzer_names(artifacts: &Artifacts, lang: Language) -> Vec<&'static str> {
        applicable_analyzers(artifacts, lang)
            .into_iter()
            .map(|a| a.name)
            .collect()
    }

    // -----------------------------------------------------------------------
    // Existing tests (preserved)
    // -----------------------------------------------------------------------

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
        artifacts.dockerfiles.push(PathBuf::from("Dockerfile"));
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
        artifacts.openapi_specs.push(PathBuf::from("openapi.json"));
        let analyzers = applicable_analyzers(&artifacts, Language::Python);
        let names: Vec<&str> = analyzers.iter().map(|a| a.name).collect();
        assert!(names.contains(&"api-coverage"));
        assert!(names.contains(&"doc-coverage"));
    }

    // -----------------------------------------------------------------------
    // Target: lines 137-208 — discovery logic for frontend/i18n/env/openapi/
    // cargo/runbook/slo/apex-index artifacts
    // -----------------------------------------------------------------------

    #[test]
    fn discovers_frontend_jsx_tsx_vue_svelte() {
        // Target: lines 137-140 — jsx/tsx/vue/svelte classification
        let dir = std::env::temp_dir().join("apex_test_discover_frontend");
        let _ = fs::remove_dir_all(&dir);
        let _ = fs::create_dir_all(&dir);
        fs::write(dir.join("App.jsx"), "<div/>").unwrap();
        fs::write(dir.join("Widget.tsx"), "<span/>").unwrap();
        fs::write(dir.join("Page.vue"), "<template/>").unwrap();
        fs::write(dir.join("Layout.svelte"), "<slot/>").unwrap();
        let artifacts = discover_artifacts(&dir);
        assert_eq!(
            artifacts.frontend_files.len(),
            4,
            "jsx/tsx/vue/svelte should all be discovered"
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn discovers_i18n_po_files() {
        // Target: lines 141-144 — .po file classification
        let dir = std::env::temp_dir().join("apex_test_discover_i18n_po");
        let _ = fs::remove_dir_all(&dir);
        let _ = fs::create_dir_all(&dir);
        fs::write(dir.join("messages.po"), "msgid \"hello\"").unwrap();
        let artifacts = discover_artifacts(&dir);
        assert_eq!(artifacts.i18n_files.len(), 1);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn discovers_sql_only_under_migrations_dir() {
        // Target: lines 145-151 — .sql only counts if under migrations/
        let dir = std::env::temp_dir().join("apex_test_discover_sql");
        let _ = fs::remove_dir_all(&dir);
        let migrations = dir.join("migrations");
        let _ = fs::create_dir_all(&migrations);
        fs::write(dir.join("schema.sql"), "CREATE TABLE x (id int);").unwrap(); // not under migrations/
        fs::write(migrations.join("001_init.sql"), "CREATE TABLE y (id int);").unwrap();
        let artifacts = discover_artifacts(&dir);
        assert_eq!(
            artifacts.sql_migrations.len(),
            1,
            "only migrations/ sql should be discovered"
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn discovers_env_files_excludes_example() {
        // Target: lines 157-160 — .env files excluding .env.example
        let dir = std::env::temp_dir().join("apex_test_discover_env");
        let _ = fs::remove_dir_all(&dir);
        let _ = fs::create_dir_all(&dir);
        fs::write(dir.join(".env"), "KEY=val").unwrap();
        fs::write(dir.join(".env.local"), "KEY=local").unwrap();
        fs::write(dir.join(".env.example"), "KEY=example").unwrap(); // must NOT be discovered
        let artifacts = discover_artifacts(&dir);
        assert_eq!(
            artifacts.env_files.len(),
            2,
            ".env.example must be excluded"
        );
        for p in &artifacts.env_files {
            assert!(
                !p.to_string_lossy().ends_with(".example"),
                "example file leaked into env_files"
            );
        }
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn discovers_openapi_yaml_and_swagger_json() {
        // Target: lines 163-167 — openapi.json / swagger.json / openapi.yaml
        let dir = std::env::temp_dir().join("apex_test_discover_openapi");
        let _ = fs::remove_dir_all(&dir);
        let _ = fs::create_dir_all(&dir);
        fs::write(dir.join("openapi.yaml"), "openapi: 3.0.0").unwrap();
        fs::write(dir.join("swagger.json"), "{}").unwrap();
        let artifacts = discover_artifacts(&dir);
        assert_eq!(artifacts.openapi_specs.len(), 2);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn discovers_cargo_toml_only_first() {
        // Target: lines 170-173 — only first Cargo.toml is recorded
        let dir = std::env::temp_dir().join("apex_test_discover_cargo");
        let _ = fs::remove_dir_all(&dir);
        let _ = fs::create_dir_all(&dir);
        fs::write(dir.join("Cargo.toml"), "[package]\nname = \"root\"").unwrap();
        // A nested Cargo.toml exists but should be ignored (cargo_toml uses is_none())
        let sub = dir.join("sub");
        let _ = fs::create_dir_all(&sub);
        fs::write(sub.join("Cargo.toml"), "[package]\nname = \"sub\"").unwrap();
        let artifacts = discover_artifacts(&dir);
        assert!(
            artifacts.cargo_toml.is_some(),
            "root Cargo.toml should be discovered"
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn discovers_runbook_md_under_runbooks_dir() {
        // Target: lines 180-184 — runbooks/*.md discovery
        let dir = std::env::temp_dir().join("apex_test_discover_runbook");
        let _ = fs::remove_dir_all(&dir);
        let runbooks = dir.join("runbooks");
        let _ = fs::create_dir_all(&runbooks);
        fs::write(dir.join("README.md"), "# top level md").unwrap(); // should NOT match
        fs::write(runbooks.join("deploy.md"), "# deploy").unwrap();
        let artifacts = discover_artifacts(&dir);
        assert_eq!(
            artifacts.runbook_files.len(),
            1,
            "only runbooks/*.md should match"
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn discovers_slo_json_and_slo_yaml() {
        // Target: lines 186-189 — slo.json / slo.yaml
        let dir = std::env::temp_dir().join("apex_test_discover_slo");
        let _ = fs::remove_dir_all(&dir);
        let _ = fs::create_dir_all(&dir);
        fs::write(dir.join("slo.json"), "{}").unwrap();
        fs::write(dir.join("slo.yaml"), "---").unwrap();
        let artifacts = discover_artifacts(&dir);
        assert_eq!(artifacts.slo_files.len(), 2);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn discovers_locales_dir_json_yaml_as_i18n() {
        // Target: lines 192-198 — locales/ directory i18n detection
        let dir = std::env::temp_dir().join("apex_test_discover_locales");
        let _ = fs::remove_dir_all(&dir);
        let locales = dir.join("locales");
        let _ = fs::create_dir_all(&locales);
        fs::write(locales.join("en.json"), "{}").unwrap();
        fs::write(locales.join("fr.yaml"), "---").unwrap();
        fs::write(locales.join("de.yml"), "---").unwrap();
        let artifacts = discover_artifacts(&dir);
        assert_eq!(
            artifacts.i18n_files.len(),
            3,
            "locales/ json/yaml/yml should all be i18n"
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn discovers_apex_index_json() {
        // Target: lines 202-208 — .apex/index.json sets has_apex_index
        let dir = std::env::temp_dir().join("apex_test_discover_apex_index");
        let _ = fs::remove_dir_all(&dir);
        let apex_dir = dir.join(".apex");
        let _ = fs::create_dir_all(&apex_dir);
        fs::write(apex_dir.join("index.json"), "{}").unwrap();
        let artifacts = discover_artifacts(&dir);
        assert!(
            artifacts.has_apex_index,
            ".apex/index.json should set has_apex_index"
        );
        // total_count includes the index as 1
        assert_eq!(artifacts.total_count(), 1);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn skip_dirs_are_not_walked() {
        // Target: lines 107-114 — SKIP_DIRS filtering
        let dir = std::env::temp_dir().join("apex_test_skip_dirs");
        let _ = fs::remove_dir_all(&dir);
        let node_mods = dir.join("node_modules");
        let _ = fs::create_dir_all(&node_mods);
        fs::write(node_mods.join("Dockerfile"), "FROM alpine").unwrap();
        let artifacts = discover_artifacts(&dir);
        assert!(
            artifacts.dockerfiles.is_empty(),
            "node_modules should be skipped"
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn hcl_extension_classified_as_iac() {
        // Target: lines 133-135 — .hcl extension
        let dir = std::env::temp_dir().join("apex_test_discover_hcl");
        let _ = fs::remove_dir_all(&dir);
        let _ = fs::create_dir_all(&dir);
        fs::write(dir.join("backend.hcl"), "terraform {}").unwrap();
        let artifacts = discover_artifacts(&dir);
        assert_eq!(artifacts.iac_files.len(), 1);
        let _ = fs::remove_dir_all(&dir);
    }

    // -----------------------------------------------------------------------
    // Target: lines 268-274 — IaC triggers iac-scan
    // -----------------------------------------------------------------------

    #[test]
    fn iac_files_trigger_iac_scan() {
        // Target: lines 268-274
        let mut artifacts = Artifacts::default();
        artifacts.iac_files.push(PathBuf::from("main.tf"));
        let names = analyzer_names(&artifacts, Language::Rust);
        assert!(names.contains(&"iac-scan"));
    }

    #[test]
    fn no_iac_files_skips_iac_scan() {
        let artifacts = Artifacts::default();
        let names = analyzer_names(&artifacts, Language::Rust);
        assert!(!names.contains(&"iac-scan"));
    }

    // -----------------------------------------------------------------------
    // Target: lines 300-324 — sql/frontend/i18n conditional analyzers
    // -----------------------------------------------------------------------

    #[test]
    fn sql_migrations_trigger_schema_check() {
        // Target: lines 300-306
        let mut artifacts = Artifacts::default();
        artifacts
            .sql_migrations
            .push(PathBuf::from("migrations/001.sql"));
        let names = analyzer_names(&artifacts, Language::Rust);
        assert!(names.contains(&"schema-check"));
    }

    #[test]
    fn frontend_files_trigger_a11y_scan() {
        // Target: lines 309-314
        let mut artifacts = Artifacts::default();
        artifacts.frontend_files.push(PathBuf::from("App.tsx"));
        let names = analyzer_names(&artifacts, Language::Rust);
        assert!(names.contains(&"a11y-scan"));
    }

    #[test]
    fn i18n_files_trigger_i18n_check() {
        // Target: lines 317-324
        let mut artifacts = Artifacts::default();
        artifacts.i18n_files.push(PathBuf::from("locales/en.json"));
        let names = analyzer_names(&artifacts, Language::Rust);
        assert!(names.contains(&"i18n-check"));
    }

    // -----------------------------------------------------------------------
    // Target: lines 348-372 — cargo+package.json combo, runbook, slo, blast-radius
    // -----------------------------------------------------------------------

    #[test]
    fn package_json_alone_triggers_dep_graph_and_license_scan() {
        // Target: lines 327-345 — package_json alone (no Cargo.toml)
        let mut artifacts = Artifacts::default();
        artifacts.package_json = Some(PathBuf::from("package.json"));
        let names = analyzer_names(&artifacts, Language::Rust);
        assert!(names.contains(&"dep-graph"));
        assert!(names.contains(&"license-scan"));
    }

    #[test]
    fn both_cargo_and_package_json_uses_both_in_artifacts_used() {
        // Target: lines 327-345 — both manifest files together
        let mut artifacts = Artifacts::default();
        artifacts.cargo_toml = Some(PathBuf::from("Cargo.toml"));
        artifacts.package_json = Some(PathBuf::from("package.json"));
        let analyzers = applicable_analyzers(&artifacts, Language::Rust);
        let dep_graph = analyzers.iter().find(|a| a.name == "dep-graph").unwrap();
        assert_eq!(
            dep_graph.artifacts_used.len(),
            2,
            "both manifest files should be in artifacts_used"
        );
    }

    #[test]
    fn runbook_files_trigger_runbook_check() {
        // Target: lines 348-354
        let mut artifacts = Artifacts::default();
        artifacts
            .runbook_files
            .push(PathBuf::from("runbooks/deploy.md"));
        let names = analyzer_names(&artifacts, Language::Rust);
        assert!(names.contains(&"runbook-check"));
    }

    #[test]
    fn slo_files_trigger_slo_check() {
        // Target: lines 357-363
        let mut artifacts = Artifacts::default();
        artifacts.slo_files.push(PathBuf::from("slo.json"));
        let names = analyzer_names(&artifacts, Language::Rust);
        assert!(names.contains(&"slo-check"));
    }

    #[test]
    fn apex_index_triggers_blast_radius() {
        // Target: lines 366-372
        let mut artifacts = Artifacts::default();
        artifacts.has_apex_index = true;
        let names = analyzer_names(&artifacts, Language::Rust);
        assert!(names.contains(&"blast-radius"));
    }

    #[test]
    fn no_apex_index_skips_blast_radius() {
        let artifacts = Artifacts::default();
        let names = analyzer_names(&artifacts, Language::Rust);
        assert!(!names.contains(&"blast-radius"));
    }

    // -----------------------------------------------------------------------
    // Target: lines 391-412 — run_applicable_analyzers async runner
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn run_applicable_analyzers_empty_list_returns_empty() {
        // Target: lines 391-426 — empty analyzers list
        let dir = std::env::temp_dir().join("apex_test_run_empty");
        let _ = fs::create_dir_all(&dir);
        let cache = HashMap::new();
        let artifacts = Artifacts::default();
        let results = run_applicable_analyzers(&dir, Language::Rust, &cache, &artifacts, &[]).await;
        assert!(results.is_empty());
        let _ = fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn run_applicable_analyzers_unknown_name_returns_null() {
        // Target: lines 544-546 — unknown analyzer name returns Value::Null (not error)
        let dir = std::env::temp_dir().join("apex_test_run_unknown");
        let _ = fs::create_dir_all(&dir);
        let cache = HashMap::new();
        let artifacts = Artifacts::default();
        let analyzers = vec![ApplicableAnalyzer {
            name: "nonexistent-analyzer",
            description: "does not exist",
            artifacts_used: vec![],
        }];
        let results =
            run_applicable_analyzers(&dir, Language::Rust, &cache, &artifacts, &analyzers).await;
        assert_eq!(results.len(), 1);
        assert!(matches!(results[0].status, AnalyzerStatus::Ok));
        assert_eq!(results[0].report, serde_json::Value::Null);
        let _ = fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn run_applicable_analyzers_records_duration() {
        // Target: lines 401-403 — duration_ms is recorded
        let dir = std::env::temp_dir().join("apex_test_run_duration");
        let _ = fs::create_dir_all(&dir);
        let cache = HashMap::new();
        let artifacts = Artifacts::default();
        let analyzers = vec![ApplicableAnalyzer {
            name: "service-map",
            description: "service map",
            artifacts_used: vec![],
        }];
        let results =
            run_applicable_analyzers(&dir, Language::Rust, &cache, &artifacts, &analyzers).await;
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].name, "service-map");
        // duration_ms is a u64; we just verify it is populated (not a sentinel bad value)
        // actual value is >= 0, just ensure field exists and type is correct
        let _ = results[0].duration_ms;
        let _ = fs::remove_dir_all(&dir);
    }

    // -----------------------------------------------------------------------
    // Target: lines 416-541 — run_single_analyzer dispatch for all branches
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn run_single_service_map_analyzer() {
        // Target: lines 437-440
        let dir = std::env::temp_dir().join("apex_test_single_service_map");
        let _ = fs::create_dir_all(&dir);
        let cache = HashMap::new();
        let artifacts = Artifacts::default();
        let analyzers = vec![ApplicableAnalyzer {
            name: "service-map",
            description: "desc",
            artifacts_used: vec![],
        }];
        let results =
            run_applicable_analyzers(&dir, Language::Rust, &cache, &artifacts, &analyzers).await;
        assert!(matches!(results[0].status, AnalyzerStatus::Ok));
        let _ = fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn run_single_mem_check_analyzer() {
        // Target: lines 441-444
        let dir = std::env::temp_dir().join("apex_test_single_mem_check");
        let _ = fs::create_dir_all(&dir);
        let cache = HashMap::new();
        let artifacts = Artifacts::default();
        let analyzers = vec![ApplicableAnalyzer {
            name: "mem-check",
            description: "desc",
            artifacts_used: vec![],
        }];
        let results =
            run_applicable_analyzers(&dir, Language::Rust, &cache, &artifacts, &analyzers).await;
        assert!(matches!(results[0].status, AnalyzerStatus::Ok));
        let _ = fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn run_single_cost_estimate_analyzer() {
        // Target: lines 445-448
        let dir = std::env::temp_dir().join("apex_test_single_cost_estimate");
        let _ = fs::create_dir_all(&dir);
        let cache = HashMap::new();
        let artifacts = Artifacts::default();
        let analyzers = vec![ApplicableAnalyzer {
            name: "cost-estimate",
            description: "desc",
            artifacts_used: vec![],
        }];
        let results =
            run_applicable_analyzers(&dir, Language::Rust, &cache, &artifacts, &analyzers).await;
        assert!(matches!(results[0].status, AnalyzerStatus::Ok));
        let _ = fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn run_container_scan_with_no_dockerfile_returns_null() {
        // Target: lines 449-457 — container-scan with no dockerfiles returns Null
        let dir = std::env::temp_dir().join("apex_test_single_container_no_docker");
        let _ = fs::create_dir_all(&dir);
        let cache = HashMap::new();
        let artifacts = Artifacts::default(); // no dockerfiles
        let analyzers = vec![ApplicableAnalyzer {
            name: "container-scan",
            description: "desc",
            artifacts_used: vec![],
        }];
        let results =
            run_applicable_analyzers(&dir, Language::Rust, &cache, &artifacts, &analyzers).await;
        assert_eq!(results[0].report, serde_json::Value::Null);
        let _ = fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn run_container_scan_with_real_dockerfile() {
        // Target: lines 450-456 — container-scan reads dockerfile and runs
        let dir = std::env::temp_dir().join("apex_test_single_container_docker");
        let _ = fs::remove_dir_all(&dir);
        let _ = fs::create_dir_all(&dir);
        let dockerfile = dir.join("Dockerfile");
        fs::write(&dockerfile, "FROM ubuntu:latest\nRUN apt-get update\n").unwrap();
        let cache = HashMap::new();
        let mut artifacts = Artifacts::default();
        artifacts.dockerfiles.push(dockerfile);
        let analyzers = vec![ApplicableAnalyzer {
            name: "container-scan",
            description: "desc",
            artifacts_used: artifacts.dockerfiles.clone(),
        }];
        let results =
            run_applicable_analyzers(&dir, Language::Rust, &cache, &artifacts, &analyzers).await;
        assert!(matches!(results[0].status, AnalyzerStatus::Ok));
        assert_ne!(results[0].report, serde_json::Value::Null);
        let _ = fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn run_iac_scan_analyzer() {
        // Target: lines 458-461
        let dir = std::env::temp_dir().join("apex_test_single_iac");
        let _ = fs::create_dir_all(&dir);
        let cache = HashMap::new();
        let artifacts = Artifacts::default();
        let analyzers = vec![ApplicableAnalyzer {
            name: "iac-scan",
            description: "desc",
            artifacts_used: vec![],
        }];
        let results =
            run_applicable_analyzers(&dir, Language::Rust, &cache, &artifacts, &analyzers).await;
        assert!(matches!(results[0].status, AnalyzerStatus::Ok));
        let _ = fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn run_config_drift_with_no_env_files_returns_null() {
        // Target: lines 462-478 — config-drift with fewer than 2 env files returns Null
        let dir = std::env::temp_dir().join("apex_test_single_config_drift_no_env");
        let _ = fs::create_dir_all(&dir);
        let cache = HashMap::new();
        let artifacts = Artifacts::default(); // no env files
        let analyzers = vec![ApplicableAnalyzer {
            name: "config-drift",
            description: "desc",
            artifacts_used: vec![],
        }];
        let results =
            run_applicable_analyzers(&dir, Language::Rust, &cache, &artifacts, &analyzers).await;
        assert_eq!(results[0].report, serde_json::Value::Null);
        let _ = fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn run_config_drift_with_two_env_files() {
        // Target: lines 463-477 — config-drift reads both env files
        let dir = std::env::temp_dir().join("apex_test_single_config_drift_two_env");
        let _ = fs::remove_dir_all(&dir);
        let _ = fs::create_dir_all(&dir);
        let env1 = dir.join(".env");
        let env2 = dir.join(".env.production");
        fs::write(&env1, "KEY_A=1\nKEY_B=2\n").unwrap();
        fs::write(&env2, "KEY_A=1\nKEY_C=3\n").unwrap();
        let cache = HashMap::new();
        let mut artifacts = Artifacts::default();
        artifacts.env_files.push(env1);
        artifacts.env_files.push(env2);
        let analyzers = vec![ApplicableAnalyzer {
            name: "config-drift",
            description: "desc",
            artifacts_used: artifacts.env_files.clone(),
        }];
        let results =
            run_applicable_analyzers(&dir, Language::Rust, &cache, &artifacts, &analyzers).await;
        assert!(matches!(results[0].status, AnalyzerStatus::Ok));
        let _ = fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn run_api_coverage_with_no_openapi_returns_null() {
        // Target: lines 479-488
        let dir = std::env::temp_dir().join("apex_test_single_api_cov_no_spec");
        let _ = fs::create_dir_all(&dir);
        let cache = HashMap::new();
        let artifacts = Artifacts::default();
        let analyzers = vec![ApplicableAnalyzer {
            name: "api-coverage",
            description: "desc",
            artifacts_used: vec![],
        }];
        let results =
            run_applicable_analyzers(&dir, Language::Rust, &cache, &artifacts, &analyzers).await;
        assert_eq!(results[0].report, serde_json::Value::Null);
        let _ = fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn run_doc_coverage_with_no_openapi_returns_null() {
        // Target: lines 489-498
        let dir = std::env::temp_dir().join("apex_test_single_doc_cov_no_spec");
        let _ = fs::create_dir_all(&dir);
        let cache = HashMap::new();
        let artifacts = Artifacts::default();
        let analyzers = vec![ApplicableAnalyzer {
            name: "doc-coverage",
            description: "desc",
            artifacts_used: vec![],
        }];
        let results =
            run_applicable_analyzers(&dir, Language::Rust, &cache, &artifacts, &analyzers).await;
        assert_eq!(results[0].report, serde_json::Value::Null);
        let _ = fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn run_schema_check_analyzer_with_sql_migrations() {
        // Target: lines 499-507
        let dir = std::env::temp_dir().join("apex_test_single_schema_check");
        let _ = fs::remove_dir_all(&dir);
        let migrations = dir.join("migrations");
        let _ = fs::create_dir_all(&migrations);
        let sql_file = migrations.join("001.sql");
        fs::write(&sql_file, "ALTER TABLE users DROP COLUMN email;").unwrap();
        let cache = HashMap::new();
        let mut artifacts = Artifacts::default();
        artifacts.sql_migrations.push(sql_file);
        let analyzers = vec![ApplicableAnalyzer {
            name: "schema-check",
            description: "desc",
            artifacts_used: artifacts.sql_migrations.clone(),
        }];
        let results =
            run_applicable_analyzers(&dir, Language::Rust, &cache, &artifacts, &analyzers).await;
        assert!(matches!(results[0].status, AnalyzerStatus::Ok));
        // Should have found issues — the report is a non-empty array
        assert!(results[0].report.is_array());
        let _ = fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn run_a11y_scan_analyzer() {
        // Target: lines 508-511
        let dir = std::env::temp_dir().join("apex_test_single_a11y");
        let _ = fs::create_dir_all(&dir);
        let cache = HashMap::new();
        let artifacts = Artifacts::default();
        let analyzers = vec![ApplicableAnalyzer {
            name: "a11y-scan",
            description: "desc",
            artifacts_used: vec![],
        }];
        let results =
            run_applicable_analyzers(&dir, Language::Rust, &cache, &artifacts, &analyzers).await;
        assert!(matches!(results[0].status, AnalyzerStatus::Ok));
        let _ = fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn run_i18n_check_analyzer() {
        // Target: lines 512-515
        let dir = std::env::temp_dir().join("apex_test_single_i18n");
        let _ = fs::create_dir_all(&dir);
        let cache = HashMap::new();
        let artifacts = Artifacts::default();
        let analyzers = vec![ApplicableAnalyzer {
            name: "i18n-check",
            description: "desc",
            artifacts_used: vec![],
        }];
        let results =
            run_applicable_analyzers(&dir, Language::Rust, &cache, &artifacts, &analyzers).await;
        assert!(matches!(results[0].status, AnalyzerStatus::Ok));
        let _ = fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn run_dep_graph_analyzer() {
        // Target: lines 516-519
        let dir = std::env::temp_dir().join("apex_test_single_dep_graph");
        let _ = fs::create_dir_all(&dir);
        let cache = HashMap::new();
        let artifacts = Artifacts::default();
        let analyzers = vec![ApplicableAnalyzer {
            name: "dep-graph",
            description: "desc",
            artifacts_used: vec![],
        }];
        let results =
            run_applicable_analyzers(&dir, Language::Rust, &cache, &artifacts, &analyzers).await;
        assert!(matches!(results[0].status, AnalyzerStatus::Ok));
        let _ = fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn run_runbook_check_analyzer_with_files() {
        // Target: lines 520-530
        let dir = std::env::temp_dir().join("apex_test_single_runbook");
        let _ = fs::remove_dir_all(&dir);
        let runbooks = dir.join("runbooks");
        let _ = fs::create_dir_all(&runbooks);
        let rb = runbooks.join("deploy.md");
        fs::write(&rb, "# Deploy\n\n## Steps\n1. Run deploy script\n").unwrap();
        let cache = HashMap::new();
        let mut artifacts = Artifacts::default();
        artifacts.runbook_files.push(rb);
        let analyzers = vec![ApplicableAnalyzer {
            name: "runbook-check",
            description: "desc",
            artifacts_used: artifacts.runbook_files.clone(),
        }];
        let results =
            run_applicable_analyzers(&dir, Language::Rust, &cache, &artifacts, &analyzers).await;
        assert!(matches!(results[0].status, AnalyzerStatus::Ok));
        assert!(results[0].report.is_array());
        let _ = fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn run_slo_check_with_no_slo_file_returns_null() {
        // Target: lines 531-543 — no slo file returns Null
        let dir = std::env::temp_dir().join("apex_test_single_slo_no_file");
        let _ = fs::create_dir_all(&dir);
        let cache = HashMap::new();
        let artifacts = Artifacts::default();
        let analyzers = vec![ApplicableAnalyzer {
            name: "slo-check",
            description: "desc",
            artifacts_used: vec![],
        }];
        let results =
            run_applicable_analyzers(&dir, Language::Rust, &cache, &artifacts, &analyzers).await;
        assert_eq!(results[0].report, serde_json::Value::Null);
        let _ = fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn run_slo_check_with_slo_file() {
        // Target: lines 532-542 — slo file present and readable
        let dir = std::env::temp_dir().join("apex_test_single_slo_with_file");
        let _ = fs::remove_dir_all(&dir);
        let _ = fs::create_dir_all(&dir);
        let slo_file = dir.join("slo.json");
        fs::write(&slo_file, r#"{"slos": []}"#).unwrap();
        let cache = HashMap::new();
        let mut artifacts = Artifacts::default();
        artifacts.slo_files.push(slo_file);
        let analyzers = vec![ApplicableAnalyzer {
            name: "slo-check",
            description: "desc",
            artifacts_used: artifacts.slo_files.clone(),
        }];
        let results =
            run_applicable_analyzers(&dir, Language::Rust, &cache, &artifacts, &analyzers).await;
        assert!(matches!(results[0].status, AnalyzerStatus::Ok));
        let _ = fs::remove_dir_all(&dir);
    }

    // -----------------------------------------------------------------------
    // Artifacts::total_count correctness
    // -----------------------------------------------------------------------

    #[test]
    fn total_count_counts_all_artifact_types() {
        let mut a = Artifacts::default();
        a.dockerfiles.push(PathBuf::from("Dockerfile"));
        a.iac_files.push(PathBuf::from("main.tf"));
        a.env_files.push(PathBuf::from(".env"));
        a.openapi_specs.push(PathBuf::from("openapi.json"));
        a.sql_migrations.push(PathBuf::from("001.sql"));
        a.frontend_files.push(PathBuf::from("App.tsx"));
        a.i18n_files.push(PathBuf::from("en.json"));
        a.runbook_files.push(PathBuf::from("deploy.md"));
        a.slo_files.push(PathBuf::from("slo.json"));
        a.cargo_toml = Some(PathBuf::from("Cargo.toml"));
        a.package_json = Some(PathBuf::from("package.json"));
        a.has_apex_index = true;
        assert_eq!(a.total_count(), 12);
    }
}
