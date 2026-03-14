//! Runbook Validation — checks runbooks against actual code.

use regex::Regex;
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

#[derive(Debug, Clone, Serialize)]
pub struct RunbookIssue {
    pub file: PathBuf,
    pub line: u32,
    pub issue: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct RunbookReport {
    pub issues: Vec<RunbookIssue>,
    pub paths_checked: usize,
    pub stale_paths: usize,
}

static FILE_PATH: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?:^|\s)(/[\w./\-]+(?:\.\w+)+)").unwrap());

pub fn validate_runbook(content: &str, runbook_path: &Path, project_root: &Path) -> RunbookReport {
    let mut issues = Vec::new();
    let mut paths_checked = 0usize;
    let mut stale_paths = 0usize;

    for (line_num, line) in content.lines().enumerate() {
        let ln = (line_num + 1) as u32;

        // Check file paths referenced in the runbook
        for cap in FILE_PATH.captures_iter(line) {
            let path_str = &cap[1];
            paths_checked += 1;
            // Check relative to project root
            let full_path = project_root.join(path_str.trim_start_matches('/'));
            if !full_path.exists()
                && !path_str.starts_with("/dev/")
                && !path_str.starts_with("/tmp/")
            {
                issues.push(RunbookIssue {
                    file: runbook_path.to_path_buf(),
                    line: ln,
                    issue: format!("Referenced path '{}' does not exist", path_str),
                });
                stale_paths += 1;
            }
        }
    }

    RunbookReport {
        issues,
        paths_checked,
        stale_paths,
    }
}

/// Validate all markdown files in a runbooks directory.
pub fn validate_runbooks_dir(dir: &Path, project_root: &Path) -> Vec<RunbookReport> {
    let mut reports = Vec::new();
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().map(|e| e == "md").unwrap_or(false) {
                if let Ok(content) = std::fs::read_to_string(&path) {
                    reports.push(validate_runbook(&content, &path, project_root));
                }
            }
        }
    }
    reports
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_stale_path() {
        let content = "Run the script at /nonexistent/path/script.sh";
        let report = validate_runbook(content, Path::new("runbook.md"), Path::new("/tmp"));
        assert_eq!(report.stale_paths, 1);
    }

    #[test]
    fn ignores_dev_tmp_paths() {
        let content = "Output goes to /dev/null and /tmp/test.log";
        let report = validate_runbook(content, Path::new("runbook.md"), Path::new("/tmp"));
        assert_eq!(report.stale_paths, 0);
    }

    #[test]
    fn empty_runbook_no_issues() {
        let report = validate_runbook(
            "# My Runbook\nNo paths here.",
            Path::new("r.md"),
            Path::new("/tmp"),
        );
        assert_eq!(report.issues.len(), 0);
    }

    #[test]
    fn report_serializes() {
        let report = RunbookReport {
            issues: vec![],
            paths_checked: 0,
            stale_paths: 0,
        };
        assert!(serde_json::to_string(&report).is_ok());
    }
}
