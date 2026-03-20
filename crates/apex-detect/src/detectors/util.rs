use crate::context::AnalysisContext;
use crate::finding::Evidence;
use apex_core::types::Language;
use regex::Regex;
use std::path::Path;
use std::sync::LazyLock;

/// Returns true if the file path looks like a test file.
pub fn is_test_file(path: &Path) -> bool {
    let s = path.to_string_lossy();
    // tests/ and test/ directories (Rust uses tests/, JS uses test/)
    s.starts_with("tests/")
        || s.starts_with("tests\\")
        || s.contains("/tests/")
        || s.contains("\\tests\\")
        || s.starts_with("test/")
        || s.starts_with("test\\")
        || s.contains("/test/")
        || s.contains("\\test\\")
        // __tests__/ (Jest convention)
        || s.contains("__tests__/")
        || s.contains("__tests__\\")
        // benches/
        || s.starts_with("benches/")
        || s.starts_with("benches\\")
        || s.contains("/benches/")
        || s.contains("\\benches\\")
        // spec/ (Ruby/JS convention)
        || s.starts_with("spec/")
        || s.contains("/spec/")
        // file name patterns
        || s.ends_with("_test.rs")
        || s.ends_with("_test.py")
        || s.ends_with("_tests.rs")
        || s.ends_with(".test.js")
        || s.ends_with(".test.ts")
        || s.ends_with(".test.tsx")
        || s.ends_with(".spec.js")
        || s.ends_with(".spec.ts")
        || s.ends_with(".spec.tsx")
        || s.ends_with("_test.go")
        || {
            // Match test_ prefix only in the filename, not in directory components
            let fname = path.file_name().and_then(|f| f.to_str()).unwrap_or("");
            fname.starts_with("test_")
        }
        || file_stem_is_test_helper(path)
}

/// Check if the file stem indicates a test utility/helper file.
fn file_stem_is_test_helper(path: &Path) -> bool {
    let stem = match path.file_stem().and_then(|s| s.to_str()) {
        Some(s) => s,
        None => return false,
    };
    stem == "testutil"
        || stem == "testutils"
        || stem == "test_util"
        || stem == "test_utils"
        || stem == "test_helpers"
        || stem == "test_helper"
        || stem == "testing"
        || stem == "conftest"
}

/// Build evidence from reachability data (coverage index) for a finding.
/// Returns an empty vec if no coverage data is available.
pub fn reachability_evidence(_ctx: &AnalysisContext, _path: &Path, _line: u32) -> Vec<Evidence> {
    // TODO: once AnalysisContext carries a BranchIndex, check if the line
    // has coverage and emit Evidence::CoverageGap for uncovered findings.
    Vec::new()
}

/// Returns true if we're inside a `#[cfg(test)]` block.
/// Tracks brace depth after seeing `#[cfg(test)]` or `mod tests`.
pub fn in_test_block(source: &str, target_line: usize) -> bool {
    let mut in_cfg_test = false;
    let mut brace_depth: i32 = 0;
    let mut cfg_test_start_depth: i32 = 0;

    for (i, line) in source.lines().enumerate() {
        if i >= target_line {
            break;
        }
        let trimmed = line.trim();

        // Detect start of test module
        if !in_cfg_test
            && (trimmed.contains("#[cfg(test)]")
                || ((trimmed == "mod tests {"
                    || trimmed.starts_with("mod tests {")
                    || trimmed == "mod tests{"
                    || trimmed.starts_with("mod tests{"))
                    && trimmed.contains('{')))
        {
            in_cfg_test = true;
            cfg_test_start_depth = brace_depth;
        }

        // Track brace depth
        for ch in line.chars() {
            match ch {
                '{' => brace_depth += 1,
                '}' => {
                    brace_depth -= 1;
                    if in_cfg_test && brace_depth <= cfg_test_start_depth {
                        in_cfg_test = false;
                    }
                }
                _ => {}
            }
        }
    }

    in_cfg_test
}

/// Strip content inside string literals so patterns inside quotes are ignored.
/// Keeps the quote characters but removes everything between them.
pub fn strip_string_literals(line: &str) -> String {
    let mut result = String::new();
    let mut in_string: Option<char> = None;
    let mut prev_backslash = false;
    for ch in line.chars() {
        if let Some(quote_char) = in_string {
            if ch == quote_char && !prev_backslash {
                in_string = None;
                result.push(quote_char);
            }
            prev_backslash = ch == '\\' && !prev_backslash;
        } else {
            if ch == '"' || ch == '\'' || ch == '`' {
                in_string = Some(ch);
            }
            result.push(ch);
        }
    }
    result
}

/// Returns true if the trimmed line is a comment in the given language.
pub fn is_comment(trimmed: &str, lang: Language) -> bool {
    if trimmed.starts_with("//")
        || trimmed.starts_with("/*")
        || trimmed.starts_with("* ")
        || trimmed == "*"
    {
        return true;
    }
    if (lang == Language::Python || lang == Language::Ruby) && trimmed.starts_with('#') {
        return true;
    }
    false
}

// ---------------------------------------------------------------------------
// Scope tracking
// ---------------------------------------------------------------------------

/// A contiguous source scope (start_line..=end_line, 0-based, inclusive).
///
/// `start_line` is the first line of the scope body (the line after the opener).
/// `end_line` is the last line of the scope body (the line before the closer, or
/// the same as start_line for one-liner bodies).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Scope {
    pub start_line: usize,
    pub end_line: usize,
}

/// Returns true if `line_idx` (0-based) falls inside any scope in the slice.
pub fn in_any_scope(scopes: &[Scope], line_idx: usize) -> bool {
    scopes
        .iter()
        .any(|s| line_idx >= s.start_line && line_idx <= s.end_line)
}

/// Determine whether `lang` uses indentation-based scope tracking (Python, Ruby)
/// rather than brace-based tracking.
fn is_indent_tracked(lang: Language) -> bool {
    matches!(lang, Language::Python | Language::Ruby)
}

/// Count the leading spaces in a line, treating one tab as 4 spaces.
fn indent_level(line: &str) -> usize {
    let mut level = 0usize;
    for ch in line.chars() {
        match ch {
            ' ' => level += 1,
            '\t' => level += 4,
            _ => break,
        }
    }
    level
}

/// Find all scopes opened by lines that match `scope_opener`.
///
/// For brace-tracked languages (Rust, JS, Java, Go, C/C++, etc.) the scope body
/// is bounded by matching `{` / `}` braces.  For indent-tracked languages
/// (Python, Ruby) the scope body is bounded by the return to the opener's
/// indentation level.
///
/// Returns a [`Vec<Scope>`] where each entry's `start_line` / `end_line` are
/// 0-based line indices *inclusive* into the body of the matched scope.
pub fn find_scopes(source: &str, lang: Language, scope_opener: &Regex) -> Vec<Scope> {
    let lines: Vec<&str> = source.lines().collect();
    let mut scopes = Vec::new();

    if is_indent_tracked(lang) {
        find_scopes_indent(&lines, scope_opener, &mut scopes);
    } else {
        find_scopes_brace(&lines, scope_opener, &mut scopes);
    }

    scopes
}

/// Scan `lines[start_line..]` character by character for matching braces.
///
/// Returns the line on which the opening `{` was found, the line on which the
/// depth returned to 0, and the final depth at the end of the scan (in case the
/// closing brace was not found within the slice).
///
/// `depth_in` is the brace depth before starting the scan.
fn scan_braces(
    lines: &[&str],
    start_line: usize,
    depth_in: i32,
) -> (i32, Option<usize>, Option<usize>) {
    let mut depth = depth_in;
    let mut open_line: Option<usize> = None;
    let mut close_line: Option<usize> = None;

    'outer: for (offset, line) in lines[start_line..].iter().enumerate() {
        let lnum = start_line + offset;
        let mut in_string: Option<char> = None;
        let mut prev_backslash = false;
        let mut chars = line.chars().peekable();

        while let Some(ch) = chars.next() {
            if let Some(quote) = in_string {
                if ch == quote && !prev_backslash {
                    in_string = None;
                }
                prev_backslash = ch == '\\' && !prev_backslash;
                continue;
            }
            prev_backslash = false;

            match ch {
                '"' | '\'' | '`' => {
                    in_string = Some(ch);
                }
                '/' if chars.peek() == Some(&'/') => break, // line comment
                '#' => break,                               // Python/Ruby line comment
                '{' => {
                    if depth == 0 && open_line.is_none() {
                        open_line = Some(lnum);
                    }
                    depth += 1;
                }
                '}' => {
                    depth -= 1;
                    if depth <= 0 && open_line.is_some() && close_line.is_none() {
                        close_line = Some(lnum);
                        break 'outer;
                    }
                }
                _ => {}
            }
        }
    }

    (depth, open_line, close_line)
}

/// Returns true if there is non-whitespace content between the first `{` and
/// the matching `}` on a single line (used to distinguish one-liners from empty bodies).
fn has_body_content_inline(line: &str) -> bool {
    // Find the first `{` then check whether anything non-whitespace precedes `}`.
    if let Some(open_pos) = line.find('{') {
        let after_open = &line[open_pos + 1..];
        // Find the matching `}` — for simple cases the next `}` is fine.
        if let Some(close_pos) = after_open.find('}') {
            let interior = &after_open[..close_pos];
            return interior.chars().any(|c| !c.is_whitespace());
        }
    }
    false
}

fn find_scopes_brace(lines: &[&str], scope_opener: &Regex, scopes: &mut Vec<Scope>) {
    let n = lines.len();
    let mut i = 0usize;

    while i < n {
        if scope_opener.is_match(lines[i]) {
            // Scan from the opener line forward to find the matching close brace.
            let (_final_depth, open_line, close_line) = scan_braces(lines, i, 0);

            match (open_line, close_line) {
                (Some(open), Some(close)) if open == close => {
                    // Same line: could be one-liner `{ body }` or empty `{}`.
                    if has_body_content_inline(lines[open]) {
                        // One-liner: the opener line itself is the scope body.
                        scopes.push(Scope {
                            start_line: open,
                            end_line: close,
                        });
                    }
                    // Empty `{}` — no interior, don't push a scope.
                    i = close + 1;
                }
                (Some(open), Some(close)) if open < close.saturating_sub(1) => {
                    // Multi-line: body is lines strictly between opener and closer.
                    scopes.push(Scope {
                        start_line: open + 1,
                        end_line: close - 1,
                    });
                    i = close + 1;
                }
                (Some(_open), Some(close)) => {
                    // Empty body on consecutive lines: `fn foo() {\n}` — no interior.
                    i = close + 1;
                }
                _ => {
                    // No closing brace found — skip opener line.
                    i += 1;
                }
            }
        } else {
            i += 1;
        }
    }
}

fn find_scopes_indent(lines: &[&str], scope_opener: &Regex, scopes: &mut Vec<Scope>) {
    let n = lines.len();
    let mut i = 0usize;

    while i < n {
        let line = lines[i];
        let trimmed = line.trim();
        if trimmed.is_empty() {
            i += 1;
            continue;
        }

        if scope_opener.is_match(line) {
            let opener_indent = indent_level(line);

            // Find the first non-empty line of the body.
            let mut j = i + 1;
            while j < n && lines[j].trim().is_empty() {
                j += 1;
            }

            if j >= n {
                i += 1;
                continue;
            }

            let body_start = j;
            let mut body_end = j;

            // Advance until we find a non-empty line whose indent <= opener_indent.
            j += 1;
            while j < n {
                let l = lines[j];
                let lt = l.trim();
                if !lt.is_empty() {
                    if indent_level(l) <= opener_indent {
                        break;
                    }
                    body_end = j;
                }
                j += 1;
            }

            scopes.push(Scope {
                start_line: body_start,
                end_line: body_end,
            });

            // Don't skip past j — nested scopes may start inside the body.
            i += 1;
        } else {
            i += 1;
        }
    }
}

// ---------------------------------------------------------------------------
// Pre-compiled scope opener patterns
// ---------------------------------------------------------------------------

/// Async function openers across brace-tracked languages.
static ASYNC_FN_BRACE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?x)
        async \s+ fn \s+         # Rust: async fn name
        | async \s+ move \s* \{  # Rust: async move {
        | async \s+ function \s+ # JS/TS: async function name
        | async \s* \(           # JS/TS: async (...)  arrow
        | async \s+ \w           # JS/TS: async keyword before param
        ",
    )
    .expect("async fn brace regex must compile")
});

/// Async function openers for indent-tracked languages.
static ASYNC_FN_INDENT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"async\s+def\s+").expect("async def regex must compile"));

/// Loop openers for brace-tracked languages.
static LOOP_BRACE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?x)
        \b for \s*   [(\[]?   # for (...) or for [ in Go/Rust
        | \b while \s* \(
        | \b loop \s* \{      # Rust: loop {
        | \b do \s* \{        # C/Java: do { ... } while
    ",
    )
    .expect("loop brace regex must compile")
});

/// Loop openers for indent-tracked languages.
static LOOP_INDENT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^\s*(?:for|while)\s+").expect("loop indent regex must compile"));

/// Error-handling openers for brace-tracked languages.
static EXCEPT_BRACE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?x)
        \b catch \s* \(       # JS/Java/C#/Kotlin: catch (e)
        | if \s+ let \s+ Err  # Rust: if let Err(
        | Err\s*\(.*\)\s*=>   # Rust match arm: Err(e) =>
        | if \s+ err \s*!=    # Go: if err != nil
        | if \s+ err \s*!= \s* nil
        ",
    )
    .expect("except brace regex must compile")
});

/// Error-handling openers for indent-tracked languages.
static EXCEPT_INDENT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^\s*except(?:\s|:)").expect("except indent regex must compile"));

// ---------------------------------------------------------------------------
// Convenience helpers
// ---------------------------------------------------------------------------

/// Find all async function scopes in `source`.
pub fn find_async_fn_scopes(source: &str, lang: Language) -> Vec<Scope> {
    if is_indent_tracked(lang) {
        find_scopes(source, lang, &ASYNC_FN_INDENT)
    } else {
        find_scopes(source, lang, &ASYNC_FN_BRACE)
    }
}

/// Find all loop scopes in `source`.
pub fn find_loop_scopes(source: &str, lang: Language) -> Vec<Scope> {
    if is_indent_tracked(lang) {
        find_scopes(source, lang, &LOOP_INDENT)
    } else {
        find_scopes(source, lang, &LOOP_BRACE)
    }
}

/// Find all error-handling scopes in `source`.
pub fn find_except_scopes(source: &str, lang: Language) -> Vec<Scope> {
    if is_indent_tracked(lang) {
        find_scopes(source, lang, &EXCEPT_INDENT)
    } else {
        find_scopes(source, lang, &EXCEPT_BRACE)
    }
}

/// Returns true if `line_idx` (0-based) is inside an async function body.
pub fn in_async_fn(source: &str, lang: Language, line_idx: usize) -> bool {
    in_any_scope(&find_async_fn_scopes(source, lang), line_idx)
}

/// Returns true if `line_idx` (0-based) is inside a loop body.
pub fn in_loop_body(source: &str, lang: Language, line_idx: usize) -> bool {
    in_any_scope(&find_loop_scopes(source, lang), line_idx)
}

/// Returns true if `line_idx` (0-based) is inside an error-handling body.
pub fn in_except_body(source: &str, lang: Language, line_idx: usize) -> bool {
    in_any_scope(&find_except_scopes(source, lang), line_idx)
}

const ENV_VAR_MARKERS: &[&str] = &[
    "env(",
    "ENV[",
    "os.environ",
    "process.env",
    "std::env",
    "getenv(",
];

/// Returns true if the line references an environment variable.
pub(crate) fn references_env_var(line: &str) -> bool {
    ENV_VAR_MARKERS.iter().any(|m| line.contains(m))
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---- is_test_file ----

    #[test]
    fn test_file_in_tests_dir() {
        assert!(is_test_file(std::path::Path::new("tests/test_foo.rs")));
    }

    #[test]
    fn test_file_in_test_dir() {
        assert!(is_test_file(std::path::Path::new("test/foo.js")));
    }

    #[test]
    fn test_file_nested_tests() {
        assert!(is_test_file(std::path::Path::new("src/tests/bar.rs")));
    }

    #[test]
    fn test_file_jest_convention() {
        assert!(is_test_file(std::path::Path::new("src/__tests__/foo.js")));
    }

    #[test]
    fn test_file_benches() {
        assert!(is_test_file(std::path::Path::new("benches/bench_sort.rs")));
        assert!(is_test_file(std::path::Path::new(
            "crates/foo/benches/bar.rs"
        )));
    }

    #[test]
    fn test_file_spec_dir() {
        assert!(is_test_file(std::path::Path::new(
            "spec/models/user_spec.rb"
        )));
        assert!(is_test_file(std::path::Path::new("app/spec/foo.rb")));
    }

    #[test]
    fn test_file_suffix_patterns() {
        assert!(is_test_file(std::path::Path::new("src/foo_test.rs")));
        assert!(is_test_file(std::path::Path::new("src/foo_test.py")));
        assert!(is_test_file(std::path::Path::new("src/foo_tests.rs")));
        assert!(is_test_file(std::path::Path::new("src/foo.test.js")));
        assert!(is_test_file(std::path::Path::new("src/foo.test.ts")));
        assert!(is_test_file(std::path::Path::new("src/foo.test.tsx")));
        assert!(is_test_file(std::path::Path::new("src/foo.spec.js")));
        assert!(is_test_file(std::path::Path::new("src/foo.spec.ts")));
        assert!(is_test_file(std::path::Path::new("src/foo.spec.tsx")));
    }

    #[test]
    fn test_file_test_prefix() {
        assert!(is_test_file(std::path::Path::new("src/test_utils.py")));
        assert!(is_test_file(std::path::Path::new("lib/test_helper.rb")));
    }

    #[test]
    fn test_file_helper_stems() {
        assert!(is_test_file(std::path::Path::new("testutil.rs")));
        assert!(is_test_file(std::path::Path::new("testutils.py")));
        assert!(is_test_file(std::path::Path::new("test_util.rs")));
        assert!(is_test_file(std::path::Path::new("test_utils.rs")));
        assert!(is_test_file(std::path::Path::new("test_helpers.rb")));
        assert!(is_test_file(std::path::Path::new("test_helper.rb")));
        assert!(is_test_file(std::path::Path::new("testing.py")));
        assert!(is_test_file(std::path::Path::new("conftest.py")));
    }

    #[test]
    fn not_test_file_regular_src() {
        assert!(!is_test_file(std::path::Path::new("src/main.rs")));
        assert!(!is_test_file(std::path::Path::new("src/lib.rs")));
        assert!(!is_test_file(std::path::Path::new("src/utils.py")));
    }

    // ---- in_test_block ----

    #[test]
    fn in_test_block_cfg_test_mod() {
        let src = "fn real() {}\n\n#[cfg(test)]\nmod tests {\n    fn inside() {}\n}\n";
        assert!(!in_test_block(src, 0)); // real()
        assert!(!in_test_block(src, 2)); // #[cfg(test)]
        assert!(in_test_block(src, 4)); // fn inside()
    }

    #[test]
    fn in_test_block_mod_tests_inline() {
        let src = "fn real() {}\nmod tests {\n    fn t() {}\n}\n";
        assert!(!in_test_block(src, 0));
        assert!(in_test_block(src, 2)); // fn t()
    }

    #[test]
    fn in_test_block_empty_source() {
        assert!(!in_test_block("", 0));
    }

    #[test]
    fn in_test_block_no_test_mod() {
        let src = "fn main() {\n    println!(\"hello\");\n}\n";
        assert!(!in_test_block(src, 1));
    }

    #[test]
    fn in_test_block_after_closing_brace() {
        let src = "#[cfg(test)]\nmod tests {\n    fn t() {}\n}\nfn after() {}\n";
        assert!(in_test_block(src, 2)); // fn t()
        assert!(!in_test_block(src, 4)); // fn after()
    }

    // ---- strip_string_literals ----

    #[test]
    fn strip_empty() {
        assert_eq!(strip_string_literals(""), "");
    }

    #[test]
    fn strip_no_strings() {
        assert_eq!(strip_string_literals("let x = 42;"), "let x = 42;");
    }

    #[test]
    fn strip_simple_string() {
        assert_eq!(
            strip_string_literals(r#"let x = "hello";"#),
            r#"let x = "";"#
        );
    }

    #[test]
    fn strip_escaped_quote() {
        assert_eq!(
            strip_string_literals(r#"let x = "say \"hi\"";"#),
            r#"let x = "";"#
        );
    }

    #[test]
    fn strip_multiple_strings() {
        assert_eq!(strip_string_literals(r#"f("a", "b")"#), r#"f("", "")"#);
    }

    // ---- is_comment ----

    #[test]
    fn comment_rust_line() {
        assert!(is_comment("// comment", Language::Rust));
    }

    #[test]
    fn comment_rust_block() {
        assert!(is_comment("/* block */", Language::Rust));
    }

    #[test]
    fn comment_star_continuation() {
        assert!(is_comment("* continued", Language::Rust));
    }

    #[test]
    fn comment_python_hash() {
        assert!(is_comment("# python comment", Language::Python));
    }

    #[test]
    fn comment_ruby_hash() {
        assert!(is_comment("# ruby comment", Language::Ruby));
    }

    #[test]
    fn comment_hash_not_in_rust() {
        // In Rust, # is an attribute prefix, not a comment
        assert!(!is_comment("#[derive(Debug)]", Language::Rust));
    }

    #[test]
    fn comment_hash_not_in_js() {
        assert!(!is_comment("# not a js comment", Language::JavaScript));
    }

    #[test]
    fn not_comment_code() {
        assert!(!is_comment("let x = 1;", Language::Rust));
        assert!(!is_comment("x = 1", Language::Python));
    }

    // ---- Windows path separators ----

    #[test]
    fn test_file_windows_paths() {
        assert!(is_test_file(std::path::Path::new("tests\\foo.rs")));
        assert!(is_test_file(std::path::Path::new("test\\bar.js")));
        assert!(is_test_file(std::path::Path::new("src\\tests\\baz.rs")));
        assert!(is_test_file(std::path::Path::new("src\\__tests__\\qux.js")));
        assert!(is_test_file(std::path::Path::new("benches\\bench.rs")));
    }

    // ---- Bug regression tests ----

    #[test]
    fn bug_is_comment_star_deref_not_comment() {
        // Bug 1: `*ptr`, `**kwargs`, `*x = 5` should not be comments
        assert!(!is_comment("*ptr", Language::Rust));
        assert!(!is_comment("**kwargs", Language::Python));
        assert!(!is_comment("*x = 5;", Language::Rust));
        // Block comment continuation with space is still a comment
        assert!(is_comment("* This is a comment", Language::Rust));
        // Lone `*` is still a comment (block comment continuation)
        assert!(is_comment("*", Language::Rust));
    }

    #[test]
    fn bug_is_test_file_test_data_dir_not_test() {
        // Bug 2: test_data/ directories should not be flagged as test files
        assert!(!is_test_file(std::path::Path::new("test_data/config.json")));
        assert!(!is_test_file(std::path::Path::new(
            "src/test_data/fixture.py"
        )));
        assert!(!is_test_file(std::path::Path::new(
            "test_fixtures/data.json"
        )));
        assert!(!is_test_file(std::path::Path::new(
            "test_resources/input.txt"
        )));
        // But actual test files with test_ prefix in filename should still match
        assert!(is_test_file(std::path::Path::new("src/test_utils.py")));
        assert!(is_test_file(std::path::Path::new("lib/test_helper.rb")));
    }

    #[test]
    fn bug_in_test_block_mod_tests_integration_not_test() {
        // Bug 3: `mod tests_integration` should not match as test block
        let src = "fn real() {}\nmod tests_integration {\n    fn t() {}\n}\n";
        assert!(!in_test_block(src, 2)); // fn t() inside tests_integration
    }

    #[test]
    fn bug_in_test_block_mod_tests_exact() {
        // Bug 3: exact `mod tests {` should still match
        let src = "mod tests {\n    fn t() {}\n}\n";
        assert!(in_test_block(src, 1));
        let src2 = "mod tests{\n    fn t() {}\n}\n";
        assert!(in_test_block(src2, 1));
    }

    #[test]
    fn bug_strip_single_quotes() {
        // Bug 4: single-quoted strings should be stripped
        assert_eq!(strip_string_literals("let x = 'hello';"), "let x = '';");
        assert_eq!(strip_string_literals("f('a', 'b')"), "f('', '')");
    }

    #[test]
    fn bug_strip_backtick_template_literals() {
        // Bug 5: JS template literals should be stripped
        assert_eq!(
            strip_string_literals("let x = `hello ${name}`;"),
            "let x = ``;"
        );
    }

    #[test]
    fn bug_go_test_file_pattern() {
        // Bug 6: Go test files use _test.go suffix
        assert!(is_test_file(std::path::Path::new("pkg/handler_test.go")));
        assert!(is_test_file(std::path::Path::new("main_test.go")));
        assert!(!is_test_file(std::path::Path::new("handler.go")));
    }

    // ---- find_scopes / in_any_scope — brace-tracked ----

    #[test]
    fn find_scopes_rust_async_fn_basic() {
        let src = "\
async fn handle() {
    let x = 1;
    do_work(x).await;
}
fn other() {}
";
        let scopes = find_async_fn_scopes(src, Language::Rust);
        assert_eq!(scopes.len(), 1);
        // body is lines 1-2 (0-based): "    let x = 1;" and "    do_work(x).await;"
        assert_eq!(scopes[0].start_line, 1);
        assert_eq!(scopes[0].end_line, 2);
    }

    #[test]
    fn find_scopes_rust_nested_for_inside_async() {
        let src = "\
async fn process() {
    for item in items {
        handle(item);
    }
}
";
        let async_scopes = find_async_fn_scopes(src, Language::Rust);
        let loop_scopes = find_loop_scopes(src, Language::Rust);
        // async fn body: lines 1-3
        assert_eq!(async_scopes.len(), 1);
        assert!(async_scopes[0].start_line <= 1);
        assert!(async_scopes[0].end_line >= 3);
        // for loop body: line 2
        assert_eq!(loop_scopes.len(), 1);
        assert_eq!(loop_scopes[0].start_line, 2);
        assert_eq!(loop_scopes[0].end_line, 2);
    }

    #[test]
    fn find_scopes_js_async_function() {
        let src = "\
async function fetchData(url) {
    const res = await fetch(url);
    return res.json();
}
";
        let scopes = find_async_fn_scopes(src, Language::JavaScript);
        assert_eq!(scopes.len(), 1);
        assert_eq!(scopes[0].start_line, 1);
        assert_eq!(scopes[0].end_line, 2);
    }

    #[test]
    fn find_scopes_one_liner_brace() {
        let src = "async fn noop() { do_work(); }\n";
        let scopes = find_async_fn_scopes(src, Language::Rust);
        // One-liner: the scope body collapses to the single line.
        assert_eq!(scopes.len(), 1);
        assert_eq!(scopes[0].start_line, scopes[0].end_line);
    }

    #[test]
    fn find_scopes_empty_body_brace() {
        let src = "async fn noop() {}\nfn other() {}\n";
        let scopes = find_async_fn_scopes(src, Language::Rust);
        // Empty body — no interior lines, so no scope recorded.
        assert_eq!(scopes.len(), 0);
    }

    // ---- find_scopes — indent-tracked (Python) ----

    #[test]
    fn find_scopes_python_async_def_basic() {
        let src = "\
async def handler():
    x = 1
    await do_work(x)

def other():
    pass
";
        let scopes = find_async_fn_scopes(src, Language::Python);
        assert_eq!(scopes.len(), 1);
        // body lines 1-2 (0-based): "    x = 1" and "    await do_work(x)"
        assert_eq!(scopes[0].start_line, 1);
        assert_eq!(scopes[0].end_line, 2);
    }

    #[test]
    fn find_scopes_python_nested_for_indent() {
        let src = "\
async def process():
    for item in items:
        handle(item)

def other():
    pass
";
        let async_scopes = find_async_fn_scopes(src, Language::Python);
        let loop_scopes = find_loop_scopes(src, Language::Python);
        assert_eq!(async_scopes.len(), 1);
        // for loop body
        assert_eq!(loop_scopes.len(), 1);
        assert_eq!(loop_scopes[0].start_line, 2); // "        handle(item)"
        assert_eq!(loop_scopes[0].end_line, 2);
    }

    #[test]
    fn find_scopes_python_except_pass() {
        let src = "\
try:
    risky()
except ValueError:
    pass
";
        let scopes = find_except_scopes(src, Language::Python);
        assert_eq!(scopes.len(), 1);
        assert_eq!(scopes[0].start_line, 3); // "    pass"
        assert_eq!(scopes[0].end_line, 3);
    }

    #[test]
    fn find_scopes_python_except_as() {
        let src = "\
try:
    risky()
except Exception as e:
    log(e)
    raise
";
        let scopes = find_except_scopes(src, Language::Python);
        assert_eq!(scopes.len(), 1);
        assert_eq!(scopes[0].start_line, 3); // "    log(e)"
        assert_eq!(scopes[0].end_line, 4); // "    raise"
    }

    // ---- in_async_fn / in_loop_body / in_except_body ----

    #[test]
    fn in_async_fn_line_inside_returns_true() {
        let src = "\
async fn handle() {
    let x = work().await;
}
";
        // Line 1 (0-based) is "    let x = work().await;"
        assert!(in_async_fn(src, Language::Rust, 1));
    }

    #[test]
    fn in_async_fn_line_outside_returns_false() {
        let src = "\
async fn handle() {
    let x = work().await;
}
fn plain() {
    plain_work();
}
";
        // Line 4 (0-based) is "    plain_work();"
        assert!(!in_async_fn(src, Language::Rust, 4));
    }

    #[test]
    fn in_loop_body_nested_inside_async_fn() {
        let src = "\
async fn process() {
    for item in items {
        handle(item);
    }
}
";
        // Line 2 is "        handle(item);"
        assert!(in_async_fn(src, Language::Rust, 2));
        assert!(in_loop_body(src, Language::Rust, 2));
    }

    #[test]
    fn in_except_body_python_reraise() {
        let src = "\
try:
    risky()
except Exception as e:
    log(e)
    raise
x = 1
";
        // Line 3 is "    log(e)", line 4 is "    raise"
        assert!(in_except_body(src, Language::Python, 3));
        assert!(in_except_body(src, Language::Python, 4));
        // Line 5 "x = 1" is outside
        assert!(!in_except_body(src, Language::Python, 5));
    }

    #[test]
    fn in_any_scope_empty_vec_always_false() {
        assert!(!in_any_scope(&[], 0));
        assert!(!in_any_scope(&[], 100));
    }

    #[test]
    fn in_any_scope_boundary_inclusive() {
        let scopes = vec![Scope {
            start_line: 3,
            end_line: 7,
        }];
        assert!(!in_any_scope(&scopes, 2));
        assert!(in_any_scope(&scopes, 3));
        assert!(in_any_scope(&scopes, 5));
        assert!(in_any_scope(&scopes, 7));
        assert!(!in_any_scope(&scopes, 8));
    }
}
