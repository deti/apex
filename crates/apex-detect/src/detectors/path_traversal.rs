//! Path traversal detector — identifies unsanitized file path access (CWE-22).

use crate::finding::{Finding, FindingCategory, Severity};
use regex::Regex;
use std::path::PathBuf;
use std::sync::LazyLock;
use uuid::Uuid;

// Pattern: open() with a variable (not a string literal).
static OPEN_VAR_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"open\(\s*([a-zA-Z_][a-zA-Z0-9_.]*)\s*[,)]"#).unwrap());
// Pattern: Path() with a variable.
static PATH_VAR_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"Path\(\s*([a-zA-Z_][a-zA-Z0-9_.]*)\s*\)"#).unwrap());
// Pattern: os.path.join with variable.
static PATH_JOIN_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"os\.path\.join\([^)]*[a-zA-Z_][a-zA-Z0-9_.]*[^)]*\)"#).unwrap());
static OPEN_LITERAL_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"open\(\s*(?:f["']|["'])"#).unwrap());

/// Sanitization indicators that suggest path is validated.
const PATH_SANITIZATION: &[&str] = &["resolve", "realpath", "abspath", "normpath", "canonicalize"];

/// Variable prefixes that suggest non-user-input (safe).
const SAFE_VAR_PREFIXES: &[&str] = &["self.", "config", "BASE", "ROOT"];

/// Scan source code for path traversal vulnerabilities.
pub fn scan_path_traversal(source: &str, file_path: &str) -> Vec<Finding> {
    let mut findings = Vec::new();
    let lines: Vec<&str> = source.lines().collect();

    for (line_num, line) in lines.iter().enumerate() {
        let line_1based = (line_num + 1) as u32;
        let trimmed = line.trim();
        let is_string_only =
            OPEN_LITERAL_RE.is_match(trimmed) && !trimmed.contains('+') && !trimmed.contains('{');

        let mut is_vuln = false;
        let mut captured_arg: Option<&str> = None;

        if let Some(cap) = OPEN_VAR_RE.captures(trimmed) {
            let arg = cap.get(1).map(|m| m.as_str()).unwrap_or("");
            // Skip if the argument looks like a string literal (starts with quote).
            if !arg.starts_with('\'') && !arg.starts_with('"') && !is_string_only {
                is_vuln = true;
                captured_arg = Some(arg);
            }
        }

        if let Some(cap) = PATH_VAR_RE.captures(trimmed) {
            let arg = cap.get(1).map(|m| m.as_str()).unwrap_or("");
            is_vuln = true;
            if captured_arg.is_none() {
                captured_arg = Some(arg);
            }
        }

        if PATH_JOIN_RE.is_match(trimmed) {
            is_vuln = true;
        }

        // Skip if the variable name suggests non-user-input.
        if is_vuln {
            if let Some(arg) = captured_arg {
                if SAFE_VAR_PREFIXES.iter().any(|p| arg.starts_with(p)) {
                    continue;
                }
            }
        }

        // Skip if sanitization is present within 5 lines.
        if is_vuln {
            let ctx_start = line_num.saturating_sub(5);
            let ctx_end = (line_num + 5).min(lines.len().saturating_sub(1));
            let has_sanitization = PATH_SANITIZATION
                .iter()
                .any(|s| lines[ctx_start..=ctx_end].iter().any(|l| l.contains(s)));
            if has_sanitization {
                continue;
            }
        }

        if is_vuln {
            findings.push(Finding {
                id: Uuid::new_v4(),
                detector: "path_traversal".into(),
                severity: Severity::High,
                category: FindingCategory::PathTraversal,
                file: PathBuf::from(file_path),
                line: Some(line_1based),
                title: "Potential path traversal via unsanitized file path".into(),
                description: format!(
                    "File operation at line {line_1based} uses a variable that may \
                     contain user-controlled path components like '../'."
                ),
                evidence: vec![],
                covered: false,
                suggestion: "Validate and sanitize the path. Use os.path.realpath() and \
                             verify the result is within the expected directory."
                    .into(),
                explanation: None,
                fix: None,
                cwe_ids: vec![22],
                    noisy: false,
            });
        }
    }

    findings
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_open_with_user_input() {
        let source = r#"
def download(request):
    filename = request.args.get('file')
    with open(filename) as f:
        return f.read()
"#;
        let findings = scan_path_traversal(source, "views.py");
        assert!(!findings.is_empty());
        assert_eq!(findings[0].category, FindingCategory::PathTraversal);
    }

    #[test]
    fn detect_os_path_join_with_user_input() {
        let source = r#"
import os
def serve(name):
    path = os.path.join('/uploads', name)
    return open(path).read()
"#;
        let findings = scan_path_traversal(source, "serve.py");
        // os.path.join with user input is suspicious.
        assert!(!findings.is_empty());
    }

    #[test]
    fn safe_hardcoded_path_not_flagged() {
        let source = r#"
def read_config():
    with open('/etc/app/config.json') as f:
        return json.load(f)
"#;
        let findings = scan_path_traversal(source, "config.py");
        assert!(findings.is_empty());
    }

    #[test]
    fn detect_pathlib_with_variable() {
        let source = r#"
from pathlib import Path
def load(user_path):
    return Path(user_path).read_text()
"#;
        let findings = scan_path_traversal(source, "loader.py");
        assert!(!findings.is_empty());
    }

    #[test]
    fn finding_has_cwe_22() {
        let source = "f = open(user_input)\ndata = f.read()";
        let findings = scan_path_traversal(source, "x.py");
        if !findings.is_empty() {
            assert!(findings[0].cwe_ids.contains(&22));
        }
    }

    #[test]
    fn skip_when_realpath_nearby() {
        let source = r#"
safe = os.path.realpath(user_input)
f = open(safe)
"#;
        let findings = scan_path_traversal(source, "safe.py");
        assert!(
            findings.is_empty(),
            "realpath nearby should suppress finding"
        );
    }

    #[test]
    fn skip_safe_variable_prefixes() {
        let source = "f = open(config_path)\n";
        let findings = scan_path_traversal(source, "app.py");
        assert!(
            findings.is_empty(),
            "config* variable should not be flagged"
        );

        let source2 = "data = Path(self.base_dir).read_text()\n";
        let findings2 = scan_path_traversal(source2, "app.py");
        assert!(
            findings2.is_empty(),
            "self.* variable should not be flagged"
        );
    }

    #[test]
    fn still_flags_user_input_without_sanitization() {
        let source = "f = open(user_path)\n";
        let findings = scan_path_traversal(source, "handler.py");
        assert!(
            !findings.is_empty(),
            "user_path without sanitization should be flagged"
        );
    }
}
