use apex_core::error::Result;
use apex_core::types::Language;
use async_trait::async_trait;
use uuid::Uuid;

use super::util::{find_loop_scopes, in_any_scope, is_comment};
use crate::context::AnalysisContext;
use crate::finding::{Finding, FindingCategory, Severity};
use crate::Detector;

pub struct RegexInLoopDetector;

// ---------------------------------------------------------------------------
// Patterns per language
// ---------------------------------------------------------------------------

/// Rust: `Regex::new(` inside a loop — the compiled regex should be cached
static RUST_REGEX_NEW: &str = "Regex::new(";

/// Python: `re.compile(` inside a loop
static PYTHON_RE_COMPILE: &str = "re.compile(";

/// JS: `new RegExp(` inside a loop
static JS_NEW_REGEXP: &str = "new RegExp(";

/// Cache/suppression indicators that show the regex is stored outside the loop.
/// These appear in the same line or nearby — if the line assigns to a LazyLock,
/// OnceLock, or a known cache pattern, we suppress.
static RUST_CACHE_INDICATORS: &[&str] = &["LazyLock", "OnceLock", "once_cell", "lazy_static"];

static PYTHON_CACHE_INDICATORS: &[&str] = &[
    "COMPILED_RE",
    "RE_CACHE",
    "lru_cache",
    "functools.cache",
    "_compiled",
];

static JS_CACHE_INDICATORS: &[&str] = &["const RE", "let RE", "var RE", "cache", "REGEX"];

fn is_suppressed(line: &str, cache_indicators: &[&str]) -> bool {
    cache_indicators.iter().any(|ind| line.contains(ind))
}

fn analyze_source(path: &std::path::Path, source: &str, lang: Language) -> Vec<Finding> {
    let lines: Vec<&str> = source.lines().collect();
    let loop_scopes = find_loop_scopes(source, lang);

    if loop_scopes.is_empty() {
        return Vec::new();
    }

    let (pattern, cache_indicators): (&str, &[&str]) = match lang {
        Language::Rust => (RUST_REGEX_NEW, RUST_CACHE_INDICATORS),
        Language::Python => (PYTHON_RE_COMPILE, PYTHON_CACHE_INDICATORS),
        Language::JavaScript => (JS_NEW_REGEXP, JS_CACHE_INDICATORS),
        _ => return Vec::new(),
    };

    let mut findings = Vec::new();

    for (line_idx, line) in lines.iter().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() || is_comment(trimmed, lang) {
            continue;
        }

        if !in_any_scope(&loop_scopes, line_idx) {
            continue;
        }

        if !line.contains(pattern) {
            continue;
        }

        // Suppression: line itself uses a cache pattern
        if is_suppressed(line, cache_indicators) {
            continue;
        }

        let line_1based = (line_idx + 1) as u32;
        findings.push(Finding {
            id: Uuid::new_v4(),
            detector: "regex-in-loop".into(),
            severity: Severity::Low,
            category: FindingCategory::SecuritySmell,
            file: path.to_path_buf(),
            line: Some(line_1based),
            title: "Regex compiled inside loop — repeated compilation overhead".into(),
            description: format!(
                "Regular expression compiled at line {} is inside a loop. \
                 Regex compilation is expensive; compiling the same pattern on each iteration \
                 wastes CPU and can cause O(n) compilation overhead.",
                line_1based
            ),
            evidence: vec![],
            covered: false,
            suggestion: match lang {
                Language::Rust => "Move `Regex::new(...)` outside the loop or use `LazyLock` / \
                     `OnceLock` for a static compiled regex."
                    .into(),
                Language::Python => {
                    "Call `re.compile(...)` once at module level or cache the result. \
                     Python's `re` module also caches the last few patterns, \
                     but explicit compilation is clearer and avoids eviction."
                        .into()
                }
                _ => "Create the RegExp once outside the loop and reuse it.".into(),
            },
            explanation: None,
            fix: None,
            cwe_ids: vec![400],
                    noisy: false,
        });
    }

    findings
}

#[async_trait]
impl Detector for RegexInLoopDetector {
    fn name(&self) -> &str {
        "regex-in-loop"
    }

    async fn analyze(&self, ctx: &AnalysisContext) -> Result<Vec<Finding>> {
        let mut findings = Vec::new();

        for (path, source) in &ctx.source_cache {
            let lang = match path.extension().and_then(|e| e.to_str()) {
                Some("rs") => Language::Rust,
                Some("py") => Language::Python,
                Some("js") | Some("jsx") => Language::JavaScript,
                Some("ts") | Some("tsx") => Language::JavaScript,
                _ => continue,
            };
            findings.extend(analyze_source(path, source, lang));
        }

        Ok(findings)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn detect_rust(source: &str) -> Vec<Finding> {
        analyze_source(&PathBuf::from("src/lib.rs"), source, Language::Rust)
    }

    fn detect_python(source: &str) -> Vec<Finding> {
        analyze_source(&PathBuf::from("src/app.py"), source, Language::Python)
    }

    fn detect_js(source: &str) -> Vec<Finding> {
        analyze_source(&PathBuf::from("src/app.js"), source, Language::JavaScript)
    }

    // ---- positive: Rust ----

    #[test]
    fn detects_regex_new_in_for_loop_rust() {
        let src = "\
fn process(items: &[&str]) {
    for item in items {
        let re = Regex::new(r\"\\d+\").unwrap();
        if re.is_match(item) { handle(item); }
    }
}
";
        let findings = detect_rust(src);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, Severity::Low);
        assert_eq!(findings[0].cwe_ids, vec![400]);
    }

    // ---- positive: Python ----

    #[test]
    fn detects_re_compile_in_loop_python() {
        let src = "\
def process(items):
    for item in items:
        pattern = re.compile(r'\\d+')
        if pattern.match(item):
            handle(item)
";
        let findings = detect_python(src);
        assert_eq!(findings.len(), 1);
    }

    // ---- positive: JS ----

    #[test]
    fn detects_new_regexp_in_loop_js() {
        let src = "\
function process(items) {
    for (const item of items) {
        const re = new RegExp('\\\\d+');
        if (re.test(item)) handle(item);
    }
}
";
        let findings = detect_js(src);
        assert_eq!(findings.len(), 1);
    }

    // ---- negative: outside loop ----

    #[test]
    fn no_finding_regex_outside_loop_rust() {
        let src = "\
fn process(items: &[&str]) {
    let re = Regex::new(r\"\\d+\").unwrap();
    for item in items {
        if re.is_match(item) { handle(item); }
    }
}
";
        let findings = detect_rust(src);
        assert_eq!(findings.len(), 0);
    }

    // ---- negative: LazyLock suppression ----

    #[test]
    fn suppressed_by_lazycell_rust() {
        let src = "\
static RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r\"\\d+\").unwrap());
fn process(items: &[&str]) {
    for item in items {
        if RE.is_match(item) { handle(item); }
    }
}
";
        let findings = detect_rust(src);
        // LazyLock line is outside the loop (line 0), so it's not flagged.
        // The loop body uses RE (not Regex::new), so no finding.
        assert_eq!(findings.len(), 0);
    }

    // ---- negative: no loop ----

    #[test]
    fn no_finding_no_loop_python() {
        let src = "\
def check(item):
    pattern = re.compile(r'\\d+')
    return pattern.match(item)
";
        let findings = detect_python(src);
        assert_eq!(findings.len(), 0);
    }

    // ---- edge: while loop ----

    #[test]
    fn detects_in_while_loop_js() {
        let src = "\
function process() {
    while (hasMore()) {
        const re = new RegExp('abc');
        check(re);
    }
}
";
        let findings = detect_js(src);
        assert_eq!(findings.len(), 1);
    }
}
