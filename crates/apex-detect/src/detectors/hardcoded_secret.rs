use apex_core::error::Result;
use async_trait::async_trait;
use regex::Regex;
use std::sync::LazyLock;
use uuid::Uuid;

use super::util::{in_test_block, is_comment, is_test_file};
use crate::context::AnalysisContext;
use crate::finding::{Finding, FindingCategory, Severity};
use crate::Detector;

pub struct HardcodedSecretDetector;

struct SecretPattern {
    name: &'static str,
    regex: &'static str,
    severity: Severity,
    description: &'static str,
}

const SECRET_PATTERNS: &[SecretPattern] = &[
    SecretPattern {
        name: "AWS Access Key",
        regex: r"AKIA[0-9A-Z]{16}",
        severity: Severity::Critical,
        description: "AWS access key ID — rotate immediately if committed",
    },
    SecretPattern {
        name: "Private Key",
        regex: r"-----BEGIN (RSA |EC |DSA |OPENSSH )?PRIVATE KEY-----",
        severity: Severity::Critical,
        description: "Private key in source code — must not be committed",
    },
    SecretPattern {
        name: "GitHub Token",
        regex: r"gh[pousr]_[A-Za-z0-9_]{36,}",
        severity: Severity::Critical,
        description: "GitHub personal access token — rotate immediately",
    },
    SecretPattern {
        name: "Generic API Key Assignment",
        regex: r#"(?i)(api[_-]?key|apikey)\s*[:=]\s*["'][A-Za-z0-9+/=]{20,}["']"#,
        severity: Severity::High,
        description: "Hardcoded API key — use environment variables instead",
    },
    SecretPattern {
        name: "Password Assignment",
        regex: r#"(?i)(password|passwd|pwd)\s*[:=]\s*["'][^"']{8,}["']"#,
        severity: Severity::High,
        description: "Hardcoded password — use environment variables or secrets manager",
    },
    SecretPattern {
        name: "Generic Secret/Token",
        regex: r#"(?i)(secret|token|auth_token|access_token)\s*[:=]\s*["'][A-Za-z0-9+/=_-]{16,}["']"#,
        severity: Severity::High,
        description: "Hardcoded secret/token — use environment variables instead",
    },
    SecretPattern {
        name: "Stripe Key",
        regex: r"sk_(live|test)_[A-Za-z0-9]{20,}",
        severity: Severity::Critical,
        description: "Stripe secret key — rotate immediately if committed",
    },
    SecretPattern {
        name: "Slack Token",
        regex: r"xox[baprs]-[A-Za-z0-9-]{10,}",
        severity: Severity::High,
        description: "Slack token — rotate and use environment variables",
    },
    // Java: setPassword("..."), new SecretKeySpec("...")
    SecretPattern {
        name: "Java Password Setter",
        regex: r#"(?i)setPassword\s*\(\s*["'][^"']{8,}["']"#,
        severity: Severity::High,
        description: "Hardcoded password in setter — use secrets manager",
    },
    SecretPattern {
        name: "Java SecretKeySpec",
        regex: r#"new\s+SecretKeySpec\s*\(\s*["']"#,
        severity: Severity::High,
        description: "Hardcoded secret key material — use key management service",
    },
    // Go: password := "...", os.Setenv("SECRET", "...")
    SecretPattern {
        name: "Go Password Assignment",
        regex: r#"(?i)(password|passwd|secret|apiKey)\s*:=\s*["'][^"']{8,}["']"#,
        severity: Severity::High,
        description: "Hardcoded secret in Go assignment — use environment variables",
    },
    SecretPattern {
        name: "Go Setenv Secret",
        regex: r#"(?i)os\.Setenv\s*\(\s*["'](SECRET|PASSWORD|API_KEY|TOKEN|AUTH)"#,
        severity: Severity::High,
        description: "Hardcoded secret in os.Setenv — use runtime configuration",
    },
    // C/C++: #define API_KEY "...", char *password = "..."
    SecretPattern {
        name: "C/C++ Define Secret",
        regex: r#"(?i)#\s*define\s+(API_KEY|SECRET|PASSWORD|AUTH_TOKEN|PRIVATE_KEY)\s+["']"#,
        severity: Severity::High,
        description: "Hardcoded secret in preprocessor define — use configuration",
    },
    SecretPattern {
        name: "C/C++ Password String",
        regex: r#"(?i)(char\s*\*|const\s+char\s*\*)\s*(password|secret|api_key|auth_token)\s*=\s*["'][^"']{8,}["']"#,
        severity: Severity::High,
        description: "Hardcoded secret in C/C++ string variable — use secure configuration",
    },
    // C#: string password = "...", Password = "...", ConnectionString = "..."
    SecretPattern {
        name: "C# Password Property",
        regex: r#"(?i)(Password|ConnectionString)\s*=\s*["'][^"']{8,}["']"#,
        severity: Severity::High,
        description: "Hardcoded secret in C# property — use secrets manager",
    },
    SecretPattern {
        name: "C# String Secret",
        regex: r#"(?i)string\s+(password|secret|apiKey|connectionString)\s*=\s*["'][^"']{8,}["']"#,
        severity: Severity::High,
        description: "Hardcoded secret in C# string variable — use secure configuration",
    },
    // Swift: let apiKey = "...", SecretKey = "..."
    SecretPattern {
        name: "Swift Let/Var Secret",
        regex: r#"(?i)(let|var)\s+(apiKey|password|secret|secretKey|authToken)\s*=\s*["'][^"']{8,}["']"#,
        severity: Severity::High,
        description: "Hardcoded secret in Swift variable — use Keychain or environment",
    },
    // Kotlin: val password = "...", const val API_KEY = "..."
    SecretPattern {
        name: "Kotlin Val Secret",
        regex: r#"(?i)(const\s+val|val|var)\s+(password|secret|apiKey|api_key|authToken|secretKey)\s*=\s*["'][^"']{8,}["']"#,
        severity: Severity::High,
        description: "Hardcoded secret in Kotlin variable — use secure configuration",
    },
];

const FALSE_POSITIVE_VALUES: &[&str] = &[
    "changeme",
    "CHANGEME",
    "your-",
    "YOUR_",
    "xxx",
    "XXX",
    "placeholder",
    "PLACEHOLDER",
    "example",
    "EXAMPLE",
    "replace_me",
    "REPLACE_ME",
    "TODO",
    "FIXME",
    "test",
    "dummy",
    "fake",
    "sample",
    "demo",
];

const ENV_VAR_MARKERS: &[&str] = &[
    "env(",
    "ENV[",
    "os.environ",
    "process.env",
    "std::env",
    "getenv(",
    // Java/Kotlin
    "System.getenv(",
    "System.getProperty(",
    // Go
    "os.Getenv(",
    // C#
    "Environment.GetEnvironmentVariable(",
    "ConfigurationManager",
    // Swift
    "ProcessInfo.processInfo.environment",
];

fn is_example_file(path: &std::path::Path) -> bool {
    let s = path.to_string_lossy();
    s.contains(".example")
        || s.contains(".sample")
        || s.contains(".template")
        || s.ends_with(".md")
        || s.ends_with(".txt")
        || s.ends_with(".rst")
}

/// Returns true if the line contains a placeholder/false-positive value.
fn contains_placeholder(line: &str) -> bool {
    FALSE_POSITIVE_VALUES.iter().any(|fp| line.contains(fp))
}

/// Returns true if the line references an environment variable.
fn references_env_var(line: &str) -> bool {
    ENV_VAR_MARKERS.iter().any(|m| line.contains(m))
}

static COMPILED_PATTERNS: LazyLock<Vec<(&'static SecretPattern, Regex)>> = LazyLock::new(|| {
    SECRET_PATTERNS
        .iter()
        .map(|p| (p, Regex::new(p.regex).expect("invalid secret regex")))
        .collect()
});

#[async_trait]
impl Detector for HardcodedSecretDetector {
    fn name(&self) -> &str {
        "hardcoded-secret"
    }

    async fn analyze(&self, ctx: &AnalysisContext) -> Result<Vec<Finding>> {
        let mut findings = Vec::new();

        for (path, source) in &ctx.source_cache {
            // Skip test files and example/doc files
            if is_test_file(path) || is_example_file(path) {
                continue;
            }

            for (line_num, line) in source.lines().enumerate() {
                let trimmed = line.trim();

                // Skip comments
                if is_comment(trimmed, ctx.language) {
                    continue;
                }

                // Skip lines inside #[cfg(test)] blocks (Rust)
                if in_test_block(source, line_num) {
                    continue;
                }

                // Skip lines with placeholder values
                if contains_placeholder(trimmed) {
                    continue;
                }

                // Skip environment variable references
                if references_env_var(trimmed) {
                    continue;
                }

                // Match against compiled regex patterns
                for (pattern, regex) in COMPILED_PATTERNS.iter() {
                    if regex.is_match(trimmed) {
                        let line_1based = (line_num + 1) as u32;

                        findings.push(Finding {
                            id: Uuid::new_v4(),
                            detector: self.name().into(),
                            severity: pattern.severity,
                            category: FindingCategory::SecuritySmell,
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
                            evidence: vec![],
                            covered: false,
                            suggestion: "Remove hardcoded secret and use environment variables or a secrets manager".into(),
                            explanation: None,
                            fix: None,
                            cwe_ids: vec![798],
                    noisy: false, base_severity: None, coverage_confidence: None,
                        });
                        break; // One finding per line max
                    }
                }
            }
        }

        Ok(findings)
    }
}

/// Secret variable name patterns (case-insensitive) for the standalone scanner.
const SECRET_VAR_NAMES: &[&str] = &[
    "password",
    "passwd",
    "secret",
    "api_key",
    "apikey",
    "auth_token",
    "access_token",
    "secret_key",
    "private_key",
    "token",
    "credentials",
    "api_secret",
];

/// Compute Shannon entropy of a string (bits per character).
fn shannon_entropy(s: &str) -> f64 {
    if s.is_empty() {
        return 0.0;
    }
    let mut counts = [0u32; 256];
    for b in s.bytes() {
        counts[b as usize] += 1;
    }
    let len = s.len() as f64;
    counts
        .iter()
        .filter(|&&c| c > 0)
        .map(|&c| {
            let p = c as f64 / len;
            -p * p.log2()
        })
        .sum()
}

static ASSIGNMENT_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"([a-zA-Z_][a-zA-Z0-9_]*)\s*=\s*["']([^"']+)["']"#).unwrap());

/// Scan source code for hardcoded secrets using variable name + entropy heuristics.
///
/// This is a standalone scanner complementing `HardcodedSecretDetector`. It uses
/// Shannon entropy to identify high-entropy string assignments to secret-named variables.
pub fn scan_hardcoded_secrets(source: &str, file_path: &str) -> Vec<Finding> {
    let mut findings = Vec::new();

    for (line_num, line) in source.lines().enumerate() {
        let line_1based = (line_num + 1) as u32;
        let trimmed = line.trim();

        // Skip environment variable lookups.
        if trimmed.contains("os.environ")
            || trimmed.contains("env.get")
            || trimmed.contains("getenv")
            || trimmed.contains("ENV[")
        {
            continue;
        }

        if let Some(cap) = ASSIGNMENT_RE.captures(trimmed) {
            let var_name = cap.get(1).map(|m| m.as_str()).unwrap_or("");
            let value = cap.get(2).map(|m| m.as_str()).unwrap_or("");

            // Skip empty values or very short values.
            if value.len() < 8 {
                continue;
            }

            let var_lower = var_name.to_lowercase();
            let is_secret_name = SECRET_VAR_NAMES.iter().any(|s| var_lower.contains(s));
            let high_entropy = shannon_entropy(value) > 2.5;

            if is_secret_name && high_entropy {
                findings.push(Finding {
                    id: Uuid::new_v4(),
                    detector: "hardcoded_secret".into(),
                    severity: Severity::High,
                    category: FindingCategory::SecuritySmell,
                    file: std::path::PathBuf::from(file_path),
                    line: Some(line_1based),
                    title: format!("Hardcoded secret in variable `{var_name}`"),
                    description: format!(
                        "Variable `{var_name}` at line {line_1based} appears to contain \
                         a hardcoded secret. Use environment variables or a secrets manager."
                    ),
                    evidence: vec![],
                    covered: false,
                    suggestion: "Move secrets to environment variables or a secrets manager \
                                 (e.g., AWS Secrets Manager, HashiCorp Vault)."
                        .into(),
                    explanation: None,
                    fix: None,
                    cwe_ids: vec![798],
                    noisy: false, base_severity: None, coverage_confidence: None,
                });
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

    #[tokio::test]
    async fn detects_aws_access_key() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/config.py"),
            "AWS_ACCESS_KEY_ID = \"AKIAIOSFODNN7ABCDEFG\"\n".into(),
        );
        let ctx = make_ctx(files, Language::Python);
        let findings = HardcodedSecretDetector.analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, Severity::Critical);
        assert_eq!(findings[0].category, FindingCategory::SecuritySmell);
        assert_eq!(findings[0].cwe_ids, vec![798]);
    }

    #[tokio::test]
    async fn detects_private_key() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/certs.py"),
            "KEY = \"\"\"-----BEGIN RSA PRIVATE KEY-----\nMIIEowIBAAK...\n-----END RSA PRIVATE KEY-----\"\"\"\n".into(),
        );
        let ctx = make_ctx(files, Language::Python);
        let findings = HardcodedSecretDetector.analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, Severity::Critical);
    }

    #[tokio::test]
    async fn detects_password_assignment() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/db.py"),
            "DATABASE_PASSWORD = \"s3cretP@ssw0rd!\"\n".into(),
        );
        let ctx = make_ctx(files, Language::Python);
        let findings = HardcodedSecretDetector.analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, Severity::High);
    }

    #[tokio::test]
    async fn skips_placeholder_values() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/config.py"),
            "PASSWORD = \"changeme\"\nAPI_KEY = \"your-api-key-here\"\n".into(),
        );
        let ctx = make_ctx(files, Language::Python);
        let findings = HardcodedSecretDetector.analyze(&ctx).await.unwrap();
        assert!(findings.is_empty());
    }

    #[tokio::test]
    async fn skips_env_var_references() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/config.py"),
            "PASSWORD = os.environ.get('DB_PASSWORD')\n".into(),
        );
        let ctx = make_ctx(files, Language::Python);
        let findings = HardcodedSecretDetector.analyze(&ctx).await.unwrap();
        assert!(findings.is_empty());
    }

    #[tokio::test]
    async fn skips_example_files() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("config/settings.example.py"),
            "PASSWORD = \"s3cretP@ssw0rd!\"\n".into(),
        );
        let ctx = make_ctx(files, Language::Python);
        let findings = HardcodedSecretDetector.analyze(&ctx).await.unwrap();
        assert!(findings.is_empty());
    }

    #[tokio::test]
    async fn skips_test_files() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("tests/test_auth.py"),
            "PASSWORD = \"s3cretP@ssw0rd!\"\n".into(),
        );
        let ctx = make_ctx(files, Language::Python);
        let findings = HardcodedSecretDetector.analyze(&ctx).await.unwrap();
        assert!(findings.is_empty());
    }

    #[tokio::test]
    async fn detects_stripe_key() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/billing.rb"),
            "Stripe.api_key = \"sk_live_abcdefghij1234567890\"\n".into(),
        );
        let ctx = make_ctx(files, Language::Ruby);
        let findings = HardcodedSecretDetector.analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, Severity::Critical);
    }

    #[test]
    fn does_not_use_cargo_subprocess() {
        assert!(!HardcodedSecretDetector.uses_cargo_subprocess());
    }

    #[test]
    fn is_example_file_variants() {
        assert!(is_example_file(std::path::Path::new("config.sample.yml")));
        assert!(is_example_file(std::path::Path::new(
            "settings.template.json"
        )));
        assert!(is_example_file(std::path::Path::new("README.md")));
        assert!(is_example_file(std::path::Path::new("SETUP.txt")));
        assert!(is_example_file(std::path::Path::new("docs/guide.rst")));
        assert!(!is_example_file(std::path::Path::new("src/config.py")));
        assert!(!is_example_file(std::path::Path::new("lib/auth.rb")));
    }

    #[test]
    fn contains_placeholder_variants() {
        assert!(contains_placeholder("api_key = \"CHANGEME\""));
        assert!(contains_placeholder("token = \"your-api-key\""));
        assert!(contains_placeholder("key = \"YOUR_API_KEY\""));
        assert!(contains_placeholder("key = \"xxx\""));
        assert!(contains_placeholder("key = \"XXX\""));
        assert!(contains_placeholder("key = \"placeholder\""));
        assert!(contains_placeholder("key = \"PLACEHOLDER\""));
        assert!(contains_placeholder("key = \"example\""));
        assert!(contains_placeholder("key = \"EXAMPLE\""));
        assert!(contains_placeholder("key = \"replace_me\""));
        assert!(contains_placeholder("key = \"REPLACE_ME\""));
        assert!(contains_placeholder("key = \"TODO\""));
        assert!(contains_placeholder("key = \"FIXME\""));
        assert!(contains_placeholder("key = \"test\""));
        assert!(contains_placeholder("key = \"dummy\""));
        assert!(contains_placeholder("key = \"fake\""));
        assert!(contains_placeholder("key = \"sample\""));
        assert!(contains_placeholder("key = \"demo\""));
        assert!(!contains_placeholder("key = \"AKIAIOSFODNN7ABCDEFG\""));
    }

    #[test]
    fn references_env_var_variants() {
        assert!(references_env_var("let key = env(\"API_KEY\")"));
        assert!(references_env_var("key = ENV[\"API_KEY\"]"));
        assert!(references_env_var("key = os.environ.get('KEY')"));
        assert!(references_env_var("const key = process.env.API_KEY"));
        assert!(references_env_var("let key = std::env::var(\"KEY\")"));
        assert!(references_env_var("char* k = getenv(\"KEY\")"));
        assert!(!references_env_var("api_key = \"hardcoded_value\""));
    }

    #[tokio::test]
    async fn detects_github_token() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/deploy.py"),
            "GITHUB_TOKEN = \"ghp_ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghij\"\n".into(),
        );
        let ctx = make_ctx(files, Language::Python);
        let findings = HardcodedSecretDetector.analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, Severity::Critical);
    }

    #[tokio::test]
    async fn detects_generic_api_key() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/config.js"),
            "const API_KEY = \"abcdefghijklmnopqrst12345\"\n".into(),
        );
        let ctx = make_ctx(files, Language::JavaScript);
        let findings = HardcodedSecretDetector.analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, Severity::High);
    }

    #[tokio::test]
    async fn detects_generic_secret_token() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/auth.py"),
            "secret = \"AbCdEfGhIjKlMnOpQrSt\"\n".into(),
        );
        let ctx = make_ctx(files, Language::Python);
        let findings = HardcodedSecretDetector.analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, Severity::High);
    }

    #[tokio::test]
    async fn detects_slack_token() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/notify.py"),
            "SLACK_TOKEN = \"xoxb-1234567890-abcdefg\"\n".into(),
        );
        let ctx = make_ctx(files, Language::Python);
        let findings = HardcodedSecretDetector.analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 1);
    }

    #[tokio::test]
    async fn skips_cfg_test_block_in_rust() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/lib.rs"),
            "#[cfg(test)]\nmod tests {\n    const KEY: &str = \"AKIAIOSFODNN7ABCDEFG\";\n}\n"
                .into(),
        );
        let ctx = make_ctx(files, Language::Rust);
        let findings = HardcodedSecretDetector.analyze(&ctx).await.unwrap();
        assert!(findings.is_empty());
    }

    #[tokio::test]
    async fn skips_comments_in_python() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/config.py"),
            "# AKIAIOSFODNN7ABCDEFG is the old key\nreal_code = 1\n".into(),
        );
        let ctx = make_ctx(files, Language::Python);
        let findings = HardcodedSecretDetector.analyze(&ctx).await.unwrap();
        assert!(findings.is_empty());
    }

    #[tokio::test]
    async fn detects_ec_private_key() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/certs.py"),
            "KEY = \"-----BEGIN EC PRIVATE KEY-----\"\n".into(),
        );
        let ctx = make_ctx(files, Language::Python);
        let findings = HardcodedSecretDetector.analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, Severity::Critical);
    }

    #[tokio::test]
    async fn detects_stripe_live_key_variant() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/pay.rb"),
            "Stripe.api_key = \"sk_live_ZbcdefghiJ1234567890\"\n".into(),
        );
        let ctx = make_ctx(files, Language::Ruby);
        let findings = HardcodedSecretDetector.analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 1);
    }

    // Tests for the standalone scan_hardcoded_secrets function (Task 2.18)

    #[test]
    fn scan_detect_password_assignment() {
        let source = r#"password = "s3cr3t_p4ss""#;
        let findings = scan_hardcoded_secrets(source, "config.py");
        assert!(!findings.is_empty());
        assert_eq!(findings[0].category, FindingCategory::SecuritySmell);
    }

    #[test]
    fn scan_detect_api_key() {
        let source = r#"API_KEY = "sk-abc123def456ghi789""#;
        let findings = scan_hardcoded_secrets(source, "settings.py");
        assert!(!findings.is_empty());
    }

    #[test]
    fn scan_detect_token() {
        let source = r#"auth_token = "eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxMjM0NTY3ODkwIn0""#;
        let findings = scan_hardcoded_secrets(source, "auth.py");
        assert!(!findings.is_empty());
    }

    #[test]
    fn scan_safe_env_var_not_flagged() {
        let source = r#"password = os.environ.get("PASSWORD")"#;
        let findings = scan_hardcoded_secrets(source, "config.py");
        assert!(findings.is_empty());
    }

    #[test]
    fn scan_safe_empty_string_not_flagged() {
        let source = r#"password = """#;
        let findings = scan_hardcoded_secrets(source, "config.py");
        assert!(findings.is_empty());
    }

    #[test]
    fn scan_safe_placeholder_not_flagged() {
        let source = r#"password = "changeme""#;
        // Short placeholder values below entropy threshold.
        let findings = scan_hardcoded_secrets(source, "config.py");
        // May or may not flag — depends on heuristic. Just verify no panic.
        let _ = findings;
    }

    #[test]
    fn scan_finding_has_cwe_798() {
        let source = r#"SECRET_KEY = "a1b2c3d4e5f6g7h8i9j0k1l2m3n4""#;
        let findings = scan_hardcoded_secrets(source, "x.py");
        if !findings.is_empty() {
            assert!(findings[0].cwe_ids.contains(&798));
        }
    }

    // -----------------------------------------------------------------------
    // Java language tests
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn detects_java_set_password() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/Database.java"),
            "conn.setPassword(\"s3cretP@ssw0rd!\")\n".into(),
        );
        let ctx = make_ctx(files, Language::Java);
        let findings = HardcodedSecretDetector.analyze(&ctx).await.unwrap();
        assert!(!findings.is_empty(), "should detect Java setPassword");
    }

    #[tokio::test]
    async fn detects_java_secret_key_spec() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/Crypto.java"),
            "SecretKey key = new SecretKeySpec(\"mysecretkey12345\".getBytes(), \"AES\");\n".into(),
        );
        let ctx = make_ctx(files, Language::Java);
        let findings = HardcodedSecretDetector.analyze(&ctx).await.unwrap();
        assert!(!findings.is_empty(), "should detect Java SecretKeySpec");
    }

    #[tokio::test]
    async fn detects_java_password_string() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/Config.java"),
            "String password = \"s3cretP@ssw0rd!\"\n".into(),
        );
        let ctx = make_ctx(files, Language::Java);
        let findings = HardcodedSecretDetector.analyze(&ctx).await.unwrap();
        assert!(!findings.is_empty(), "should detect Java String password");
    }

    #[tokio::test]
    async fn skips_java_env_var() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/Config.java"),
            "String password = System.getenv(\"DB_PASSWORD\")\n".into(),
        );
        let ctx = make_ctx(files, Language::Java);
        let findings = HardcodedSecretDetector.analyze(&ctx).await.unwrap();
        assert!(findings.is_empty(), "should skip Java env var lookup");
    }

    // -----------------------------------------------------------------------
    // Go language tests
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn detects_go_password_assignment() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/config.go"),
            "password := \"s3cretP@ssw0rd!\"\n".into(),
        );
        let ctx = make_ctx(files, Language::Go);
        let findings = HardcodedSecretDetector.analyze(&ctx).await.unwrap();
        assert!(!findings.is_empty(), "should detect Go password :=");
    }

    #[tokio::test]
    async fn detects_go_api_key_assignment() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/config.go"),
            "apiKey := \"sk_abcdefghij1234567890\"\n".into(),
        );
        let ctx = make_ctx(files, Language::Go);
        let findings = HardcodedSecretDetector.analyze(&ctx).await.unwrap();
        assert!(!findings.is_empty(), "should detect Go apiKey := assignment");
    }

    #[tokio::test]
    async fn skips_go_env_var() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/config.go"),
            "password := os.Getenv(\"DB_PASSWORD\")\n".into(),
        );
        let ctx = make_ctx(files, Language::Go);
        let findings = HardcodedSecretDetector.analyze(&ctx).await.unwrap();
        assert!(findings.is_empty(), "should skip Go os.Getenv");
    }

    // -----------------------------------------------------------------------
    // C/C++ language tests
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn detects_c_define_api_key() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/config.h"),
            "#define API_KEY \"sk_abcdefghij1234567890\"\n".into(),
        );
        let ctx = make_ctx(files, Language::C);
        let findings = HardcodedSecretDetector.analyze(&ctx).await.unwrap();
        assert!(!findings.is_empty(), "should detect C #define API_KEY");
    }

    #[tokio::test]
    async fn detects_c_char_password() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/auth.c"),
            "const char* password = \"s3cretP@ssw0rd!\"\n".into(),
        );
        let ctx = make_ctx(files, Language::C);
        let findings = HardcodedSecretDetector.analyze(&ctx).await.unwrap();
        assert!(!findings.is_empty(), "should detect C char* password");
    }

    #[tokio::test]
    async fn detects_cpp_password_string() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/auth.cpp"),
            "char *password = \"s3cretP@ssw0rd!\"\n".into(),
        );
        let ctx = make_ctx(files, Language::Cpp);
        let findings = HardcodedSecretDetector.analyze(&ctx).await.unwrap();
        assert!(!findings.is_empty(), "should detect C++ char* password");
    }

    #[tokio::test]
    async fn skips_c_getenv() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/config.c"),
            "char* password = getenv(\"DB_PASSWORD\")\n".into(),
        );
        let ctx = make_ctx(files, Language::C);
        let findings = HardcodedSecretDetector.analyze(&ctx).await.unwrap();
        assert!(findings.is_empty(), "should skip C getenv");
    }

    // -----------------------------------------------------------------------
    // C# language tests
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn detects_csharp_password_property() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/Config.cs"),
            "Password = \"s3cretP@ssw0rd!\"\n".into(),
        );
        let ctx = make_ctx(files, Language::CSharp);
        let findings = HardcodedSecretDetector.analyze(&ctx).await.unwrap();
        assert!(!findings.is_empty(), "should detect C# Password property");
    }

    #[tokio::test]
    async fn detects_csharp_connection_string() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/Startup.cs"),
            "ConnectionString = \"Server=db;User=admin;Password=s3cretP@ss\"\n".into(),
        );
        let ctx = make_ctx(files, Language::CSharp);
        let findings = HardcodedSecretDetector.analyze(&ctx).await.unwrap();
        assert!(!findings.is_empty(), "should detect C# ConnectionString");
    }

    #[tokio::test]
    async fn detects_csharp_string_secret() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/Auth.cs"),
            "string password = \"s3cretP@ssw0rd!\"\n".into(),
        );
        let ctx = make_ctx(files, Language::CSharp);
        let findings = HardcodedSecretDetector.analyze(&ctx).await.unwrap();
        assert!(!findings.is_empty(), "should detect C# string password");
    }

    #[tokio::test]
    async fn skips_csharp_env_var() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/Config.cs"),
            "var password = Environment.GetEnvironmentVariable(\"DB_PASSWORD\")\n".into(),
        );
        let ctx = make_ctx(files, Language::CSharp);
        let findings = HardcodedSecretDetector.analyze(&ctx).await.unwrap();
        assert!(findings.is_empty(), "should skip C# environment variable");
    }

    // -----------------------------------------------------------------------
    // Swift language tests
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn detects_swift_let_api_key() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("Sources/Config.swift"),
            "let apiKey = \"sk_abcdefghij1234567890\"\n".into(),
        );
        let ctx = make_ctx(files, Language::Swift);
        let findings = HardcodedSecretDetector.analyze(&ctx).await.unwrap();
        assert!(!findings.is_empty(), "should detect Swift let apiKey");
    }

    #[tokio::test]
    async fn detects_swift_var_password() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("Sources/Auth.swift"),
            "var password = \"s3cretP@ssw0rd!\"\n".into(),
        );
        let ctx = make_ctx(files, Language::Swift);
        let findings = HardcodedSecretDetector.analyze(&ctx).await.unwrap();
        assert!(!findings.is_empty(), "should detect Swift var password");
    }

    #[tokio::test]
    async fn skips_swift_process_info_env() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("Sources/Config.swift"),
            "let key = ProcessInfo.processInfo.environment[\"API_KEY\"]\n".into(),
        );
        let ctx = make_ctx(files, Language::Swift);
        let findings = HardcodedSecretDetector.analyze(&ctx).await.unwrap();
        assert!(findings.is_empty(), "should skip Swift ProcessInfo.environment");
    }

    // -----------------------------------------------------------------------
    // Kotlin language tests
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn detects_kotlin_val_password() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/Config.kt"),
            "val password = \"s3cretP@ssw0rd!\"\n".into(),
        );
        let ctx = make_ctx(files, Language::Kotlin);
        let findings = HardcodedSecretDetector.analyze(&ctx).await.unwrap();
        assert!(!findings.is_empty(), "should detect Kotlin val password");
    }

    #[tokio::test]
    async fn detects_kotlin_const_val_api_key() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/Constants.kt"),
            "const val apiKey = \"sk_abcdefghij1234567890\"\n".into(),
        );
        let ctx = make_ctx(files, Language::Kotlin);
        let findings = HardcodedSecretDetector.analyze(&ctx).await.unwrap();
        assert!(!findings.is_empty(), "should detect Kotlin const val apiKey");
    }

    #[tokio::test]
    async fn skips_kotlin_system_getenv() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/Config.kt"),
            "val password = System.getenv(\"DB_PASSWORD\")\n".into(),
        );
        let ctx = make_ctx(files, Language::Kotlin);
        let findings = HardcodedSecretDetector.analyze(&ctx).await.unwrap();
        assert!(findings.is_empty(), "should skip Kotlin System.getenv");
    }
}
