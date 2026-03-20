//! Insecure deserialization detector — identifies unsafe deserialization patterns (CWE-502).

use crate::finding::{Finding, FindingCategory, Severity};
use std::path::PathBuf;
use uuid::Uuid;

/// Dangerous deserialization function patterns.
const UNSAFE_DESER_PATTERNS: &[&str] = &[
    "pickle.loads(",
    "pickle.load(",
    "marshal.loads(",
    "marshal.load(",
    "unserialize(",
    "ObjectInputStream",
];

/// Scan source code for insecure deserialization vulnerabilities.
pub fn scan_insecure_deserialization(source: &str, file_path: &str) -> Vec<Finding> {
    let mut findings = Vec::new();

    for (line_num, line) in source.lines().enumerate() {
        let line_1based = (line_num + 1) as u32;
        let trimmed = line.trim();

        // Skip comments.
        if trimmed.starts_with('#') || trimmed.starts_with("//") {
            continue;
        }

        let mut is_vuln = false;

        // Check direct dangerous deserialization calls.
        for pattern in UNSAFE_DESER_PATTERNS {
            if trimmed.contains(pattern) {
                is_vuln = true;
                break;
            }
        }

        // Check yaml.load without SafeLoader (but skip yaml.safe_load).
        if trimmed.contains("yaml.load(")
            && !trimmed.contains("yaml.safe_load(")
            && !trimmed.contains("SafeLoader")
            && !trimmed.contains("Loader=yaml.SafeLoader")
        {
            is_vuln = true;
        }

        if is_vuln {
            findings.push(Finding {
                id: Uuid::new_v4(),
                detector: "insecure_deserialization".into(),
                severity: Severity::High,
                category: FindingCategory::SecuritySmell,
                file: PathBuf::from(file_path),
                line: Some(line_1based),
                title: "Insecure deserialization of untrusted data".into(),
                description: format!(
                    "Unsafe deserialization at line {line_1based}. \
                     Deserializing untrusted data can lead to remote code execution."
                ),
                evidence: vec![],
                covered: false,
                suggestion: "Use safe alternatives: yaml.safe_load(), json.loads(), \
                             or restrict deserialization to trusted sources."
                    .into(),
                explanation: None,
                fix: None,
                cwe_ids: vec![502],
                    noisy: false,
            });
        }
    }

    findings
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_pickle_loads() {
        let source = "import pickle\ndata = pickle.loads(user_input)\n";
        let findings = scan_insecure_deserialization(source, "app.py");
        assert!(!findings.is_empty());
        assert!(findings[0].cwe_ids.contains(&502));
    }

    #[test]
    fn detect_yaml_load_without_safe_loader() {
        let source = "import yaml\nconfig = yaml.load(data)\n";
        let findings = scan_insecure_deserialization(source, "config.py");
        assert!(!findings.is_empty());
    }

    #[test]
    fn skip_yaml_safe_load() {
        let source = "import yaml\nconfig = yaml.safe_load(data)\n";
        let findings = scan_insecure_deserialization(source, "config.py");
        assert!(findings.is_empty());
    }

    #[test]
    fn skip_yaml_load_with_safe_loader() {
        let source = "config = yaml.load(data, Loader=SafeLoader)\n";
        let findings = scan_insecure_deserialization(source, "config.py");
        assert!(findings.is_empty());
    }

    #[test]
    fn detect_marshal_loads() {
        let source = "import marshal\nobj = marshal.loads(raw)\n";
        let findings = scan_insecure_deserialization(source, "loader.py");
        assert!(!findings.is_empty());
    }

    #[test]
    fn detect_php_unserialize() {
        let source = "$obj = unserialize($_GET['data']);\n";
        let findings = scan_insecure_deserialization(source, "handler.php");
        assert!(!findings.is_empty());
    }

    #[test]
    fn no_false_positive_on_comments() {
        let source = "# pickle.loads is dangerous\n// unserialize is bad\n";
        let findings = scan_insecure_deserialization(source, "notes.py");
        assert!(findings.is_empty());
    }

    #[test]
    fn multiple_findings_in_one_file() {
        let source = "a = pickle.loads(x)\nb = marshal.loads(y)\nc = yaml.load(z)\n";
        let findings = scan_insecure_deserialization(source, "bad.py");
        assert_eq!(findings.len(), 3);
    }
}
