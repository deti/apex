//! Cross-Site Request Forgery (CSRF) detector (CWE-352).
//!
//! Detects state-changing HTTP handlers missing CSRF protection across
//! Python (Django/Flask), JavaScript (Express), Java (Spring), Ruby (Rails),
//! and Go web frameworks.

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

pub struct CsrfDetector;

// ── Django ──────────────────────────────────────────────────────────────

static DJANGO_CSRF_EXEMPT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"@csrf_exempt").expect("invalid regex"));

static DJANGO_POST_HANDLER: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"request\.method\s*==\s*['"]POST['"]"#).expect("invalid regex")
});

// ── Flask ───────────────────────────────────────────────────────────────

static FLASK_METHODS_POST: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"methods\s*=\s*\[.*['"]POST['"]"#).expect("invalid regex")
});

// ── Express ─────────────────────────────────────────────────────────────

static EXPRESS_MUTATION_ROUTE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"\.\s*(?:post|put|delete|patch)\s*\("#).expect("invalid regex")
});

// ── Spring ──────────────────────────────────────────────────────────────

static SPRING_CSRF_DISABLE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"csrf\s*\(\s*\)\s*\.\s*disable\s*\("#).expect("invalid regex")
});

// ── Rails ───────────────────────────────────────────────────────────────

static RAILS_SKIP_CSRF: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"skip_before_action\s+:verify_authenticity_token").expect("invalid regex")
});

// ── Go ──────────────────────────────────────────────────────────────────

static GO_HANDLE_FUNC: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?:HandleFunc|Handle)\s*\("#).expect("invalid regex")
});

static GO_POST_METHOD: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"r\.Method\s*==\s*"POST"|\.Methods\s*\(\s*"POST""#).expect("invalid regex")
});

/// CSRF middleware/protection indicators per framework.
const CSRF_PROTECTION_INDICATORS: &[&str] = &[
    "CsrfViewMiddleware",
    "CSRFProtect",
    "csrf_protect",
    "csurf",
    "csrf(",
    "csrf_token",
    "csrftoken",
    "gorilla/csrf",
    "nosurf",
    "csrf.Protect",
    "AntiForgeryToken",
    "_csrf",
    "authenticity_token",
];

fn file_has_csrf_protection(source: &str) -> bool {
    let lower = source.to_lowercase();
    CSRF_PROTECTION_INDICATORS
        .iter()
        .any(|ind| lower.contains(&ind.to_lowercase()))
}

#[async_trait]
impl Detector for CsrfDetector {
    fn name(&self) -> &str {
        "csrf"
    }

    async fn analyze(&self, ctx: &AnalysisContext) -> Result<Vec<Finding>> {
        let mut findings = Vec::new();

        for (path, source) in &ctx.source_cache {
            if is_test_file(path) {
                continue;
            }

            match ctx.language {
                Language::Python => {
                    // Check for @csrf_exempt decorator
                    for (line_num, line) in source.lines().enumerate() {
                        let trimmed = line.trim();
                        if is_comment(trimmed, Language::Python) {
                            continue;
                        }
                        let line_1based = (line_num + 1) as u32;

                        if DJANGO_CSRF_EXEMPT.is_match(trimmed) {
                            findings.push(Finding {
                                id: Uuid::new_v4(),
                                detector: self.name().into(),
                                severity: Severity::High,
                                category: FindingCategory::SecuritySmell,
                                file: path.clone(),
                                line: Some(line_1based),
                                title: "CSRF protection explicitly disabled".into(),
                                description: format!(
                                    "@csrf_exempt disables CSRF protection on handler at {}:{}",
                                    path.display(),
                                    line_1based
                                ),
                                evidence: vec![],
                                covered: false,
                                suggestion: "Remove @csrf_exempt and use proper CSRF tokens for POST handlers".into(),
                                explanation: None,
                                fix: None,
                                cwe_ids: vec![352],
                                noisy: false,
                            });
                        }
                    }

                    // Check Flask POST routes without CSRFProtect
                    if !file_has_csrf_protection(source) {
                        for (line_num, line) in source.lines().enumerate() {
                            let trimmed = line.trim();
                            if is_comment(trimmed, Language::Python) {
                                continue;
                            }
                            if FLASK_METHODS_POST.is_match(trimmed) {
                                let line_1based = (line_num + 1) as u32;
                                findings.push(Finding {
                                    id: Uuid::new_v4(),
                                    detector: self.name().into(),
                                    severity: Severity::Medium,
                                    category: FindingCategory::SecuritySmell,
                                    file: path.clone(),
                                    line: Some(line_1based),
                                    title: "Flask POST route without CSRF protection".into(),
                                    description: format!(
                                        "POST route at {}:{} has no CSRFProtect middleware",
                                        path.display(),
                                        line_1based
                                    ),
                                    evidence: vec![],
                                    covered: false,
                                    suggestion: "Add flask_wtf.CSRFProtect to your Flask application".into(),
                                    explanation: None,
                                    fix: None,
                                    cwe_ids: vec![352],
                                    noisy: false,
                                });
                            }
                        }
                    }
                }
                Language::JavaScript => {
                    // Express: POST/PUT/DELETE routes without csurf middleware
                    if !file_has_csrf_protection(source) {
                        for (line_num, line) in source.lines().enumerate() {
                            let trimmed = line.trim();
                            if is_comment(trimmed, Language::JavaScript) {
                                continue;
                            }
                            if EXPRESS_MUTATION_ROUTE.is_match(trimmed) {
                                let line_1based = (line_num + 1) as u32;
                                findings.push(Finding {
                                    id: Uuid::new_v4(),
                                    detector: self.name().into(),
                                    severity: Severity::Medium,
                                    category: FindingCategory::SecuritySmell,
                                    file: path.clone(),
                                    line: Some(line_1based),
                                    title: "Express mutation route without CSRF protection".into(),
                                    description: format!(
                                        "State-changing route at {}:{} has no CSRF middleware",
                                        path.display(),
                                        line_1based
                                    ),
                                    evidence: vec![],
                                    covered: false,
                                    suggestion: "Add csurf middleware to protect state-changing routes".into(),
                                    explanation: None,
                                    fix: None,
                                    cwe_ids: vec![352],
                                    noisy: false,
                                });
                            }
                        }
                    }
                }
                Language::Java => {
                    // Spring: csrf().disable() explicitly disables CSRF
                    for (line_num, line) in source.lines().enumerate() {
                        let trimmed = line.trim();
                        if is_comment(trimmed, Language::Java) {
                            continue;
                        }
                        if SPRING_CSRF_DISABLE.is_match(trimmed) {
                            let line_1based = (line_num + 1) as u32;
                            findings.push(Finding {
                                id: Uuid::new_v4(),
                                detector: self.name().into(),
                                severity: Severity::High,
                                category: FindingCategory::InsecureConfig,
                                file: path.clone(),
                                line: Some(line_1based),
                                title: "Spring Security CSRF protection disabled".into(),
                                description: format!(
                                    "csrf().disable() at {}:{} disables built-in CSRF protection",
                                    path.display(),
                                    line_1based
                                ),
                                evidence: vec![],
                                covered: false,
                                suggestion: "Remove csrf().disable() unless this is a stateless REST API using token auth".into(),
                                explanation: None,
                                fix: None,
                                cwe_ids: vec![352],
                                noisy: false,
                            });
                        }
                    }
                }
                Language::Ruby => {
                    // Rails: skip_before_action :verify_authenticity_token
                    for (line_num, line) in source.lines().enumerate() {
                        let trimmed = line.trim();
                        if is_comment(trimmed, Language::Ruby) {
                            continue;
                        }
                        if RAILS_SKIP_CSRF.is_match(trimmed) {
                            let line_1based = (line_num + 1) as u32;
                            findings.push(Finding {
                                id: Uuid::new_v4(),
                                detector: self.name().into(),
                                severity: Severity::High,
                                category: FindingCategory::SecuritySmell,
                                file: path.clone(),
                                line: Some(line_1based),
                                title: "Rails CSRF verification skipped".into(),
                                description: format!(
                                    "skip_before_action :verify_authenticity_token at {}:{} disables CSRF protection",
                                    path.display(),
                                    line_1based
                                ),
                                evidence: vec![],
                                covered: false,
                                suggestion: "Remove skip_before_action :verify_authenticity_token and use proper CSRF tokens".into(),
                                explanation: None,
                                fix: None,
                                cwe_ids: vec![352],
                                noisy: false,
                            });
                        }
                    }
                }
                Language::Go => {
                    // Go: mutation handlers without CSRF middleware
                    if !file_has_csrf_protection(source) {
                        for (line_num, line) in source.lines().enumerate() {
                            let trimmed = line.trim();
                            if is_comment(trimmed, Language::Go) {
                                continue;
                            }
                            if GO_POST_METHOD.is_match(trimmed)
                                || (GO_HANDLE_FUNC.is_match(trimmed)
                                    && DJANGO_POST_HANDLER.is_match(source))
                            {
                                let line_1based = (line_num + 1) as u32;
                                findings.push(Finding {
                                    id: Uuid::new_v4(),
                                    detector: self.name().into(),
                                    severity: Severity::Medium,
                                    category: FindingCategory::SecuritySmell,
                                    file: path.clone(),
                                    line: Some(line_1based),
                                    title: "Go mutation handler without CSRF protection".into(),
                                    description: format!(
                                        "Handler at {}:{} accepts mutations without CSRF middleware",
                                        path.display(),
                                        line_1based
                                    ),
                                    evidence: vec![],
                                    covered: false,
                                    suggestion: "Add gorilla/csrf or nosurf middleware to protect mutation handlers".into(),
                                    explanation: None,
                                    fix: None,
                                    cwe_ids: vec![352],
                                    noisy: false,
                                });
                            }
                        }
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
    use crate::context::AnalysisContext;
    use apex_core::types::Language;
    use std::collections::HashMap;
    use std::path::PathBuf;

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
    async fn detects_django_csrf_exempt() {
        let ctx = make_ctx(
            "src/views.py",
            "@csrf_exempt\ndef update_profile(request):\n    pass\n",
            Language::Python,
        );
        let findings = CsrfDetector.analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, Severity::High);
        assert_eq!(findings[0].cwe_ids, vec![352]);
    }

    #[tokio::test]
    async fn detects_flask_post_without_csrf() {
        let ctx = make_ctx(
            "src/app.py",
            "@app.route('/submit', methods=['POST'])\ndef submit():\n    pass\n",
            Language::Python,
        );
        let findings = CsrfDetector.analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, Severity::Medium);
    }

    #[tokio::test]
    async fn no_finding_flask_with_csrf_protect() {
        let ctx = make_ctx(
            "src/app.py",
            "from flask_wtf import CSRFProtect\ncsrf = CSRFProtect(app)\n@app.route('/submit', methods=['POST'])\ndef submit():\n    pass\n",
            Language::Python,
        );
        let findings = CsrfDetector.analyze(&ctx).await.unwrap();
        assert!(findings.is_empty());
    }

    #[tokio::test]
    async fn detects_express_post_without_csurf() {
        let ctx = make_ctx(
            "src/app.js",
            "app.post('/api/transfer', (req, res) => { });\n",
            Language::JavaScript,
        );
        let findings = CsrfDetector.analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].cwe_ids, vec![352]);
    }

    #[tokio::test]
    async fn no_finding_express_with_csurf() {
        let ctx = make_ctx(
            "src/app.js",
            "const csurf = require('csurf');\napp.use(csurf());\napp.post('/api/transfer', (req, res) => { });\n",
            Language::JavaScript,
        );
        let findings = CsrfDetector.analyze(&ctx).await.unwrap();
        assert!(findings.is_empty());
    }

    #[tokio::test]
    async fn detects_spring_csrf_disable() {
        let ctx = make_ctx(
            "src/SecurityConfig.java",
            "http.csrf().disable();\n",
            Language::Java,
        );
        let findings = CsrfDetector.analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, Severity::High);
        assert_eq!(findings[0].category, FindingCategory::InsecureConfig);
    }

    #[tokio::test]
    async fn detects_rails_skip_csrf() {
        let ctx = make_ctx(
            "app/controllers/api_controller.rb",
            "class ApiController < ApplicationController\n  skip_before_action :verify_authenticity_token\nend\n",
            Language::Ruby,
        );
        let findings = CsrfDetector.analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, Severity::High);
    }

    #[tokio::test]
    async fn detects_go_post_without_csrf() {
        let ctx = make_ctx(
            "main.go",
            "r.Methods(\"POST\").HandlerFunc(handleTransfer)\n",
            Language::Go,
        );
        let findings = CsrfDetector.analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, Severity::Medium);
    }

    #[tokio::test]
    async fn skips_test_files() {
        let ctx = make_ctx(
            "tests/test_views.py",
            "@csrf_exempt\ndef test_handler(request):\n    pass\n",
            Language::Python,
        );
        let findings = CsrfDetector.analyze(&ctx).await.unwrap();
        assert!(findings.is_empty());
    }

    #[test]
    fn detector_name() {
        assert_eq!(CsrfDetector.name(), "csrf");
    }
}
