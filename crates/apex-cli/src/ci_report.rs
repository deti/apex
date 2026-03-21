//! `apex ci-report` — compare findings between base and head JSON reports.
//!
//! Reads two JSON files produced by `apex audit --output-format json`, computes
//! new / resolved / unchanged findings, and outputs a markdown or JSON summary.
//! Exit code: 0 if no new high/critical findings, 1 otherwise.

use color_eyre::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::PathBuf;

// ---------------------------------------------------------------------------
// Lightweight finding representation for comparison
// ---------------------------------------------------------------------------

/// Minimal finding shape that matches `apex audit --output-format json` output.
#[derive(Debug, Clone, Deserialize)]
pub struct AuditFinding {
    pub severity: String,
    pub file: String,
    pub line: Option<u32>,
    pub title: String,
    pub description: String,
    pub detector: String,
    #[serde(default)]
    pub suggestion: String,
}

/// Identity key used for matching findings across base/head.
#[derive(Debug, Clone, Hash, PartialEq, Eq)]
struct FindingKey {
    file: String,
    line: Option<u32>,
    detector: String,
}

impl AuditFinding {
    fn key(&self) -> FindingKey {
        FindingKey {
            file: self.file.clone(),
            line: self.line,
            detector: self.detector.clone(),
        }
    }

    fn is_high_or_critical(&self) -> bool {
        let s = self.severity.to_uppercase();
        s == "HIGH" || s == "CRITICAL" || s == "High" || s == "Critical"
    }
}

// ---------------------------------------------------------------------------
// Report types
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize)]
pub struct CiReportOutput {
    pub new_count: usize,
    pub resolved_count: usize,
    pub unchanged_count: usize,
    pub new_findings: Vec<ReportFinding>,
    pub resolved_findings: Vec<ReportFinding>,
    pub has_new_high_critical: bool,
}

#[derive(Debug, Serialize)]
pub struct ReportFinding {
    pub severity: String,
    pub file: String,
    pub line: Option<u32>,
    pub description: String,
    pub detector: String,
}

impl From<&AuditFinding> for ReportFinding {
    fn from(f: &AuditFinding) -> Self {
        Self {
            severity: f.severity.clone(),
            file: f.file.clone(),
            line: f.line,
            description: f.title.clone(),
            detector: f.detector.clone(),
        }
    }
}

// ---------------------------------------------------------------------------
// Core comparison logic
// ---------------------------------------------------------------------------

pub fn compare_findings(
    base: &[AuditFinding],
    head: &[AuditFinding],
) -> CiReportOutput {
    let base_keys: HashSet<FindingKey> = base.iter().map(|f| f.key()).collect();
    let head_keys: HashSet<FindingKey> = head.iter().map(|f| f.key()).collect();

    let new_findings: Vec<ReportFinding> = head
        .iter()
        .filter(|f| !base_keys.contains(&f.key()))
        .map(ReportFinding::from)
        .collect();

    let resolved_findings: Vec<ReportFinding> = base
        .iter()
        .filter(|f| !head_keys.contains(&f.key()))
        .map(ReportFinding::from)
        .collect();

    let unchanged_count = head
        .iter()
        .filter(|f| base_keys.contains(&f.key()))
        .count();

    let has_new_high_critical = head
        .iter()
        .filter(|f| !base_keys.contains(&f.key()))
        .any(|f| f.is_high_or_critical());

    CiReportOutput {
        new_count: new_findings.len(),
        resolved_count: resolved_findings.len(),
        unchanged_count,
        new_findings,
        resolved_findings,
        has_new_high_critical,
    }
}

// ---------------------------------------------------------------------------
// Formatting
// ---------------------------------------------------------------------------

pub fn format_markdown(report: &CiReportOutput) -> String {
    let mut out = String::new();

    out.push_str("## APEX CI Report\n\n");
    out.push_str(&format!(
        "**New findings:** {} | **Resolved:** {} | **Unchanged:** {}\n",
        report.new_count, report.resolved_count, report.unchanged_count
    ));

    if !report.new_findings.is_empty() {
        out.push_str("\n### New Findings (not in base)\n");
        out.push_str("| Severity | File | Line | Description |\n");
        out.push_str("|----------|------|-----:|-------------|\n");
        for f in &report.new_findings {
            let line_str = f.line.map(|l| l.to_string()).unwrap_or_default();
            out.push_str(&format!(
                "| {} | {} | {} | {} |\n",
                f.severity.to_uppercase(),
                f.file,
                line_str,
                f.description
            ));
        }
    }

    if !report.resolved_findings.is_empty() {
        out.push_str("\n### Resolved Findings (fixed since base)\n");
        out.push_str("| Severity | File | Line | Description |\n");
        out.push_str("|----------|------|-----:|-------------|\n");
        for f in &report.resolved_findings {
            let line_str = f.line.map(|l| l.to_string()).unwrap_or_default();
            out.push_str(&format!(
                "| {} | {} | {} | {} |\n",
                f.severity.to_uppercase(),
                f.file,
                line_str,
                f.description
            ));
        }
    }

    out
}

pub fn format_json(report: &CiReportOutput) -> Result<String> {
    Ok(serde_json::to_string_pretty(report)?)
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

pub fn run_ci_report(base_path: &PathBuf, head_path: &PathBuf, json_output: bool) -> Result<bool> {
    let base_data = std::fs::read_to_string(base_path)
        .map_err(|e| color_eyre::eyre::eyre!("cannot read base file {}: {e}", base_path.display()))?;
    let head_data = std::fs::read_to_string(head_path)
        .map_err(|e| color_eyre::eyre::eyre!("cannot read head file {}: {e}", head_path.display()))?;

    let base: Vec<AuditFinding> = serde_json::from_str(&base_data)
        .map_err(|e| color_eyre::eyre::eyre!("cannot parse base JSON: {e}"))?;
    let head: Vec<AuditFinding> = serde_json::from_str(&head_data)
        .map_err(|e| color_eyre::eyre::eyre!("cannot parse head JSON: {e}"))?;

    let report = compare_findings(&base, &head);

    if json_output {
        println!("{}", format_json(&report)?);
    } else {
        print!("{}", format_markdown(&report));
    }

    Ok(report.has_new_high_critical)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn make_finding(severity: &str, file: &str, line: Option<u32>, detector: &str) -> AuditFinding {
        AuditFinding {
            severity: severity.to_string(),
            file: file.to_string(),
            line,
            title: format!("{detector} finding"),
            description: format!("Description for {detector}"),
            detector: detector.to_string(),
            suggestion: String::new(),
        }
    }

    #[test]
    fn new_findings_detected() {
        let base = vec![make_finding("Medium", "a.py", Some(10), "sql_injection")];
        let head = vec![
            make_finding("Medium", "a.py", Some(10), "sql_injection"),
            make_finding("High", "b.py", Some(42), "xss"),
        ];

        let report = compare_findings(&base, &head);
        assert_eq!(report.new_count, 1);
        assert_eq!(report.resolved_count, 0);
        assert_eq!(report.unchanged_count, 1);
        assert_eq!(report.new_findings[0].file, "b.py");
        assert_eq!(report.new_findings[0].detector, "xss");
    }

    #[test]
    fn resolved_findings_detected() {
        let base = vec![
            make_finding("Medium", "a.py", Some(10), "sql_injection"),
            make_finding("Low", "c.py", Some(18), "missing_timeout"),
        ];
        let head = vec![make_finding("Medium", "a.py", Some(10), "sql_injection")];

        let report = compare_findings(&base, &head);
        assert_eq!(report.new_count, 0);
        assert_eq!(report.resolved_count, 1);
        assert_eq!(report.unchanged_count, 1);
        assert_eq!(report.resolved_findings[0].file, "c.py");
    }

    #[test]
    fn exit_code_high_critical() {
        // No new high/critical → no failure
        let base = vec![];
        let head = vec![make_finding("Low", "a.py", Some(1), "info_leak")];
        let report = compare_findings(&base, &head);
        assert!(!report.has_new_high_critical);

        // New HIGH → failure
        let head2 = vec![make_finding("High", "a.py", Some(1), "sql_injection")];
        let report2 = compare_findings(&base, &head2);
        assert!(report2.has_new_high_critical);

        // New CRITICAL → failure
        let head3 = vec![make_finding("Critical", "a.py", Some(1), "rce")];
        let report3 = compare_findings(&base, &head3);
        assert!(report3.has_new_high_critical);
    }

    #[test]
    fn empty_inputs() {
        let report = compare_findings(&[], &[]);
        assert_eq!(report.new_count, 0);
        assert_eq!(report.resolved_count, 0);
        assert_eq!(report.unchanged_count, 0);
        assert!(!report.has_new_high_critical);
    }

    #[test]
    fn markdown_output_format() {
        let base = vec![make_finding("Medium", "old.py", Some(18), "timeout")];
        let head = vec![make_finding("High", "new.py", Some(42), "sql_injection")];
        let report = compare_findings(&base, &head);
        let md = format_markdown(&report);

        assert!(md.contains("## APEX CI Report"));
        assert!(md.contains("**New findings:** 1"));
        assert!(md.contains("**Resolved:** 1"));
        assert!(md.contains("**Unchanged:** 0"));
        assert!(md.contains("### New Findings"));
        assert!(md.contains("new.py"));
        assert!(md.contains("### Resolved Findings"));
        assert!(md.contains("old.py"));
    }

    #[test]
    fn json_output_format() {
        let base = vec![];
        let head = vec![make_finding("High", "a.py", Some(1), "xss")];
        let report = compare_findings(&base, &head);
        let json = format_json(&report).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed["new_count"], 1);
        assert_eq!(parsed["resolved_count"], 0);
        assert_eq!(parsed["has_new_high_critical"], true);
    }

    #[test]
    fn existing_high_not_flagged() {
        // A HIGH finding that exists in both base and head is unchanged, not new.
        let base = vec![make_finding("High", "a.py", Some(1), "sql_injection")];
        let head = vec![make_finding("High", "a.py", Some(1), "sql_injection")];
        let report = compare_findings(&base, &head);
        assert!(!report.has_new_high_critical);
        assert_eq!(report.new_count, 0);
        assert_eq!(report.unchanged_count, 1);
    }

    #[test]
    fn run_ci_report_reads_files() {
        let dir = tempfile::tempdir().unwrap();
        let base_path = dir.path().join("base.json");
        let head_path = dir.path().join("head.json");

        let base_json = serde_json::json!([
            {"severity": "Medium", "file": "a.py", "line": 10, "title": "t", "description": "d", "detector": "det1", "suggestion": "s"}
        ]);
        let head_json = serde_json::json!([
            {"severity": "Medium", "file": "a.py", "line": 10, "title": "t", "description": "d", "detector": "det1", "suggestion": "s"},
            {"severity": "High", "file": "b.py", "line": 5, "title": "t2", "description": "d2", "detector": "det2", "suggestion": "s"}
        ]);

        std::fs::write(&base_path, serde_json::to_string(&base_json).unwrap()).unwrap();
        std::fs::write(&head_path, serde_json::to_string(&head_json).unwrap()).unwrap();

        let has_new = run_ci_report(&base_path, &head_path, true).unwrap();
        assert!(has_new); // new HIGH finding
    }
}
