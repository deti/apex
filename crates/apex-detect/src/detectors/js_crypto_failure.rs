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

pub struct JsCryptoFailureDetector;

/// Matches crypto.createHash with weak algorithms (md5, sha1).
static WEAK_HASH: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"crypto\.createHash\s*\(\s*['"](?:md5|sha1)['"]"#)
        .expect("invalid weak hash regex")
});

/// Matches deprecated crypto.createCipher (not createCipheriv).
static DEPRECATED_CIPHER: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"crypto\.createCipher\s*\(").expect("invalid deprecated cipher regex")
});

/// Matches Math.random() usage.
static MATH_RANDOM: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"Math\.random\s*\(\s*\)").expect("invalid Math.random regex"));

const SECURITY_KEYWORDS: &[&str] = &[
    "token", "key", "secret", "password", "salt", "nonce", "csrf", "auth",
];

#[async_trait]
impl Detector for JsCryptoFailureDetector {
    fn name(&self) -> &str {
        "js-crypto-failure"
    }

    async fn analyze(&self, ctx: &AnalysisContext) -> Result<Vec<Finding>> {
        let mut findings = Vec::new();

        if ctx.language != Language::JavaScript {
            return Ok(findings);
        }

        for (path, source) in &ctx.source_cache {
            if is_test_file(path) {
                continue;
            }

            for (line_num, line) in source.lines().enumerate() {
                let trimmed = line.trim();

                if is_comment(trimmed, ctx.language) {
                    continue;
                }

                let line_1based = (line_num + 1) as u32;

                // Check for weak hash algorithms
                if WEAK_HASH.is_match(trimmed) {
                    findings.push(Finding {
                        id: Uuid::new_v4(),
                        detector: self.name().into(),
                        severity: Severity::Medium,
                        category: FindingCategory::SecuritySmell,
                        file: path.clone(),
                        line: Some(line_1based),
                        title: format!("Weak hash algorithm at line {}", line_1based),
                        description: format!(
                            "Weak hash algorithm (MD5 or SHA-1) used in {}:{}. \
                             These are cryptographically broken for security purposes.",
                            path.display(),
                            line_1based
                        ),
                        evidence: vec![],
                        covered: false,
                        suggestion: "Use SHA-256 or stronger: crypto.createHash('sha256')".into(),
                        explanation: None,
                        fix: None,
                        cwe_ids: vec![327, 328],
                    noisy: false, base_severity: None, coverage_confidence: None,
                    });
                    continue;
                }

                // Check for deprecated createCipher (not createCipheriv)
                if DEPRECATED_CIPHER.is_match(trimmed) && !trimmed.contains("createCipheriv") {
                    findings.push(Finding {
                        id: Uuid::new_v4(),
                        detector: self.name().into(),
                        severity: Severity::Medium,
                        category: FindingCategory::SecuritySmell,
                        file: path.clone(),
                        line: Some(line_1based),
                        title: format!("Deprecated crypto.createCipher at line {}", line_1based),
                        description: format!(
                            "Deprecated crypto.createCipher used in {}:{}. \
                             It derives the IV from the key, making it predictable.",
                            path.display(),
                            line_1based
                        ),
                        evidence: vec![],
                        covered: false,
                        suggestion: "Use crypto.createCipheriv with a random IV instead".into(),
                        explanation: None,
                        fix: None,
                        cwe_ids: vec![327, 328],
                    noisy: false, base_severity: None, coverage_confidence: None,
                    });
                    continue;
                }

                // Check for Math.random() near security keywords
                if MATH_RANDOM.is_match(trimmed) {
                    let lower = trimmed.to_lowercase();
                    let near_security = SECURITY_KEYWORDS.iter().any(|kw| lower.contains(kw));
                    if near_security {
                        findings.push(Finding {
                            id: Uuid::new_v4(),
                            detector: self.name().into(),
                            severity: Severity::Medium,
                            category: FindingCategory::SecuritySmell,
                            file: path.clone(),
                            line: Some(line_1based),
                            title: format!(
                                "Math.random() used for security purpose at line {}",
                                line_1based
                            ),
                            description: format!(
                                "Math.random() used near security context in {}:{}. \
                                 Math.random() is not cryptographically secure.",
                                path.display(),
                                line_1based
                            ),
                            evidence: vec![],
                            covered: false,
                            suggestion:
                                "Use crypto.randomBytes() or crypto.getRandomValues() instead"
                                    .into(),
                            explanation: None,
                            fix: None,
                            cwe_ids: vec![327, 328],
                    noisy: false, base_severity: None, coverage_confidence: None,
                        });
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

    #[tokio::test]
    async fn detects_md5_hash() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/hash.js"),
            "const hash = crypto.createHash('md5');\n".into(),
        );
        let ctx = make_ctx(files, Language::JavaScript);
        let findings = JsCryptoFailureDetector.analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, Severity::Medium);
        assert_eq!(findings[0].category, FindingCategory::SecuritySmell);
        assert_eq!(findings[0].cwe_ids, vec![327, 328]);
    }

    #[tokio::test]
    async fn detects_sha1_hash() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/hash.js"),
            "const hash = crypto.createHash(\"sha1\");\n".into(),
        );
        let ctx = make_ctx(files, Language::JavaScript);
        let findings = JsCryptoFailureDetector.analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 1);
    }

    #[tokio::test]
    async fn detects_deprecated_create_cipher() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/encrypt.js"),
            "const cipher = crypto.createCipher(\"aes-128-cbc\", key);\n".into(),
        );
        let ctx = make_ctx(files, Language::JavaScript);
        let findings = JsCryptoFailureDetector.analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 1);
    }

    #[tokio::test]
    async fn skips_sha256_hash() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/hash.js"),
            "const hash = crypto.createHash('sha256');\n".into(),
        );
        let ctx = make_ctx(files, Language::JavaScript);
        let findings = JsCryptoFailureDetector.analyze(&ctx).await.unwrap();
        assert!(findings.is_empty());
    }

    #[tokio::test]
    async fn skips_create_cipheriv() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/encrypt.js"),
            "const cipher = crypto.createCipheriv(\"aes-256-gcm\", key, iv);\n".into(),
        );
        let ctx = make_ctx(files, Language::JavaScript);
        let findings = JsCryptoFailureDetector.analyze(&ctx).await.unwrap();
        assert!(findings.is_empty());
    }

    #[tokio::test]
    async fn detects_math_random_for_token() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/auth.js"),
            "const token = Math.random().toString(36);\n".into(),
        );
        let ctx = make_ctx(files, Language::JavaScript);
        let findings = JsCryptoFailureDetector.analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 1);
    }

    #[tokio::test]
    async fn skips_math_random_without_security_context() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/game.js"),
            "const roll = Math.random() * 6;\n".into(),
        );
        let ctx = make_ctx(files, Language::JavaScript);
        let findings = JsCryptoFailureDetector.analyze(&ctx).await.unwrap();
        assert!(findings.is_empty());
    }

    #[tokio::test]
    async fn skips_non_javascript() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/hash.py"),
            "h = crypto.createHash('md5')\n".into(),
        );
        let ctx = make_ctx(files, Language::Python);
        let findings = JsCryptoFailureDetector.analyze(&ctx).await.unwrap();
        assert!(findings.is_empty());
    }

    #[tokio::test]
    async fn skips_test_files() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("test/crypto.test.js"),
            "const hash = crypto.createHash('md5');\n".into(),
        );
        let ctx = make_ctx(files, Language::JavaScript);
        let findings = JsCryptoFailureDetector.analyze(&ctx).await.unwrap();
        assert!(findings.is_empty());
    }

    #[tokio::test]
    async fn skips_comments() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/hash.js"),
            "// crypto.createHash('md5')\n".into(),
        );
        let ctx = make_ctx(files, Language::JavaScript);
        let findings = JsCryptoFailureDetector.analyze(&ctx).await.unwrap();
        assert!(findings.is_empty());
    }

    #[test]
    fn does_not_use_cargo_subprocess() {
        assert!(!JsCryptoFailureDetector.uses_cargo_subprocess());
    }
}
