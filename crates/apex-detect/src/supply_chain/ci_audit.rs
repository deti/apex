use serde::{Deserialize, Serialize};
use std::path::Path;

/// A GitHub Action reference found in a workflow file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionReference {
    pub workflow_file: String,
    pub action: String,
    pub version_ref: String,
    pub is_sha_pinned: bool,
    pub line: u32,
}

/// Known compromised actions (maintained list).
const KNOWN_COMPROMISED: &[&str] = &[
    "aquasecurity/trivy-action",
    "aquasecurity/setup-trivy",
    "Checkmarx/kics-github-action",
];

/// Result of auditing CI/CD workflow files.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CiAuditReport {
    pub workflow_files_scanned: usize,
    pub total_action_refs: usize,
    pub unpinned_actions: Vec<ActionReference>,
    pub compromised_actions: Vec<ActionReference>,
    pub all_actions: Vec<ActionReference>,
}

/// Audit GitHub Actions workflow files in a project.
pub fn audit_github_actions(project_root: &Path) -> CiAuditReport {
    let workflows_dir = project_root.join(".github").join("workflows");
    let mut all_actions = Vec::new();
    let mut workflow_count = 0;

    let empty_report = CiAuditReport {
        workflow_files_scanned: 0,
        total_action_refs: 0,
        unpinned_actions: vec![],
        compromised_actions: vec![],
        all_actions: vec![],
    };

    if !workflows_dir.exists() {
        return empty_report;
    }

    let entries = match std::fs::read_dir(&workflows_dir) {
        Ok(e) => e,
        Err(_) => return empty_report,
    };

    for entry in entries.flatten() {
        let path = entry.path();
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
        if ext != "yml" && ext != "yaml" {
            continue;
        }
        workflow_count += 1;

        let content = match std::fs::read_to_string(&path) {
            Ok(c) => c,
            Err(_) => continue,
        };

        let rel_path = path
            .strip_prefix(project_root)
            .unwrap_or(&path)
            .to_string_lossy()
            .to_string();

        // Parse "uses:" lines
        // Pattern: uses: owner/repo@ref or uses: owner/repo/path@ref
        for (i, line) in content.lines().enumerate() {
            let trimmed = line.trim();
            if !trimmed.starts_with("uses:") && !trimmed.starts_with("- uses:") {
                continue;
            }

            let uses_value = trimmed
                .trim_start_matches("- ")
                .trim_start_matches("uses:")
                .trim()
                .trim_matches('"')
                .trim_matches('\'');

            // Skip local actions (./...)
            if uses_value.starts_with("./") || uses_value.starts_with(".\\") {
                continue;
            }

            // Split on @
            let parts: Vec<&str> = uses_value.splitn(2, '@').collect();
            if parts.len() != 2 {
                continue;
            }

            let action = parts[0].to_string();
            let version_ref = parts[1].to_string();

            // Check if SHA-pinned: 40-char hex string
            let is_sha =
                version_ref.len() >= 40 && version_ref.chars().all(|c| c.is_ascii_hexdigit());

            all_actions.push(ActionReference {
                workflow_file: rel_path.clone(),
                action,
                version_ref,
                is_sha_pinned: is_sha,
                line: (i + 1) as u32,
            });
        }
    }

    let unpinned: Vec<ActionReference> = all_actions
        .iter()
        .filter(|a| !a.is_sha_pinned)
        .cloned()
        .collect();

    let compromised: Vec<ActionReference> = all_actions
        .iter()
        .filter(|a| {
            KNOWN_COMPROMISED
                .iter()
                .any(|&known| a.action.to_lowercase() == known.to_lowercase())
        })
        .cloned()
        .collect();

    CiAuditReport {
        workflow_files_scanned: workflow_count,
        total_action_refs: all_actions.len(),
        unpinned_actions: unpinned,
        compromised_actions: compromised,
        all_actions,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_uses_line() {
        let dir = tempfile::tempdir().unwrap();
        let wf_dir = dir.path().join(".github").join("workflows");
        std::fs::create_dir_all(&wf_dir).unwrap();
        std::fs::write(
            wf_dir.join("ci.yml"),
            r#"
name: CI
on: push
jobs:
  test:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: actions/setup-python@8d9ed9ac5c53483de85588cdf95a591a75ab9f55
      - uses: aquasecurity/trivy-action@v0.18.0
"#,
        )
        .unwrap();

        let report = audit_github_actions(dir.path());
        assert_eq!(report.workflow_files_scanned, 1);
        assert_eq!(report.total_action_refs, 3);
        assert_eq!(report.unpinned_actions.len(), 2); // checkout@v4 and trivy@v0.18.0
        assert_eq!(report.compromised_actions.len(), 1); // trivy-action

        // setup-python is SHA-pinned
        let pinned: Vec<_> = report
            .all_actions
            .iter()
            .filter(|a| a.is_sha_pinned)
            .collect();
        assert_eq!(pinned.len(), 1);
        assert!(pinned[0].action.contains("setup-python"));
    }

    #[test]
    fn no_workflows_dir() {
        let dir = tempfile::tempdir().unwrap();
        let report = audit_github_actions(dir.path());
        assert_eq!(report.workflow_files_scanned, 0);
        assert_eq!(report.total_action_refs, 0);
    }
}
