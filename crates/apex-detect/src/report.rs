use serde::{Deserialize, Serialize};

use crate::finding::{Finding, Severity};
use crate::hunt_hints::{HuntHintConfig, HuntHints};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnalysisReport {
    pub findings: Vec<Finding>,
    pub detector_status: Vec<(String, bool)>,
}

impl AnalysisReport {
    /// Build hunt priority hints from this report's findings.
    ///
    /// Callers (e.g. `apex run` orchestrator) can pass the returned `HuntHints`
    /// to `HuntHints::security_boost_for(file, line)` when scoring uncovered
    /// branches, so that branches near security findings are explored first.
    pub fn hunt_hints(&self) -> HuntHints {
        HuntHints::from_findings(&self.findings, HuntHintConfig::default())
    }

    pub fn security_summary(&self) -> SecuritySummary {
        let mut critical = 0;
        let mut high = 0;
        let mut medium = 0;
        let mut low = 0;

        for f in &self.findings {
            match f.severity {
                Severity::Critical => critical += 1,
                Severity::High => high += 1,
                Severity::Medium => medium += 1,
                Severity::Low => low += 1,
                Severity::Info => {}
            }
        }

        let detectors_run: Vec<String> = self
            .detector_status
            .iter()
            .map(|(name, _)| name.clone())
            .collect();

        let top_risk = self
            .findings
            .iter()
            .filter(|f| f.severity.rank() <= Severity::High.rank())
            .min_by_key(|f| (f.severity.rank(), f.covered as u8))
            .map(|f| format!("{} — {}", f.file.display(), f.title));

        SecuritySummary {
            critical,
            high,
            medium,
            low,
            detectors_run,
            top_risk,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecuritySummary {
    pub critical: usize,
    pub high: usize,
    pub medium: usize,
    pub low: usize,
    pub detectors_run: Vec<String>,
    pub top_risk: Option<String>,
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
        }
    }

    #[test]
    fn security_summary_counts_severities() {
        let report = AnalysisReport {
            findings: vec![
                make_finding(Severity::Critical),
                make_finding(Severity::High),
                make_finding(Severity::High),
                make_finding(Severity::Medium),
                make_finding(Severity::Low),
                make_finding(Severity::Info),
            ],
            detector_status: vec![("test".into(), true)],
        };
        let summary = report.security_summary();
        assert_eq!(summary.critical, 1);
        assert_eq!(summary.high, 2);
        assert_eq!(summary.medium, 1);
        assert_eq!(summary.low, 1);
    }

    #[test]
    fn empty_report_gives_zero_summary() {
        let report = AnalysisReport {
            findings: vec![],
            detector_status: vec![],
        };
        let summary = report.security_summary();
        assert_eq!(summary.critical, 0);
        assert_eq!(summary.high, 0);
        assert!(summary.top_risk.is_none());
    }

    #[test]
    fn security_summary_serializes() {
        let summary = SecuritySummary {
            critical: 1,
            high: 2,
            medium: 3,
            low: 4,
            detectors_run: vec!["panic".into()],
            top_risk: Some("src/main.rs — uncovered panic".into()),
        };
        let json = serde_json::to_string(&summary).unwrap();
        assert!(json.contains("\"critical\":1"));
        assert!(json.contains("\"top_risk\""));
    }

    #[test]
    fn security_summary_top_risk_selects_highest_severity() {
        let report = AnalysisReport {
            findings: vec![
                {
                    let mut f = make_finding(Severity::Medium);
                    f.file = PathBuf::from("src/a.rs");
                    f.title = "medium issue".into();
                    f
                },
                {
                    let mut f = make_finding(Severity::Critical);
                    f.file = PathBuf::from("src/b.rs");
                    f.title = "critical issue".into();
                    f
                },
                {
                    let mut f = make_finding(Severity::High);
                    f.file = PathBuf::from("src/c.rs");
                    f.title = "high issue".into();
                    f
                },
            ],
            detector_status: vec![("test".into(), true)],
        };
        let summary = report.security_summary();
        assert!(summary.top_risk.is_some());
        let risk = summary.top_risk.unwrap();
        assert!(risk.contains("critical issue"));
    }

    #[test]
    fn security_summary_info_only_has_no_top_risk() {
        let report = AnalysisReport {
            findings: vec![make_finding(Severity::Info)],
            detector_status: vec![("test".into(), true)],
        };
        let summary = report.security_summary();
        assert_eq!(summary.critical, 0);
        assert_eq!(summary.high, 0);
        assert_eq!(summary.medium, 0);
        assert_eq!(summary.low, 0);
        assert!(summary.top_risk.is_none());
    }

    #[test]
    fn security_summary_detectors_run_list() {
        let report = AnalysisReport {
            findings: vec![],
            detector_status: vec![
                ("panic".into(), true),
                ("deps".into(), false),
                ("unsafe".into(), true),
            ],
        };
        let summary = report.security_summary();
        assert_eq!(summary.detectors_run.len(), 3);
        assert_eq!(summary.detectors_run[0], "panic");
        assert_eq!(summary.detectors_run[1], "deps");
        assert_eq!(summary.detectors_run[2], "unsafe");
    }

    #[test]
    fn security_summary_top_risk_prefers_uncovered() {
        let mut f1 = make_finding(Severity::Critical);
        f1.covered = true;
        f1.file = PathBuf::from("src/a.rs");
        f1.title = "covered critical".into();

        let mut f2 = make_finding(Severity::Critical);
        f2.covered = false;
        f2.file = PathBuf::from("src/b.rs");
        f2.title = "uncovered critical".into();

        let report = AnalysisReport {
            findings: vec![f1, f2],
            detector_status: vec![],
        };
        let summary = report.security_summary();
        let risk = summary.top_risk.unwrap();
        // min_by_key sorts by (severity.rank(), covered as u8)
        // covered=false → 0, covered=true → 1, so uncovered comes first
        assert!(risk.contains("uncovered critical"));
    }

    #[test]
    fn analysis_report_serde_roundtrip() {
        let report = AnalysisReport {
            findings: vec![make_finding(Severity::High)],
            detector_status: vec![("test".into(), true)],
        };
        let json = serde_json::to_string(&report).unwrap();
        let report2: AnalysisReport = serde_json::from_str(&json).unwrap();
        assert_eq!(report2.findings.len(), 1);
        assert_eq!(report2.detector_status.len(), 1);
    }

    #[test]
    fn hunt_hints_from_report_boosts_nearby_branch() {
        use std::path::Path;
        let mut f = make_finding(Severity::Critical);
        f.file = PathBuf::from("src/auth.rs");
        f.line = Some(50);
        let report = AnalysisReport {
            findings: vec![f],
            detector_status: vec![],
        };
        let hints = report.hunt_hints();
        // Branch at line 50 in the same file should receive a boost
        assert!(hints.security_boost_for(Path::new("src/auth.rs"), 50) > 0.0);
        // Branch in a different file should not
        assert_eq!(hints.security_boost_for(Path::new("src/other.rs"), 50), 0.0);
        // There should be 1 file with hints
        assert_eq!(hints.file_count(), 1);
    }
}
