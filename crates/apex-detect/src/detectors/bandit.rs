//! Bandit-inspired Python security pattern detector.
//!
//! Checks for well-known dangerous function calls using regex patterns,
//! mapped to Bandit rule IDs and CWE numbers.

use apex_core::error::Result;
use apex_core::types::Language;
use async_trait::async_trait;
use regex::Regex;
use uuid::Uuid;

use super::util::{is_comment, is_test_file};
use crate::context::AnalysisContext;
use crate::finding::{Evidence, Finding, FindingCategory, Severity};
use crate::Detector;

struct BanditRule {
    id: &'static str,
    pattern: &'static str,
    cwe: u32,
    severity: Severity,
    category: FindingCategory,
    title: &'static str,
    suggestion: &'static str,
    /// Optional: if the line also matches this pattern, suppress the finding.
    suppressor: Option<&'static str>,
}

const BANDIT_RULES: &[BanditRule] = &[
    BanditRule {
        id: "B102",
        pattern: r"\bexec\s*\(",
        cwe: 78,
        severity: Severity::High,
        category: FindingCategory::Injection,
        title: "Use of exec() detected",
        suggestion: "Avoid exec(); use safer alternatives like importlib or AST-based evaluation",
        suppressor: None,
    },
    BanditRule {
        id: "B103",
        pattern: r"os\.chmod\s*\(.*0o?7[67][67]",
        cwe: 732,
        severity: Severity::Medium,
        category: FindingCategory::SecuritySmell,
        title: "os.chmod setting permissive file permissions",
        suggestion: "Avoid overly permissive file modes; use 0o755 or more restrictive",
        suppressor: None,
    },
    BanditRule {
        id: "B104",
        pattern: r#"(?:bind\s*\(\s*.*0\.0\.0\.0|INADDR_ANY)"#,
        cwe: 1327,
        severity: Severity::Medium,
        category: FindingCategory::SecuritySmell,
        title: "Binding to all interfaces (0.0.0.0 / INADDR_ANY)",
        suggestion: "Bind to a specific interface instead of 0.0.0.0",
        suppressor: None,
    },
    BanditRule {
        id: "B108",
        pattern: r#"(?:"|')/tmp(?:"|')"#,
        cwe: 377,
        severity: Severity::Low,
        category: FindingCategory::SecuritySmell,
        title: "Hardcoded /tmp path detected",
        suggestion:
            "Use tempfile.mkdtemp() or tempfile.NamedTemporaryFile instead of hardcoded /tmp",
        suppressor: None,
    },
    BanditRule {
        id: "B301",
        pattern: r"pickle\.loads?\s*\(",
        cwe: 502,
        severity: Severity::High,
        category: FindingCategory::UnsafeCode,
        title: "Pickle deserialization detected",
        suggestion:
            "Pickle can execute arbitrary code; use JSON or other safe serialization formats",
        suppressor: None,
    },
    BanditRule {
        id: "B303",
        pattern: r"hashlib\.(?:md5|sha1)\s*\(",
        cwe: 328,
        severity: Severity::Medium,
        category: FindingCategory::SecuritySmell,
        title: "Use of insecure hash function (MD5/SHA1)",
        suggestion: "Use hashlib.sha256() or hashlib.sha3_256() instead",
        suppressor: None,
    },
    BanditRule {
        id: "B306",
        pattern: r"tempfile\.mktemp\s*\(",
        cwe: 377,
        severity: Severity::Medium,
        category: FindingCategory::SecuritySmell,
        title: "Use of insecure tempfile.mktemp()",
        suggestion: "Use tempfile.mkstemp() or tempfile.NamedTemporaryFile() instead",
        suppressor: None,
    },
    BanditRule {
        id: "B307",
        pattern: r"\beval\s*\(",
        cwe: 78,
        severity: Severity::High,
        category: FindingCategory::UnsafeCode,
        title: "Use of eval() detected",
        suggestion: "Avoid eval(); use ast.literal_eval() for safe expression evaluation",
        suppressor: None,
    },
    BanditRule {
        id: "B320",
        pattern: r"(?:ElementTree|etree)\.parse\s*\(",
        cwe: 611,
        severity: Severity::Medium,
        category: FindingCategory::SecuritySmell,
        title: "XML parsing vulnerable to XXE attacks",
        suggestion: "Use defusedxml.ElementTree instead of xml.etree.ElementTree",
        suppressor: None,
    },
    BanditRule {
        id: "B324",
        pattern: r#"hashlib\.new\s*\(\s*(?:"|')(?:md5|sha1)(?:"|')"#,
        cwe: 328,
        severity: Severity::Medium,
        category: FindingCategory::SecuritySmell,
        title: "Use of insecure hash via hashlib.new()",
        suggestion: "Use hashlib.new('sha256') or stronger algorithm",
        suppressor: None,
    },
    BanditRule {
        id: "B501",
        pattern: r"verify\s*=\s*False",
        cwe: 295,
        severity: Severity::High,
        category: FindingCategory::SecuritySmell,
        title: "TLS certificate verification disabled",
        suggestion: "Do not disable certificate verification; fix the certificate chain instead",
        suppressor: None,
    },
    BanditRule {
        id: "B506",
        pattern: r"yaml\.load\s*\(",
        cwe: 502,
        severity: Severity::Medium,
        category: FindingCategory::UnsafeCode,
        title: "Unsafe yaml.load() without SafeLoader",
        suggestion: "Use yaml.safe_load() or pass Loader=yaml.SafeLoader",
        suppressor: Some(r"(?i)SafeLoader|CSafeLoader|safe_load"),
    },
    BanditRule {
        id: "B602",
        pattern: r"shell\s*=\s*True",
        cwe: 78,
        severity: Severity::High,
        category: FindingCategory::Injection,
        title: "subprocess call with shell=True",
        suggestion: "Use shell=False and pass arguments as a list instead",
        suppressor: None,
    },
];

/// A compiled version of a BanditRule for efficient repeated matching.
struct CompiledRule {
    rule: &'static BanditRule,
    regex: Regex,
    suppressor_re: Option<Regex>,
}

pub struct BanditRuleDetector;

impl BanditRuleDetector {
    fn compile_rules() -> Vec<CompiledRule> {
        BANDIT_RULES
            .iter()
            .map(|rule| CompiledRule {
                rule,
                regex: Regex::new(rule.pattern).expect("invalid bandit rule regex"),
                suppressor_re: rule
                    .suppressor
                    .map(|s| Regex::new(s).expect("invalid suppressor regex")),
            })
            .collect()
    }
}

#[async_trait]
impl Detector for BanditRuleDetector {
    fn name(&self) -> &str {
        "bandit"
    }

    async fn analyze(&self, ctx: &AnalysisContext) -> Result<Vec<Finding>> {
        // Only applies to Python
        if ctx.language != Language::Python {
            return Ok(Vec::new());
        }

        let compiled = Self::compile_rules();
        let mut findings = Vec::new();

        for (path, source) in &ctx.source_cache {
            if is_test_file(path) {
                continue;
            }

            for (line_idx, line) in source.lines().enumerate() {
                let trimmed = line.trim();

                if trimmed.is_empty() || is_comment(trimmed, Language::Python) {
                    continue;
                }

                for cr in &compiled {
                    if !cr.regex.is_match(trimmed) {
                        continue;
                    }

                    // Check suppressor (e.g., SafeLoader suppresses B506)
                    if let Some(ref sup) = cr.suppressor_re {
                        if sup.is_match(trimmed) {
                            continue;
                        }
                    }

                    let line_1based = (line_idx + 1) as u32;

                    findings.push(Finding {
                        id: Uuid::new_v4(),
                        detector: self.name().into(),
                        severity: cr.rule.severity,
                        category: cr.rule.category,
                        file: path.clone(),
                        line: Some(line_1based),
                        title: format!("[{}] {} (CWE-{})", cr.rule.id, cr.rule.title, cr.rule.cwe),
                        description: format!(
                            "Bandit rule {} matched in {}:{}",
                            cr.rule.id,
                            path.display(),
                            line_1based,
                        ),
                        evidence: vec![Evidence::StaticAnalysis {
                            tool: "bandit".into(),
                            rule_id: cr.rule.id.into(),
                            sarif: serde_json::json!({
                                "cwe": format!("CWE-{}", cr.rule.cwe),
                                "line": trimmed,
                            }),
                        }],
                        covered: false,
                        suggestion: cr.rule.suggestion.into(),
                        explanation: None,
                        fix: None,
                        cwe_ids: vec![cr.rule.cwe],
                    });
                }
            }
        }

        // B110 requires multi-line scanning (except: followed by pass)
        for (path, source) in &ctx.source_cache {
            if is_test_file(path) {
                continue;
            }
            findings.extend(find_b110_findings(path, source));
        }

        Ok(findings)
    }
}

/// Scan for B110: bare `except:` followed by `pass` on the next non-empty line.
fn find_b110_findings(path: &std::path::Path, source: &str) -> Vec<Finding> {
    let lines: Vec<&str> = source.lines().collect();
    let mut findings = Vec::new();

    for (i, line) in lines.iter().enumerate() {
        let trimmed = line.trim();
        if is_comment(trimmed, Language::Python) {
            continue;
        }
        if trimmed == "except:" || trimmed == "except:  " || trimmed.starts_with("except:") {
            // Check if trimmed is exactly a bare except (no exception type)
            let bare = trimmed.trim_end();
            if bare != "except:" {
                continue;
            }
            // Look at subsequent non-empty lines for `pass`
            for next_line in &lines[(i + 1)..] {
                let next = next_line.trim();
                if next.is_empty() || is_comment(next, Language::Python) {
                    continue;
                }
                if next == "pass" {
                    let line_1based = (i + 1) as u32;
                    findings.push(Finding {
                        id: Uuid::new_v4(),
                        detector: "bandit".into(),
                        severity: Severity::Low,
                        category: FindingCategory::SecuritySmell,
                        file: path.to_path_buf(),
                        line: Some(line_1based),
                        title: "[B110] Try/except/pass detected (CWE-390)".into(),
                        description: format!(
                            "Bandit rule B110 matched in {}:{}",
                            path.display(),
                            line_1based,
                        ),
                        evidence: vec![Evidence::StaticAnalysis {
                            tool: "bandit".into(),
                            rule_id: "B110".into(),
                            sarif: serde_json::json!({
                                "cwe": "CWE-390",
                                "line": trimmed,
                            }),
                        }],
                        covered: false,
                        suggestion: "Do not silently swallow exceptions; log or handle them".into(),
                        explanation: None,
                        fix: None,
                        cwe_ids: vec![390],
                    });
                }
                break; // only check the first non-empty line after except:
            }
        }
    }

    findings
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

    fn make_ctx(files: HashMap<PathBuf, String>, lang: Language) -> AnalysisContext {
        AnalysisContext {
            language: lang,
            source_cache: files,
            ..AnalysisContext::test_default()
        }
    }

    fn py(name: &str, code: &str) -> HashMap<PathBuf, String> {
        let mut m = HashMap::new();
        m.insert(PathBuf::from(name), code.into());
        m
    }

    // ---- B102: exec() ----

    #[tokio::test]
    async fn b102_exec_detected() {
        let files = py("src/app.py", "exec(user_code)\n");
        let ctx = make_ctx(files, Language::Python);
        let findings = BanditRuleDetector.analyze(&ctx).await.unwrap();
        assert!(findings.iter().any(|f| f.title.contains("B102")));
        let f = findings.iter().find(|f| f.title.contains("B102")).unwrap();
        assert_eq!(f.severity, Severity::High);
        assert_eq!(f.category, FindingCategory::Injection);
    }

    // ---- B103: os.chmod with permissive mode ----

    #[tokio::test]
    async fn b103_chmod_permissive() {
        let files = py("src/setup.py", "os.chmod(path, 0o777)\n");
        let ctx = make_ctx(files, Language::Python);
        let findings = BanditRuleDetector.analyze(&ctx).await.unwrap();
        assert!(findings.iter().any(|f| f.title.contains("B103")));
    }

    #[tokio::test]
    async fn b103_chmod_safe_ignored() {
        let files = py("src/setup.py", "os.chmod(path, 0o755)\n");
        let ctx = make_ctx(files, Language::Python);
        let findings = BanditRuleDetector.analyze(&ctx).await.unwrap();
        assert!(!findings.iter().any(|f| f.title.contains("B103")));
    }

    // ---- B104: bind to 0.0.0.0 ----

    #[tokio::test]
    async fn b104_bind_all_interfaces() {
        let files = py("src/server.py", "sock.bind(('0.0.0.0', 8080))\n");
        let ctx = make_ctx(files, Language::Python);
        let findings = BanditRuleDetector.analyze(&ctx).await.unwrap();
        assert!(findings.iter().any(|f| f.title.contains("B104")));
    }

    #[tokio::test]
    async fn b104_inaddr_any() {
        let files = py("src/server.py", "addr = INADDR_ANY\n");
        let ctx = make_ctx(files, Language::Python);
        let findings = BanditRuleDetector.analyze(&ctx).await.unwrap();
        assert!(findings.iter().any(|f| f.title.contains("B104")));
    }

    // ---- B108: hardcoded /tmp ----

    #[tokio::test]
    async fn b108_hardcoded_tmp() {
        let files = py("src/io.py", "path = \"/tmp\"\n");
        let ctx = make_ctx(files, Language::Python);
        let findings = BanditRuleDetector.analyze(&ctx).await.unwrap();
        assert!(findings.iter().any(|f| f.title.contains("B108")));
        let f = findings.iter().find(|f| f.title.contains("B108")).unwrap();
        assert_eq!(f.severity, Severity::Low);
    }

    #[tokio::test]
    async fn b108_hardcoded_tmp_single_quote() {
        let files = py("src/io.py", "path = '/tmp'\n");
        let ctx = make_ctx(files, Language::Python);
        let findings = BanditRuleDetector.analyze(&ctx).await.unwrap();
        assert!(findings.iter().any(|f| f.title.contains("B108")));
    }

    // ---- B110: except: pass (multi-line) ----

    #[test]
    fn b110_except_pass() {
        let findings = find_b110_findings(
            std::path::Path::new("src/app.py"),
            "try:\n    do_thing()\nexcept:\n    pass\n",
        );
        assert_eq!(findings.len(), 1);
        assert!(findings[0].title.contains("B110"));
        assert_eq!(findings[0].severity, Severity::Low);
    }

    #[test]
    fn b110_except_with_type_not_triggered() {
        let findings = find_b110_findings(
            std::path::Path::new("src/app.py"),
            "try:\n    do_thing()\nexcept ValueError:\n    pass\n",
        );
        assert!(findings.is_empty());
    }

    #[test]
    fn b110_except_with_handler_not_triggered() {
        let findings = find_b110_findings(
            std::path::Path::new("src/app.py"),
            "try:\n    do_thing()\nexcept:\n    log.error('failed')\n",
        );
        assert!(findings.is_empty());
    }

    // ---- B301: pickle ----

    #[tokio::test]
    async fn b301_pickle_load() {
        let files = py("src/data.py", "obj = pickle.load(f)\n");
        let ctx = make_ctx(files, Language::Python);
        let findings = BanditRuleDetector.analyze(&ctx).await.unwrap();
        assert!(findings.iter().any(|f| f.title.contains("B301")));
        let f = findings.iter().find(|f| f.title.contains("B301")).unwrap();
        assert_eq!(f.severity, Severity::High);
        assert_eq!(f.category, FindingCategory::UnsafeCode);
    }

    #[tokio::test]
    async fn b301_pickle_loads() {
        let files = py("src/data.py", "obj = pickle.loads(data)\n");
        let ctx = make_ctx(files, Language::Python);
        let findings = BanditRuleDetector.analyze(&ctx).await.unwrap();
        assert!(findings.iter().any(|f| f.title.contains("B301")));
    }

    // ---- B303: weak hash ----

    #[tokio::test]
    async fn b303_md5() {
        let files = py("src/hash.py", "h = hashlib.md5(data)\n");
        let ctx = make_ctx(files, Language::Python);
        let findings = BanditRuleDetector.analyze(&ctx).await.unwrap();
        assert!(findings.iter().any(|f| f.title.contains("B303")));
        let f = findings.iter().find(|f| f.title.contains("B303")).unwrap();
        assert_eq!(f.severity, Severity::Medium);
    }

    #[tokio::test]
    async fn b303_sha1() {
        let files = py("src/hash.py", "h = hashlib.sha1(data)\n");
        let ctx = make_ctx(files, Language::Python);
        let findings = BanditRuleDetector.analyze(&ctx).await.unwrap();
        assert!(findings.iter().any(|f| f.title.contains("B303")));
    }

    // ---- B306: tempfile.mktemp ----

    #[tokio::test]
    async fn b306_mktemp() {
        let files = py("src/tmp.py", "path = tempfile.mktemp()\n");
        let ctx = make_ctx(files, Language::Python);
        let findings = BanditRuleDetector.analyze(&ctx).await.unwrap();
        assert!(findings.iter().any(|f| f.title.contains("B306")));
        let f = findings.iter().find(|f| f.title.contains("B306")).unwrap();
        assert_eq!(f.severity, Severity::Medium);
    }

    // ---- B307: eval() ----

    #[tokio::test]
    async fn b307_eval() {
        let files = py("src/calc.py", "result = eval(expr)\n");
        let ctx = make_ctx(files, Language::Python);
        let findings = BanditRuleDetector.analyze(&ctx).await.unwrap();
        assert!(findings.iter().any(|f| f.title.contains("B307")));
        let f = findings.iter().find(|f| f.title.contains("B307")).unwrap();
        assert_eq!(f.severity, Severity::High);
        assert_eq!(f.category, FindingCategory::UnsafeCode);
    }

    // ---- B320: XML parsing XXE ----

    #[tokio::test]
    async fn b320_elementtree_parse() {
        let files = py("src/xml.py", "tree = ElementTree.parse(f)\n");
        let ctx = make_ctx(files, Language::Python);
        let findings = BanditRuleDetector.analyze(&ctx).await.unwrap();
        assert!(findings.iter().any(|f| f.title.contains("B320")));
        let f = findings.iter().find(|f| f.title.contains("B320")).unwrap();
        assert_eq!(f.severity, Severity::Medium);
    }

    #[tokio::test]
    async fn b320_etree_parse() {
        let files = py("src/xml.py", "tree = etree.parse(path)\n");
        let ctx = make_ctx(files, Language::Python);
        let findings = BanditRuleDetector.analyze(&ctx).await.unwrap();
        assert!(findings.iter().any(|f| f.title.contains("B320")));
    }

    // ---- B324: hashlib.new with weak algo ----

    #[tokio::test]
    async fn b324_hashlib_new_md5() {
        let files = py("src/hash.py", "h = hashlib.new('md5')\n");
        let ctx = make_ctx(files, Language::Python);
        let findings = BanditRuleDetector.analyze(&ctx).await.unwrap();
        assert!(findings.iter().any(|f| f.title.contains("B324")));
    }

    #[tokio::test]
    async fn b324_hashlib_new_sha1() {
        let files = py("src/hash.py", "h = hashlib.new(\"sha1\")\n");
        let ctx = make_ctx(files, Language::Python);
        let findings = BanditRuleDetector.analyze(&ctx).await.unwrap();
        assert!(findings.iter().any(|f| f.title.contains("B324")));
    }

    // ---- B501: verify=False ----

    #[tokio::test]
    async fn b501_verify_false() {
        let files = py("src/http.py", "requests.get(url, verify=False)\n");
        let ctx = make_ctx(files, Language::Python);
        let findings = BanditRuleDetector.analyze(&ctx).await.unwrap();
        assert!(findings.iter().any(|f| f.title.contains("B501")));
        let f = findings.iter().find(|f| f.title.contains("B501")).unwrap();
        assert_eq!(f.severity, Severity::High);
    }

    #[tokio::test]
    async fn b501_verify_false_with_spaces() {
        let files = py("src/http.py", "requests.get(url, verify = False)\n");
        let ctx = make_ctx(files, Language::Python);
        let findings = BanditRuleDetector.analyze(&ctx).await.unwrap();
        assert!(findings.iter().any(|f| f.title.contains("B501")));
    }

    // ---- B506: yaml.load without SafeLoader ----

    #[tokio::test]
    async fn b506_yaml_load_unsafe() {
        let files = py("src/config.py", "data = yaml.load(f)\n");
        let ctx = make_ctx(files, Language::Python);
        let findings = BanditRuleDetector.analyze(&ctx).await.unwrap();
        assert!(findings.iter().any(|f| f.title.contains("B506")));
        let f = findings.iter().find(|f| f.title.contains("B506")).unwrap();
        assert_eq!(f.severity, Severity::Medium);
    }

    #[tokio::test]
    async fn b506_yaml_load_with_safe_loader_suppressed() {
        let files = py("src/config.py", "data = yaml.load(f, Loader=SafeLoader)\n");
        let ctx = make_ctx(files, Language::Python);
        let findings = BanditRuleDetector.analyze(&ctx).await.unwrap();
        assert!(!findings.iter().any(|f| f.title.contains("B506")));
    }

    // ---- B602: shell=True ----

    #[tokio::test]
    async fn b602_shell_true() {
        let files = py("src/run.py", "subprocess.call(cmd, shell=True)\n");
        let ctx = make_ctx(files, Language::Python);
        let findings = BanditRuleDetector.analyze(&ctx).await.unwrap();
        assert!(findings.iter().any(|f| f.title.contains("B602")));
        let f = findings.iter().find(|f| f.title.contains("B602")).unwrap();
        assert_eq!(f.severity, Severity::High);
        assert_eq!(f.category, FindingCategory::Injection);
    }

    // ---- Cross-cutting: skips test files ----

    #[tokio::test]
    async fn skips_test_files() {
        let files = py("tests/test_app.py", "eval(x)\nexec(y)\npickle.load(f)\n");
        let ctx = make_ctx(files, Language::Python);
        let findings = BanditRuleDetector.analyze(&ctx).await.unwrap();
        assert!(findings.is_empty());
    }

    // ---- Cross-cutting: skips comments ----

    #[tokio::test]
    async fn skips_comments() {
        let files = py("src/app.py", "# eval(user_input)\n");
        let ctx = make_ctx(files, Language::Python);
        let findings = BanditRuleDetector.analyze(&ctx).await.unwrap();
        assert!(findings.is_empty());
    }

    // ---- Cross-cutting: non-Python language ----

    #[tokio::test]
    async fn ignores_non_python() {
        let files = py("src/main.rs", "eval(x)\n");
        let ctx = make_ctx(files, Language::Rust);
        let findings = BanditRuleDetector.analyze(&ctx).await.unwrap();
        assert!(findings.is_empty());
    }

    // ---- Detector name ----

    #[test]
    fn detector_name() {
        assert_eq!(BanditRuleDetector.name(), "bandit");
    }

    // ---- Evidence structure ----

    #[tokio::test]
    async fn findings_have_static_analysis_evidence() {
        let files = py("src/app.py", "eval(x)\n");
        let ctx = make_ctx(files, Language::Python);
        let findings = BanditRuleDetector.analyze(&ctx).await.unwrap();
        assert!(!findings.is_empty());
        let f = &findings[0];
        assert_eq!(f.evidence.len(), 1);
        match &f.evidence[0] {
            Evidence::StaticAnalysis {
                tool,
                rule_id,
                sarif,
            } => {
                assert_eq!(tool, "bandit");
                assert!(!rule_id.is_empty());
                assert!(sarif.get("cwe").is_some());
            }
            _ => panic!("Expected StaticAnalysis evidence"),
        }
    }

    // ---- Multiple findings on separate lines ----

    #[tokio::test]
    async fn multiple_findings_different_lines() {
        let files = py("src/bad.py", "eval(x)\nexec(y)\npickle.load(f)\n");
        let ctx = make_ctx(files, Language::Python);
        let findings = BanditRuleDetector.analyze(&ctx).await.unwrap();
        // eval triggers B307, exec triggers B102, pickle.load triggers B301
        assert!(findings.len() >= 3);
    }

    // ---- Empty source ----

    #[tokio::test]
    async fn empty_source_no_findings() {
        let files = py("src/empty.py", "");
        let ctx = make_ctx(files, Language::Python);
        let findings = BanditRuleDetector.analyze(&ctx).await.unwrap();
        assert!(findings.is_empty());
    }

    // ---- B110 via the analyze method ----

    #[tokio::test]
    async fn b110_via_analyze() {
        let files = py("src/app.py", "try:\n    do_thing()\nexcept:\n    pass\n");
        let ctx = make_ctx(files, Language::Python);
        let findings = BanditRuleDetector.analyze(&ctx).await.unwrap();
        assert!(findings.iter().any(|f| f.title.contains("B110")));
    }

    // ---- BUG: B104 regex false positive on bind() without 0.0.0.0 ----
    // Pattern r#"(?:bind\s*\(\s*.*0\.0\.0\.0|INADDR_ANY)"# has `.*` that
    // matches greedily — but also fires on lines containing INADDR_ANY as
    // a substring in comments or strings, regardless of context.

    #[tokio::test]
    async fn b104_bind_not_triggered_on_safe_localhost() {
        // bind to localhost should NOT trigger B104
        let files = py("src/server.py", "sock.bind(('127.0.0.1', 8080))\n");
        let ctx = make_ctx(files, Language::Python);
        let findings = BanditRuleDetector.analyze(&ctx).await.unwrap();
        assert!(
            !findings.iter().any(|f| f.title.contains("B104")),
            "binding to 127.0.0.1 should not trigger B104"
        );
    }

    // ---- BUG: B110 except: as last line of file (no subsequent lines) ----
    // This is actually safe (no panic) because &lines[i+1..] returns empty
    // slice when i+1 == len. Documenting it as a verified non-bug.
    #[test]
    fn b110_except_as_last_line_no_panic() {
        let findings = find_b110_findings(
            std::path::Path::new("src/app.py"),
            "try:\n    do_thing()\nexcept:",
        );
        // No panic, no finding (no `pass` follows)
        assert!(findings.is_empty());
    }
}
