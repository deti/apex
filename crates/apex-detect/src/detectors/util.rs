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
        || s.contains("/test_")
        || s.contains("\\test_")
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
                || (trimmed.starts_with("mod tests") && trimmed.contains('{')))
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
    let mut in_string = false;
    let mut prev_backslash = false;
    for ch in line.chars() {
        if in_string {
            if ch == '"' && !prev_backslash {
                in_string = false;
                result.push('"');
            }
            prev_backslash = ch == '\\' && !prev_backslash;
        } else {
            if ch == '"' {
                in_string = true;
            }
            result.push(ch);
        }
    }
    result
}

/// Returns true if the trimmed line is a comment in the given language.
pub fn is_comment(trimmed: &str, lang: Language) -> bool {
    if trimmed.starts_with("//") || trimmed.starts_with("/*") || trimmed.starts_with('*') {
        return true;
    }
    if (lang == Language::Python || lang == Language::Ruby) && trimmed.starts_with('#') {
        return true;
    }
    false
}
