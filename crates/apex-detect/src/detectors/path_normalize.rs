use async_trait::async_trait;
use uuid::Uuid;

use super::util::is_test_file;
use crate::context::AnalysisContext;
use crate::finding::{Finding, FindingCategory, Severity};
use crate::Detector;
use apex_core::error::Result;
use apex_core::types::Language;

pub struct PathNormalizationDetector;

// Function signature keywords for each language.
const PYTHON_FN_KEYWORDS: &[&str] = &["def "];
const JS_FN_KEYWORDS: &[&str] = &["function ", "=> {", "=> (", "const ", "let ", "var "];
const RUST_FN_KEYWORDS: &[&str] = &["fn "];

// Normalization / safe-path calls that indicate the developer handled the input.
const PYTHON_NORM_CALLS: &[&str] = &[
    "os.path.normpath",
    "os.path.realpath",
    "os.path.abspath",
    ".resolve()",
    "safe_join",
    "send_from_directory",
];

const JS_NORM_CALLS: &[&str] = &[
    "path.normalize",
    "path.resolve",
    "new URL(",
    "url.parse",
    "new URL",
];

const RUST_NORM_CALLS: &[&str] = &[
    ".canonicalize()",
    ".normalize()",
    ".clean()",
    "fs::canonicalize",
    "path_clean",
];

// Validation checks that also count as safe — these protect without full normalisation.
const VALIDATION_PATTERNS: &[&str] = &[
    "\"..\"..",   // Rust: contains("..")
    "\"..\"",    // any language string literal ".."
    "'..'",      // Python/JS single-quoted
    "dotdot",
    "\"//\"",
    "'//",
    "traversal",
];

/// Returns `true` if the function signature on `sig_line` suggests it handles
/// path / URL input.
fn sig_has_path_param(sig_line: &str) -> bool {
    let lower = sig_line.to_lowercase();
    // Check for parameter names or type annotations that suggest path/URL input.
    for pat in &["url", "path", "uri"] {
        if lower.contains(pat) {
            return true;
        }
    }
    false
}

/// Given the source lines of a file, collect the line ranges (start..=end) of
/// every function body that has a path/URL parameter.
fn collect_suspect_function_ranges(source: &str, lang: Language) -> Vec<(usize, usize)> {
    let lines: Vec<&str> = source.lines().collect();
    let fn_keywords: &[&str] = match lang {
        Language::Python => PYTHON_FN_KEYWORDS,
        Language::JavaScript => JS_FN_KEYWORDS,
        Language::Rust => RUST_FN_KEYWORDS,
        _ => return vec![],
    };

    let mut ranges = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i].trim();
        let is_fn_line = fn_keywords.iter().any(|kw| line.contains(kw));

        if is_fn_line && sig_has_path_param(line) {
            // Collect lines until the end of this function.
            // Simple heuristic: for Python, collect until we see a dedented non-blank
            // line or another `def`; for Rust/JS, track brace depth.
            let fn_start = i;
            let fn_end = match lang {
                Language::Python => find_python_fn_end(&lines, i),
                _ => find_brace_fn_end(&lines, i),
            };
            ranges.push((fn_start, fn_end));
            i = fn_end + 1;
            continue;
        }
        i += 1;
    }
    ranges
}

/// Find the end line of a Python function starting at `start`.
fn find_python_fn_end(lines: &[&str], start: usize) -> usize {
    // Determine indentation of the `def` line.
    let def_indent = lines[start].len() - lines[start].trim_start().len();
    let mut last = start;
    for (i, line) in lines.iter().enumerate().skip(start + 1) {
        if line.trim().is_empty() {
            continue;
        }
        let this_indent = line.len() - line.trim_start().len();
        if this_indent <= def_indent {
            // We've left the function.
            break;
        }
        last = i;
    }
    // If the function body was never entered (single-line or EOF), include at
    // least the next line so we scan the body.
    if last == start {
        last = (start + 1).min(lines.len().saturating_sub(1));
    }
    last
}

/// Find the end line of a Rust/JS function by tracking brace depth.
fn find_brace_fn_end(lines: &[&str], start: usize) -> usize {
    let mut depth: i32 = 0;
    let mut started = false;
    for (i, line) in lines.iter().enumerate().skip(start) {
        for ch in line.chars() {
            match ch {
                '{' => {
                    depth += 1;
                    started = true;
                }
                '}' => {
                    depth -= 1;
                    if started && depth <= 0 {
                        return i;
                    }
                }
                _ => {}
            }
        }
    }
    lines.len().saturating_sub(1)
}

/// Check whether the given source slice contains any normalization or validation call.
fn has_normalization(body_lines: &[&str], lang: Language) -> bool {
    let norm_calls: &[&str] = match lang {
        Language::Python => PYTHON_NORM_CALLS,
        Language::JavaScript => JS_NORM_CALLS,
        Language::Rust => RUST_NORM_CALLS,
        _ => &[],
    };

    for line in body_lines {
        let lower = line.to_lowercase();
        for call in norm_calls {
            if lower.contains(call) {
                return true;
            }
        }
        for vpat in VALIDATION_PATTERNS {
            if line.contains(vpat) {
                return true;
            }
        }
    }
    false
}

#[async_trait]
impl Detector for PathNormalizationDetector {
    fn name(&self) -> &str {
        "path-normalize"
    }

    fn uses_cargo_subprocess(&self) -> bool {
        false
    }

    async fn analyze(&self, ctx: &AnalysisContext) -> Result<Vec<Finding>> {
        let mut findings = Vec::new();

        for (path, source) in &ctx.source_cache {
            // Skip test files.
            if is_test_file(path) {
                continue;
            }

            // Only scan languages we know about.
            let lang = ctx.language;
            if !matches!(
                lang,
                Language::Python | Language::JavaScript | Language::Rust
            ) {
                continue;
            }

            let lines: Vec<&str> = source.lines().collect();
            let ranges = collect_suspect_function_ranges(source, lang);

            for (fn_start, fn_end) in ranges {
                let body = &lines[fn_start..=fn_end.min(lines.len().saturating_sub(1))];

                if !has_normalization(body, lang) {
                    let line_1based = (fn_start + 1) as u32;
                    findings.push(Finding {
                        id: Uuid::new_v4(),
                        detector: self.name().into(),
                        severity: Severity::Medium,
                        category: FindingCategory::PathTraversal,
                        file: path.clone(),
                        line: Some(line_1based),
                        title: format!(
                            "Missing path/URL normalization at line {line_1based}"
                        ),
                        description: format!(
                            "Function at {}:{} accepts a path/URL parameter but does \
                             not normalize or validate it, risking path traversal.",
                            path.display(),
                            line_1based
                        ),
                        evidence: vec![],
                        covered: false,
                        suggestion: "Normalize the path with os.path.normpath / \
                            path.normalize / .canonicalize() before use, or validate \
                            that it does not contain `..` / `//` sequences."
                            .into(),
                        explanation: None,
                        fix: None,
                    });
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
    use apex_core::command::RealCommandRunner;
    use apex_coverage::CoverageOracle;
    use std::collections::HashMap;
    use std::path::PathBuf;
    use std::sync::Arc;

    fn make_ctx_with_source(
        filename: &str,
        source: &str,
        lang: Language,
    ) -> AnalysisContext {
        let mut source_cache = HashMap::new();
        source_cache.insert(PathBuf::from(filename), source.to_string());

        AnalysisContext {
            target_root: PathBuf::from("/tmp/test"),
            language: lang,
            oracle: Arc::new(CoverageOracle::new()),
            file_paths: HashMap::new(),
            known_bugs: vec![],
            source_cache,
            fuzz_corpus: None,
            config: DetectConfig::default(),
            runner: Arc::new(RealCommandRunner),
        }
    }

    // 1. Python function with `url` param, no normalization → finding
    #[tokio::test]
    async fn detects_url_param_without_normalization() {
        let src = "\
def fetch(url):
    resp = requests.get(url)
    return resp
";
        let ctx = make_ctx_with_source("src/app.py", src, Language::Python);
        let findings = PathNormalizationDetector.analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 1, "expected 1 finding, got: {findings:?}");
        assert_eq!(findings[0].category, FindingCategory::PathTraversal);
        assert_eq!(findings[0].severity, Severity::Medium);
    }

    // 2. Python function using safe_join → no finding
    #[tokio::test]
    async fn no_finding_when_safe_join_used() {
        let src = "\
def serve(path):
    safe = safe_join(BASE_DIR, path)
    return open(safe).read()
";
        let ctx = make_ctx_with_source("src/views.py", src, Language::Python);
        let findings = PathNormalizationDetector.analyze(&ctx).await.unwrap();
        assert!(findings.is_empty(), "expected no findings, got: {findings:?}");
    }

    // 3. Rust fn with path param using fs::read → finding (no canonicalize)
    #[tokio::test]
    async fn detects_rust_missing_canonicalize() {
        let src = r#"
fn read_file(path: &Path) -> Vec<u8> {
    fs::read(path).unwrap()
}
"#;
        let ctx = make_ctx_with_source("src/reader.rs", src, Language::Rust);
        let findings = PathNormalizationDetector.analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 1, "expected 1 finding, got: {findings:?}");
        assert_eq!(findings[0].category, FindingCategory::PathTraversal);
    }

    // 4. File in tests/ directory → no finding
    #[tokio::test]
    async fn ignores_test_files() {
        let src = "\
def load(path):
    return open(path).read()
";
        let ctx = make_ctx_with_source("tests/test_load.py", src, Language::Python);
        let findings = PathNormalizationDetector.analyze(&ctx).await.unwrap();
        assert!(findings.is_empty(), "expected no findings for test file");
    }

    // 5. Python using os.path.normpath → no finding
    #[tokio::test]
    async fn no_finding_when_normpath_used() {
        let src = "\
def open_file(path):
    safe = os.path.normpath(path)
    return open(safe).read()
";
        let ctx = make_ctx_with_source("src/files.py", src, Language::Python);
        let findings = PathNormalizationDetector.analyze(&ctx).await.unwrap();
        assert!(findings.is_empty(), "expected no findings, got: {findings:?}");
    }

    // 6. JS function with path param, no normalization → finding
    #[tokio::test]
    async fn detects_js_missing_path_normalize() {
        let src = "\
function serveFile(path) {
    const data = fs.readFileSync(path);
    return data;
}
";
        let ctx =
            make_ctx_with_source("src/server.js", src, Language::JavaScript);
        let findings = PathNormalizationDetector.analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 1, "expected 1 finding, got: {findings:?}");
        assert_eq!(findings[0].category, FindingCategory::PathTraversal);
    }

    // 7. JS using path.resolve → no finding
    #[tokio::test]
    async fn no_finding_when_path_resolve_used() {
        let src = "\
function serveFile(path) {
    const safe = path.resolve(BASE, path);
    return fs.readFileSync(safe);
}
";
        let ctx =
            make_ctx_with_source("src/server.js", src, Language::JavaScript);
        let findings = PathNormalizationDetector.analyze(&ctx).await.unwrap();
        assert!(findings.is_empty(), "expected no findings, got: {findings:?}");
    }
}
