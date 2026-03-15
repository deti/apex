//! Command injection detector — identifies unsanitized shell execution (CWE-78).

use crate::finding::{Finding, FindingCategory, Severity};
use regex::Regex;
use std::path::PathBuf;
use std::sync::LazyLock;
use uuid::Uuid;

/// Shell execution function patterns that are dangerous with user input.
const DANGEROUS_FUNCS: &[&str] = &[
    "os.system(",
    "os.popen(",
    "commands.getoutput(",
    "commands.getstatusoutput(",
];

static SHELL_TRUE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"subprocess\.\w+\([^)]*shell\s*=\s*True"#).unwrap());

/// Scan source code for command injection vulnerabilities.
pub fn scan_command_injection(source: &str, file_path: &str) -> Vec<Finding> {
    let mut findings = Vec::new();

    for (line_num, line) in source.lines().enumerate() {
        let line_1based = (line_num + 1) as u32;
        let trimmed = line.trim();

        let mut is_vuln = false;

        // Check direct dangerous function calls.
        for func in DANGEROUS_FUNCS {
            if trimmed.contains(func) {
                is_vuln = true;
                break;
            }
        }

        // Check subprocess with shell=True.
        if SHELL_TRUE_RE.is_match(trimmed) {
            is_vuln = true;
        }

        if is_vuln {
            findings.push(Finding {
                id: Uuid::new_v4(),
                detector: "command_injection".into(),
                severity: Severity::Critical,
                category: FindingCategory::Injection,
                file: PathBuf::from(file_path),
                line: Some(line_1based),
                title: "Potential command injection via shell execution".into(),
                description: format!(
                    "Shell command execution at line {line_1based} may allow \
                     command injection if input is user-controlled."
                ),
                evidence: vec![],
                covered: false,
                suggestion: "Use subprocess.run() with a list of arguments (no shell=True). \
                             Never pass unsanitized user input to shell commands."
                    .into(),
                explanation: None,
                fix: None,
                cwe_ids: vec![78],
            });
        }
    }

    findings
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_os_system() {
        let source = r#"
import os
def run_cmd(user_cmd):
    os.system(user_cmd)
"#;
        let findings = scan_command_injection(source, "cmd.py");
        assert!(!findings.is_empty());
        assert_eq!(findings[0].category, FindingCategory::Injection);
    }

    #[test]
    fn detect_subprocess_shell_true() {
        let source = r#"
import subprocess
def run(cmd):
    subprocess.call(cmd, shell=True)
"#;
        let findings = scan_command_injection(source, "run.py");
        assert!(!findings.is_empty());
    }

    #[test]
    fn detect_os_popen() {
        let source = r#"
def execute(cmd):
    os.popen(cmd)
"#;
        let findings = scan_command_injection(source, "exec.py");
        assert!(!findings.is_empty());
    }

    #[test]
    fn safe_subprocess_without_shell() {
        let source = r#"
import subprocess
def run_safe(args):
    subprocess.run(["ls", "-la"])
"#;
        let findings = scan_command_injection(source, "safe.py");
        assert!(findings.is_empty());
    }

    #[test]
    fn finding_has_cwe_78() {
        let source = "os.system(cmd)";
        let findings = scan_command_injection(source, "x.py");
        assert!(!findings.is_empty());
        assert!(findings[0].cwe_ids.contains(&78));
    }
}
