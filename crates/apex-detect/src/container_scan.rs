//! Container Image Scanning — checks Dockerfiles for security issues.

use regex::Regex;
use serde::Serialize;
use std::path::PathBuf;
use std::sync::LazyLock;

#[derive(Debug, Clone, Copy, Serialize)]
pub enum ContainerSeverity {
    Critical,
    High,
    Medium,
    Low,
}

#[derive(Debug, Clone, Serialize)]
pub struct ContainerIssue {
    pub severity: ContainerSeverity,
    pub file: PathBuf,
    pub line: u32,
    pub rule: String,
    pub description: String,
    pub suggestion: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ContainerReport {
    pub issues: Vec<ContainerIssue>,
    pub files_scanned: usize,
}

static RUN_AS_ROOT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)^USER\s+root").unwrap());
static LATEST_TAG: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)^FROM\s+\S+:latest").unwrap());
static NO_TAG: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)^FROM\s+(\w+)\s*$").unwrap());
static ADD_INSTEAD_COPY: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)^ADD\s+").unwrap());
static CURL_PIPE_SH: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"curl.*\|\s*(?:sh|bash)").unwrap());
#[allow(dead_code)]
static APT_NO_VERSION: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)apt-get\s+install\s+\w+(?:\s+\w+)*\s*$").unwrap());
static EXPOSE_SSH: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)^EXPOSE\s+22\b").unwrap());

pub fn scan_dockerfile(content: &str, path: &PathBuf) -> ContainerReport {
    let mut issues = Vec::new();
    let mut has_user_directive = false;

    let rules: Vec<(&LazyLock<Regex>, &str, ContainerSeverity, &str, &str)> = vec![
        (
            &RUN_AS_ROOT,
            "no-root-user",
            ContainerSeverity::High,
            "Container runs as root",
            "Add USER directive with non-root user",
        ),
        (
            &LATEST_TAG,
            "no-latest-tag",
            ContainerSeverity::Medium,
            "Using :latest tag — builds are not reproducible",
            "Pin to specific version tag",
        ),
        (
            &NO_TAG,
            "pin-base-image",
            ContainerSeverity::Medium,
            "Base image without tag",
            "Pin to specific version",
        ),
        (
            &ADD_INSTEAD_COPY,
            "use-copy",
            ContainerSeverity::Low,
            "ADD used instead of COPY",
            "Use COPY unless extracting archives",
        ),
        (
            &CURL_PIPE_SH,
            "no-curl-pipe",
            ContainerSeverity::Critical,
            "curl | sh pattern — supply chain risk",
            "Download, verify checksum, then execute",
        ),
        (
            &EXPOSE_SSH,
            "no-ssh",
            ContainerSeverity::High,
            "SSH port exposed in container",
            "Remove SSH — use kubectl exec or docker exec",
        ),
    ];

    for (line_num, line) in content.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let ln = (line_num + 1) as u32;

        if trimmed.to_uppercase().starts_with("USER")
            && !trimmed.to_uppercase().contains("ROOT")
        {
            has_user_directive = true;
        }

        for (re, rule, sev, desc, sugg) in &rules {
            if re.is_match(trimmed) {
                issues.push(ContainerIssue {
                    severity: *sev,
                    file: path.clone(),
                    line: ln,
                    rule: rule.to_string(),
                    description: desc.to_string(),
                    suggestion: sugg.to_string(),
                });
            }
        }
    }

    if !has_user_directive {
        issues.push(ContainerIssue {
            severity: ContainerSeverity::High,
            file: path.clone(),
            line: 0,
            rule: "user-directive".into(),
            description: "No USER directive — container runs as root by default".into(),
            suggestion: "Add USER nonroot:nonroot".into(),
        });
    }

    ContainerReport {
        issues,
        files_scanned: 1,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_latest_tag() {
        let r = scan_dockerfile(
            "FROM node:latest\nRUN npm install",
            &PathBuf::from("Dockerfile"),
        );
        assert!(r.issues.iter().any(|i| i.rule == "no-latest-tag"));
    }

    #[test]
    fn detects_no_user() {
        let r = scan_dockerfile(
            "FROM alpine:3.18\nRUN apk add curl",
            &PathBuf::from("Dockerfile"),
        );
        assert!(r.issues.iter().any(|i| i.rule == "user-directive"));
    }

    #[test]
    fn user_directive_passes() {
        let r = scan_dockerfile(
            "FROM alpine:3.18\nUSER app\nRUN echo hi",
            &PathBuf::from("Dockerfile"),
        );
        assert!(!r.issues.iter().any(|i| i.rule == "user-directive"));
    }

    #[test]
    fn detects_curl_pipe() {
        let r = scan_dockerfile(
            "FROM ubuntu\nRUN curl https://x.com/install.sh | sh",
            &PathBuf::from("Dockerfile"),
        );
        assert!(r.issues.iter().any(|i| i.rule == "no-curl-pipe"));
    }

    #[test]
    fn clean_dockerfile() {
        let r = scan_dockerfile(
            "FROM rust:1.75-slim\nCOPY . .\nUSER app\nCMD [\"./app\"]",
            &PathBuf::from("Dockerfile"),
        );
        assert!(r
            .issues
            .iter()
            .all(|i| i.rule != "no-latest-tag" && i.rule != "no-curl-pipe"));
    }
}
