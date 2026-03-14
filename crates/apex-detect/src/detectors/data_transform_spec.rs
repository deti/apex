//! Data Transform Spec Mining.
//! Detects unpaired data transformation calls — e.g., `base64.encode` without
//! a corresponding `base64.decode` in the same module.  Missing inverse
//! transforms often indicate data that is encoded/serialized but never decoded,
//! or compressed without a decompression path, which can mask bugs and violate
//! the principle of reversible transforms.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use apex_core::error::Result;
use apex_core::types::Language;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::context::AnalysisContext;
use crate::finding::{Finding, FindingCategory, Severity};
use crate::Detector;

/// A paired data-transform specification (forward + inverse).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataTransformSpec {
    /// Human-readable category (e.g. "base64", "json", "zlib").
    pub category: String,
    /// Forward transform pattern (e.g. "base64.b64encode").
    pub forward: String,
    /// Inverse transform pattern (e.g. "base64.b64decode").
    pub inverse: String,
}

// ---------------------------------------------------------------------------
// Transform pair definitions per language
// ---------------------------------------------------------------------------

/// (category, forward_pattern, inverse_pattern)
type TransformPair = (&'static str, &'static str, &'static str);

const PYTHON_TRANSFORMS: &[TransformPair] = &[
    ("base64", "base64.b64encode", "base64.b64decode"),
    ("base64", "base64.encode", "base64.decode"),
    ("json", "json.dumps", "json.loads"),
    ("json", "json.dump", "json.load"),
    ("zlib", "zlib.compress", "zlib.decompress"),
    ("gzip", "gzip.compress", "gzip.decompress"),
    ("pickle", "pickle.dumps", "pickle.loads"),
    ("pickle", "pickle.dump", "pickle.load"),
    ("marshal", "marshal.dumps", "marshal.loads"),
    ("url", "urllib.parse.quote", "urllib.parse.unquote"),
    ("url", "urllib.parse.urlencode", "urllib.parse.parse_qs"),
];

const RUST_TRANSFORMS: &[TransformPair] = &[
    ("base64", "base64::encode", "base64::decode"),
    ("serde_json", "serde_json::to_string", "serde_json::from_str"),
    ("serde_json", "serde_json::to_vec", "serde_json::from_slice"),
    ("serde_json", "to_string(", "from_str("),
    ("bincode", "bincode::serialize", "bincode::deserialize"),
    ("flate2", "GzEncoder", "GzDecoder"),
    ("flate2", "ZlibEncoder", "ZlibDecoder"),
    ("encrypt", ".encrypt(", ".decrypt("),
];

const JS_TRANSFORMS: &[TransformPair] = &[
    ("base64", "btoa(", "atob("),
    ("base64", "Buffer.from(", "toString("),
    ("json", "JSON.stringify(", "JSON.parse("),
    ("url", "encodeURIComponent(", "decodeURIComponent("),
    ("url", "encodeURI(", "decodeURI("),
    ("encrypt", ".encrypt(", ".decrypt("),
    ("zlib", "zlib.deflate", "zlib.inflate"),
    ("gzip", "zlib.gzip", "zlib.gunzip"),
];

fn transforms_for_language(lang: Language) -> &'static [TransformPair] {
    match lang {
        Language::Python => PYTHON_TRANSFORMS,
        Language::Rust => RUST_TRANSFORMS,
        Language::JavaScript => JS_TRANSFORMS,
        _ => &[],
    }
}

// ---------------------------------------------------------------------------
// Detector
// ---------------------------------------------------------------------------

pub struct DataTransformSpecMiner;

#[async_trait]
impl Detector for DataTransformSpecMiner {
    fn name(&self) -> &str {
        "data-transform-spec"
    }

    async fn analyze(&self, ctx: &AnalysisContext) -> Result<Vec<Finding>> {
        let pairs = transforms_for_language(ctx.language);
        if pairs.is_empty() {
            return Ok(vec![]);
        }

        let mut findings = Vec::new();

        for (path, source) in &ctx.source_cache {
            // Skip test files
            if super::util::is_test_file(path) {
                continue;
            }

            let file_findings = analyze_file(path, source, pairs);
            findings.extend(file_findings);
        }

        Ok(findings)
    }
}

/// Check if `haystack` contains `pattern` with a word boundary after it.
/// This prevents `json.dump` from matching inside `json.dumps`.
fn contains_pattern(haystack: &str, pattern: &str) -> bool {
    // Patterns ending with `(` are self-delimiting — no boundary check needed.
    if pattern.ends_with('(') {
        return haystack.contains(pattern);
    }
    let mut start = 0;
    while let Some(pos) = haystack[start..].find(pattern) {
        let end = start + pos + pattern.len();
        let next_char = haystack[end..].chars().next();
        let bounded = match next_char {
            None => true,
            Some(c) => !c.is_alphanumeric(),
        };
        if bounded {
            return true;
        }
        start = start + pos + 1;
    }
    false
}

/// Analyze a single file for unpaired transforms.
fn analyze_file(path: &Path, source: &str, pairs: &[TransformPair]) -> Vec<Finding> {
    // Collect which patterns appear and on which lines.
    let mut forward_lines: HashMap<&str, Vec<u32>> = HashMap::new();
    let mut inverse_present: HashSet<&str> = HashSet::new();

    for (line_num, line) in source.lines().enumerate() {
        let trimmed = line.trim();
        // Skip comments (simple heuristic)
        if trimmed.starts_with("//") || trimmed.starts_with('#') || trimmed.starts_with("/*") {
            continue;
        }

        for &(_, fwd, inv) in pairs {
            if contains_pattern(trimmed, fwd) {
                forward_lines
                    .entry(fwd)
                    .or_default()
                    .push((line_num + 1) as u32);
            }
            if contains_pattern(trimmed, inv) {
                inverse_present.insert(inv);
            }
        }
    }

    let mut findings = Vec::new();

    for &(category, fwd, inv) in pairs {
        if let Some(lines) = forward_lines.get(fwd) {
            if !inverse_present.contains(inv) {
                // Forward transform found but no inverse in this file.
                // Report on the first occurrence.
                let line = lines[0];
                findings.push(Finding {
                    id: Uuid::new_v4(),
                    detector: "data-transform-spec".into(),
                    severity: Severity::Low,
                    category: FindingCategory::LogicBug,
                    file: path.to_path_buf(),
                    line: Some(line),
                    title: format!(
                        "Unpaired {category} transform: `{fwd}` without `{inv}`"
                    ),
                    description: format!(
                        "File {} uses `{fwd}` (line {line}) but never calls `{inv}`. \
                         This may indicate data that is transformed but never reversed.",
                        path.display()
                    ),
                    evidence: vec![],
                    covered: false,
                    suggestion: format!(
                        "Add a corresponding `{inv}` call, or verify the one-way transform is intentional."
                    ),
                    explanation: None,
                    fix: None,
                    cwe_ids: vec![754],
                });
            }
        }
    }

    findings
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::AnalysisContext;
    use std::collections::HashMap;
    use std::path::PathBuf;

    fn make_ctx(source_files: HashMap<PathBuf, String>, lang: Language) -> AnalysisContext {
        AnalysisContext {
            language: lang,
            source_cache: source_files,
            ..AnalysisContext::test_default()
        }
    }

    // -- Python tests --

    #[tokio::test]
    async fn python_paired_json_no_finding() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/codec.py"),
            "import json\ndef roundtrip(x):\n    s = json.dumps(x)\n    return json.loads(s)\n"
                .into(),
        );
        let ctx = make_ctx(files, Language::Python);
        let findings = DataTransformSpecMiner.analyze(&ctx).await.unwrap();
        assert!(findings.is_empty());
    }

    #[tokio::test]
    async fn python_unpaired_json_dumps_finding() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/writer.py"),
            "import json\ndef save(x):\n    return json.dumps(x)\n".into(),
        );
        let ctx = make_ctx(files, Language::Python);
        let findings = DataTransformSpecMiner.analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 1);
        assert!(findings[0].title.contains("json.dumps"));
        assert!(findings[0].title.contains("json.loads"));
        assert_eq!(findings[0].cwe_ids, vec![754]);
        assert_eq!(findings[0].line, Some(3));
    }

    #[tokio::test]
    async fn python_unpaired_base64_encode() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/encode.py"),
            "import base64\ndef enc(data):\n    return base64.b64encode(data)\n".into(),
        );
        let ctx = make_ctx(files, Language::Python);
        let findings = DataTransformSpecMiner.analyze(&ctx).await.unwrap();
        assert!(findings.iter().any(|f| f.title.contains("base64")));
    }

    // -- Rust tests --

    #[tokio::test]
    async fn rust_paired_serde_json_no_finding() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/codec.rs"),
            "fn roundtrip(v: &Value) {\n    let s = serde_json::to_string(v).unwrap();\n    let _: Value = serde_json::from_str(&s).unwrap();\n}\n".into(),
        );
        let ctx = make_ctx(files, Language::Rust);
        let findings = DataTransformSpecMiner.analyze(&ctx).await.unwrap();
        // The serde_json pair is matched, so no finding for that pair.
        // (The generic to_string/from_str pair is also matched.)
        let serde_findings: Vec<_> = findings
            .iter()
            .filter(|f| f.title.contains("serde_json"))
            .collect();
        assert!(serde_findings.is_empty());
    }

    #[tokio::test]
    async fn rust_unpaired_base64_encode() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/util.rs"),
            "fn encode(data: &[u8]) -> String {\n    base64::encode(data)\n}\n".into(),
        );
        let ctx = make_ctx(files, Language::Rust);
        let findings = DataTransformSpecMiner.analyze(&ctx).await.unwrap();
        assert!(findings.iter().any(|f| f.title.contains("base64::encode")));
    }

    // -- JavaScript tests --

    #[tokio::test]
    async fn js_paired_json_no_finding() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/util.js"),
            "function roundtrip(obj) {\n  const s = JSON.stringify(obj);\n  return JSON.parse(s);\n}\n".into(),
        );
        let ctx = make_ctx(files, Language::JavaScript);
        let findings = DataTransformSpecMiner.analyze(&ctx).await.unwrap();
        let json_findings: Vec<_> = findings.iter().filter(|f| f.title.contains("JSON")).collect();
        assert!(json_findings.is_empty());
    }

    #[tokio::test]
    async fn js_unpaired_btoa() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/encode.js"),
            "function encode(s) {\n  return btoa(s);\n}\n".into(),
        );
        let ctx = make_ctx(files, Language::JavaScript);
        let findings = DataTransformSpecMiner.analyze(&ctx).await.unwrap();
        assert!(findings.iter().any(|f| f.title.contains("btoa(")));
    }

    // -- Cross-cutting tests --

    #[tokio::test]
    async fn skips_test_files() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("tests/test_codec.py"),
            "import json\njson.dumps({})\n".into(),
        );
        let ctx = make_ctx(files, Language::Python);
        let findings = DataTransformSpecMiner.analyze(&ctx).await.unwrap();
        assert!(findings.is_empty());
    }

    #[tokio::test]
    async fn empty_source_cache() {
        let ctx = make_ctx(HashMap::new(), Language::Python);
        let findings = DataTransformSpecMiner.analyze(&ctx).await.unwrap();
        assert!(findings.is_empty());
    }

    #[test]
    fn unsupported_language_returns_empty() {
        let pairs = transforms_for_language(Language::Java);
        assert!(pairs.is_empty());
    }
}
