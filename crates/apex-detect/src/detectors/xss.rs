//! Cross-Site Scripting (XSS) detector (CWE-79).
//!
//! Detects XSS patterns beyond basic security_pattern.rs coverage:
//! - Template injection: |safe (Django), {% autoescape false %} (Jinja2), {!! $var !!} (Blade)
//! - DOM XSS: innerHTML, document.write, outerHTML, .html() (jQuery)
//! - React: dangerouslySetInnerHTML
//! - Vue: v-html

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

pub struct XssDetector;

struct XssPattern {
    name: &'static str,
    regex: &'static LazyLock<Regex>,
    severity: Severity,
    description: &'static str,
    suggestion: &'static str,
}

// ── Template injection ──────────────────────────────────────────────────

static DJANGO_SAFE_FILTER: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\|\s*safe\b").expect("invalid regex"));

static JINJA2_AUTOESCAPE_OFF: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"\{%\s*autoescape\s+false\s*%\}"#).expect("invalid regex")
});

static BLADE_UNESCAPED: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\{!!\s*\$\S+\s*!!\}").expect("invalid regex"));

// ── DOM XSS ─────────────────────────────────────────────────────────────

static INNERHTML_ASSIGN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\.innerHTML\s*=").expect("invalid regex"));

static OUTERHTML_ASSIGN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\.outerHTML\s*=").expect("invalid regex"));

static DOCUMENT_WRITE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"document\.write(?:ln)?\s*\(").expect("invalid regex"));

static JQUERY_HTML: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\.\s*html\s*\(\s*[a-zA-Z_$]").expect("invalid regex"));

// ── React ───────────────────────────────────────────────────────────────

static DANGEROUSLY_SET_HTML: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"dangerouslySetInnerHTML").expect("invalid regex"));

// ── Vue ─────────────────────────────────────────────────────────────────

static VUE_V_HTML: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"v-html\s*=").expect("invalid regex"));

/// DOMPurify and similar sanitizer markers that suppress findings.
const XSS_SANITIZERS: &[&str] = &[
    "DOMPurify",
    "sanitize",
    "escapeHtml",
    "escape_html",
    "xss(",
    "bleach.clean",
    "strip_tags",
    "htmlspecialchars",
    "encodeURIComponent",
    "mark_safe",   // Django intentional — still flag but note
    "SafeString",
];

fn line_has_sanitizer(line: &str) -> bool {
    XSS_SANITIZERS.iter().any(|s| line.contains(s))
}

static TEMPLATE_PATTERNS: &[XssPattern] = &[
    XssPattern {
        name: "Django |safe filter",
        regex: &DJANGO_SAFE_FILTER,
        severity: Severity::High,
        description: "Django |safe filter bypasses auto-escaping, allowing XSS if input is user-controlled",
        suggestion: "Remove |safe filter or sanitize input before marking as safe",
    },
    XssPattern {
        name: "Jinja2 autoescape disabled",
        regex: &JINJA2_AUTOESCAPE_OFF,
        severity: Severity::High,
        description: "Jinja2 autoescape false disables HTML escaping for the entire block",
        suggestion: "Remove {% autoescape false %} or use |e filter on individual variables",
    },
    XssPattern {
        name: "Blade unescaped output",
        regex: &BLADE_UNESCAPED,
        severity: Severity::High,
        description: "Blade {!! !!} outputs raw HTML without escaping",
        suggestion: "Use {{ }} instead of {!! !!} unless output is known-safe",
    },
];

static DOM_PATTERNS: &[XssPattern] = &[
    XssPattern {
        name: "innerHTML assignment",
        regex: &INNERHTML_ASSIGN,
        severity: Severity::High,
        description: "Direct innerHTML assignment can execute injected scripts",
        suggestion: "Use textContent instead of innerHTML, or sanitize with DOMPurify",
    },
    XssPattern {
        name: "outerHTML assignment",
        regex: &OUTERHTML_ASSIGN,
        severity: Severity::High,
        description: "Direct outerHTML assignment can execute injected scripts",
        suggestion: "Avoid outerHTML with user input; use textContent or DOMPurify",
    },
    XssPattern {
        name: "document.write",
        regex: &DOCUMENT_WRITE,
        severity: Severity::High,
        description: "document.write can inject arbitrary HTML including scripts",
        suggestion: "Use DOM manipulation methods (createElement, appendChild) instead of document.write",
    },
    XssPattern {
        name: "jQuery .html()",
        regex: &JQUERY_HTML,
        severity: Severity::Medium,
        description: "jQuery .html() with dynamic content can lead to XSS",
        suggestion: "Use .text() instead of .html() for user-provided content",
    },
];

static FRAMEWORK_PATTERNS: &[XssPattern] = &[
    XssPattern {
        name: "React dangerouslySetInnerHTML",
        regex: &DANGEROUSLY_SET_HTML,
        severity: Severity::High,
        description: "dangerouslySetInnerHTML bypasses React's built-in XSS protection",
        suggestion: "Avoid dangerouslySetInnerHTML; sanitize with DOMPurify if unavoidable",
    },
    XssPattern {
        name: "Vue v-html directive",
        regex: &VUE_V_HTML,
        severity: Severity::High,
        description: "v-html renders raw HTML, bypassing Vue's template escaping",
        suggestion: "Use text interpolation {{ }} instead of v-html, or sanitize input",
    },
];

fn check_patterns(
    patterns: &[XssPattern],
    detector_name: &str,
    path: &std::path::Path,
    source: &str,
    lang: Language,
    findings: &mut Vec<Finding>,
) {
    for (line_num, line) in source.lines().enumerate() {
        let trimmed = line.trim();
        if is_comment(trimmed, lang) {
            continue;
        }
        if line_has_sanitizer(trimmed) {
            continue;
        }

        for pattern in patterns {
            if pattern.regex.is_match(trimmed) {
                let line_1based = (line_num + 1) as u32;
                findings.push(Finding {
                    id: Uuid::new_v4(),
                    detector: detector_name.into(),
                    severity: pattern.severity,
                    category: FindingCategory::Injection,
                    file: path.to_path_buf(),
                    line: Some(line_1based),
                    title: format!("XSS: {}", pattern.name),
                    description: format!(
                        "{} at {}:{}",
                        pattern.description,
                        path.display(),
                        line_1based
                    ),
                    evidence: vec![],
                    covered: false,
                    suggestion: pattern.suggestion.into(),
                    explanation: None,
                    fix: None,
                    cwe_ids: vec![79],
                    noisy: false, base_severity: None, coverage_confidence: None,
                });
                break; // one finding per line
            }
        }
    }
}

#[async_trait]
impl Detector for XssDetector {
    fn name(&self) -> &str {
        "xss"
    }

    async fn analyze(&self, ctx: &AnalysisContext) -> Result<Vec<Finding>> {
        let mut findings = Vec::new();

        for (path, source) in &ctx.source_cache {
            if is_test_file(path) {
                continue;
            }

            let path_str = path.to_string_lossy();

            // Template patterns — check in Python templates and PHP
            if matches!(ctx.language, Language::Python)
                || path_str.ends_with(".html")
                || path_str.ends_with(".jinja2")
                || path_str.ends_with(".j2")
                || path_str.ends_with(".blade.php")
            {
                check_patterns(
                    TEMPLATE_PATTERNS,
                    self.name(),
                    path,
                    source,
                    ctx.language,
                    &mut findings,
                );
            }

            // DOM XSS — JavaScript/TypeScript
            if matches!(ctx.language, Language::JavaScript)
                || path_str.ends_with(".js")
                || path_str.ends_with(".ts")
                || path_str.ends_with(".jsx")
                || path_str.ends_with(".tsx")
            {
                check_patterns(
                    DOM_PATTERNS,
                    self.name(),
                    path,
                    source,
                    ctx.language,
                    &mut findings,
                );
            }

            // Framework patterns — React/Vue
            if matches!(ctx.language, Language::JavaScript)
                || path_str.ends_with(".jsx")
                || path_str.ends_with(".tsx")
                || path_str.ends_with(".vue")
            {
                check_patterns(
                    FRAMEWORK_PATTERNS,
                    self.name(),
                    path,
                    source,
                    ctx.language,
                    &mut findings,
                );
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
    async fn detects_django_safe_filter() {
        let ctx = make_ctx(
            "templates/profile.html",
            "<div>{{ user_bio|safe }}</div>\n",
            Language::Python,
        );
        let findings = XssDetector.analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].cwe_ids, vec![79]);
        assert_eq!(findings[0].severity, Severity::High);
    }

    #[tokio::test]
    async fn detects_jinja2_autoescape_false() {
        let ctx = make_ctx(
            "templates/page.jinja2",
            "{% autoescape false %}\n<div>{{ content }}</div>\n{% endautoescape %}\n",
            Language::Python,
        );
        let findings = XssDetector.analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 1);
    }

    #[tokio::test]
    async fn detects_innerhtml_assignment() {
        let ctx = make_ctx(
            "src/app.js",
            "element.innerHTML = userInput;\n",
            Language::JavaScript,
        );
        let findings = XssDetector.analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 1);
        assert!(findings[0].title.contains("innerHTML"));
    }

    #[tokio::test]
    async fn detects_document_write() {
        let ctx = make_ctx(
            "src/legacy.js",
            "document.write(data);\n",
            Language::JavaScript,
        );
        let findings = XssDetector.analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 1);
    }

    #[tokio::test]
    async fn detects_dangerously_set_inner_html() {
        let ctx = make_ctx(
            "src/Component.jsx",
            "<div dangerouslySetInnerHTML={{ __html: userContent }} />\n",
            Language::JavaScript,
        );
        let findings = XssDetector.analyze(&ctx).await.unwrap();
        assert!(!findings.is_empty());
        assert!(findings.iter().any(|f| f.title.contains("dangerouslySetInnerHTML")));
    }

    #[tokio::test]
    async fn detects_vue_v_html() {
        let ctx = make_ctx(
            "src/Component.vue",
            "<div v-html=\"userContent\"></div>\n",
            Language::JavaScript,
        );
        let findings = XssDetector.analyze(&ctx).await.unwrap();
        assert!(!findings.is_empty());
        assert!(findings.iter().any(|f| f.title.contains("v-html")));
    }

    #[tokio::test]
    async fn no_finding_with_dompurify() {
        let ctx = make_ctx(
            "src/app.js",
            "element.innerHTML = DOMPurify.sanitize(userInput);\n",
            Language::JavaScript,
        );
        let findings = XssDetector.analyze(&ctx).await.unwrap();
        assert!(findings.is_empty());
    }

    #[tokio::test]
    async fn detects_jquery_html() {
        let ctx = make_ctx(
            "src/app.js",
            "$('#output').html(userInput);\n",
            Language::JavaScript,
        );
        let findings = XssDetector.analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, Severity::Medium);
    }

    #[tokio::test]
    async fn skips_test_files() {
        let ctx = make_ctx(
            "tests/test_xss.js",
            "element.innerHTML = payload;\n",
            Language::JavaScript,
        );
        let findings = XssDetector.analyze(&ctx).await.unwrap();
        assert!(findings.is_empty());
    }

    #[tokio::test]
    async fn detects_blade_unescaped() {
        let ctx = make_ctx(
            "resources/views/profile.blade.php",
            "<div>{!! $user->bio !!}</div>\n",
            Language::Python, // PHP not in Language enum; detected by file extension
        );
        let findings = XssDetector.analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 1);
    }

    #[test]
    fn detector_name() {
        assert_eq!(XssDetector.name(), "xss");
    }
}
