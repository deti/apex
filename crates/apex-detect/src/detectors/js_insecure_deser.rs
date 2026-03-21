//! JavaScript/TypeScript insecure deserialization detector (CWE-502).
//!
//! Catches unsafe YAML loading, eval of parsed JSON, dynamic Function
//! construction, and serialize-javascript with `unsafe: true`.

use apex_core::error::Result;
use apex_core::types::Language;
use async_trait::async_trait;
use regex::Regex;
use std::sync::LazyLock;
use uuid::Uuid;

use super::util::{in_test_block, is_comment, is_test_file};
use crate::context::AnalysisContext;
use crate::finding::{Finding, FindingCategory, Severity};
use crate::Detector;

pub struct JsInsecureDeserDetector;

struct DeserPattern {
    name: &'static str,
    regex: &'static str,
    /// Lines matching any of these are safe — skip the finding.
    safe_indicators: &'static [&'static str],
    description: &'static str,
}

const DESER_PATTERNS: &[DeserPattern] = &[
    DeserPattern {
        name: "Unsafe yaml.load",
        regex: r"yaml\.load\s*\(",
        safe_indicators: &["yaml.safeLoad", "yaml.SAFE_SCHEMA", "safe_load"],
        description:
            "yaml.load() without safe schema — use yaml.safeLoad or yaml.load with SAFE_SCHEMA",
    },
    DeserPattern {
        name: "eval(JSON.parse(...))",
        regex: r"eval\s*\(\s*JSON\.parse\s*\(",
        safe_indicators: &[],
        description: "eval(JSON.parse(...)) — code execution via deserialized JSON",
    },
    DeserPattern {
        name: "new Function with variable",
        regex: r"new\s+Function\s*\(",
        safe_indicators: &[],
        description: "new Function() with dynamic argument — equivalent to eval",
    },
    DeserPattern {
        name: "serialize-javascript unsafe",
        regex: r"serialize\s*\([^)]*unsafe\s*:\s*true",
        safe_indicators: &[],
        description: "serialize-javascript with unsafe: true — allows arbitrary code in output",
    },
];

static COMPILED_DESER: LazyLock<Vec<(&'static DeserPattern, Regex)>> = LazyLock::new(|| {
    DESER_PATTERNS
        .iter()
        .map(|p| (p, Regex::new(p.regex).expect("invalid deser regex")))
        .collect()
});

#[async_trait]
impl Detector for JsInsecureDeserDetector {
    fn name(&self) -> &str {
        "js-insecure-deser"
    }

    async fn analyze(&self, ctx: &AnalysisContext) -> Result<Vec<Finding>> {
        if ctx.language != Language::JavaScript {
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

                if in_test_block(source, line_num) {
                    continue;
                }

                for (pattern, regex) in COMPILED_DESER.iter() {
                    if regex.is_match(trimmed) {
                        // Check safe indicators on the same line
                        let is_safe = pattern
                            .safe_indicators
                            .iter()
                            .any(|ind| trimmed.contains(ind));
                        if is_safe {
                            continue;
                        }

                        let line_1based = (line_num + 1) as u32;

                        findings.push(Finding {
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
                                "Insecure deserialization pattern `{}` found in {}:{}",
                                pattern.name,
                                path.display(),
                                line_1based
                            ),
                            evidence: vec![],
                            covered: false,
                            suggestion: "Use safe deserialization methods (yaml.safeLoad, avoid eval on parsed data)".into(),
                            explanation: None,
                            fix: None,
                            cwe_ids: vec![502],
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
    async fn detects_yaml_load() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/config.js"),
            "const data = yaml.load(rawInput);\n".into(),
        );
        let ctx = make_ctx(files, Language::JavaScript);
        let findings = JsInsecureDeserDetector.analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, Severity::High);
        assert_eq!(findings[0].category, FindingCategory::Injection);
        assert_eq!(findings[0].cwe_ids, vec![502]);
    }

    #[tokio::test]
    async fn detects_eval_json_parse() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/handler.js"),
            "const result = eval(JSON.parse(input));\n".into(),
        );
        let ctx = make_ctx(files, Language::JavaScript);
        let findings = JsInsecureDeserDetector.analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].cwe_ids, vec![502]);
    }

    #[tokio::test]
    async fn detects_new_function() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/exec.js"),
            "const fn = new Function(userCode);\n".into(),
        );
        let ctx = make_ctx(files, Language::JavaScript);
        let findings = JsInsecureDeserDetector.analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 1);
    }

    #[tokio::test]
    async fn detects_serialize_unsafe() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/serial.js"),
            "const out = serialize(data, { unsafe: true });\n".into(),
        );
        let ctx = make_ctx(files, Language::JavaScript);
        let findings = JsInsecureDeserDetector.analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 1);
    }

    #[tokio::test]
    async fn safe_yaml_safe_load_no_finding() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/config.js"),
            "const data = yaml.safeLoad(rawInput);\n".into(),
        );
        let ctx = make_ctx(files, Language::JavaScript);
        let findings = JsInsecureDeserDetector.analyze(&ctx).await.unwrap();
        assert!(findings.is_empty());
    }

    #[tokio::test]
    async fn safe_yaml_load_with_safe_schema_no_finding() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/config.js"),
            "const data = yaml.load(rawInput, { schema: yaml.SAFE_SCHEMA });\n".into(),
        );
        let ctx = make_ctx(files, Language::JavaScript);
        let findings = JsInsecureDeserDetector.analyze(&ctx).await.unwrap();
        assert!(findings.is_empty());
    }

    #[tokio::test]
    async fn safe_json_parse_alone_no_finding() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/parse.js"),
            "const data = JSON.parse(input);\n".into(),
        );
        let ctx = make_ctx(files, Language::JavaScript);
        let findings = JsInsecureDeserDetector.analyze(&ctx).await.unwrap();
        assert!(findings.is_empty());
    }

    #[tokio::test]
    async fn skips_non_js_language() {
        let mut files = HashMap::new();
        files.insert(PathBuf::from("src/config.py"), "yaml.load(data)\n".into());
        let ctx = make_ctx(files, Language::Python);
        let findings = JsInsecureDeserDetector.analyze(&ctx).await.unwrap();
        assert!(findings.is_empty());
    }

    #[tokio::test]
    async fn skips_test_files() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("tests/test_deser.js"),
            "const data = yaml.load(rawInput);\n".into(),
        );
        let ctx = make_ctx(files, Language::JavaScript);
        let findings = JsInsecureDeserDetector.analyze(&ctx).await.unwrap();
        assert!(findings.is_empty());
    }

    #[tokio::test]
    async fn skips_comments() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/config.js"),
            "// yaml.load(data) is unsafe\nconst safe = 1;\n".into(),
        );
        let ctx = make_ctx(files, Language::JavaScript);
        let findings = JsInsecureDeserDetector.analyze(&ctx).await.unwrap();
        assert!(findings.is_empty());
    }

    #[test]
    fn does_not_use_cargo_subprocess() {
        assert!(!JsInsecureDeserDetector.uses_cargo_subprocess());
    }
}
