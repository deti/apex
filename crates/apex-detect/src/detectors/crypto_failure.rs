//! Cryptographic failure detector — identifies weak crypto patterns (CWE-327/328/330).

use crate::finding::{Finding, FindingCategory, Severity};
use std::path::PathBuf;
use uuid::Uuid;

/// Weak hash algorithms.
const WEAK_HASHES: &[&str] = &[
    "MD5",
    "SHA1",
    "md5(",
    "sha1(",
    "hashlib.md5",
    "hashlib.sha1",
];

/// Weak cipher algorithms.
const WEAK_CIPHERS: &[&str] = &["DES", "RC4", "Blowfish", "ECB"];

/// Non-cryptographic random usage in security contexts.
const INSECURE_RANDOM: &[&str] = &["random.random(", "Math.random(", "rand("];

/// Security context indicators (nearby means random is security-sensitive).
const SECURITY_CONTEXT: &[&str] = &[
    "token", "secret", "password", "key", "salt", "nonce", "csrf", "session",
];

/// Safe alternatives that should not be flagged.
const SAFE_PATTERNS: &[&str] = &[
    "sha256",
    "sha384",
    "sha512",
    "SHA256",
    "SHA384",
    "SHA512",
    "secrets.",
    "os.urandom",
    "SystemRandom",
    "token_bytes",
    "token_hex",
    "PBKDF2",
    "bcrypt",
    "argon2",
    "scrypt",
];

/// Check if a pattern match is at a word boundary — the characters immediately
/// before and after the match must not be alphanumeric/underscore.
fn is_word_boundary_match(haystack: &str, pattern: &str) -> bool {
    let mut start = 0;
    while let Some(pos) = haystack[start..].find(pattern) {
        let abs_pos = start + pos;
        let before_ok = abs_pos == 0 || !haystack.as_bytes()[abs_pos - 1].is_ascii_alphanumeric();
        let after_pos = abs_pos + pattern.len();
        let after_ok =
            after_pos >= haystack.len() || !haystack.as_bytes()[after_pos].is_ascii_alphanumeric();
        if before_ok && after_ok {
            return true;
        }
        start = abs_pos + 1;
        if start >= haystack.len() {
            break;
        }
    }
    false
}

/// Scan source code for cryptographic failure vulnerabilities.
pub fn scan_crypto_failure(source: &str, file_path: &str) -> Vec<Finding> {
    let mut findings = Vec::new();

    for (line_num, line) in source.lines().enumerate() {
        let line_1based = (line_num + 1) as u32;
        let trimmed = line.trim();

        // Skip comments.
        if trimmed.starts_with('#') || trimmed.starts_with("//") || trimmed.starts_with('*') {
            continue;
        }

        // Skip lines with safe patterns.
        if SAFE_PATTERNS.iter().any(|s| trimmed.contains(s)) {
            continue;
        }

        // Check weak hashes — patterns that contain '(' are substring-safe already.
        for pattern in WEAK_HASHES {
            let matched = if pattern.contains('(') {
                trimmed.contains(pattern)
            } else {
                is_word_boundary_match(trimmed, pattern)
            };
            if matched {
                findings.push(make_finding(
                    file_path,
                    line_1based,
                    Severity::Medium,
                    "Weak hash algorithm detected",
                    &format!(
                        "Weak hash function at line {line_1based}. \
                         MD5 and SHA1 are cryptographically broken."
                    ),
                    "Use SHA-256 or stronger (hashlib.sha256, SHA-256).",
                    328,
                ));
                break;
            }
        }

        // Check weak ciphers — all short uppercase tokens need word-boundary check.
        for pattern in WEAK_CIPHERS {
            let matched = is_word_boundary_match(trimmed, pattern);
            if matched {
                findings.push(make_finding(
                    file_path,
                    line_1based,
                    Severity::High,
                    "Weak cipher or mode detected",
                    &format!(
                        "Weak cipher/mode at line {line_1based}. \
                         {pattern} is considered insecure."
                    ),
                    "Use AES-GCM or ChaCha20-Poly1305 with authenticated encryption.",
                    327,
                ));
                break;
            }
        }

        // Check insecure random in security context.
        for pattern in INSECURE_RANDOM {
            if trimmed.contains(pattern) {
                let in_security_ctx = SECURITY_CONTEXT
                    .iter()
                    .any(|ctx| trimmed.to_lowercase().contains(ctx));
                if in_security_ctx {
                    findings.push(make_finding(
                        file_path,
                        line_1based,
                        Severity::Medium,
                        "Non-cryptographic random used in security context",
                        &format!(
                            "Insecure random at line {line_1based}. \
                             random.random()/Math.random() are not cryptographically secure."
                        ),
                        "Use secrets.token_bytes(), os.urandom(), or crypto.getRandomValues().",
                        330,
                    ));
                    break;
                }
            }
        }

        // Check hardcoded keys/IVs.
        if (trimmed.contains("key =")
            || trimmed.contains("key=")
            || trimmed.contains("iv =")
            || trimmed.contains("iv="))
            && (trimmed.contains("\"") || trimmed.contains("b\"") || trimmed.contains("b'"))
        {
            // Heuristic: line assigns a key/iv to a string literal.
            let lower = trimmed.to_lowercase();
            if lower.contains("key") || lower.contains(" iv") {
                findings.push(make_finding(
                    file_path,
                    line_1based,
                    Severity::High,
                    "Hardcoded cryptographic key or IV",
                    &format!(
                        "Hardcoded key/IV at line {line_1based}. \
                         Keys should be stored securely, not in source code."
                    ),
                    "Use environment variables, key management services, or config files.",
                    321,
                ));
            }
        }
    }

    findings
}

fn make_finding(
    file_path: &str,
    line: u32,
    severity: Severity,
    title: &str,
    description: &str,
    suggestion: &str,
    cwe: u32,
) -> Finding {
    Finding {
        id: Uuid::new_v4(),
        detector: "crypto_failure".into(),
        severity,
        category: FindingCategory::SecuritySmell,
        file: PathBuf::from(file_path),
        line: Some(line),
        title: title.into(),
        description: description.into(),
        evidence: vec![],
        covered: false,
        suggestion: suggestion.into(),
        explanation: None,
        fix: None,
        cwe_ids: vec![cwe],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_md5() {
        let source = "import hashlib\nh = hashlib.md5(data)\n";
        let findings = scan_crypto_failure(source, "hash.py");
        assert!(!findings.is_empty());
        assert!(findings[0].cwe_ids.contains(&328));
    }

    #[test]
    fn detect_sha1() {
        let source = "digest = SHA1(message)\n";
        let findings = scan_crypto_failure(source, "crypto.py");
        assert!(!findings.is_empty());
    }

    #[test]
    fn detect_des() {
        let source = "cipher = DES.new(key, DES.MODE_CBC)\n";
        let findings = scan_crypto_failure(source, "encrypt.py");
        assert!(!findings.is_empty());
        assert!(findings[0].cwe_ids.contains(&327));
    }

    #[test]
    fn detect_ecb_mode() {
        let source = "cipher = AES.new(key, AES.MODE_ECB)\n";
        let findings = scan_crypto_failure(source, "encrypt.py");
        assert!(!findings.is_empty());
    }

    #[test]
    fn detect_random_in_security_context() {
        let source = "token = random.random()\n";
        let findings = scan_crypto_failure(source, "auth.py");
        assert!(!findings.is_empty());
        assert!(findings[0].cwe_ids.contains(&330));
    }

    #[test]
    fn detect_hardcoded_key() {
        let source = "key = b\"supersecretkey12\"\n";
        let findings = scan_crypto_failure(source, "config.py");
        assert!(!findings.is_empty());
        assert!(findings[0].cwe_ids.contains(&321));
    }

    #[test]
    fn skip_sha256() {
        let source = "h = hashlib.sha256(data)\n";
        let findings = scan_crypto_failure(source, "hash.py");
        assert!(findings.is_empty());
    }

    #[test]
    fn skip_secrets_token_bytes() {
        let source = "token = secrets.token_bytes(32)\n";
        let findings = scan_crypto_failure(source, "auth.py");
        assert!(findings.is_empty());
    }

    #[test]
    fn detect_multiple_issues() {
        let source = "h = hashlib.md5(x)\ncipher = DES.new(k)\n";
        let findings = scan_crypto_failure(source, "bad.py");
        assert!(findings.len() >= 2);
    }

    #[test]
    fn no_false_positive_on_comments() {
        let source = "# MD5 is insecure\n// Don't use DES\n";
        let findings = scan_crypto_failure(source, "notes.py");
        assert!(findings.is_empty());
    }

    #[test]
    fn no_false_positive_on_describes_or_nodes() {
        let source = "DESCRIBES the algorithm\nreturn NODES\n";
        let findings = scan_crypto_failure(source, "doc.py");
        let cipher_findings: Vec<_> = findings
            .iter()
            .filter(|f| f.title.contains("cipher"))
            .collect();
        assert!(
            cipher_findings.is_empty(),
            "DESCRIBES/NODES should not match DES"
        );
    }

    #[test]
    fn detect_des_standalone_and_dotted() {
        let source = "cipher = DES.new(key)\n";
        let findings = scan_crypto_failure(source, "enc.py");
        assert!(!findings.is_empty(), "DES.new should be flagged");

        let source2 = "algo = DES\n";
        let findings2 = scan_crypto_failure(source2, "enc2.py");
        assert!(!findings2.is_empty(), "standalone DES should be flagged");
    }
}
