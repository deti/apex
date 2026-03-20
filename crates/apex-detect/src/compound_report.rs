//! Compound report merging detector findings with analyzer results.

use serde::Serialize;

use crate::analyzer_registry::{AnalyzerResult, AnalyzerStatus, Artifacts};
use crate::report::AnalysisReport;

/// A combined report containing both detection findings and analyzer results.
#[derive(Debug, Clone, Serialize)]
pub struct CompoundReport {
    pub detection: AnalysisReport,
    pub analyzers: Vec<AnalyzerResult>,
    pub artifacts_discovered: Artifacts,
    pub summary: CompoundSummary,
}

/// Summary statistics for the compound report.
#[derive(Debug, Clone, Serialize)]
pub struct CompoundSummary {
    pub detector_findings: usize,
    pub analyzers_run: usize,
    pub analyzers_ok: usize,
    pub analyzers_failed: usize,
    pub artifacts_found: usize,
}

impl CompoundReport {
    /// Create a new compound report from detection results and analyzer results.
    pub fn new(
        detection: AnalysisReport,
        analyzers: Vec<AnalyzerResult>,
        artifacts: Artifacts,
    ) -> Self {
        let analyzers_ok = analyzers
            .iter()
            .filter(|a| matches!(a.status, AnalyzerStatus::Ok))
            .count();
        let analyzers_failed = analyzers
            .iter()
            .filter(|a| matches!(a.status, AnalyzerStatus::Failed(_)))
            .count();

        let summary = CompoundSummary {
            detector_findings: detection.findings.len(),
            analyzers_run: analyzers.len(),
            analyzers_ok,
            analyzers_failed,
            artifacts_found: artifacts.total_count(),
        };

        CompoundReport {
            detection,
            analyzers,
            artifacts_discovered: artifacts,
            summary,
        }
    }

    /// Total number of detector findings.
    pub fn total_findings(&self) -> usize {
        self.detection.findings.len()
    }

    /// Number of analyzers that were run.
    pub fn analyzers_run(&self) -> usize {
        self.analyzers.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::finding::{Finding, FindingCategory, Severity};
    use std::path::PathBuf;

    fn make_finding(severity: Severity) -> Finding {
        Finding {
            id: uuid::Uuid::nil(),
            detector: "test".into(),
            severity,
            category: FindingCategory::PanicPath,
            file: PathBuf::from("test.rs"),
            line: Some(1),
            title: "t".into(),
            description: "d".into(),
            evidence: vec![],
            covered: false,
            suggestion: "s".into(),
            explanation: None,
            fix: None,
            cwe_ids: vec![],
                    noisy: false,
        }
    }

    fn empty_analysis_report() -> AnalysisReport {
        AnalysisReport {
            findings: vec![],
            detector_status: vec![],
        }
    }

    #[test]
    fn empty_compound_report() {
        let report = CompoundReport::new(empty_analysis_report(), vec![], Artifacts::default());
        assert_eq!(report.total_findings(), 0);
        assert_eq!(report.analyzers_run(), 0);
        assert_eq!(report.summary.analyzers_ok, 0);
        assert_eq!(report.summary.analyzers_failed, 0);
        assert_eq!(report.summary.artifacts_found, 0);
    }

    #[test]
    fn counts_detector_findings() {
        let detection = AnalysisReport {
            findings: vec![
                make_finding(Severity::High),
                make_finding(Severity::Medium),
                make_finding(Severity::Low),
            ],
            detector_status: vec![("test".into(), true)],
        };

        let analyzers = vec![
            AnalyzerResult {
                name: "service-map".into(),
                description: "Discover inter-service dependencies".into(),
                status: AnalyzerStatus::Ok,
                report: serde_json::Value::Null,
                duration_ms: 5,
            },
            AnalyzerResult {
                name: "iac-scan".into(),
                description: "Scan IaC for misconfigurations".into(),
                status: AnalyzerStatus::Failed("parse error".into()),
                report: serde_json::Value::Null,
                duration_ms: 2,
            },
        ];

        let report = CompoundReport::new(detection, analyzers, Artifacts::default());
        assert_eq!(report.total_findings(), 3);
        assert_eq!(report.analyzers_run(), 2);
        assert_eq!(report.summary.analyzers_ok, 1);
        assert_eq!(report.summary.analyzers_failed, 1);
        assert_eq!(report.summary.detector_findings, 3);
    }

    #[test]
    fn serializes_to_json() {
        let report = CompoundReport::new(
            empty_analysis_report(),
            vec![AnalyzerResult {
                name: "service-map".into(),
                description: "Discover inter-service dependencies".into(),
                status: AnalyzerStatus::Ok,
                report: serde_json::json!({"services": 3}),
                duration_ms: 12,
            }],
            Artifacts::default(),
        );

        let json = serde_json::to_string_pretty(&report).unwrap();
        assert!(json.contains("\"service-map\""));
        assert!(json.contains("\"analyzers_run\": 1"));
        assert!(json.contains("\"services\": 3"));
    }

    #[test]
    fn summary_counts_skipped() {
        let analyzers = vec![
            AnalyzerResult {
                name: "a".into(),
                description: "".into(),
                status: AnalyzerStatus::Ok,
                report: serde_json::Value::Null,
                duration_ms: 1,
            },
            AnalyzerResult {
                name: "b".into(),
                description: "".into(),
                status: AnalyzerStatus::Skipped("no artifacts".into()),
                report: serde_json::Value::Null,
                duration_ms: 0,
            },
            AnalyzerResult {
                name: "c".into(),
                description: "".into(),
                status: AnalyzerStatus::Failed("boom".into()),
                report: serde_json::Value::Null,
                duration_ms: 3,
            },
        ];

        let report = CompoundReport::new(empty_analysis_report(), analyzers, Artifacts::default());
        assert_eq!(report.summary.analyzers_run, 3);
        assert_eq!(report.summary.analyzers_ok, 1);
        assert_eq!(report.summary.analyzers_failed, 1);
    }
}
