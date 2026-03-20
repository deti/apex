use apex_core::error::Result;
use apex_core::types::Language;
use async_trait::async_trait;
use regex::Regex;
use std::sync::LazyLock;
use uuid::Uuid;

use super::util::{is_comment, is_test_file};
use crate::context::AnalysisContext;
use crate::finding::{Finding, FindingCategory, Severity};
use crate::Detector;

pub struct SessionSecurityDetector;

// Python patterns for hardcoded secret keys
static PY_APP_SECRET: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"app\.secret_key\s*=\s*['"]"#).expect("invalid regex"));

static PY_SECRET_KEY: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"SECRET_KEY\s*=\s*['"]"#).expect("invalid regex"));

const PY_ENV_MARKERS: &[&str] = &["os.environ", "os.getenv", "config[", "settings."];

// JavaScript patterns
static JS_SESSION_CALL: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"session\s*\(").expect("invalid regex"));

static JS_COOKIE_SESSION: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"require\s*\(\s*['"]cookie-session['"]\s*\)"#).expect("invalid regex")
});

static JS_HARDCODED_SECRET: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"secret\s*:\s*['"][^'"]+['"]"#).expect("invalid regex"));

fn py_uses_env(line: &str) -> bool {
    PY_ENV_MARKERS.iter().any(|m| line.contains(m))
}

fn collect_session_block(lines: &[&str], start: usize) -> (String, u32, u32) {
    // Collect lines from start until we find the matching closing paren
    let mut block = String::new();
    let mut depth: i32 = 0;
    let mut started = false;
    let end_line = start;

    for (i, &line) in lines[start..].iter().enumerate() {
        block.push_str(line);
        block.push('\n');

        for ch in line.chars() {
            match ch {
                '(' => {
                    depth += 1;
                    started = true;
                }
                ')' => {
                    depth -= 1;
                    if started && depth <= 0 {
                        return (block, (start + 1) as u32, (start + i + 1) as u32);
                    }
                }
                _ => {}
            }
        }
    }

    (block, (start + 1) as u32, (end_line + 1) as u32)
}

#[async_trait]
impl Detector for SessionSecurityDetector {
    fn name(&self) -> &str {
        "session-security"
    }

    async fn analyze(&self, ctx: &AnalysisContext) -> Result<Vec<Finding>> {
        let mut findings = Vec::new();

        for (path, source) in &ctx.source_cache {
            if is_test_file(path) {
                continue;
            }

            match ctx.language {
                Language::Python => {
                    for (line_num, line) in source.lines().enumerate() {
                        let trimmed = line.trim();
                        if is_comment(trimmed, Language::Python) {
                            continue;
                        }
                        if py_uses_env(trimmed) {
                            continue;
                        }

                        let line_1based = (line_num + 1) as u32;

                        if PY_APP_SECRET.is_match(trimmed) || PY_SECRET_KEY.is_match(trimmed) {
                            findings.push(Finding {
                                id: Uuid::new_v4(),
                                detector: self.name().into(),
                                severity: Severity::High,
                                category: FindingCategory::InsecureConfig,
                                file: path.clone(),
                                line: Some(line_1based),
                                title: "Hardcoded session secret key".into(),
                                description: format!(
                                    "Secret key is hardcoded as a string literal in {}:{}. \
                                     Use environment variables or a secrets manager instead.",
                                    path.display(),
                                    line_1based
                                ),
                                evidence: vec![],
                                covered: false,
                                suggestion: "Load the secret key from an environment variable (e.g., os.environ['SECRET_KEY'])".into(),
                                explanation: None,
                                fix: None,
                                cwe_ids: vec![798],
                    noisy: false,
                            });
                        }
                    }
                }
                Language::JavaScript => {
                    let lines: Vec<&str> = source.lines().collect();
                    let mut i = 0;
                    while i < lines.len() {
                        let trimmed = lines[i].trim();
                        if is_comment(trimmed, Language::JavaScript) {
                            i += 1;
                            continue;
                        }

                        // Detect session( or cookie-session usage
                        let has_session_call = JS_SESSION_CALL.is_match(trimmed);
                        let has_cookie_session = JS_COOKIE_SESSION.is_match(trimmed);

                        if has_session_call || has_cookie_session {
                            let (block, start_line, _end_line) = collect_session_block(&lines, i);

                            // Check for hardcoded secret
                            if JS_HARDCODED_SECRET.is_match(&block) {
                                // Find the exact line with the secret
                                let secret_line = block
                                    .lines()
                                    .enumerate()
                                    .find(|(_, l)| JS_HARDCODED_SECRET.is_match(l));
                                let finding_line = secret_line
                                    .map(|(offset, _)| start_line + offset as u32)
                                    .unwrap_or(start_line);

                                findings.push(Finding {
                                    id: Uuid::new_v4(),
                                    detector: self.name().into(),
                                    severity: Severity::High,
                                    category: FindingCategory::InsecureConfig,
                                    file: path.clone(),
                                    line: Some(finding_line),
                                    title: "Hardcoded session secret".into(),
                                    description: format!(
                                        "Session secret is hardcoded in {}:{}. \
                                         Use environment variables instead.",
                                        path.display(),
                                        finding_line
                                    ),
                                    evidence: vec![],
                                    covered: false,
                                    suggestion: "Use process.env.SESSION_SECRET instead of a hardcoded string".into(),
                                    explanation: None,
                                    fix: None,
                                    cwe_ids: vec![798],
                    noisy: false,
                                });
                            }

                            // Check for missing security flags
                            let block_lower = block.to_lowercase();

                            if !block_lower.contains("secure")
                                || !block_lower.contains("secure: true")
                            {
                                findings.push(Finding {
                                    id: Uuid::new_v4(),
                                    detector: self.name().into(),
                                    severity: Severity::Medium,
                                    category: FindingCategory::InsecureConfig,
                                    file: path.clone(),
                                    line: Some(start_line),
                                    title: "Session cookie missing secure flag".into(),
                                    description: format!(
                                        "Session configuration in {}:{} does not set secure: true. \
                                         Cookies may be sent over unencrypted connections.",
                                        path.display(),
                                        start_line
                                    ),
                                    evidence: vec![],
                                    covered: false,
                                    suggestion:
                                        "Add secure: true to the session cookie configuration"
                                            .into(),
                                    explanation: None,
                                    fix: None,
                                    cwe_ids: vec![614],
                    noisy: false,
                                });
                            }

                            if !block_lower.contains("httponly")
                                || !block_lower.contains("httponly: true")
                            {
                                findings.push(Finding {
                                    id: Uuid::new_v4(),
                                    detector: self.name().into(),
                                    severity: Severity::Medium,
                                    category: FindingCategory::InsecureConfig,
                                    file: path.clone(),
                                    line: Some(start_line),
                                    title: "Session cookie missing httpOnly flag".into(),
                                    description: format!(
                                        "Session configuration in {}:{} does not set httpOnly: true. \
                                         Cookies may be accessible to client-side scripts.",
                                        path.display(),
                                        start_line
                                    ),
                                    evidence: vec![],
                                    covered: false,
                                    suggestion: "Add httpOnly: true to the session cookie configuration".into(),
                                    explanation: None,
                                    fix: None,
                                    cwe_ids: vec![1004],
                    noisy: false,
                                });
                            }

                            if !block_lower.contains("samesite") {
                                findings.push(Finding {
                                    id: Uuid::new_v4(),
                                    detector: self.name().into(),
                                    severity: Severity::Low,
                                    category: FindingCategory::InsecureConfig,
                                    file: path.clone(),
                                    line: Some(start_line),
                                    title: "Session cookie missing sameSite attribute".into(),
                                    description: format!(
                                        "Session configuration in {}:{} does not set sameSite. \
                                         This may leave the application vulnerable to CSRF attacks.",
                                        path.display(),
                                        start_line
                                    ),
                                    evidence: vec![],
                                    covered: false,
                                    suggestion: "Add sameSite: 'strict' or sameSite: 'lax' to the session cookie configuration".into(),
                                    explanation: None,
                                    fix: None,
                                    cwe_ids: vec![352],
                    noisy: false,
                                });
                            }
                        }

                        i += 1;
                    }
                }
                _ => {}
            }
        }

        Ok(findings)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::DetectConfig;
    use crate::context::AnalysisContext;
    use apex_core::types::Language;
    use apex_coverage::CoverageOracle;
    use std::collections::HashMap;
    use std::path::PathBuf;
    use std::sync::Arc;

    fn make_ctx(filename: &str, source: &str, lang: Language) -> AnalysisContext {
        let mut files = HashMap::new();
        files.insert(PathBuf::from(filename), source.to_string());
        AnalysisContext {
            language: lang,
            source_cache: files,
            ..AnalysisContext::test_default()
        }
    }

    #[tokio::test]
    async fn detects_flask_hardcoded_secret_key() {
        let ctx = make_ctx(
            "src/app.py",
            "from flask import Flask\napp = Flask(__name__)\napp.secret_key = 'super-secret'\n",
            Language::Python,
        );
        let findings = SessionSecurityDetector.analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, Severity::High);
        assert_eq!(findings[0].category, FindingCategory::InsecureConfig);
        assert_eq!(findings[0].line, Some(3));
    }

    #[tokio::test]
    async fn detects_django_secret_key() {
        let ctx = make_ctx(
            "src/settings.py",
            "DEBUG = True\nSECRET_KEY = 'my-secret'\n",
            Language::Python,
        );
        let findings = SessionSecurityDetector.analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, Severity::High);
        assert_eq!(findings[0].line, Some(2));
    }

    #[tokio::test]
    async fn no_finding_when_env_var_used() {
        let ctx = make_ctx(
            "src/app.py",
            "import os\napp.secret_key = os.environ.get('SECRET_KEY')\n",
            Language::Python,
        );
        let findings = SessionSecurityDetector.analyze(&ctx).await.unwrap();
        assert!(findings.is_empty());
    }

    #[tokio::test]
    async fn no_finding_when_config_used() {
        let ctx = make_ctx(
            "src/settings.py",
            "SECRET_KEY = config['SECRET_KEY']\n",
            Language::Python,
        );
        let findings = SessionSecurityDetector.analyze(&ctx).await.unwrap();
        assert!(findings.is_empty());
    }

    #[tokio::test]
    async fn detects_express_session_hardcoded_secret() {
        let ctx = make_ctx(
            "src/app.js",
            "const session = require('express-session');\napp.use(session({\n  secret: 'keyboard cat',\n  resave: false\n}));\n",
            Language::JavaScript,
        );
        let findings = SessionSecurityDetector.analyze(&ctx).await.unwrap();
        // Should detect: hardcoded secret, missing secure, missing httpOnly, missing sameSite
        let hardcoded: Vec<_> = findings
            .iter()
            .filter(|f| f.severity == Severity::High)
            .collect();
        assert!(!hardcoded.is_empty(), "should detect hardcoded secret");
    }

    #[tokio::test]
    async fn detects_missing_secure_flag() {
        let ctx = make_ctx(
            "src/app.js",
            "app.use(session({\n  secret: process.env.SECRET,\n  httpOnly: true,\n  sameSite: 'strict'\n}));\n",
            Language::JavaScript,
        );
        let findings = SessionSecurityDetector.analyze(&ctx).await.unwrap();
        let secure_findings: Vec<_> = findings
            .iter()
            .filter(|f| f.title.contains("secure flag"))
            .collect();
        assert_eq!(secure_findings.len(), 1);
        assert_eq!(secure_findings[0].severity, Severity::Medium);
    }

    #[tokio::test]
    async fn detects_missing_httponly() {
        let ctx = make_ctx(
            "src/app.js",
            "app.use(session({\n  secret: process.env.SECRET,\n  secure: true,\n  sameSite: 'strict'\n}));\n",
            Language::JavaScript,
        );
        let findings = SessionSecurityDetector.analyze(&ctx).await.unwrap();
        let httponly_findings: Vec<_> = findings
            .iter()
            .filter(|f| f.title.contains("httpOnly"))
            .collect();
        assert_eq!(httponly_findings.len(), 1);
        assert_eq!(httponly_findings[0].severity, Severity::Medium);
    }

    #[tokio::test]
    async fn no_finding_for_secure_session() {
        let ctx = make_ctx(
            "src/app.js",
            "app.use(session({\n  secret: process.env.SECRET,\n  cookie: {\n    secure: true,\n    httpOnly: true,\n    sameSite: 'strict'\n  }\n}));\n",
            Language::JavaScript,
        );
        let findings = SessionSecurityDetector.analyze(&ctx).await.unwrap();
        // No hardcoded secret (uses env), and all security flags present
        assert!(
            findings.is_empty(),
            "expected no findings for secure session config, got: {:?}",
            findings.iter().map(|f| &f.title).collect::<Vec<_>>()
        );
    }

    #[tokio::test]
    async fn skips_test_files() {
        let ctx = make_ctx(
            "tests/test_app.py",
            "app.secret_key = 'test-secret'\n",
            Language::Python,
        );
        let findings = SessionSecurityDetector.analyze(&ctx).await.unwrap();
        assert!(findings.is_empty());
    }

    #[test]
    fn detector_name() {
        assert_eq!(SessionSecurityDetector.name(), "session-security");
    }
}
