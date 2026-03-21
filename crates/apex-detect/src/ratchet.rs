//! Security ratchet gate: prevents introducing new HIGH/CRITICAL findings.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::finding::{Finding, Severity};

/// Unique key for a finding used in baseline comparison.
type FindingKey = (PathBuf, Option<u32>, String);

fn finding_key(f: &Finding) -> FindingKey {
    (f.file.clone(), f.line, f.detector.clone())
}

/// Returns findings in `current` that are NOT in `baseline` (by file + line + detector)
/// and have severity >= High (i.e., rank <= High's rank of 1).
pub fn security_delta(current: &[Finding], baseline: &[Finding]) -> Vec<Finding> {
    let baseline_keys: HashSet<FindingKey> = baseline.iter().map(finding_key).collect();
    let high_threshold = Severity::High.rank();

    current
        .iter()
        .filter(|f| f.severity.rank() <= high_threshold && !baseline_keys.contains(&finding_key(f)))
        .cloned()
        .collect()
}

/// Persisted security baseline for ratchet comparisons.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecurityBaseline {
    pub findings: Vec<Finding>,
}

impl SecurityBaseline {
    /// Default baseline path: `<project_root>/.apex/security-baseline.json`
    pub fn default_path(project_root: &Path) -> PathBuf {
        project_root.join(".apex").join("security-baseline.json")
    }

    /// Save baseline to disk.
    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let json = serde_json::to_string_pretty(&self)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        std::fs::write(path, json)
    }

    /// Load baseline from disk. Returns empty baseline if file does not exist.
    pub fn load(path: &Path) -> std::io::Result<Self> {
        if !path.exists() {
            return Ok(Self {
                findings: Vec::new(),
            });
        }
        let data = std::fs::read_to_string(path)?;
        serde_json::from_str(&data)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::finding::{FindingCategory, Severity};
    use std::path::PathBuf;

    fn make_finding(detector: &str, file: &str, line: u32, severity: Severity) -> Finding {
        Finding {
            id: uuid::Uuid::new_v4(),
            detector: detector.into(),
            severity,
            category: FindingCategory::PanicPath,
            file: PathBuf::from(file),
            line: Some(line),
            title: format!("{detector} finding"),
            description: "desc".into(),
            evidence: vec![],
            covered: false,
            suggestion: "fix it".into(),
            explanation: None,
            fix: None,
            cwe_ids: vec![],
                    noisy: false, base_severity: None, coverage_confidence: None,
        }
    }

    #[test]
    fn security_delta_passes_when_no_new_findings() {
        let baseline = vec![make_finding("det", "src/lib.rs", 10, Severity::High)];
        let current = vec![make_finding("det", "src/lib.rs", 10, Severity::High)];
        let delta = security_delta(&current, &baseline);
        assert!(delta.is_empty(), "expected no new findings");
    }

    #[test]
    fn security_delta_fails_on_new_high_finding() {
        let baseline = vec![make_finding("det", "src/lib.rs", 10, Severity::High)];
        let current = vec![
            make_finding("det", "src/lib.rs", 10, Severity::High),
            make_finding("det", "src/lib.rs", 20, Severity::High),
        ];
        let delta = security_delta(&current, &baseline);
        assert_eq!(delta.len(), 1);
        assert_eq!(delta[0].line, Some(20));
    }

    #[test]
    fn security_delta_ignores_low_severity_findings() {
        let baseline: Vec<Finding> = vec![];
        let current = vec![
            make_finding("det", "src/lib.rs", 10, Severity::Low),
            make_finding("det", "src/lib.rs", 20, Severity::Medium),
            make_finding("det", "src/lib.rs", 30, Severity::Info),
        ];
        let delta = security_delta(&current, &baseline);
        assert!(
            delta.is_empty(),
            "low/medium/info findings should be ignored"
        );
    }

    #[test]
    fn security_delta_detects_findings_at_new_locations() {
        let baseline = vec![make_finding("det", "src/lib.rs", 10, Severity::Critical)];
        let current = vec![
            make_finding("det", "src/lib.rs", 10, Severity::Critical),
            make_finding("det", "src/other.rs", 5, Severity::Critical),
            make_finding("other-det", "src/lib.rs", 10, Severity::High),
        ];
        let delta = security_delta(&current, &baseline);
        assert_eq!(delta.len(), 2);
        // New file
        assert!(delta
            .iter()
            .any(|f| f.file == PathBuf::from("src/other.rs")));
        // Same file+line but different detector
        assert!(delta.iter().any(|f| f.detector == "other-det"));
    }

    #[test]
    fn baseline_roundtrip_serialization() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(".apex").join("security-baseline.json");

        let baseline = SecurityBaseline {
            findings: vec![
                make_finding("det-a", "src/lib.rs", 10, Severity::High),
                make_finding("det-b", "src/main.rs", 42, Severity::Critical),
            ],
        };

        baseline.save(&path).unwrap();
        let loaded = SecurityBaseline::load(&path).unwrap();

        assert_eq!(loaded.findings.len(), 2);
        assert_eq!(loaded.findings[0].detector, "det-a");
        assert_eq!(loaded.findings[0].line, Some(10));
        assert_eq!(loaded.findings[1].detector, "det-b");
        assert_eq!(loaded.findings[1].severity, Severity::Critical);
    }

    #[test]
    fn baseline_load_missing_file_returns_empty() {
        let path = PathBuf::from("/nonexistent/.apex/security-baseline.json");
        let baseline = SecurityBaseline::load(&path).unwrap();
        assert!(baseline.findings.is_empty());
    }

    #[test]
    fn security_delta_empty_inputs() {
        let delta = security_delta(&[], &[]);
        assert!(delta.is_empty());
    }

    #[test]
    fn security_delta_critical_detected() {
        let baseline: Vec<Finding> = vec![];
        let current = vec![make_finding("det", "src/lib.rs", 1, Severity::Critical)];
        let delta = security_delta(&current, &baseline);
        assert_eq!(delta.len(), 1);
        assert_eq!(delta[0].severity, Severity::Critical);
    }
}
