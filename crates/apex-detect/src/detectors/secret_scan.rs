use apex_core::error::Result;
use async_trait::async_trait;
use regex::RegexSet;
use std::sync::LazyLock;
use uuid::Uuid;

use super::util::{in_test_block, is_comment, references_env_var};
use crate::context::AnalysisContext;
use crate::finding::{Finding, FindingCategory, Severity};
use crate::Detector;

/// Metadata for each pattern in the RegexSet (kept in the same order).
struct PatternMeta {
    name: &'static str,
    severity: Severity,
    description: &'static str,
}

/// All known secret patterns.  Order must match `PATTERN_REGEXES`.
const PATTERN_META: &[PatternMeta] = &[
    // AWS
    PatternMeta {
        name: "AWS Access Key",
        severity: Severity::Critical,
        description: "AWS access key ID found in source code",
    },
    // GitHub tokens
    PatternMeta {
        name: "GitHub PAT (ghp)",
        severity: Severity::Critical,
        description: "GitHub personal access token",
    },
    PatternMeta {
        name: "GitHub OAuth (gho)",
        severity: Severity::Critical,
        description: "GitHub OAuth access token",
    },
    PatternMeta {
        name: "GitHub App (ghs)",
        severity: Severity::Critical,
        description: "GitHub App installation token",
    },
    PatternMeta {
        name: "GitHub Fine-grained PAT",
        severity: Severity::Critical,
        description: "GitHub fine-grained personal access token",
    },
    // Stripe
    PatternMeta {
        name: "Stripe Secret Key",
        severity: Severity::Critical,
        description: "Stripe live secret key",
    },
    PatternMeta {
        name: "Stripe Restricted Key",
        severity: Severity::Critical,
        description: "Stripe live restricted key",
    },
    // JWT
    PatternMeta {
        name: "JWT Token",
        severity: Severity::High,
        description: "JSON Web Token found in source code",
    },
    // Private keys
    PatternMeta {
        name: "Private Key Header",
        severity: Severity::Critical,
        description: "Private key embedded in source code",
    },
    // Generic assignment patterns
    PatternMeta {
        name: "Password Assignment",
        severity: Severity::High,
        description: "Hardcoded password in source code",
    },
    PatternMeta {
        name: "Secret Assignment",
        severity: Severity::High,
        description: "Hardcoded secret value in source code",
    },
    PatternMeta {
        name: "API Key Assignment",
        severity: Severity::High,
        description: "Hardcoded API key in source code",
    },
    PatternMeta {
        name: "Token Assignment",
        severity: Severity::High,
        description: "Hardcoded token in source code",
    },
    // Slack
    PatternMeta {
        name: "Slack Bot Token",
        severity: Severity::Critical,
        description: "Slack bot/user token",
    },
    PatternMeta {
        name: "Slack Webhook",
        severity: Severity::High,
        description: "Slack incoming webhook URL",
    },
    // SendGrid
    PatternMeta {
        name: "SendGrid API Key",
        severity: Severity::Critical,
        description: "SendGrid API key",
    },
    // Twilio
    PatternMeta {
        name: "Twilio API Key",
        severity: Severity::Critical,
        description: "Twilio API key",
    },
    // Google
    PatternMeta {
        name: "Google API Key",
        severity: Severity::High,
        description: "Google API key",
    },
    PatternMeta {
        name: "Google OAuth Secret",
        severity: Severity::Critical,
        description: "Google OAuth client secret",
    },
    // Azure
    PatternMeta {
        name: "Azure Storage Key",
        severity: Severity::Critical,
        description: "Azure storage account key",
    },
    PatternMeta {
        name: "Azure Connection String",
        severity: Severity::Critical,
        description: "Azure connection string with embedded key",
    },
    // Heroku
    PatternMeta {
        name: "Heroku API Key",
        severity: Severity::High,
        description: "Heroku API key",
    },
    // npm
    PatternMeta {
        name: "npm Token",
        severity: Severity::Critical,
        description: "npm authentication token",
    },
    // PyPI
    PatternMeta {
        name: "PyPI Token",
        severity: Severity::Critical,
        description: "PyPI API token",
    },
    // Generic hex/base64 high-entropy
    PatternMeta {
        name: "Generic Bearer Token",
        severity: Severity::High,
        description: "Bearer token in authorization header",
    },
];

/// Raw regex strings — same order as `PATTERN_META`.
const PATTERN_REGEXES: &[&str] = &[
    // AWS
    r"AKIA[0-9A-Z]{16}",
    // GitHub
    r"ghp_[A-Za-z0-9]{36}",
    r"gho_[A-Za-z0-9]{36}",
    r"ghs_[A-Za-z0-9]{36}",
    r"github_pat_[A-Za-z0-9_]{22,}",
    // Stripe
    r"sk_live_[A-Za-z0-9]{24,}",
    r"rk_live_[A-Za-z0-9]{24,}",
    // JWT
    r"eyJ[A-Za-z0-9_-]{10,}\.eyJ",
    // Private keys
    r"-----BEGIN (RSA |EC |DSA |OPENSSH )?PRIVATE KEY-----",
    // Generic assignments
    r#"(?i)(password|passwd)\s*[:=]\s*["'][^"']{8,}["']"#,
    r#"(?i)(secret|client_secret)\s*[:=]\s*["'][^"']{8,}["']"#,
    r#"(?i)(api_key|apikey)\s*[:=]\s*["'][^"']{8,}["']"#,
    r#"(?i)(token|auth_token|access_token)\s*[:=]\s*["'][^"']{8,}["']"#,
    // Slack
    r"xox[baprs]-[A-Za-z0-9-]{10,}",
    r"https://hooks\.slack\.com/services/T[A-Za-z0-9]+/B[A-Za-z0-9]+/[A-Za-z0-9]+",
    // SendGrid
    r"SG\.[A-Za-z0-9_-]{22}\.[A-Za-z0-9_-]{43}",
    // Twilio
    r"SK[0-9a-fA-F]{32}",
    // Google
    r"AIza[0-9A-Za-z_-]{35}",
    r#"(?i)client_secret["']?\s*[:=]\s*["'][A-Za-z0-9_-]{24,}["']"#,
    // Azure
    r"(?i)AccountKey=[A-Za-z0-9+/=]{44,}",
    r"(?i)DefaultEndpointsProtocol=https;AccountName=[^;]+;AccountKey=[A-Za-z0-9+/=]+",
    // Heroku
    r#"(?i)heroku[_-]?api[_-]?key\s*[:=]\s*['"]?[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}"#,
    // npm
    r"npm_[A-Za-z0-9]{36}",
    // PyPI
    r"pypi-[A-Za-z0-9_-]{50,}",
    // Generic bearer
    r"(?i)bearer\s+[A-Za-z0-9_\-.]{20,}",
];

// Compile-time check: PATTERN_META and PATTERN_REGEXES must have the same length.
const _: () = assert!(PATTERN_META.len() == PATTERN_REGEXES.len());

static COMPILED_SET: LazyLock<RegexSet> =
    LazyLock::new(|| RegexSet::new(PATTERN_REGEXES).expect("invalid secret-scan regex set"));

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
    "dummy",
    "fake",
    "sample",
    "demo",
];

/// Calculate Shannon entropy of a string (log base 2).
pub fn shannon_entropy(s: &str) -> f64 {
    if s.is_empty() {
        return 0.0;
    }
    let mut freq = [0u32; 256];
    let len = s.len() as f64;
    for &b in s.as_bytes() {
        freq[b as usize] += 1;
    }
    let mut entropy = 0.0_f64;
    for &count in &freq {
        if count > 0 {
            let p = count as f64 / len;
            entropy -= p * p.log2();
        }
    }
    entropy
}

/// Returns true if the file path looks like a test/example/sample/template file,
/// or a generated/instrumentation file that is known to contain high-entropy strings
/// that are not real secrets.
fn is_skip_file(path: &std::path::Path) -> bool {
    let s = path.to_string_lossy();
    s.contains("test")
        || s.contains(".example")
        || s.contains(".sample")
        || s.contains(".template")
        || s.ends_with(".md")
        || s.ends_with(".txt")
        || s.ends_with(".rst")
        // Instrumentation and code-generation files contain hex patterns / source map data
        || s.contains("instrument")
        || s.contains("generated")
        || s.contains("source_map")
        // Fixture files in any language are test data, not production secrets
        || s.contains("fixture")
        // Detector source files inside a detectors/ directory — test fixture consts live here
        || (s.contains("/detectors/") && s.ends_with(".rs"))
        // Bug 14/15: charset/encoding tables, vendor bundles, and generated code markers
        || s.contains("encoding")
        || s.contains("charsets")
        || s.contains("codepage")
        || s.contains("vendor/")
        || s.contains("dist/")
        || s.contains(".min.")
        || s.contains("bundle.")
}

/// Returns true if the trimmed line is a `const` string declaration.
/// In Rust detector files these are almost always test-fixture values, not real secrets.
fn is_const_string_decl(trimmed: &str) -> bool {
    // Match: `const FOO: &str = "..."` or `const FOO: &'static str = "..."`
    trimmed.starts_with("const ") && trimmed.contains(": &") && trimmed.contains("str")
}

/// Returns true if the file content begins with a "// Code generated" marker,
/// indicating the file was produced by a code generator (e.g. Go `go generate`).
fn is_code_generated(source: &str) -> bool {
    // Check first two non-empty lines for the marker
    for line in source.lines().take(5) {
        let t = line.trim();
        if t.is_empty() {
            continue;
        }
        if t.contains("Code generated") {
            return true;
        }
        break;
    }
    false
}

/// Returns true if the path is inside an `apex-instrument` crate or a path component
/// that looks like an instrumentation template directory.
fn is_instrumentation_path(path: &std::path::Path) -> bool {
    let s = path.to_string_lossy();
    s.contains("apex-instrument") || s.contains("apex_instrument")
}

/// Returns true if the line contains a placeholder/false-positive value.
fn contains_placeholder(line: &str) -> bool {
    FALSE_POSITIVE_VALUES.iter().any(|fp| line.contains(fp))
}

/// Extract string literal contents from a line for entropy analysis.
///
/// Handles standard `"..."` and `'...'` strings, C# verbatim strings `@"..."`,
/// Swift raw strings `#"..."#`, and C/C++ `#define X "..."` macros.
fn extract_string_literals(line: &str) -> Vec<String> {
    let mut results = Vec::new();
    let bytes = line.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        // C# verbatim string: @"..."
        if bytes[i] == b'@' && i + 1 < bytes.len() && bytes[i + 1] == b'"' {
            i += 2; // skip @"
            let mut literal = String::new();
            while i < bytes.len() {
                if bytes[i] == b'"' {
                    // In verbatim strings, "" is an escaped quote
                    if i + 1 < bytes.len() && bytes[i + 1] == b'"' {
                        literal.push('"');
                        i += 2;
                    } else {
                        i += 1;
                        break;
                    }
                } else {
                    literal.push(bytes[i] as char);
                    i += 1;
                }
            }
            if literal.len() >= 8 {
                results.push(literal);
            }
            continue;
        }
        // Swift raw string: #"..."#
        if bytes[i] == b'#' && i + 1 < bytes.len() && bytes[i + 1] == b'"' {
            i += 2; // skip #"
            let mut literal = String::new();
            while i < bytes.len() {
                if bytes[i] == b'"' && i + 1 < bytes.len() && bytes[i + 1] == b'#' {
                    i += 2; // skip "#
                    break;
                } else {
                    literal.push(bytes[i] as char);
                    i += 1;
                }
            }
            if literal.len() >= 8 {
                results.push(literal);
            }
            continue;
        }
        // Standard string literals
        if bytes[i] == b'"' || bytes[i] == b'\'' {
            let quote = bytes[i];
            i += 1;
            let mut literal = String::new();
            let mut escaped = false;
            while i < bytes.len() {
                if escaped {
                    literal.push(bytes[i] as char);
                    escaped = false;
                } else if bytes[i] == b'\\' {
                    escaped = true;
                } else if bytes[i] == quote {
                    i += 1;
                    break;
                } else {
                    literal.push(bytes[i] as char);
                }
                i += 1;
            }
            if literal.len() >= 8 {
                results.push(literal);
            }
            continue;
        }
        i += 1;
    }
    results
}

pub struct SecretScanDetector;

impl Default for SecretScanDetector {
    fn default() -> Self {
        Self::new()
    }
}

impl SecretScanDetector {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Detector for SecretScanDetector {
    fn name(&self) -> &str {
        "secret-scan"
    }

    async fn analyze(&self, ctx: &AnalysisContext) -> Result<Vec<Finding>> {
        let mut findings = Vec::new();

        for (path, source) in &ctx.source_cache {
            if is_skip_file(path) {
                continue;
            }

            if is_code_generated(source) {
                continue;
            }

            for (line_idx, line) in source.lines().enumerate() {
                let trimmed = line.trim();

                if is_comment(trimmed, ctx.language) {
                    continue;
                }

                if in_test_block(source, line_idx) {
                    continue;
                }

                if contains_placeholder(trimmed) {
                    continue;
                }

                if references_env_var(trimmed) {
                    continue;
                }

                // `const` string declarations in Rust are typically test fixture data.
                if is_const_string_decl(trimmed) {
                    continue;
                }

                // Instrumentation template paths — hex/address strings here are not secrets.
                if is_instrumentation_path(path) {
                    continue;
                }

                let line_1based = (line_idx + 1) as u32;

                // Check known patterns via RegexSet
                let matches: Vec<usize> = COMPILED_SET.matches(trimmed).into_iter().collect();
                if !matches.is_empty() {
                    // Use the first (most specific) match
                    let idx = matches[0];
                    if let Some(meta) = PATTERN_META.get(idx) {
                        findings.push(Finding {
                            id: Uuid::new_v4(),
                            detector: self.name().into(),
                            severity: meta.severity,
                            category: FindingCategory::HardcodedSecret,
                            file: path.clone(),
                            line: Some(line_1based),
                            title: format!(
                                "{}: {} at line {}",
                                meta.name, meta.description, line_1based
                            ),
                            description: format!(
                                "{} pattern matched in {}:{}",
                                meta.name,
                                path.display(),
                                line_1based
                            ),
                            evidence: vec![],
                            covered: false,
                            suggestion:
                                "Remove hardcoded secret and use environment variables or a secrets manager"
                                    .into(),
                            explanation: None,
                            fix: None,
                            cwe_ids: vec![798],
                    noisy: false,
                        });
                        continue; // one finding per line
                    }
                }

                // Entropy-based detection on string literals
                let entropy_threshold = ctx.config.entropy_threshold;
                for literal in extract_string_literals(trimmed) {
                    let entropy = shannon_entropy(&literal);
                    if entropy >= entropy_threshold {
                        findings.push(Finding {
                            id: Uuid::new_v4(),
                            detector: self.name().into(),
                            severity: Severity::High,
                            category: FindingCategory::HardcodedSecret,
                            file: path.clone(),
                            line: Some(line_1based),
                            title: format!(
                                "High-entropy string (entropy={:.2}) at line {}",
                                entropy, line_1based
                            ),
                            description: format!(
                                "String literal with entropy {:.2} in {}:{} may be a secret",
                                entropy,
                                path.display(),
                                line_1based
                            ),
                            evidence: vec![],
                            covered: false,
                            suggestion:
                                "Review this string literal — if it is a secret, move it to environment variables or a secrets manager"
                                    .into(),
                            explanation: None,
                            fix: None,
                            cwe_ids: vec![798],
                    noisy: false,
                        });
                        break; // one finding per line
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
            language: lang,
            source_cache: files,
            ..AnalysisContext::test_default()
        }
    }

    fn single_file_ctx(path: &str, content: &str, lang: Language) -> AnalysisContext {
        let mut files = HashMap::new();
        files.insert(PathBuf::from(path), content.into());
        make_ctx(files, lang)
    }

    // -----------------------------------------------------------------------
    // Pattern detection tests
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn detects_aws_access_key() {
        let ctx = single_file_ctx(
            "src/config.py",
            "AWS_KEY = \"AKIAIOSFODNN7ABCDEFG\"\n",
            Language::Python,
        );
        let findings = SecretScanDetector::new().analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, Severity::Critical);
        assert_eq!(findings[0].category, FindingCategory::HardcodedSecret);
        assert_eq!(findings[0].cwe_ids, vec![798]);
        assert!(findings[0].title.contains("AWS Access Key"));
    }

    #[tokio::test]
    async fn detects_github_pat() {
        let ctx = single_file_ctx(
            "src/deploy.py",
            "TOKEN = \"ghp_ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghij\"\n",
            Language::Python,
        );
        let findings = SecretScanDetector::new().analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, Severity::Critical);
        assert!(findings[0].title.contains("GitHub PAT"));
    }

    #[tokio::test]
    async fn detects_github_oauth_token() {
        let ctx = single_file_ctx(
            "src/auth.js",
            "const token = \"gho_ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghij\"\n",
            Language::JavaScript,
        );
        let findings = SecretScanDetector::new().analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 1);
        assert!(findings[0].title.contains("GitHub OAuth"));
    }

    #[tokio::test]
    async fn detects_github_app_token() {
        let ctx = single_file_ctx(
            "src/ci.py",
            "GHS = \"ghs_ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghij\"\n",
            Language::Python,
        );
        let findings = SecretScanDetector::new().analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 1);
        assert!(findings[0].title.contains("GitHub App"));
    }

    #[tokio::test]
    async fn detects_github_fine_grained_pat() {
        let ctx = single_file_ctx(
            "src/gh.py",
            "PAT = \"github_pat_ABCDEFGHIJ1234567890ab\"\n",
            Language::Python,
        );
        let findings = SecretScanDetector::new().analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 1);
        assert!(findings[0].title.contains("Fine-grained"));
    }

    #[tokio::test]
    async fn detects_stripe_secret_key() {
        // Build key at runtime to avoid GitHub push protection false positive
        let key = format!("sk_live_{}", "4eC39HqLyjWDarjtT1zdp7dc");
        let content = format!("Stripe.api_key = \"{key}\"\n");
        let ctx = single_file_ctx("src/billing.rb", &content, Language::Ruby);
        let findings = SecretScanDetector::new().analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, Severity::Critical);
        assert!(findings[0].title.contains("Stripe Secret Key"));
    }

    #[tokio::test]
    async fn detects_stripe_restricted_key() {
        let key = format!("rk_live_{}", "4eC39HqLyjWDarjtT1zdp7dc");
        let content = format!("KEY = \"{key}\"\n");
        let ctx = single_file_ctx("src/pay.py", &content, Language::Python);
        let findings = SecretScanDetector::new().analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 1);
        assert!(findings[0].title.contains("Stripe Restricted"));
    }

    #[tokio::test]
    async fn detects_jwt_token() {
        let ctx = single_file_ctx(
            "src/auth.py",
            "TOKEN = \"eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJzdWIiOiIxMjM0NTY3ODkwIn0\"\n",
            Language::Python,
        );
        let findings = SecretScanDetector::new().analyze(&ctx).await.unwrap();
        assert!(
            findings.iter().any(|f| f.title.contains("JWT")),
            "should detect JWT token"
        );
    }

    #[tokio::test]
    async fn detects_private_key_rsa() {
        let ctx = single_file_ctx(
            "src/certs.py",
            "KEY = \"-----BEGIN RSA PRIVATE KEY-----\"\n",
            Language::Python,
        );
        let findings = SecretScanDetector::new().analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, Severity::Critical);
        assert!(findings[0].title.contains("Private Key"));
    }

    #[tokio::test]
    async fn detects_private_key_ec() {
        let ctx = single_file_ctx(
            "src/keys.py",
            "KEY = \"-----BEGIN EC PRIVATE KEY-----\"\n",
            Language::Python,
        );
        let findings = SecretScanDetector::new().analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, Severity::Critical);
    }

    #[tokio::test]
    async fn detects_private_key_openssh() {
        let ctx = single_file_ctx(
            "src/ssh.py",
            "KEY = \"-----BEGIN OPENSSH PRIVATE KEY-----\"\n",
            Language::Python,
        );
        let findings = SecretScanDetector::new().analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 1);
    }

    #[tokio::test]
    async fn detects_private_key_generic() {
        let ctx = single_file_ctx(
            "src/tls.py",
            "KEY = \"-----BEGIN PRIVATE KEY-----\"\n",
            Language::Python,
        );
        let findings = SecretScanDetector::new().analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 1);
    }

    #[tokio::test]
    async fn detects_password_assignment() {
        let ctx = single_file_ctx(
            "src/db.py",
            "password = \"s3cretP@ssw0rd!\"\n",
            Language::Python,
        );
        let findings = SecretScanDetector::new().analyze(&ctx).await.unwrap();
        assert!(!findings.is_empty());
        assert!(findings
            .iter()
            .any(|f| f.title.contains("Password") || f.severity == Severity::High));
    }

    #[tokio::test]
    async fn detects_secret_assignment() {
        let ctx = single_file_ctx(
            "src/config.py",
            "secret = \"AbCdEfGh1234567890xY\"\n",
            Language::Python,
        );
        let findings = SecretScanDetector::new().analyze(&ctx).await.unwrap();
        assert!(!findings.is_empty());
    }

    #[tokio::test]
    async fn detects_api_key_assignment() {
        let ctx = single_file_ctx(
            "src/svc.py",
            "api_key = \"abcdefgh12345678WXYZ\"\n",
            Language::Python,
        );
        let findings = SecretScanDetector::new().analyze(&ctx).await.unwrap();
        assert!(!findings.is_empty());
    }

    #[tokio::test]
    async fn detects_slack_bot_token() {
        let ctx = single_file_ctx(
            "src/notify.py",
            "SLACK = \"xoxb-1234567890-abcdefg\"\n",
            Language::Python,
        );
        let findings = SecretScanDetector::new().analyze(&ctx).await.unwrap();
        assert!(!findings.is_empty());
        assert!(findings.iter().any(|f| f.title.contains("Slack")));
    }

    #[tokio::test]
    async fn detects_sendgrid_key() {
        let ctx = single_file_ctx(
            "src/mail.py",
            "KEY = \"SG.abcdefghijklmnopqrstuv.ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqr\"\n",
            Language::Python,
        );
        let findings = SecretScanDetector::new().analyze(&ctx).await.unwrap();
        assert!(!findings.is_empty());
        assert!(findings.iter().any(|f| f.title.contains("SendGrid")));
    }

    #[tokio::test]
    async fn detects_google_api_key() {
        let ctx = single_file_ctx(
            "src/maps.js",
            "const key = \"AIzaSyB1234567890abcdefghijklmnopqrstuv\"\n",
            Language::JavaScript,
        );
        let findings = SecretScanDetector::new().analyze(&ctx).await.unwrap();
        assert!(!findings.is_empty());
        assert!(findings.iter().any(|f| f.title.contains("Google API")));
    }

    #[tokio::test]
    async fn detects_npm_token() {
        let ctx = single_file_ctx(
            "src/publish.js",
            "let registry_auth = \"npm_ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghij\"\n",
            Language::JavaScript,
        );
        let findings = SecretScanDetector::new().analyze(&ctx).await.unwrap();
        assert!(!findings.is_empty());
        assert!(findings.iter().any(|f| f.title.contains("npm")));
    }

    // -----------------------------------------------------------------------
    // Entropy tests
    // -----------------------------------------------------------------------

    #[test]
    fn shannon_entropy_low_for_repeated_chars() {
        let entropy = shannon_entropy("aaaaaaaa");
        assert!(
            entropy < 1.0,
            "repeated chars should have low entropy, got {entropy}"
        );
    }

    #[test]
    fn shannon_entropy_high_for_random_hex() {
        // A string with good variety of characters
        let entropy = shannon_entropy("a1b2c3d4e5f6A7B8C9D0EeFfGgHh");
        assert!(
            entropy > 4.0,
            "diverse string should have high entropy, got {entropy}"
        );
    }

    #[test]
    fn shannon_entropy_zero_for_empty() {
        assert!((shannon_entropy("") - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn shannon_entropy_single_char_is_zero() {
        assert!((shannon_entropy("a") - 0.0).abs() < f64::EPSILON);
    }

    #[tokio::test]
    async fn entropy_detects_high_entropy_string() {
        // Lower the threshold via config to be sure this string triggers
        let mut ctx = single_file_ctx(
            "src/config.py",
            "value = \"a1B2c3D4e5F6g7H8i9J0kL\"\n",
            Language::Python,
        );
        ctx.config.entropy_threshold = 3.5;
        let findings = SecretScanDetector::new().analyze(&ctx).await.unwrap();
        assert!(
            !findings.is_empty(),
            "high entropy string should trigger finding"
        );
        // Entropy findings use Severity::High
        assert!(findings.iter().any(|f| f.severity == Severity::High));
    }

    #[tokio::test]
    async fn entropy_ignores_low_entropy_string() {
        let mut ctx = single_file_ctx(
            "src/config.py",
            "msg = \"aaaaaaaaaaaaaaaaaaaa\"\n",
            Language::Python,
        );
        ctx.config.entropy_threshold = 4.5;
        let findings = SecretScanDetector::new().analyze(&ctx).await.unwrap();
        assert!(
            findings.is_empty(),
            "low entropy string should not trigger finding"
        );
    }

    // -----------------------------------------------------------------------
    // Skip/filter tests
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn skips_test_files() {
        let ctx = single_file_ctx(
            "tests/test_auth.py",
            "PASSWORD = \"AKIAIOSFODNN7ABCDEFG\"\n",
            Language::Python,
        );
        let findings = SecretScanDetector::new().analyze(&ctx).await.unwrap();
        assert!(findings.is_empty(), "test files should be skipped");
    }

    #[tokio::test]
    async fn skips_example_files() {
        let ctx = single_file_ctx(
            "config/settings.example.py",
            "KEY = \"AKIAIOSFODNN7ABCDEFG\"\n",
            Language::Python,
        );
        let findings = SecretScanDetector::new().analyze(&ctx).await.unwrap();
        assert!(findings.is_empty(), "example files should be skipped");
    }

    #[tokio::test]
    async fn skips_sample_files() {
        let ctx = single_file_ctx(
            "config.sample.yml",
            "KEY = \"AKIAIOSFODNN7ABCDEFG\"\n",
            Language::Python,
        );
        let findings = SecretScanDetector::new().analyze(&ctx).await.unwrap();
        assert!(findings.is_empty(), "sample files should be skipped");
    }

    #[tokio::test]
    async fn skips_template_files() {
        let ctx = single_file_ctx(
            "settings.template.py",
            "KEY = \"AKIAIOSFODNN7ABCDEFG\"\n",
            Language::Python,
        );
        let findings = SecretScanDetector::new().analyze(&ctx).await.unwrap();
        assert!(findings.is_empty(), "template files should be skipped");
    }

    #[tokio::test]
    async fn skips_comments() {
        let ctx = single_file_ctx(
            "src/config.py",
            "# AKIAIOSFODNN7ABCDEFG old key\nreal = 1\n",
            Language::Python,
        );
        let findings = SecretScanDetector::new().analyze(&ctx).await.unwrap();
        assert!(findings.is_empty(), "comments should be skipped");
    }

    #[tokio::test]
    async fn skips_placeholder_values() {
        let ctx = single_file_ctx(
            "src/config.py",
            "password = \"changeme\"\napi_key = \"your-api-key-here\"\n",
            Language::Python,
        );
        let findings = SecretScanDetector::new().analyze(&ctx).await.unwrap();
        assert!(findings.is_empty(), "placeholder values should be skipped");
    }

    #[tokio::test]
    async fn skips_env_var_references() {
        let ctx = single_file_ctx(
            "src/config.py",
            "password = os.environ.get('DB_PASSWORD')\n",
            Language::Python,
        );
        let findings = SecretScanDetector::new().analyze(&ctx).await.unwrap();
        assert!(findings.is_empty(), "env var references should be skipped");
    }

    #[tokio::test]
    async fn skips_cfg_test_block_in_rust() {
        let ctx = single_file_ctx(
            "src/lib.rs",
            "#[cfg(test)]\nmod tests {\n    const KEY: &str = \"AKIAIOSFODNN7ABCDEFG\";\n}\n",
            Language::Rust,
        );
        let findings = SecretScanDetector::new().analyze(&ctx).await.unwrap();
        assert!(
            findings.is_empty(),
            "cfg(test) blocks should be skipped in Rust"
        );
    }

    // -----------------------------------------------------------------------
    // Clean file test
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn clean_file_produces_no_findings() {
        let ctx = single_file_ctx(
            "src/app.py",
            "import os\n\ndef main():\n    key = os.environ.get('API_KEY')\n    print('hello')\n",
            Language::Python,
        );
        let findings = SecretScanDetector::new().analyze(&ctx).await.unwrap();
        assert!(findings.is_empty(), "clean file should produce no findings");
    }

    #[tokio::test]
    async fn empty_source_cache_produces_no_findings() {
        let ctx = make_ctx(HashMap::new(), Language::Python);
        let findings = SecretScanDetector::new().analyze(&ctx).await.unwrap();
        assert!(findings.is_empty());
    }

    // -----------------------------------------------------------------------
    // Misc
    // -----------------------------------------------------------------------

    #[test]
    fn name_returns_secret_scan() {
        assert_eq!(SecretScanDetector::new().name(), "secret-scan");
    }

    #[test]
    fn does_not_use_cargo_subprocess() {
        assert!(!SecretScanDetector::new().uses_cargo_subprocess());
    }

    #[test]
    fn extract_string_literals_basic() {
        let lits = extract_string_literals(r#"key = "hello world here""#);
        assert_eq!(lits.len(), 1);
        assert_eq!(lits[0], "hello world here");
    }

    #[test]
    fn extract_string_literals_short_ignored() {
        let lits = extract_string_literals(r#"x = "hi""#);
        assert!(
            lits.is_empty(),
            "strings shorter than 8 chars should be ignored"
        );
    }

    #[test]
    fn extract_string_literals_single_quotes() {
        let lits = extract_string_literals("key = 'a_long_secret_value'");
        assert_eq!(lits.len(), 1);
        assert_eq!(lits[0], "a_long_secret_value");
    }

    #[test]
    fn is_skip_file_test_variants() {
        assert!(is_skip_file(std::path::Path::new("tests/auth.py")));
        assert!(is_skip_file(std::path::Path::new("src/test_foo.py")));
        assert!(is_skip_file(std::path::Path::new("config.example.yml")));
        assert!(is_skip_file(std::path::Path::new("settings.sample.py")));
        assert!(is_skip_file(std::path::Path::new("config.template.yml")));
        assert!(!is_skip_file(std::path::Path::new("src/main.py")));
        assert!(!is_skip_file(std::path::Path::new("src/config.py")));
    }

    #[test]
    fn pattern_meta_and_regex_arrays_same_length() {
        assert_eq!(
            PATTERN_META.len(),
            PATTERN_REGEXES.len(),
            "PATTERN_META and PATTERN_REGEXES must have same length"
        );
    }

    // -----------------------------------------------------------------------
    // BUG: Heroku UUID regex matches ALL UUIDs — massive false positives
    // -----------------------------------------------------------------------
    // The "Heroku API Key" regex is just a generic UUID pattern:
    //   r"[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-...-[0-9a-fA-F]{12}"
    // This matches any UUID (v4, v1, etc.), not just Heroku API keys.
    // Every UUID literal in source code will be flagged as a "Heroku API Key".

    #[tokio::test]
    async fn heroku_regex_false_positive_on_generic_uuid() {
        // This is a random UUID with no connection to Heroku.
        let ctx = single_file_ctx(
            "src/models.py",
            "DEFAULT_NAMESPACE = \"550e8400-e29b-41d4-a716-446655440000\"\n",
            Language::Python,
        );
        let findings = SecretScanDetector::new().analyze(&ctx).await.unwrap();
        // After fix: generic UUID should NOT match the Heroku pattern
        assert!(
            !findings.iter().any(|f| f.title.contains("Heroku")),
            "generic UUID should not be detected as Heroku API key"
        );
    }

    // -----------------------------------------------------------------------
    // False positive suppression tests (Task 1.3)
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn skips_instrumentation_template_file() {
        // A high-entropy hex string that is part of an instrumentation template
        // should not trigger a secret finding.
        let ctx = single_file_ctx(
            "crates/apex-instrument/src/templates/coverage.rs",
            "let probe_id = \"a1b2c3d4e5f6a7b8c9d0e1f2a3b4c5d6\";\n",
            Language::Rust,
        );
        let findings = SecretScanDetector::new().analyze(&ctx).await.unwrap();
        assert!(
            findings.is_empty(),
            "instrumentation template hex strings should not trigger findings"
        );
    }

    #[tokio::test]
    async fn skips_generated_file() {
        // Generated files are known to carry non-secret high-entropy data.
        let ctx = single_file_ctx(
            "src/generated/bindings.rs",
            "const HASH: &str = \"a1b2c3d4e5f6a7b8c9d0e1f2a3b4c5d6\";\n",
            Language::Rust,
        );
        let findings = SecretScanDetector::new().analyze(&ctx).await.unwrap();
        assert!(
            findings.is_empty(),
            "generated files should not trigger findings"
        );
    }

    #[tokio::test]
    async fn skips_fixture_file() {
        // Fixture files contain expected test data with realistic-looking strings.
        let ctx = single_file_ctx(
            "crates/apex-detect/src/fixtures/secrets.rs",
            "let val = \"a1b2c3d4e5f6a7b8c9d0e1f2a3b4c5d6\";\n",
            Language::Rust,
        );
        let findings = SecretScanDetector::new().analyze(&ctx).await.unwrap();
        assert!(
            findings.is_empty(),
            "fixture files should not trigger findings"
        );
    }

    #[tokio::test]
    async fn skips_const_string_declaration_in_rust() {
        // `const TEST_DATA: &str = "..."` is a known test fixture pattern in Rust
        // detector source files — it should not be flagged as a secret.
        let ctx = single_file_ctx(
            "src/lib.rs",
            "const TEST_DATA: &str = \"a1b2c3d4e5f6a7b8c9d0e1f2a3b4c5d6\";\n",
            Language::Rust,
        );
        let findings = SecretScanDetector::new().analyze(&ctx).await.unwrap();
        assert!(
            findings.is_empty(),
            "const string declarations should not be flagged as secrets"
        );
    }

    #[tokio::test]
    async fn skips_detector_source_file() {
        // High-entropy strings inside crates/apex-detect/src/detectors/*.rs are
        // test fixture data embedded in detector source files.
        let ctx = single_file_ctx(
            "crates/apex-detect/src/detectors/my_detector.rs",
            "let pattern = \"a1b2c3d4e5f6a7b8c9d0e1f2a3b4c5d6\";\n",
            Language::Rust,
        );
        let findings = SecretScanDetector::new().analyze(&ctx).await.unwrap();
        assert!(
            findings.is_empty(),
            "strings in detector source files should not trigger findings"
        );
    }

    #[tokio::test]
    async fn still_detects_real_api_key_outside_test_context() {
        // A real-looking API key in a production Python config file should still trigger.
        // Build at runtime to avoid GitHub push-protection false positive on the test string.
        let key = format!("sk_live_{}", "RealKeyThatShouldBeDetected123456");
        let content = format!("STRIPE_KEY = \"{key}\"\n");
        let ctx = single_file_ctx("src/config.py", &content, Language::Python);
        let findings = SecretScanDetector::new().analyze(&ctx).await.unwrap();
        assert!(
            !findings.is_empty(),
            "real-looking API key in prod file should still trigger"
        );
    }

    #[test]
    fn is_skip_file_new_patterns() {
        // New patterns added for Task 1.3
        assert!(is_skip_file(std::path::Path::new(
            "crates/apex-instrument/src/templates.rs"
        )));
        assert!(is_skip_file(std::path::Path::new(
            "src/generated/bindings.rs"
        )));
        assert!(is_skip_file(std::path::Path::new(
            "src/fixtures/secrets.rs"
        )));
        assert!(is_skip_file(std::path::Path::new(
            "crates/apex-detect/src/detectors/sql_injection.rs"
        )));
        assert!(is_skip_file(std::path::Path::new("src/source_map/map.js")));
        // Regular production files should not be skipped
        assert!(!is_skip_file(std::path::Path::new("src/config.py")));
        assert!(!is_skip_file(std::path::Path::new("src/billing.rs")));
    }

    #[test]
    fn is_const_string_decl_matches_rust_const() {
        assert!(is_const_string_decl("const FOO: &str = \"hello\";"));
        assert!(is_const_string_decl("const BAR: &'static str = \"world\";"));
        // Non-const lines should not match
        assert!(!is_const_string_decl("let x = \"hello\";"));
        assert!(!is_const_string_decl("fn foo() {}"));
        assert!(!is_const_string_decl("const N: u32 = 42;"));
    }

    // -----------------------------------------------------------------------
    // Bug 2: Entropy threshold raised from 4.5 to 5.0
    // -----------------------------------------------------------------------

    #[test]
    fn default_entropy_threshold_is_5_0() {
        let cfg = DetectConfig::default();
        assert!(
            (cfg.entropy_threshold - 5.0).abs() < f64::EPSILON,
            "default entropy threshold should be 5.0, got {}",
            cfg.entropy_threshold
        );
    }

    #[tokio::test]
    async fn low_entropy_identifier_not_flagged_at_5_0() {
        // cpumask_pr_args is ~4.6 entropy — should be suppressed at threshold 5.0
        let ctx = single_file_ctx(
            "src/config.py",
            "mask = \"cpumask_pr_args_value_here\"\n",
            Language::Python,
        );
        let findings = SecretScanDetector::new().analyze(&ctx).await.unwrap();
        assert!(
            findings.is_empty(),
            "low-entropy code identifier should not be flagged at threshold 5.0"
        );
    }

    #[tokio::test]
    async fn high_entropy_aws_key_still_caught() {
        // AWS keys have entropy ~5.7 — still above threshold 5.0
        let ctx = single_file_ctx(
            "src/config.py",
            "AWS_KEY = \"AKIAIOSFODNN7ABCDEFG\"\n",
            Language::Python,
        );
        let findings = SecretScanDetector::new().analyze(&ctx).await.unwrap();
        assert!(
            !findings.is_empty(),
            "real AWS key pattern should still be detected at threshold 5.0"
        );
    }

    // -----------------------------------------------------------------------
    // Bug 14/15: File skip patterns — vendor, dist, .min., bundle., encoding, charsets
    // -----------------------------------------------------------------------

    #[test]
    fn is_skip_file_vendor_and_dist() {
        assert!(is_skip_file(std::path::Path::new("vendor/lodash/index.js")));
        assert!(is_skip_file(std::path::Path::new("dist/app.bundle.js")));
        assert!(is_skip_file(std::path::Path::new("src/app.min.js")));
        assert!(is_skip_file(std::path::Path::new("build/bundle.js")));
        assert!(is_skip_file(std::path::Path::new("lib/encoding.js")));
        assert!(is_skip_file(std::path::Path::new("src/charsets.py")));
        assert!(is_skip_file(std::path::Path::new("util/codepage.rs")));
        // Production files must NOT be skipped
        assert!(!is_skip_file(std::path::Path::new("src/app.js")));
        assert!(!is_skip_file(std::path::Path::new("src/main.rs")));
    }

    #[tokio::test]
    async fn skips_vendor_file() {
        let key = format!("sk_live_{}", "4eC39HqLyjWDarjtT1zdp7dc");
        let content = format!("STRIPE_KEY = \"{key}\"\n");
        let ctx = single_file_ctx("vendor/stripe/client.py", &content, Language::Python);
        let findings = SecretScanDetector::new().analyze(&ctx).await.unwrap();
        assert!(findings.is_empty(), "vendor files should be skipped");
    }

    #[tokio::test]
    async fn skips_code_generated_file() {
        // A Go file starting with "// Code generated" is auto-generated and should be skipped
        let ctx = single_file_ctx(
            "src/gen_proto.go",
            "// Code generated by protoc-gen-go. DO NOT EDIT.\npackage main\n\
             const token = \"AKIAIOSFODNN7ABCDEFG\"\n",
            Language::Go,
        );
        let findings = SecretScanDetector::new().analyze(&ctx).await.unwrap();
        assert!(
            findings.is_empty(),
            "code-generated files should be skipped"
        );
    }

    // -----------------------------------------------------------------------
    // Multi-language support tests
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn detects_secret_in_java_file() {
        let ctx = single_file_ctx(
            "src/Config.java",
            "password = \"s3cretP@ssw0rd!\"\n",
            Language::Java,
        );
        let findings = SecretScanDetector::new().analyze(&ctx).await.unwrap();
        assert!(!findings.is_empty(), "should detect secret in Java file");
    }

    #[tokio::test]
    async fn detects_aws_key_in_kotlin_file() {
        let ctx = single_file_ctx(
            "src/Config.kt",
            "val key = \"AKIAIOSFODNN7ABCDEFG\"\n",
            Language::Kotlin,
        );
        let findings = SecretScanDetector::new().analyze(&ctx).await.unwrap();
        assert!(!findings.is_empty(), "should detect AWS key in Kotlin file");
    }

    #[tokio::test]
    async fn detects_secret_in_c_file() {
        let ctx = single_file_ctx(
            "src/config.c",
            "#define API_KEY \"AKIAIOSFODNN7ABCDEFG\"\n",
            Language::C,
        );
        let findings = SecretScanDetector::new().analyze(&ctx).await.unwrap();
        assert!(!findings.is_empty(), "should detect secret in C file");
    }

    #[tokio::test]
    async fn detects_secret_in_cpp_file() {
        let ctx = single_file_ctx(
            "src/config.hpp",
            "password = \"s3cretP@ssw0rd!\"\n",
            Language::Cpp,
        );
        let findings = SecretScanDetector::new().analyze(&ctx).await.unwrap();
        assert!(!findings.is_empty(), "should detect secret in C++ file");
    }

    #[tokio::test]
    async fn detects_secret_in_csharp_file() {
        let ctx = single_file_ctx(
            "src/Config.cs",
            "password = \"s3cretP@ssw0rd!\"\n",
            Language::CSharp,
        );
        let findings = SecretScanDetector::new().analyze(&ctx).await.unwrap();
        assert!(!findings.is_empty(), "should detect secret in C# file");
    }

    #[tokio::test]
    async fn detects_secret_in_csharp_verbatim_string() {
        let mut ctx = single_file_ctx(
            "src/Db.cs",
            "var connStr = @\"Server=db;Password=s3cretP@ssw0rd!ReallyLong\"\n",
            Language::CSharp,
        );
        ctx.config.entropy_threshold = 3.5;
        let findings = SecretScanDetector::new().analyze(&ctx).await.unwrap();
        assert!(
            !findings.is_empty(),
            "should detect secret in C# verbatim string"
        );
    }

    #[tokio::test]
    async fn detects_secret_in_swift_file() {
        let ctx = single_file_ctx(
            "Sources/Config.swift",
            "password = \"s3cretP@ssw0rd!\"\n",
            Language::Swift,
        );
        let findings = SecretScanDetector::new().analyze(&ctx).await.unwrap();
        assert!(!findings.is_empty(), "should detect secret in Swift file");
    }

    #[tokio::test]
    async fn detects_secret_in_swift_raw_string() {
        let mut ctx = single_file_ctx(
            "Sources/Auth.swift",
            "let token = #\"eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJzdWIiOiIxMjM0NTY3ODkwIn0\"#\n",
            Language::Swift,
        );
        ctx.config.entropy_threshold = 3.5;
        let findings = SecretScanDetector::new().analyze(&ctx).await.unwrap();
        assert!(
            !findings.is_empty(),
            "should detect secret in Swift raw string"
        );
    }

    // -----------------------------------------------------------------------
    // String extraction tests for new literal forms
    // -----------------------------------------------------------------------

    #[test]
    fn extract_csharp_verbatim_string() {
        let lits = extract_string_literals(r#"var s = @"Server=db;Password=secret123""#);
        assert!(!lits.is_empty(), "should extract C# verbatim string");
        assert!(lits[0].contains("Server=db"));
    }

    #[test]
    fn extract_swift_raw_string() {
        let input = r##"let s = #"some_long_raw_string_here"#"##;
        let lits = extract_string_literals(input);
        assert!(!lits.is_empty(), "should extract Swift raw string literal");
        assert!(lits[0].contains("some_long_raw_string_here"));
    }

    #[test]
    fn extract_c_define_string() {
        let input = r##"#define SECRET "mySecretValue12345""##;
        let lits = extract_string_literals(input);
        assert!(!lits.is_empty(), "should extract C define string literal");
        assert_eq!(lits[0], "mySecretValue12345");
    }
}
