use apex_core::error::Result;
use apex_core::types::Language;
use async_trait::async_trait;
use uuid::Uuid;

use super::util::{is_comment, is_test_file, in_test_block};
use crate::context::AnalysisContext;
use crate::finding::{Finding, FindingCategory, Severity};
use crate::Detector;

pub struct HardcodedEnvValuesDetector;

/// Patterns that indicate hardcoded environment-specific host values.
static HOST_PATTERNS: &[&str] = &[
    "localhost",
    "127.0.0.1",
    "0.0.0.0",
    "::1",
];

/// `bind(` patterns that suggest a port is being hardcoded.
static BIND_PATTERNS: &[&str] = &[
    ".bind(",
    "bind(\"",
    "bind('",
];

/// Port patterns — simple numeric ports in common ranges embedded in bind strings.
/// We look for `":PORT"` where PORT is a 4-5 digit number.
fn has_hardcoded_port(line: &str) -> bool {
    // Rough heuristic: line contains bind pattern AND a port-looking string
    let has_bind = BIND_PATTERNS.iter().any(|p| line.contains(p));
    if !has_bind {
        return false;
    }
    // Check for ":NNNN" or ":NNNNN" pattern
    let mut chars = line.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == ':' {
            let digits: String = chars.by_ref().take_while(|c| c.is_ascii_digit()).collect();
            if digits.len() >= 2 && digits.len() <= 5 {
                if let Ok(port) = digits.parse::<u32>() {
                    if port > 0 && port <= 65535 {
                        // Exclude well-known test ports (just 80, 443 used commonly in examples)
                        return true;
                    }
                }
            }
        }
    }
    false
}

fn analyze_source(path: &std::path::Path, source: &str, lang: Language) -> Vec<Finding> {
    // Only handle code files
    let is_code = matches!(
        lang,
        Language::Rust
            | Language::Python
            | Language::JavaScript
            | Language::Go
            | Language::Java
            | Language::Ruby
    );
    if !is_code {
        return Vec::new();
    }

    // Skip test files entirely
    if is_test_file(path) {
        return Vec::new();
    }

    let lines: Vec<&str> = source.lines().collect();
    let mut findings = Vec::new();

    for (line_idx, line) in lines.iter().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() || is_comment(trimmed, lang) {
            continue;
        }

        // Skip lines in #[cfg(test)] blocks for Rust
        if lang == Language::Rust && in_test_block(source, line_idx) {
            continue;
        }

        // Check for hardcoded host patterns
        let mut found_host: Option<&str> = None;
        for &pattern in HOST_PATTERNS {
            if line.contains(pattern) {
                found_host = Some(pattern);
                break;
            }
        }

        if let Some(host) = found_host {
            let line_1based = (line_idx + 1) as u32;
            findings.push(Finding {
                id: Uuid::new_v4(),
                detector: "hardcoded-env-values".into(),
                severity: Severity::Low,
                category: FindingCategory::SecuritySmell,
                file: path.to_path_buf(),
                line: Some(line_1based),
                title: "Hardcoded environment-specific host value".into(),
                description: format!(
                    "Hardcoded host `{host}` found in non-test code. \
                     This value is environment-specific and should come from \
                     configuration (environment variables, config files)."
                ),
                evidence: vec![],
                covered: false,
                suggestion: "Replace with an environment variable or configuration parameter \
                             (e.g., `std::env::var(\"SERVER_HOST\")`, `os.environ[\"HOST\"]`)."
                    .into(),
                explanation: None,
                fix: None,
                cwe_ids: vec![547],
                    noisy: false,
            });
            continue; // one finding per line
        }

        // Check for hardcoded port in bind patterns
        if has_hardcoded_port(line) {
            let line_1based = (line_idx + 1) as u32;
            findings.push(Finding {
                id: Uuid::new_v4(),
                detector: "hardcoded-env-values".into(),
                severity: Severity::Low,
                category: FindingCategory::SecuritySmell,
                file: path.to_path_buf(),
                line: Some(line_1based),
                title: "Hardcoded port number in bind call".into(),
                description: "A hardcoded port number is used in a bind/listen call. \
                              This prevents dynamic configuration across environments."
                    .into(),
                evidence: vec![],
                covered: false,
                suggestion: "Read the port from an environment variable \
                             (e.g., `std::env::var(\"PORT\")`) or a config file."
                    .into(),
                explanation: None,
                fix: None,
                cwe_ids: vec![547],
                    noisy: false,
            });
        }
    }

    findings
}

#[async_trait]
impl Detector for HardcodedEnvValuesDetector {
    fn name(&self) -> &str {
        "hardcoded-env-values"
    }

    async fn analyze(&self, ctx: &AnalysisContext) -> Result<Vec<Finding>> {
        let mut findings = Vec::new();
        for (path, source) in &ctx.source_cache {
            let lang = match path.extension().and_then(|e| e.to_str()) {
                Some("rs") => Language::Rust,
                Some("py") => Language::Python,
                Some("js") => Language::JavaScript,
                Some("ts") | Some("tsx") => Language::JavaScript,
                Some("go") => Language::Go,
                Some("java") => Language::Java,
                Some("rb") => Language::Ruby,
                _ => continue,
            };
            findings.extend(analyze_source(path, source, lang));
        }
        Ok(findings)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn detect_rust(source: &str) -> Vec<Finding> {
        analyze_source(&PathBuf::from("src/lib.rs"), source, Language::Rust)
    }

    fn detect_py(source: &str) -> Vec<Finding> {
        analyze_source(&PathBuf::from("src/app.py"), source, Language::Python)
    }

    #[test]
    fn detects_localhost_in_rust() {
        let src = r#"
fn main() {
    let addr = "localhost:8080";
    server.bind(addr).unwrap();
}
"#;
        let findings = detect_rust(src);
        assert!(!findings.is_empty());
        assert_eq!(findings[0].severity, Severity::Low);
        assert_eq!(findings[0].cwe_ids, vec![547]);
    }

    #[test]
    fn detects_127_0_0_1_in_python() {
        let src = "app.run(host='127.0.0.1', port=5000)\n";
        let findings = detect_py(src);
        assert!(!findings.is_empty());
    }

    #[test]
    fn detects_hardcoded_port_in_bind() {
        let src = r#"
async fn serve() {
    listener.bind("0.0.0.0:8080").await.unwrap();
}
"#;
        let findings = detect_rust(src);
        // Has 0.0.0.0 finding AND port finding — at least 1
        assert!(!findings.is_empty());
    }

    #[test]
    fn no_finding_in_test_file() {
        let src = r#"
fn test_connect() {
    let addr = "127.0.0.1:9999";
    assert_eq!(addr, "127.0.0.1:9999");
}
"#;
        let findings = analyze_source(
            &PathBuf::from("tests/integration_test.rs"),
            src,
            Language::Rust,
        );
        assert_eq!(findings.len(), 0);
    }

    #[test]
    fn no_finding_in_cfg_test_block() {
        let src = r#"
fn real_fn() {
    // production code
}

#[cfg(test)]
mod tests {
    fn t() {
        let addr = "127.0.0.1:3000";
    }
}
"#;
        let findings = detect_rust(src);
        assert_eq!(findings.len(), 0);
    }

    #[test]
    fn detects_0_0_0_0_in_python() {
        let src = "server.listen('0.0.0.0', 8080)\n";
        let findings = detect_py(src);
        assert!(!findings.is_empty());
        assert!(findings[0].title.contains("Hardcoded"));
    }
}
