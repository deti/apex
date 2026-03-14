use crate::context::AnalysisContext;
use crate::finding::Evidence;
use apex_core::types::Language;
use std::path::Path;

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
                || ((trimmed == "mod tests {" || trimmed.starts_with("mod tests {") || trimmed == "mod tests{" || trimmed.starts_with("mod tests{")) && trimmed.contains('{')))
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
    if trimmed.starts_with("//") || trimmed.starts_with("/*") || trimmed.starts_with("* ") || trimmed == "*" {
        return true;
    }
    if (lang == Language::Python || lang == Language::Ruby) && trimmed.starts_with('#') {
        return true;
    }
    false
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
        assert_eq!(
            strip_string_literals("f('a', 'b')"),
            "f('', '')"
        );
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
}
