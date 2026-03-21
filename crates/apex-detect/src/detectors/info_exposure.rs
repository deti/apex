//! Information Exposure detector (CWE-200).
//!
//! Detects information disclosure patterns:
//! - DEBUG = True in production config
//! - Stack traces in HTTP responses
//! - Sensitive fields in API serializers
//! - Verbose error messages returned to users
//! - Server version headers exposed

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

pub struct InfoExposureDetector;

// ── Debug mode ──────────────────────────────────────────────────────────

static PY_DEBUG_TRUE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^\s*DEBUG\s*=\s*True\b").expect("invalid regex"));

// ── Stack traces in responses ───────────────────────────────────────────

static PY_TRACEBACK_IN_RETURN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?:return|Response|JsonResponse|jsonify)\s*\(.*(?:traceback|stacktrace|stack_trace)")
        .expect("invalid regex")
});

static JS_STACK_IN_RESPONSE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?:res\.(?:json|send|status)\s*\(|response\s*=).*(?:\.stack|\.message|err\.toString)")
        .expect("invalid regex")
});

static JAVA_PRINT_STACK: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\.printStackTrace\s*\(").expect("invalid regex")
});

// ── Sensitive fields in serializers ─────────────────────────────────────

static SENSITIVE_FIELD: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r#"['"](?:password|passwd|secret|ssn|social_security|credit_card|card_number|cvv|token|api_key|private_key)['"]"#,
    )
    .expect("invalid regex")
});

/// Context markers that indicate a serializer/API response definition.
const SERIALIZER_CONTEXTS: &[&str] = &[
    "Serializer",
    "serializer",
    "fields",
    "Schema",
    "schema",
    "ResponseModel",
    "response_model",
    "JsonProperty",
    "DataMember",
    "to_json",
    "to_dict",
    "as_json",
];

// ── Server version exposure ─────────────────────────────────────────────

static SERVER_HEADER: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"['"](?:Server|X-Powered-By|X-AspNet-Version)['"]\s*[,:=]"#).expect("invalid regex")
});

// ── Verbose error messages ──────────────────────────────────────────────

static VERBOSE_ERROR: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?:res\.(?:json|send)|JsonResponse|jsonify|Response)\s*\(.*(?:str\s*\(\s*e\s*\)|e\.(?:message|getMessage|toString))",
    )
    .expect("invalid regex")
});

/// Markers indicating production-safe error handling.
const ERROR_HANDLING_MARKERS: &[&str] = &[
    "logging",
    "logger",
    "sentry",
    "bugsnag",
    "rollbar",
    "log.error",
    "log.warn",
    "console.error",
];

fn line_has_error_handling(line: &str) -> bool {
    let lower = line.to_lowercase();
    ERROR_HANDLING_MARKERS.iter().any(|m| lower.contains(m))
}

#[async_trait]
impl Detector for InfoExposureDetector {
    fn name(&self) -> &str {
        "info-exposure"
    }

    async fn analyze(&self, ctx: &AnalysisContext) -> Result<Vec<Finding>> {
        let mut findings = Vec::new();

        for (path, source) in &ctx.source_cache {
            if is_test_file(path) {
                continue;
            }

            let path_str = path.to_string_lossy();

            for (line_num, line) in source.lines().enumerate() {
                let trimmed = line.trim();
                if is_comment(trimmed, ctx.language) {
                    continue;
                }
                let line_1based = (line_num + 1) as u32;

                // DEBUG = True in settings files
                if (path_str.contains("settings") || path_str.contains("config"))
                    && PY_DEBUG_TRUE.is_match(trimmed)
                {
                    findings.push(Finding {
                        id: Uuid::new_v4(),
                        detector: self.name().into(),
                        severity: Severity::High,
                        category: FindingCategory::InsecureConfig,
                        file: path.clone(),
                        line: Some(line_1based),
                        title: "Debug mode enabled in configuration".into(),
                        description: format!(
                            "DEBUG = True at {}:{} may expose sensitive information in production",
                            path.display(),
                            line_1based
                        ),
                        evidence: vec![],
                        covered: false,
                        suggestion: "Set DEBUG = False in production or load from environment variable".into(),
                        explanation: None,
                        fix: None,
                        cwe_ids: vec![200],
                        noisy: false,
                    });
                }

                // Stack traces in responses
                if PY_TRACEBACK_IN_RETURN.is_match(trimmed) {
                    findings.push(Finding {
                        id: Uuid::new_v4(),
                        detector: self.name().into(),
                        severity: Severity::High,
                        category: FindingCategory::SecuritySmell,
                        file: path.clone(),
                        line: Some(line_1based),
                        title: "Stack trace exposed in HTTP response".into(),
                        description: format!(
                            "Stack trace returned in response at {}:{}",
                            path.display(),
                            line_1based
                        ),
                        evidence: vec![],
                        covered: false,
                        suggestion: "Log stack traces server-side and return generic error messages to clients".into(),
                        explanation: None,
                        fix: None,
                        cwe_ids: vec![200],
                        noisy: false,
                    });
                }

                // JS stack in response
                if ctx.language == Language::JavaScript
                    && JS_STACK_IN_RESPONSE.is_match(trimmed)
                    && !line_has_error_handling(trimmed)
                {
                    findings.push(Finding {
                        id: Uuid::new_v4(),
                        detector: self.name().into(),
                        severity: Severity::Medium,
                        category: FindingCategory::SecuritySmell,
                        file: path.clone(),
                        line: Some(line_1based),
                        title: "Error details exposed in HTTP response".into(),
                        description: format!(
                            "Error stack or message returned in response at {}:{}",
                            path.display(),
                            line_1based
                        ),
                        evidence: vec![],
                        covered: false,
                        suggestion: "Return generic error messages; log details server-side".into(),
                        explanation: None,
                        fix: None,
                        cwe_ids: vec![200],
                        noisy: false,
                    });
                }

                // Java printStackTrace (often leaks to output)
                if ctx.language == Language::Java && JAVA_PRINT_STACK.is_match(trimmed) {
                    findings.push(Finding {
                        id: Uuid::new_v4(),
                        detector: self.name().into(),
                        severity: Severity::Medium,
                        category: FindingCategory::SecuritySmell,
                        file: path.clone(),
                        line: Some(line_1based),
                        title: "Stack trace printed via printStackTrace".into(),
                        description: format!(
                            "printStackTrace() at {}:{} may leak stack traces to users",
                            path.display(),
                            line_1based
                        ),
                        evidence: vec![],
                        covered: false,
                        suggestion: "Use a logging framework (SLF4J/Log4j) instead of printStackTrace()".into(),
                        explanation: None,
                        fix: None,
                        cwe_ids: vec![200],
                        noisy: false,
                    });
                }

                // Sensitive fields in serializers/API responses
                if SERIALIZER_CONTEXTS.iter().any(|ctx_marker| source.contains(ctx_marker)) {
                    let lower = trimmed.to_lowercase();
                    if !lower.contains("exclude")
                        && !lower.contains("write_only")
                        && !lower.contains("writeonly")
                        && !lower.contains("hidden")
                        && !lower.contains("redact")
                    {
                        for m in SENSITIVE_FIELD.find_iter(trimmed) {
                            let field_name = m.as_str().trim_matches(|c| c == '\'' || c == '"');
                            findings.push(Finding {
                                id: Uuid::new_v4(),
                                detector: self.name().into(),
                                severity: Severity::High,
                                category: FindingCategory::SecuritySmell,
                                file: path.clone(),
                                line: Some(line_1based),
                                title: format!("Sensitive field '{}' exposed in API response", field_name),
                                description: format!(
                                    "Sensitive field '{}' in serializer/response at {}:{}",
                                    field_name,
                                    path.display(),
                                    line_1based
                                ),
                                evidence: vec![],
                                covered: false,
                                suggestion: "Exclude sensitive fields from API responses or mark them as write-only".into(),
                                explanation: None,
                                fix: None,
                                cwe_ids: vec![200],
                                noisy: false,
                            });
                        }
                    }
                }

                // Server version headers
                if SERVER_HEADER.is_match(trimmed) {
                    findings.push(Finding {
                        id: Uuid::new_v4(),
                        detector: self.name().into(),
                        severity: Severity::Low,
                        category: FindingCategory::InsecureConfig,
                        file: path.clone(),
                        line: Some(line_1based),
                        title: "Server version header exposed".into(),
                        description: format!(
                            "Server/version header set at {}:{} reveals technology stack",
                            path.display(),
                            line_1based
                        ),
                        evidence: vec![],
                        covered: false,
                        suggestion: "Remove or obfuscate server version headers".into(),
                        explanation: None,
                        fix: None,
                        cwe_ids: vec![200],
                        noisy: false,
                    });
                }

                // Verbose error messages
                if VERBOSE_ERROR.is_match(trimmed) && !line_has_error_handling(trimmed) {
                    findings.push(Finding {
                        id: Uuid::new_v4(),
                        detector: self.name().into(),
                        severity: Severity::Medium,
                        category: FindingCategory::SecuritySmell,
                        file: path.clone(),
                        line: Some(line_1based),
                        title: "Verbose error message in response".into(),
                        description: format!(
                            "Raw exception message returned in response at {}:{}",
                            path.display(),
                            line_1based
                        ),
                        evidence: vec![],
                        covered: false,
                        suggestion: "Return user-friendly error messages; log detailed errors server-side".into(),
                        explanation: None,
                        fix: None,
                        cwe_ids: vec![200],
                        noisy: false,
                    });
                }
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
    async fn detects_debug_true_in_settings() {
        let ctx = make_ctx(
            "src/settings.py",
            "SECRET_KEY = os.environ['KEY']\nDEBUG = True\nALLOWED_HOSTS = ['*']\n",
            Language::Python,
        );
        let findings = InfoExposureDetector.analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, Severity::High);
        assert_eq!(findings[0].cwe_ids, vec![200]);
    }

    #[tokio::test]
    async fn no_finding_debug_in_non_settings() {
        let ctx = make_ctx(
            "src/utils.py",
            "DEBUG = True\n",
            Language::Python,
        );
        let findings = InfoExposureDetector.analyze(&ctx).await.unwrap();
        assert!(findings.is_empty());
    }

    #[tokio::test]
    async fn detects_traceback_in_response() {
        let ctx = make_ctx(
            "src/views.py",
            "except Exception as e:\n    return JsonResponse({'error': traceback.format_exc()})\n",
            Language::Python,
        );
        let findings = InfoExposureDetector.analyze(&ctx).await.unwrap();
        assert!(!findings.is_empty());
        assert!(findings.iter().any(|f| f.title.contains("Stack trace")));
    }

    #[tokio::test]
    async fn detects_js_error_stack_in_response() {
        let ctx = make_ctx(
            "src/app.js",
            "app.use((err, req, res, next) => {\n  res.json({ error: err.stack });\n});\n",
            Language::JavaScript,
        );
        let findings = InfoExposureDetector.analyze(&ctx).await.unwrap();
        assert!(!findings.is_empty());
    }

    #[tokio::test]
    async fn detects_java_print_stack_trace() {
        let ctx = make_ctx(
            "src/Handler.java",
            "catch (Exception e) {\n    e.printStackTrace();\n}\n",
            Language::Java,
        );
        let findings = InfoExposureDetector.analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 1);
        assert!(findings[0].title.contains("printStackTrace"));
    }

    #[tokio::test]
    async fn detects_sensitive_field_in_serializer() {
        let ctx = make_ctx(
            "src/serializers.py",
            "class UserSerializer(Serializer):\n    fields = ['username', 'email', 'password', 'ssn']\n",
            Language::Python,
        );
        let findings = InfoExposureDetector.analyze(&ctx).await.unwrap();
        assert!(findings.len() >= 2); // password and ssn
    }

    #[tokio::test]
    async fn no_finding_sensitive_field_excluded() {
        let ctx = make_ctx(
            "src/serializers.py",
            "class UserSerializer(Serializer):\n    exclude = ['password']\n",
            Language::Python,
        );
        let findings = InfoExposureDetector.analyze(&ctx).await.unwrap();
        assert!(findings.is_empty());
    }

    #[tokio::test]
    async fn detects_server_version_header() {
        let ctx = make_ctx(
            "src/app.js",
            "res.setHeader('X-Powered-By', 'Express 4.18');\n",
            Language::JavaScript,
        );
        let findings = InfoExposureDetector.analyze(&ctx).await.unwrap();
        assert!(!findings.is_empty());
        assert!(findings.iter().any(|f| f.title.contains("version header")));
    }

    #[tokio::test]
    async fn detects_verbose_error_python() {
        let ctx = make_ctx(
            "src/api.py",
            "except Exception as e:\n    return JsonResponse({'error': str(e)})\n",
            Language::Python,
        );
        let findings = InfoExposureDetector.analyze(&ctx).await.unwrap();
        assert!(!findings.is_empty());
    }

    #[tokio::test]
    async fn skips_test_files() {
        let ctx = make_ctx(
            "tests/test_settings.py",
            "DEBUG = True\n",
            Language::Python,
        );
        let findings = InfoExposureDetector.analyze(&ctx).await.unwrap();
        assert!(findings.is_empty());
    }

    #[test]
    fn detector_name() {
        assert_eq!(InfoExposureDetector.name(), "info-exposure");
    }
}
