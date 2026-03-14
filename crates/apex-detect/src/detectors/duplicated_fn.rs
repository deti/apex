use apex_core::error::Result;
use async_trait::async_trait;
use regex::Regex;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::LazyLock;
use uuid::Uuid;

use super::util::is_test_file;
use crate::context::AnalysisContext;
use crate::finding::{Finding, FindingCategory, Severity};
use crate::Detector;

pub struct DuplicatedFnDetector;

static FN_DEF_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^\s*(?:pub\s+)?fn\s+(\w+)\s*\(").expect("invalid fn def regex")
});

/// Extract free-standing function names from source, skipping functions inside
/// `impl` blocks and `#[cfg(test)] mod tests` blocks.
fn extract_free_functions(source: &str) -> Vec<String> {
    let mut functions = Vec::new();
    let mut impl_depth: i32 = 0; // >0 means we are inside an impl block
    let mut test_block = false;
    let mut test_block_start_depth: i32 = 0;
    let mut brace_depth: i32 = 0;
    let mut in_impl = false;

    for line in source.lines() {
        let trimmed = line.trim();

        // Detect #[cfg(test)] or `mod tests {`
        if !test_block
            && (trimmed.contains("#[cfg(test)]")
                || (trimmed.starts_with("mod tests") && trimmed.contains('{')))
        {
            test_block = true;
            test_block_start_depth = brace_depth;
        }

        // Detect `impl` block start (but not `fn` lines that happen to contain "impl")
        if !in_impl
            && !test_block
            && (trimmed.starts_with("impl ")
                || trimmed.starts_with("impl<")
                || trimmed.starts_with("unsafe impl "))
        {
            in_impl = true;
            impl_depth = brace_depth;
        }

        // Try to match function definition only if not inside impl or test block
        if !in_impl && !test_block {
            if let Some(caps) = FN_DEF_RE.captures(line) {
                if let Some(name) = caps.get(1) {
                    functions.push(name.as_str().to_string());
                }
            }
        }

        // Track brace depth
        for ch in line.chars() {
            match ch {
                '{' => brace_depth += 1,
                '}' => {
                    brace_depth -= 1;
                    if in_impl && brace_depth <= impl_depth {
                        in_impl = false;
                    }
                    if test_block && brace_depth <= test_block_start_depth {
                        test_block = false;
                    }
                }
                _ => {}
            }
        }
    }

    functions
}

#[async_trait]
impl Detector for DuplicatedFnDetector {
    fn name(&self) -> &str {
        "duplicated-fn"
    }

    async fn analyze(&self, ctx: &AnalysisContext) -> Result<Vec<Finding>> {
        // Pass 1: collect function names per file
        let mut fn_to_files: HashMap<String, Vec<PathBuf>> = HashMap::new();

        for (path, source) in &ctx.source_cache {
            if is_test_file(path) {
                continue;
            }

            let fns = extract_free_functions(source);
            for name in fns {
                fn_to_files
                    .entry(name)
                    .or_default()
                    .push(path.clone());
            }
        }

        // Pass 2: report duplicates
        let mut findings = Vec::new();

        // Sort keys for deterministic output
        let mut keys: Vec<_> = fn_to_files.keys().cloned().collect();
        keys.sort();

        for name in keys {
            let files = &fn_to_files[&name];
            if files.len() >= 2 {
                let file_list: Vec<String> =
                    files.iter().map(|p| p.display().to_string()).collect();
                findings.push(Finding {
                    id: Uuid::new_v4(),
                    detector: self.name().into(),
                    severity: Severity::Low,
                    category: FindingCategory::SecuritySmell,
                    file: files[0].clone(),
                    line: None,
                    title: format!(
                        "Duplicated function `{name}` defined in {} files",
                        files.len()
                    ),
                    description: format!(
                        "Function `{name}` is defined in multiple files: {}. \
                         Consider extracting to a shared module to avoid divergence.",
                        file_list.join(", ")
                    ),
                    evidence: vec![],
                    covered: false,
                    suggestion: "Extract the duplicated function into a shared module".into(),
                    explanation: None,
                    fix: None,
                    cwe_ids: vec![1041],
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
    async fn detects_duplicate_free_function() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/a.rs"),
            "fn fnv1a_hash(s: &str) -> u64 {\n    0\n}\n".into(),
        );
        files.insert(
            PathBuf::from("src/b.rs"),
            "fn fnv1a_hash(s: &str) -> u64 {\n    0\n}\n".into(),
        );
        let ctx = make_ctx(files, Language::Rust);
        let findings = DuplicatedFnDetector.analyze(&ctx).await.unwrap();
        assert!(!findings.is_empty());
        assert_eq!(findings[0].severity, Severity::Low);
        assert_eq!(findings[0].category, FindingCategory::SecuritySmell);
        assert_eq!(findings[0].cwe_ids, vec![1041]);
        assert!(findings[0].title.contains("fnv1a_hash"));
    }

    #[tokio::test]
    async fn no_finding_for_different_functions() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/a.rs"),
            "fn foo() {}\n".into(),
        );
        files.insert(
            PathBuf::from("src/b.rs"),
            "fn bar() {}\n".into(),
        );
        let ctx = make_ctx(files, Language::Rust);
        let findings = DuplicatedFnDetector.analyze(&ctx).await.unwrap();
        assert!(findings.is_empty());
    }

    #[tokio::test]
    async fn skips_functions_inside_impl_blocks() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/a.rs"),
            "impl Display for A {\n    fn fmt(&self) {}\n}\n".into(),
        );
        files.insert(
            PathBuf::from("src/b.rs"),
            "impl Display for B {\n    fn fmt(&self) {}\n}\n".into(),
        );
        let ctx = make_ctx(files, Language::Rust);
        let findings = DuplicatedFnDetector.analyze(&ctx).await.unwrap();
        assert!(findings.is_empty());
    }

    #[tokio::test]
    async fn skips_test_files() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("tests/a.rs"),
            "fn helper() {}\n".into(),
        );
        files.insert(
            PathBuf::from("tests/b.rs"),
            "fn helper() {}\n".into(),
        );
        let ctx = make_ctx(files, Language::Rust);
        let findings = DuplicatedFnDetector.analyze(&ctx).await.unwrap();
        assert!(findings.is_empty());
    }

    #[tokio::test]
    async fn skips_functions_in_cfg_test_blocks() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/a.rs"),
            "fn real_fn() {}\n\n#[cfg(test)]\nmod tests {\n    fn helper() {}\n}\n".into(),
        );
        files.insert(
            PathBuf::from("src/b.rs"),
            "fn other_fn() {}\n\n#[cfg(test)]\nmod tests {\n    fn helper() {}\n}\n".into(),
        );
        let ctx = make_ctx(files, Language::Rust);
        let findings = DuplicatedFnDetector.analyze(&ctx).await.unwrap();
        assert!(findings.is_empty());
    }

    #[tokio::test]
    async fn one_finding_per_duplicate_group() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/a.rs"),
            "fn compute() {}\n".into(),
        );
        files.insert(
            PathBuf::from("src/b.rs"),
            "fn compute() {}\n".into(),
        );
        files.insert(
            PathBuf::from("src/c.rs"),
            "fn compute() {}\n".into(),
        );
        let ctx = make_ctx(files, Language::Rust);
        let findings = DuplicatedFnDetector.analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 1);
        assert!(findings[0].title.contains("3 files"));
    }

    #[tokio::test]
    async fn detects_pub_fn_duplicates() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/a.rs"),
            "pub fn init() {}\n".into(),
        );
        files.insert(
            PathBuf::from("src/b.rs"),
            "pub fn init() {}\n".into(),
        );
        let ctx = make_ctx(files, Language::Rust);
        let findings = DuplicatedFnDetector.analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 1);
        assert!(findings[0].title.contains("init"));
    }

    #[test]
    fn extract_free_functions_basic() {
        let source = "fn foo() {}\nfn bar() {}\n";
        let fns = extract_free_functions(source);
        assert_eq!(fns, vec!["foo", "bar"]);
    }

    #[test]
    fn extract_free_functions_skips_impl() {
        let source = "impl Foo {\n    fn method(&self) {}\n}\nfn free() {}\n";
        let fns = extract_free_functions(source);
        assert_eq!(fns, vec!["free"]);
    }

    #[test]
    fn extract_free_functions_skips_cfg_test() {
        let source = "fn real() {}\n#[cfg(test)]\nmod tests {\n    fn helper() {}\n}\n";
        let fns = extract_free_functions(source);
        assert_eq!(fns, vec!["real"]);
    }

    #[test]
    fn does_not_use_cargo_subprocess() {
        assert!(!DuplicatedFnDetector.uses_cargo_subprocess());
    }
}
