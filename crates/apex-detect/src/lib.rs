//! Bug detection and security analysis pipeline for APEX.
//!
//! Detectors identify panic patterns, security vulnerabilities, and code quality issues.

pub mod a11y_scan;
pub mod api_coverage;
pub mod api_diff;
pub mod bench_diff;
pub mod compliance;
pub mod config;
pub mod config_drift;
pub mod container_scan;
pub mod context;
pub mod cost_estimate;
pub mod cvss;
pub mod dep_graph;
pub mod detectors;
pub mod doc_coverage;
pub mod finding;
pub mod i18n_check;
pub mod iac_scan;
pub mod incident_match;
pub mod lockfile;
pub mod mem_check;
pub mod migration_check;
pub mod perf_diff;
pub mod pipeline;
pub mod ratchet;
pub mod report;
pub mod resource_profile;
pub mod rules;
pub mod runbook_check;
pub mod sarif;
pub mod sbom;
pub mod sca;
pub mod schema_check;
pub mod service_map;
pub mod slo_check;
pub mod test_data;
pub mod threat;
pub mod threat_model;
pub mod trace_analysis;
pub mod vuln_pipeline;

pub use config::{DetectConfig, DetectMode};
pub use context::AnalysisContext;
pub use finding::{Evidence, Finding, FindingCategory, Fix, Severity};
pub use pipeline::DetectorPipeline;
pub use report::{AnalysisReport, SecuritySummary};

use apex_core::error::Result;
use async_trait::async_trait;

/// A pluggable detector that analyzes code for bugs/security issues.
#[async_trait]
pub trait Detector: Send + Sync {
    fn name(&self) -> &str;
    async fn analyze(&self, ctx: &AnalysisContext) -> Result<Vec<Finding>>;
    fn uses_cargo_subprocess(&self) -> bool {
        false
    }
}
