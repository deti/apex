//! IaC Security Scanning — checks Terraform/IaC files for misconfigurations.

use regex::Regex;
use serde::Serialize;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::LazyLock;

#[derive(Debug, Clone, Copy, Serialize)]
pub enum IacSeverity {
    Critical,
    High,
    Medium,
    Low,
}

#[derive(Debug, Clone, Serialize)]
pub struct IacIssue {
    pub severity: IacSeverity,
    pub file: PathBuf,
    pub line: u32,
    pub rule: String,
    pub description: String,
    pub suggestion: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct IacReport {
    pub issues: Vec<IacIssue>,
    pub files_scanned: usize,
    pub critical_count: usize,
    pub high_count: usize,
}

static PUBLIC_ACCESS: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r#"(?i)(?:public.access|acl\s*=\s*"public|publicly.accessible\s*=\s*true)"#,
    )
    .unwrap()
});
static NO_ENCRYPTION: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?i)(?:encrypted\s*=\s*false|storage_encrypted\s*=\s*false)"#).unwrap()
});
static OPEN_CIDR: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"(?:0\.0\.0\.0/0|::/0)"#).unwrap());
static WILDCARD_IAM: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r#"(?i)(?:actions?\s*=\s*\[?\s*"\*"|effect\s*=\s*"Allow".*resource\s*=\s*"\*")"#,
    )
    .unwrap()
});
static NO_LOGGING: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r#"(?i)(?:logging\s*=\s*false|enable_logging\s*=\s*false|access_logs\s*\{[^}]*enabled\s*=\s*false)"#,
    )
    .unwrap()
});
static HARDCODED_SECRET: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?i)(?:password|secret_key|api_key)\s*=\s*"[^$][^"]{8,}""#).unwrap()
});

pub fn scan_iac(source_cache: &HashMap<PathBuf, String>) -> IacReport {
    let mut issues = Vec::new();
    let mut files_scanned = 0;

    let rules: Vec<(&LazyLock<Regex>, &str, IacSeverity, &str, &str)> = vec![
        (
            &PUBLIC_ACCESS,
            "no-public-access",
            IacSeverity::Critical,
            "Public access enabled",
            "Disable public access",
        ),
        (
            &NO_ENCRYPTION,
            "encryption-enabled",
            IacSeverity::High,
            "Encryption disabled",
            "Enable encryption at rest",
        ),
        (
            &OPEN_CIDR,
            "no-open-cidr",
            IacSeverity::Critical,
            "Open CIDR 0.0.0.0/0 allows all traffic",
            "Restrict to specific CIDR ranges",
        ),
        (
            &WILDCARD_IAM,
            "no-wildcard-iam",
            IacSeverity::Critical,
            "Wildcard IAM permissions",
            "Follow least-privilege principle",
        ),
        (
            &NO_LOGGING,
            "logging-enabled",
            IacSeverity::Medium,
            "Logging disabled",
            "Enable access logging",
        ),
        (
            &HARDCODED_SECRET,
            "no-hardcoded-secrets",
            IacSeverity::Critical,
            "Hardcoded secret in IaC",
            "Use a secrets manager (Vault, AWS Secrets Manager)",
        ),
    ];

    for (path, source) in source_cache {
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
        if !matches!(ext, "tf" | "hcl" | "json" | "yaml" | "yml") {
            continue;
        }
        files_scanned += 1;

        for (line_num, line) in source.lines().enumerate() {
            let ln = (line_num + 1) as u32;
            for (re, rule, sev, desc, sugg) in &rules {
                if re.is_match(line) {
                    issues.push(IacIssue {
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
    }

    let critical_count = issues
        .iter()
        .filter(|i| matches!(i.severity, IacSeverity::Critical))
        .count();
    let high_count = issues
        .iter()
        .filter(|i| matches!(i.severity, IacSeverity::High))
        .count();

    IacReport {
        issues,
        files_scanned,
        critical_count,
        high_count,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_public_access() {
        let mut c = HashMap::new();
        c.insert(PathBuf::from("main.tf"), r#"acl = "public-read""#.into());
        let r = scan_iac(&c);
        assert_eq!(r.critical_count, 1);
    }

    #[test]
    fn detects_open_cidr() {
        let mut c = HashMap::new();
        c.insert(
            PathBuf::from("sg.tf"),
            r#"cidr_blocks = ["0.0.0.0/0"]"#.into(),
        );
        let r = scan_iac(&c);
        assert!(r.critical_count >= 1);
    }

    #[test]
    fn detects_no_encryption() {
        let mut c = HashMap::new();
        c.insert(PathBuf::from("db.tf"), "encrypted = false".into());
        let r = scan_iac(&c);
        assert_eq!(r.high_count, 1);
    }

    #[test]
    fn skips_non_iac_files() {
        let mut c = HashMap::new();
        c.insert(PathBuf::from("app.py"), r#"acl = "public-read""#.into());
        let r = scan_iac(&c);
        assert_eq!(r.files_scanned, 0);
    }

    #[test]
    fn clean_iac_no_issues() {
        let mut c = HashMap::new();
        c.insert(
            PathBuf::from("main.tf"),
            "resource \"aws_instance\" \"web\" {\n  ami = \"ami-123\"\n}".into(),
        );
        let r = scan_iac(&c);
        assert!(r.issues.is_empty());
    }
}
