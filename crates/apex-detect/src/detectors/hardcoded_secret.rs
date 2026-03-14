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
                        });
                        break; // One finding per line max
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
    use crate::config::DetectConfig;
    use crate::context::AnalysisContext;
    use apex_core::types::Language;
    use apex_coverage::CoverageOracle;
    use std::collections::HashMap;
    use std::path::PathBuf;
    use std::sync::Arc;

    fn make_ctx(files: HashMap<PathBuf, String>, lang: Language) -> AnalysisContext {
        AnalysisContext {
            target_root: PathBuf::from("/tmp/test"),
            language: lang,
            oracle: Arc::new(CoverageOracle::new()),
            file_paths: HashMap::new(),
            known_bugs: vec![],
            source_cache: files,
            fuzz_corpus: None,
            config: DetectConfig::default(),
            runner: Arc::new(apex_core::command::RealCommandRunner),
            cpg: None,
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
}
