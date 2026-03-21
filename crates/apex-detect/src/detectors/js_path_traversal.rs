//! JavaScript/TypeScript path traversal detector (CWE-22).
//!
//! Catches fs.readFile, fs.writeFile, and res.sendFile calls where the
//! first argument is a variable (not a string literal), indicating possible
//! path traversal when user input flows into file-system operations.

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

pub struct JsPathTraversalDetector;

struct FsPattern {
    name: &'static str,
    /// Regex to detect the call. Capture group 1 is the first argument.
    call_regex: &'static str,
    description: &'static str,
}

const FS_PATTERNS: &[FsPattern] = &[
    FsPattern {
        name: "fs.readFile",
        call_regex: r"fs\.readFile(?:Sync)?\s*\(\s*([^,\)]+)",
        description: "fs.readFile with dynamic path — potential path traversal",
    },
    FsPattern {
        name: "fs.writeFile",
        call_regex: r"fs\.writeFile(?:Sync)?\s*\(\s*([^,\)]+)",
        description: "fs.writeFile with dynamic path — potential path traversal",
    },
    FsPattern {
        name: "res.sendFile",
        call_regex: r"res\.sendFile\s*\(\s*([^,\)]+)",
        description: "res.sendFile with dynamic path — potential path traversal in Express",
    },
];

/// Safe argument patterns: string literals, __dirname, __filename, path.resolve constants.
const SAFE_ARG_PREFIXES: &[&str] = &[
    "\"",         // string literal (double quote)
    "'",          // string literal (single quote)
    "`",          // template literal
    "__dirname",  // Node.js directory constant
    "__filename", // Node.js filename constant
];

static COMPILED_FS: LazyLock<Vec<(&'static FsPattern, Regex)>> = LazyLock::new(|| {
    FS_PATTERNS
        .iter()
        .map(|p| (p, Regex::new(p.call_regex).expect("invalid fs regex")))
        .collect()
});

fn is_safe_argument(arg: &str) -> bool {
    let trimmed = arg.trim();
    SAFE_ARG_PREFIXES
        .iter()
        .any(|prefix| trimmed.starts_with(prefix))
}

#[async_trait]
impl Detector for JsPathTraversalDetector {
    fn name(&self) -> &str {
        "js-path-traversal"
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

                for (pattern, regex) in COMPILED_FS.iter() {
                    if let Some(caps) = regex.captures(trimmed) {
                        let arg = caps.get(1).map(|m| m.as_str()).unwrap_or("");
                        if is_safe_argument(arg) {
                            continue;
                        }

                        let line_1based = (line_num + 1) as u32;

                        findings.push(Finding {
                            id: Uuid::new_v4(),
                            detector: self.name().into(),
                            severity: Severity::High,
                            category: FindingCategory::PathTraversal,
                            file: path.clone(),
                            line: Some(line_1based),
                            title: format!(
                                "{}: {} at line {}",
                                pattern.name, pattern.description, line_1based
                            ),
                            description: format!(
                                "Path traversal pattern `{}` found in {}:{}",
                                pattern.name,
                                path.display(),
                                line_1based
                            ),
                            evidence: vec![],
                            covered: false,
                            suggestion: "Validate file paths against a base directory using path.resolve and startsWith checks".into(),
                            explanation: None,
                            fix: None,
                            cwe_ids: vec![22],
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
    async fn detects_fs_read_file_variable() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/files.js"),
            "const data = fs.readFile(userPath, 'utf8');\n".into(),
        );
        let ctx = make_ctx(files, Language::JavaScript);
        let findings = JsPathTraversalDetector.analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, Severity::High);
        assert_eq!(findings[0].category, FindingCategory::PathTraversal);
        assert_eq!(findings[0].cwe_ids, vec![22]);
    }

    #[tokio::test]
    async fn detects_res_send_file_variable() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/routes.js"),
            "app.get('/file', (req, res) => { res.sendFile(filePath); });\n".into(),
        );
        let ctx = make_ctx(files, Language::JavaScript);
        let findings = JsPathTraversalDetector.analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 1);
    }

    #[tokio::test]
    async fn detects_fs_write_file_variable() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/upload.js"),
            "fs.writeFile(outputPath, data);\n".into(),
        );
        let ctx = make_ctx(files, Language::JavaScript);
        let findings = JsPathTraversalDetector.analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 1);
    }

    #[tokio::test]
    async fn safe_literal_path_no_finding() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/config.js"),
            "fs.readFile(\"./config.json\", 'utf8');\n".into(),
        );
        let ctx = make_ctx(files, Language::JavaScript);
        let findings = JsPathTraversalDetector.analyze(&ctx).await.unwrap();
        assert!(findings.is_empty());
    }

    #[tokio::test]
    async fn safe_dirname_no_finding() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/config.js"),
            "fs.readFile(__dirname + \"/config.json\", 'utf8');\n".into(),
        );
        let ctx = make_ctx(files, Language::JavaScript);
        let findings = JsPathTraversalDetector.analyze(&ctx).await.unwrap();
        assert!(findings.is_empty());
    }

    #[tokio::test]
    async fn safe_single_quote_literal_no_finding() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/config.js"),
            "fs.readFile('./data.txt');\n".into(),
        );
        let ctx = make_ctx(files, Language::JavaScript);
        let findings = JsPathTraversalDetector.analyze(&ctx).await.unwrap();
        assert!(findings.is_empty());
    }

    #[tokio::test]
    async fn detects_read_file_sync_variable() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/sync.js"),
            "const data = fs.readFileSync(userPath);\n".into(),
        );
        let ctx = make_ctx(files, Language::JavaScript);
        let findings = JsPathTraversalDetector.analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 1);
    }

    #[tokio::test]
    async fn skips_non_js_language() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/main.py"),
            "fs.readFile(userPath);\n".into(),
        );
        let ctx = make_ctx(files, Language::Python);
        let findings = JsPathTraversalDetector.analyze(&ctx).await.unwrap();
        assert!(findings.is_empty());
    }

    #[tokio::test]
    async fn skips_test_files() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("tests/test_files.js"),
            "fs.readFile(userPath);\n".into(),
        );
        let ctx = make_ctx(files, Language::JavaScript);
        let findings = JsPathTraversalDetector.analyze(&ctx).await.unwrap();
        assert!(findings.is_empty());
    }

    #[tokio::test]
    async fn skips_comments() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/files.js"),
            "// fs.readFile(userPath);\nconst x = 1;\n".into(),
        );
        let ctx = make_ctx(files, Language::JavaScript);
        let findings = JsPathTraversalDetector.analyze(&ctx).await.unwrap();
        assert!(findings.is_empty());
    }

    #[test]
    fn does_not_use_cargo_subprocess() {
        assert!(!JsPathTraversalDetector.uses_cargo_subprocess());
    }

    #[test]
    fn is_safe_argument_literals() {
        assert!(is_safe_argument("\"./config.json\""));
        assert!(is_safe_argument("'./config.json'"));
        assert!(is_safe_argument("`./config.json`"));
        assert!(is_safe_argument("__dirname + \"/config.json\""));
        assert!(is_safe_argument("__filename"));
        assert!(!is_safe_argument("userPath"));
        assert!(!is_safe_argument("req.params.file"));
    }
}
