//! Multi-language SSRF detector (CWE-918).
//!
//! Catches server-side request forgery patterns across all 11 supported languages
//! where user-controlled URLs may reach HTTP client functions.

use apex_core::error::Result;
use apex_core::types::Language;
use async_trait::async_trait;
use regex::Regex;
use std::sync::LazyLock;
use uuid::Uuid;

use super::util::{is_comment, is_test_file, taint_reaches_sink};
use crate::context::AnalysisContext;
use crate::finding::{Finding, FindingCategory, Severity};
use crate::Detector;

pub struct MultiSsrfDetector;

struct LangPattern {
    lang: Language,
    name: &'static str,
    regex: Regex,
    description: &'static str,
}

/// URL validation/allowlist indicators that suppress findings.
const SSRF_SANITIZATION: &[&str] = &[
    "allowlist",
    "whitelist",
    "allowed_hosts",
    "validate_url",
    "is_safe_url",
    "urlvalidator",
    "ssrf_filter",
    "internal_only",
];

static PATTERNS: LazyLock<Vec<LangPattern>> = LazyLock::new(|| {
    vec![
        // ── Python ──────────────────────────────────────────────────
        LangPattern {
            lang: Language::Python,
            name: "requests HTTP",
            regex: Regex::new(r#"requests\.(?:get|post|put|delete|patch|head)\s*\(\s*[a-zA-Z_]"#)
                .unwrap(),
            description: "HTTP request with potentially user-controlled URL",
        },
        LangPattern {
            lang: Language::Python,
            name: "urllib urlopen",
            regex: Regex::new(r#"(?:urlopen|urllib\.request\.urlopen)\s*\(\s*[a-zA-Z_]"#).unwrap(),
            description: "URL open with potentially user-controlled URL",
        },
        LangPattern {
            lang: Language::Python,
            name: "httpx client",
            regex: Regex::new(r#"httpx\.(?:get|post|put|delete|patch)\s*\(\s*[a-zA-Z_]"#).unwrap(),
            description: "httpx request with potentially user-controlled URL",
        },
        // ── JavaScript ──────────────────────────────────────────────
        LangPattern {
            lang: Language::JavaScript,
            name: "fetch",
            regex: Regex::new(r#"fetch\s*\(\s*[a-zA-Z_]"#).unwrap(),
            description: "Fetch with potentially user-controlled URL",
        },
        LangPattern {
            lang: Language::JavaScript,
            name: "axios",
            regex: Regex::new(r#"axios\.(?:get|post|put|delete|patch)\s*\(\s*[a-zA-Z_]"#).unwrap(),
            description: "Axios request with potentially user-controlled URL",
        },
        LangPattern {
            lang: Language::JavaScript,
            name: "http.request",
            regex: Regex::new(r#"https?\.(?:get|request)\s*\(\s*[a-zA-Z_]"#).unwrap(),
            description: "HTTP request with potentially user-controlled URL",
        },
        // ── Java ────────────────────────────────────────────────────
        LangPattern {
            lang: Language::Java,
            name: "URL constructor",
            regex: Regex::new(r#"new\s+URL\s*\(\s*[a-zA-Z_]"#).unwrap(),
            description: "URL construction with potentially user-controlled input",
        },
        LangPattern {
            lang: Language::Java,
            name: "HttpClient",
            regex: Regex::new(r#"HttpClient\.\w+\s*\(\s*\)\s*\.(?:send|newCall)"#).unwrap(),
            description: "HTTP client with potentially user-controlled request",
        },
        LangPattern {
            lang: Language::Java,
            name: "HttpURLConnection",
            regex: Regex::new(r#"\.openConnection\s*\("#).unwrap(),
            description: "URL connection potentially controlled by user input",
        },
        // ── Go ──────────────────────────────────────────────────────
        LangPattern {
            lang: Language::Go,
            name: "http.Get/Post",
            regex: Regex::new(r#"http\.(?:Get|Post|PostForm|Head)\s*\(\s*[a-zA-Z_]"#).unwrap(),
            description: "HTTP request with potentially user-controlled URL",
        },
        LangPattern {
            lang: Language::Go,
            name: "http.NewRequest",
            regex: Regex::new(r#"http\.NewRequest\s*\([^,]*,\s*[a-zA-Z_]"#).unwrap(),
            description: "HTTP request construction with potentially user-controlled URL",
        },
        // ── Ruby ────────────────────────────────────────────────────
        LangPattern {
            lang: Language::Ruby,
            name: "Net::HTTP",
            regex: Regex::new(r#"Net::HTTP\.(?:get|post|start)\s*\(\s*[a-zA-Z_]"#).unwrap(),
            description: "HTTP request with potentially user-controlled URL",
        },
        LangPattern {
            lang: Language::Ruby,
            name: "open-uri",
            regex: Regex::new(r#"(?:URI\.open|open)\s*\(\s*[a-zA-Z_]"#).unwrap(),
            description: "URI open with potentially user-controlled URL",
        },
        // ── C# ──────────────────────────────────────────────────────
        LangPattern {
            lang: Language::CSharp,
            name: "HttpClient",
            regex: Regex::new(r#"HttpClient\s*\(\s*\)\s*\.(?:GetAsync|PostAsync|SendAsync)\s*\(\s*[a-zA-Z_]"#).unwrap(),
            description: "HTTP request with potentially user-controlled URL",
        },
        LangPattern {
            lang: Language::CSharp,
            name: "WebClient",
            regex: Regex::new(r#"WebClient\s*\(\s*\)\s*\.(?:DownloadString|DownloadFile)\s*\(\s*[a-zA-Z_]"#).unwrap(),
            description: "WebClient request with potentially user-controlled URL",
        },
        // ── Rust ────────────────────────────────────────────────────
        LangPattern {
            lang: Language::Rust,
            name: "reqwest",
            regex: Regex::new(r#"reqwest::(?:get|Client)"#).unwrap(),
            description: "HTTP client potentially using user-controlled URL",
        },
        // ── Kotlin ──────────────────────────────────────────────────
        LangPattern {
            lang: Language::Kotlin,
            name: "URL constructor",
            regex: Regex::new(r#"URL\s*\(\s*[a-zA-Z_]"#).unwrap(),
            description: "URL construction with potentially user-controlled input",
        },
        // ── Swift ───────────────────────────────────────────────────
        LangPattern {
            lang: Language::Swift,
            name: "URLSession",
            regex: Regex::new(r#"URLSession\.\w+\.dataTask\s*\(\s*with:\s*[a-zA-Z_]"#).unwrap(),
            description: "URL session request with potentially user-controlled URL",
        },
    ]
});

/// Returns true when the surrounding context has SSRF mitigation.
fn has_ssrf_mitigation(line: &str) -> bool {
    let lower = line.to_lowercase();
    SSRF_SANITIZATION.iter().any(|s| lower.contains(s))
}

#[async_trait]
impl Detector for MultiSsrfDetector {
    fn name(&self) -> &str {
        "multi-ssrf"
    }

    async fn analyze(&self, ctx: &AnalysisContext) -> Result<Vec<Finding>> {
        if ctx.language == Language::Wasm {
            return Ok(Vec::new());
        }

        let mut findings = Vec::new();

        for (path, source) in &ctx.source_cache {
            if is_test_file(path) {
                continue;
            }

            for (line_num, line) in source.lines().enumerate() {
                let trimmed = line.trim();

                if is_comment(trimmed, ctx.language) {
                    continue;
                }

                if has_ssrf_mitigation(trimmed) {
                    continue;
                }

                for pattern in PATTERNS.iter() {
                    if pattern.lang != ctx.language {
                        continue;
                    }

                    if pattern.regex.is_match(trimmed) {
                        let line_1based = (line_num + 1) as u32;

                        let mut finding = Finding {
                            id: Uuid::new_v4(),
                            detector: self.name().into(),
                            severity: Severity::High,
                            category: FindingCategory::Injection,
                            file: path.clone(),
                            line: Some(line_1based),
                            title: format!(
                                "{}: {} at line {}",
                                pattern.name, pattern.description, line_1based
                            ),
                            description: format!(
                                "{} pattern matched in {}:{}",
                                pattern.name,
                                path.display(),
                                line_1based
                            ),
                            evidence: super::util::reachability_evidence(ctx, path, line_1based),
                            covered: false,
                            suggestion:
                                "Validate and allowlist URLs before making server-side requests. \
                                 Never allow user input to control the destination of HTTP requests."
                                    .into(),
                            explanation: None,
                            fix: None,
                            cwe_ids: vec![918],
                            noisy: false, base_severity: None, coverage_confidence: None,
                        };

                        // Check taint flow if CPG is available — downgrade instead of discard.
                        if let Some(has_taint) = taint_reaches_sink(
                            ctx,
                            path,
                            line_1based,
                            &["user_input", "request", "args", "params", "url", "endpoint"],
                        ) {
                            if !has_taint {
                                finding.noisy = true;
                                finding.severity = Severity::Low;
                                finding.description = format!(
                                    "{} (no taint flow detected — likely safe)",
                                    finding.description
                                );
                            }
                        }

                        findings.push(finding);
                        break;
                    }
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

    fn make_ctx(files: HashMap<PathBuf, String>, lang: Language) -> AnalysisContext {
        AnalysisContext {
            language: lang,
            source_cache: files,
            ..AnalysisContext::test_default()
        }
    }

    fn single_file(name: &str, content: &str) -> HashMap<PathBuf, String> {
        let mut m = HashMap::new();
        m.insert(PathBuf::from(name), content.into());
        m
    }

    #[tokio::test]
    async fn detects_python_requests_get() {
        let files = single_file("src/app.py", "resp = requests.get(user_url)\n");
        let ctx = make_ctx(files, Language::Python);
        let findings = MultiSsrfDetector.analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].cwe_ids, vec![918]);
    }

    #[tokio::test]
    async fn detects_js_fetch() {
        let files = single_file("src/app.js", "const resp = await fetch(userUrl)\n");
        let ctx = make_ctx(files, Language::JavaScript);
        let findings = MultiSsrfDetector.analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 1);
    }

    #[tokio::test]
    async fn detects_go_http_get() {
        let files = single_file("src/main.go", "resp, err := http.Get(targetUrl)\n");
        let ctx = make_ctx(files, Language::Go);
        let findings = MultiSsrfDetector.analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 1);
    }

    #[tokio::test]
    async fn skips_allowlisted() {
        let files = single_file("src/app.py", "if validate_url(url): requests.get(url)\n");
        let ctx = make_ctx(files, Language::Python);
        let findings = MultiSsrfDetector.analyze(&ctx).await.unwrap();
        assert!(findings.is_empty());
    }

    #[tokio::test]
    async fn skips_test_files() {
        let files = single_file("tests/test_http.py", "requests.get(user_url)\n");
        let ctx = make_ctx(files, Language::Python);
        let findings = MultiSsrfDetector.analyze(&ctx).await.unwrap();
        assert!(findings.is_empty());
    }

    #[test]
    fn does_not_use_cargo_subprocess() {
        assert!(!MultiSsrfDetector.uses_cargo_subprocess());
    }

    // -----------------------------------------------------------------------
    // Taint flow integration via CPG
    // -----------------------------------------------------------------------

    fn make_ctx_with_cpg(
        files: HashMap<PathBuf, String>,
        lang: Language,
        cpg: apex_cpg::Cpg,
    ) -> AnalysisContext {
        use std::sync::Arc;
        AnalysisContext {
            language: lang,
            source_cache: files,
            cpg: Some(Arc::new(cpg)),
            ..AnalysisContext::test_default()
        }
    }

    // CPG with taint flow → finding stays at original severity (High)
    //
    // For SSRF the indicators include "url". We add an Identifier "url" on line 1,
    // connected via ReachingDef from a Parameter, so taint_reaches_sink returns Some(true).
    #[tokio::test]
    async fn taint_flow_present_keeps_original_severity() {
        use apex_cpg::{EdgeKind, NodeKind};

        let mut cpg = apex_cpg::Cpg::new();
        let param = cpg.add_node(NodeKind::Parameter {
            name: "url".into(),
            index: 0,
        });
        let sink_id = cpg.add_node(NodeKind::Identifier {
            name: "url".into(),
            line: 1,
        });
        cpg.add_edge(param, sink_id, EdgeKind::ReachingDef { variable: "url".into() });

        let files = single_file("src/app.py", "resp = requests.get(user_url)\n");
        let ctx = make_ctx_with_cpg(files, Language::Python, cpg);
        let findings = MultiSsrfDetector.analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 1);
        assert!(!findings[0].noisy, "taint flow present — should not be noisy");
        assert_eq!(
            findings[0].severity,
            Severity::High,
            "taint flow present — should stay High"
        );
    }

    // CPG with no taint flow → finding downgraded to noisy + Low
    //
    // We put a matching identifier on line 1 but no ReachingDef from any source.
    #[tokio::test]
    async fn no_taint_flow_downgrades_to_noisy_low() {
        use apex_cpg::NodeKind;

        let mut cpg = apex_cpg::Cpg::new();
        // Sink candidate matches indicator "url", but no taint source connected.
        cpg.add_node(NodeKind::Identifier {
            name: "url".into(),
            line: 1,
        });

        let files = single_file("src/app.py", "resp = requests.get(user_url)\n");
        let ctx = make_ctx_with_cpg(files, Language::Python, cpg);
        let findings = MultiSsrfDetector.analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 1);
        assert!(findings[0].noisy, "no taint flow — should be noisy");
        assert_eq!(
            findings[0].severity,
            Severity::Low,
            "no taint flow — should be downgraded to Low"
        );
        assert!(
            findings[0].description.contains("no taint flow"),
            "description should mention no taint flow"
        );
    }

    // No CPG → finding stays at original severity (fallback to pattern matching)
    #[tokio::test]
    async fn no_cpg_falls_back_to_pattern_severity() {
        let files = single_file("src/app.py", "resp = requests.get(user_url)\n");
        let ctx = make_ctx(files, Language::Python);
        let findings = MultiSsrfDetector.analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 1);
        assert!(!findings[0].noisy, "no CPG — should not be noisy");
        assert_eq!(
            findings[0].severity,
            Severity::High,
            "no CPG — should stay at pattern severity"
        );
    }
}
