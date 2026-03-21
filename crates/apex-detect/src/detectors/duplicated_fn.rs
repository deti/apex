use apex_core::error::Result;
use apex_core::types::Language;
use async_trait::async_trait;
use regex::Regex;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::LazyLock;
use uuid::Uuid;

use crate::context::AnalysisContext;
use crate::finding::{Finding, FindingCategory, Severity};
use crate::Detector;

pub struct DuplicatedFnDetector;

// Rust: `fn name(` or `pub fn name(` etc.
static RUST_FN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^\s*(?:pub(?:\(crate\))?\s+)?(?:async\s+)?fn\s+(\w+)\s*[<(]").unwrap()
});

// Python: `def name(`
static PY_FN: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^def\s+(\w+)\s*\(").unwrap());

// JavaScript: `function name(` or `export function name(` or `async function name(`
static JS_FN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(?:export\s+)?(?:async\s+)?function\s+(\w+)\s*\(").unwrap());

/// Extract free (non-method) function names from source for the given language.
fn extract_free_functions(source: &str, language: Language) -> Vec<String> {
    let mut names = Vec::new();

    match language {
        Language::Rust => {
            // In Rust, we use brace depth to skip impl blocks.
            // Free functions are at brace depth 0.
            let mut brace_depth: i32 = 0;
            for line in source.lines() {
                let trimmed = line.trim();
                if brace_depth == 0 {
                    if let Some(cap) = RUST_FN.captures(trimmed) {
                        if let Some(m) = cap.get(1) {
                            names.push(m.as_str().to_string());
                        }
                    }
                }
                for ch in line.chars() {
                    match ch {
                        '{' => brace_depth += 1,
                        '}' => brace_depth -= 1,
                        _ => {}
                    }
                }
            }
        }
        Language::Python => {
            // In Python, free functions are `def` at column 0 (no indentation).
            // Methods inside `class` blocks are indented.
            for line in source.lines() {
                // Only match lines with no leading whitespace
                if line.starts_with("def ") {
                    if let Some(cap) = PY_FN.captures(line) {
                        if let Some(m) = cap.get(1) {
                            names.push(m.as_str().to_string());
                        }
                    }
                }
            }
        }
        Language::JavaScript => {
            // In JS, free functions are at brace depth 0.
            // Methods inside class {} are at depth >= 1.
            let mut brace_depth: i32 = 0;
            for line in source.lines() {
                let trimmed = line.trim();
                if brace_depth == 0 {
                    if let Some(cap) = JS_FN.captures(trimmed) {
                        if let Some(m) = cap.get(1) {
                            names.push(m.as_str().to_string());
                        }
                    }
                }
                for ch in line.chars() {
                    match ch {
                        '{' => brace_depth += 1,
                        '}' => brace_depth -= 1,
                        _ => {}
                    }
                }
            }
        }
        _ => {}
    }

    names
}

#[async_trait]
impl Detector for DuplicatedFnDetector {
    fn name(&self) -> &str {
        "duplicated-fn"
    }

    async fn analyze(&self, ctx: &AnalysisContext) -> Result<Vec<Finding>> {
        match ctx.language {
            Language::Rust | Language::Python | Language::JavaScript => {}
            _ => return Ok(vec![]),
        }

        // Collect all free function names across all files
        let mut fn_locations: HashMap<String, Vec<PathBuf>> = HashMap::new();

        for (path, source) in &ctx.source_cache {
            let fns = extract_free_functions(source, ctx.language);
            for name in fns {
                fn_locations.entry(name).or_default().push(path.clone());
            }
        }

        let mut findings = Vec::new();

        for (fn_name, locations) in &fn_locations {
            if locations.len() > 1 {
                let file_list: Vec<String> =
                    locations.iter().map(|p| p.display().to_string()).collect();
                findings.push(Finding {
                    id: Uuid::new_v4(),
                    detector: self.name().into(),
                    severity: Severity::Low,
                    category: FindingCategory::LogicBug,
                    file: locations[0].clone(),
                    line: None,
                    title: format!(
                        "Duplicated function `{}` found in {} files",
                        fn_name,
                        locations.len()
                    ),
                    description: format!(
                        "Function `{}` is defined in multiple files: {}. \
                         Consider extracting to a shared module.",
                        fn_name,
                        file_list.join(", ")
                    ),
                    evidence: vec![],
                    covered: false,
                    suggestion: "Extract the duplicated function into a shared module".into(),
                    explanation: None,
                    fix: None,
                    cwe_ids: vec![],
                    noisy: false, base_severity: None, coverage_confidence: None,
                });
            }
        }

        Ok(findings)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::AnalysisContext;

    fn make_ctx(files: HashMap<PathBuf, String>, lang: Language) -> AnalysisContext {
        AnalysisContext {
            language: lang,
            source_cache: files,
            ..AnalysisContext::test_default()
        }
    }

    // ---- Rust ----

    #[tokio::test]
    async fn detects_duplicated_fn_rust() {
        let mut files = HashMap::new();
        files.insert(PathBuf::from("src/a.rs"), "fn helper() {}\n".into());
        files.insert(PathBuf::from("src/b.rs"), "fn helper() {}\n".into());
        let ctx = make_ctx(files, Language::Rust);
        let findings = DuplicatedFnDetector.analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 1);
        assert!(findings[0].title.contains("helper"));
        assert_eq!(findings[0].severity, Severity::Low);
    }

    #[tokio::test]
    async fn no_finding_for_unique_fns_rust() {
        let mut files = HashMap::new();
        files.insert(PathBuf::from("src/a.rs"), "fn alpha() {}\n".into());
        files.insert(PathBuf::from("src/b.rs"), "fn beta() {}\n".into());
        let ctx = make_ctx(files, Language::Rust);
        let findings = DuplicatedFnDetector.analyze(&ctx).await.unwrap();
        assert!(findings.is_empty());
    }

    #[tokio::test]
    async fn skips_methods_in_impl_block_rust() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/a.rs"),
            "impl Foo {\n    fn helper(&self) {}\n}\n".into(),
        );
        files.insert(
            PathBuf::from("src/b.rs"),
            "impl Bar {\n    fn helper(&self) {}\n}\n".into(),
        );
        let ctx = make_ctx(files, Language::Rust);
        let findings = DuplicatedFnDetector.analyze(&ctx).await.unwrap();
        assert!(findings.is_empty());
    }

    // ---- Python ----

    #[tokio::test]
    async fn detects_duplicated_fn_python() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/a.py"),
            "def helper():\n    pass\n".into(),
        );
        files.insert(
            PathBuf::from("src/b.py"),
            "def helper():\n    pass\n".into(),
        );
        let ctx = make_ctx(files, Language::Python);
        let findings = DuplicatedFnDetector.analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 1);
        assert!(findings[0].title.contains("helper"));
    }

    #[tokio::test]
    async fn skips_methods_in_class_python() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/a.py"),
            "class Foo:\n    def helper(self):\n        pass\n".into(),
        );
        files.insert(
            PathBuf::from("src/b.py"),
            "class Bar:\n    def helper(self):\n        pass\n".into(),
        );
        let ctx = make_ctx(files, Language::Python);
        let findings = DuplicatedFnDetector.analyze(&ctx).await.unwrap();
        assert!(findings.is_empty());
    }

    #[tokio::test]
    async fn no_finding_unique_fns_python() {
        let mut files = HashMap::new();
        files.insert(PathBuf::from("src/a.py"), "def alpha():\n    pass\n".into());
        files.insert(PathBuf::from("src/b.py"), "def beta():\n    pass\n".into());
        let ctx = make_ctx(files, Language::Python);
        let findings = DuplicatedFnDetector.analyze(&ctx).await.unwrap();
        assert!(findings.is_empty());
    }

    // ---- JavaScript ----

    #[tokio::test]
    async fn detects_duplicated_fn_js() {
        let mut files = HashMap::new();
        files.insert(PathBuf::from("src/a.js"), "function helper() {}\n".into());
        files.insert(PathBuf::from("src/b.js"), "function helper() {}\n".into());
        let ctx = make_ctx(files, Language::JavaScript);
        let findings = DuplicatedFnDetector.analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 1);
        assert!(findings[0].title.contains("helper"));
    }

    #[tokio::test]
    async fn detects_duplicated_export_fn_js() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/a.js"),
            "export function helper() {}\n".into(),
        );
        files.insert(
            PathBuf::from("src/b.js"),
            "export async function helper() {}\n".into(),
        );
        let ctx = make_ctx(files, Language::JavaScript);
        let findings = DuplicatedFnDetector.analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 1);
    }

    #[tokio::test]
    async fn skips_methods_in_class_js() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/a.js"),
            "class Foo {\n    function helper() {}\n}\n".into(),
        );
        files.insert(
            PathBuf::from("src/b.js"),
            "class Bar {\n    function helper() {}\n}\n".into(),
        );
        let ctx = make_ctx(files, Language::JavaScript);
        let findings = DuplicatedFnDetector.analyze(&ctx).await.unwrap();
        assert!(findings.is_empty());
    }

    // ---- Unsupported language ----

    #[tokio::test]
    async fn skips_unsupported_language() {
        let mut files = HashMap::new();
        files.insert(PathBuf::from("src/Main.java"), "void helper() {}\n".into());
        files.insert(PathBuf::from("src/Other.java"), "void helper() {}\n".into());
        let ctx = make_ctx(files, Language::Java);
        let findings = DuplicatedFnDetector.analyze(&ctx).await.unwrap();
        assert!(findings.is_empty());
    }

    // ---- extract_free_functions unit tests ----

    #[test]
    fn extract_rust_free_fns() {
        let src = "fn alpha() {}\npub fn beta() {}\nimpl X {\n    fn method() {}\n}\n";
        let fns = extract_free_functions(src, Language::Rust);
        assert_eq!(fns, vec!["alpha", "beta"]);
    }

    #[test]
    fn extract_python_free_fns() {
        let src = "def alpha():\n    pass\nclass Foo:\n    def method(self):\n        pass\ndef beta():\n    pass\n";
        let fns = extract_free_functions(src, Language::Python);
        assert_eq!(fns, vec!["alpha", "beta"]);
    }

    #[test]
    fn extract_js_free_fns() {
        let src = "function alpha() {}\nclass Foo {\n    function method() {}\n}\nexport function beta() {}\n";
        let fns = extract_free_functions(src, Language::JavaScript);
        assert_eq!(fns, vec!["alpha", "beta"]);
    }

    #[test]
    fn extract_rust_async_fn() {
        let src = "pub async fn serve(addr: &str) {}\n";
        let fns = extract_free_functions(src, Language::Rust);
        assert_eq!(fns, vec!["serve"]);
    }

    #[test]
    fn extract_js_async_fn() {
        let src = "export async function fetchData() {}\n";
        let fns = extract_free_functions(src, Language::JavaScript);
        assert_eq!(fns, vec!["fetchData"]);
    }

    #[test]
    fn does_not_use_cargo_subprocess() {
        assert!(!DuplicatedFnDetector.uses_cargo_subprocess());
    }
}
