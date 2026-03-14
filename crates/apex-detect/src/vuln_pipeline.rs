//! Vulnerability detection pipeline orchestrator.
//!
//! Coordinates multiple detectors (triage, HAGNN, dual encoder) and aggregates
//! their findings with configurable limits.

use crate::finding::{Finding, Severity};

/// Configuration for the vulnerability detection pipeline.
#[derive(Debug, Clone)]
pub struct VulnPipelineConfig {
    /// Whether triage-based detection is enabled.
    pub triage_enabled: bool,
    /// Whether the HAGNN detector is active.
    pub use_hagnn: bool,
    /// Whether the dual encoder detector is active.
    pub use_dual_encoder: bool,
    /// Maximum number of findings to retain after truncation.
    pub max_findings: usize,
}

impl Default for VulnPipelineConfig {
    fn default() -> Self {
        Self {
            triage_enabled: true,
            use_hagnn: false,
            use_dual_encoder: false,
            max_findings: 100,
        }
    }
}

/// Aggregate statistics from a pipeline run.
#[derive(Debug, Clone, Default)]
pub struct PipelineStats {
    /// Total number of findings.
    pub total_findings: usize,
    /// Count by severity.
    pub critical: usize,
    pub high: usize,
    pub medium: usize,
    pub low: usize,
    pub info: usize,
    /// Number of detectors that ran.
    pub detectors_run: usize,
    /// Number of detectors that failed.
    pub detectors_failed: usize,
}

impl PipelineStats {
    /// Compute stats from a slice of findings.
    pub fn from_findings(findings: &[Finding]) -> Self {
        let mut stats = Self {
            total_findings: findings.len(),
            ..Default::default()
        };
        for f in findings {
            match f.severity {
                Severity::Critical => stats.critical += 1,
                Severity::High => stats.high += 1,
                Severity::Medium => stats.medium += 1,
                Severity::Low => stats.low += 1,
                Severity::Info => stats.info += 1,
            }
        }
        stats
    }
}

/// Orchestrates multiple vulnerability detectors.
#[derive(Debug)]
pub struct VulnDetectionPipeline {
    pub config: VulnPipelineConfig,
}

impl VulnDetectionPipeline {
    /// Create a new pipeline with the given config.
    pub fn new(config: VulnPipelineConfig) -> Self {
        Self { config }
    }

    /// Return names of detectors that are currently active.
    pub fn active_detector_names(&self) -> Vec<&str> {
        let mut names = Vec::new();
        if self.config.triage_enabled {
            names.push("triage");
        }
        if self.config.use_hagnn {
            names.push("hagnn");
        }
        if self.config.use_dual_encoder {
            names.push("dual-encoder");
        }
        names
    }

    /// Sort findings by severity (most critical first) and truncate to `max_findings`.
    pub fn truncate_findings(&self, mut findings: Vec<Finding>) -> Vec<Finding> {
        findings.sort_by_key(|f| f.severity.rank());
        findings.truncate(self.config.max_findings);
        findings
    }
}

impl Default for VulnDetectionPipeline {
    fn default() -> Self {
        Self::new(VulnPipelineConfig::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use uuid::Uuid;
    use crate::finding::FindingCategory;

    fn make_finding(severity: Severity) -> Finding {
        Finding {
            id: Uuid::new_v4(),
            detector: "test".into(),
            severity,
            category: FindingCategory::SecuritySmell,
            file: PathBuf::from("test.rs"),
            line: Some(1),
            title: format!("{:?} finding", severity),
            description: "test".into(),
            evidence: vec![],
            covered: false,
            suggestion: "fix".into(),
            explanation: None,
            fix: None,
            cwe_ids: vec![],
        }
    }

    #[test]
    fn config_defaults() {
        let cfg = VulnPipelineConfig::default();
        assert!(cfg.triage_enabled);
        assert!(!cfg.use_hagnn);
        assert!(!cfg.use_dual_encoder);
        assert_eq!(cfg.max_findings, 100);
    }

    #[test]
    fn pipeline_new() {
        let p = VulnDetectionPipeline::new(VulnPipelineConfig::default());
        assert!(p.config.triage_enabled);
    }

    #[test]
    fn pipeline_default() {
        let p = VulnDetectionPipeline::default();
        assert!(p.config.triage_enabled);
        assert!(!p.config.use_hagnn);
    }

    #[test]
    fn active_detector_names_default() {
        let p = VulnDetectionPipeline::default();
        assert_eq!(p.active_detector_names(), vec!["triage"]);
    }

    #[test]
    fn active_detector_names_with_hagnn() {
        let p = VulnDetectionPipeline::new(VulnPipelineConfig {
            use_hagnn: true,
            ..Default::default()
        });
        assert_eq!(p.active_detector_names(), vec!["triage", "hagnn"]);
    }

    #[test]
    fn active_detector_names_with_dual_encoder() {
        let p = VulnDetectionPipeline::new(VulnPipelineConfig {
            use_dual_encoder: true,
            ..Default::default()
        });
        assert_eq!(p.active_detector_names(), vec!["triage", "dual-encoder"]);
    }

    #[test]
    fn active_detector_names_with_all() {
        let p = VulnDetectionPipeline::new(VulnPipelineConfig {
            use_hagnn: true,
            use_dual_encoder: true,
            ..Default::default()
        });
        assert_eq!(
            p.active_detector_names(),
            vec!["triage", "hagnn", "dual-encoder"]
        );
    }

    #[test]
    fn truncate_findings_respects_max() {
        let p = VulnDetectionPipeline::new(VulnPipelineConfig {
            max_findings: 2,
            ..Default::default()
        });
        let findings = vec![
            make_finding(Severity::Low),
            make_finding(Severity::Critical),
            make_finding(Severity::High),
            make_finding(Severity::Info),
        ];
        let result = p.truncate_findings(findings);
        assert_eq!(result.len(), 2);
        // Should keep Critical and High (lowest rank values)
        assert_eq!(result[0].severity, Severity::Critical);
        assert_eq!(result[1].severity, Severity::High);
    }

    #[test]
    fn truncate_findings_no_truncation() {
        let p = VulnDetectionPipeline::default();
        let findings = vec![
            make_finding(Severity::High),
            make_finding(Severity::Low),
        ];
        let result = p.truncate_findings(findings);
        assert_eq!(result.len(), 2);
    }

    #[test]
    fn truncate_findings_empty() {
        let p = VulnDetectionPipeline::default();
        let result = p.truncate_findings(vec![]);
        assert!(result.is_empty());
    }

    #[test]
    fn pipeline_stats_from_findings() {
        let findings = vec![
            make_finding(Severity::Critical),
            make_finding(Severity::Critical),
            make_finding(Severity::High),
            make_finding(Severity::Medium),
            make_finding(Severity::Low),
            make_finding(Severity::Info),
        ];
        let stats = PipelineStats::from_findings(&findings);
        assert_eq!(stats.total_findings, 6);
        assert_eq!(stats.critical, 2);
        assert_eq!(stats.high, 1);
        assert_eq!(stats.medium, 1);
        assert_eq!(stats.low, 1);
        assert_eq!(stats.info, 1);
        assert_eq!(stats.detectors_run, 0);
        assert_eq!(stats.detectors_failed, 0);
    }

    #[test]
    fn pipeline_stats_from_findings_empty() {
        let stats = PipelineStats::from_findings(&[]);
        assert_eq!(stats.total_findings, 0);
        assert_eq!(stats.critical, 0);
        assert_eq!(stats.high, 0);
        assert_eq!(stats.medium, 0);
        assert_eq!(stats.low, 0);
        assert_eq!(stats.info, 0);
    }

    #[test]
    fn debug_impls() {
        let cfg = VulnPipelineConfig::default();
        let dbg = format!("{:?}", cfg);
        assert!(dbg.contains("VulnPipelineConfig"));

        let stats = PipelineStats::default();
        let dbg = format!("{:?}", stats);
        assert!(dbg.contains("PipelineStats"));

        let p = VulnDetectionPipeline::default();
        let dbg = format!("{:?}", p);
        assert!(dbg.contains("VulnDetectionPipeline"));
    }
}
