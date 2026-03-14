//! Bug detection and security analysis pipeline for APEX.
//!
//! Detectors identify panic patterns, security vulnerabilities, and code quality issues.

pub mod api_diff;
pub mod config;
pub mod context;
pub mod cvss;
pub mod dep_graph;
pub mod detectors;
pub mod doc_coverage;
pub mod finding;
pub mod lockfile;
pub mod pipeline;
pub mod ratchet;
pub mod report;
pub mod runbook_check;
pub mod sarif;
pub mod sbom;
pub mod sca;
pub mod slo_check;
pub mod threat_model;
pub mod vuln_pipeline;

pub use config::DetectConfig;
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
