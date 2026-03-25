use apex_core::types::Language;
use regex::Regex;
use std::path::Path;
use std::sync::OnceLock;

/// A regex pattern extracted from source code.
#[derive(Debug, Clone)]
pub struct ExtractedRegex {
    pub pattern: String,
    pub file: std::path::PathBuf,
    pub line: u32,
    pub language: Language,
    pub flags: Vec<String>,
}

// ---------------------------------------------------------------------------
// Per-language compiled patterns (lazily initialised)
// ---------------------------------------------------------------------------

struct PythonPatterns {
    // re.<func>(r"..." or "..." or r'...' or '...')
    compile: Regex,
}

struct JsPatterns {
    // /pattern/flags  — only after = ( , return ; to avoid division
    literal: Regex,
    // new RegExp("..." or '...') or RegExp("..." or '...')
    constructor: Regex,
}

struct RustPatterns {
    // Regex::new(r"..." or "...") or RegexBuilder::new("...")
    new: Regex,
}

struct GoPatterns {
    // regexp.Compile("...") or regexp.MustCompile("...")
    compile: Regex,
}

struct JavaPatterns {
    // Pattern.compile("...")
    compile: Regex,
}

struct RubyPatterns {
    // /pattern/flags  — literal (Ruby context is unambiguous)
    literal: Regex,
    // Regexp.new("..." or '...')
    constructor: Regex,
}

fn python_patterns() -> &'static PythonPatterns {
    static CELL: OnceLock<PythonPatterns> = OnceLock::new();
    CELL.get_or_init(|| {
        // Matches re.<func>(  followed by r"...", "...", r'...', '...'
        // Captured groups: 1 = double-quote content, 2 = single-quote content
        let compile = Regex::new(
            r#"re\.(?:compile|match|search|findall|sub|fullmatch|finditer|split|subn)\s*\(\s*(?:r"([^"\\]*(?:\\.[^"\\]*)*)"|"([^"\\]*(?:\\.[^"\\]*)*)"|r'([^'\\]*(?:\\.[^'\\]*)*)'|'([^'\\]*(?:\\.[^'\\]*)*)')"#,
        )
        .expect("python compile regex");
        PythonPatterns { compile }
    })
}

fn js_patterns() -> &'static JsPatterns {
    static CELL: OnceLock<JsPatterns> = OnceLock::new();
    CELL.get_or_init(|| {
        // Literal regex: preceded by one of = ( , ; return ! &| ? :
        // Capture group 1 = pattern, group 2 = flags (may be empty)
        let literal = Regex::new(r#"(?:^|[=\(,;!&|?:\[])[\t ]*/((?:[^/\\\n]|\\.)+)/([gimsuy]*)"#)
            .expect("js literal regex");
        // new RegExp("...") / new RegExp('...') / RegExp("...") / RegExp('...')
        let constructor = Regex::new(
            r#"(?:new\s+)?RegExp\s*\(\s*(?:"([^"\\]*(?:\\.[^"\\]*)*)"|'([^'\\]*(?:\\.[^'\\]*)*)')"#,
        )
        .expect("js constructor regex");
        JsPatterns {
            literal,
            constructor,
        }
    })
}

fn rust_patterns() -> &'static RustPatterns {
    static CELL: OnceLock<RustPatterns> = OnceLock::new();
    CELL.get_or_init(|| {
        let new = Regex::new(
            r#"Regex(?:Builder)?::new\s*\(\s*(?:r"([^"\\]*(?:\\.[^"\\]*)*)"|"([^"\\]*(?:\\.[^"\\]*)*)")"#,
        )
        .expect("rust regex");
        RustPatterns { new }
    })
}

fn go_patterns() -> &'static GoPatterns {
    static CELL: OnceLock<GoPatterns> = OnceLock::new();
    CELL.get_or_init(|| {
        let compile =
            Regex::new(r#"regexp\.(?:Compile|MustCompile)\s*\(\s*"([^"\\]*(?:\\.[^"\\]*)*)""#)
                .expect("go regex");
        GoPatterns { compile }
    })
}

fn java_patterns() -> &'static JavaPatterns {
    static CELL: OnceLock<JavaPatterns> = OnceLock::new();
    CELL.get_or_init(|| {
        let compile = Regex::new(r#"Pattern\.compile\s*\(\s*"([^"\\]*(?:\\.[^"\\]*)*)""#)
            .expect("java regex");
        JavaPatterns { compile }
    })
}

fn ruby_patterns() -> &'static RubyPatterns {
    static CELL: OnceLock<RubyPatterns> = OnceLock::new();
    CELL.get_or_init(|| {
        // Ruby regex literals can appear at the start of a line or after = ( , ; !
        let literal = Regex::new(r#"(?:^|[=\(,;!\[])[\t ]*/((?:[^/\\\n]|\\.)+)/([imxouesn]*)"#)
            .expect("ruby literal regex");
        let constructor = Regex::new(
            r#"Regexp\.new\s*\(\s*(?:"([^"\\]*(?:\\.[^"\\]*)*)"|'([^'\\]*(?:\\.[^'\\]*)*)')"#,
        )
        .expect("ruby constructor regex");
        RubyPatterns {
            literal,
            constructor,
        }
    })
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Returns true if the trimmed line looks like a comment in the given language.
fn is_comment_line(line: &str, lang: Language) -> bool {
    let t = line.trim_start();
    match lang {
        Language::Python | Language::Ruby => t.starts_with('#'),
        Language::JavaScript
        | Language::Java
        | Language::Rust
        | Language::Go
        | Language::Cpp
        | Language::C
        | Language::Kotlin
        | Language::Swift
        | Language::CSharp => t.starts_with("//") || t.starts_with("/*") || t.starts_with('*'),
        Language::Wasm => t.starts_with(";;"),
    }
}

/// Pick the first non-empty capture group from a set of capture groups.
fn first_capture<'h>(caps: &regex::Captures<'h>, indices: &[usize]) -> Option<String> {
    for &i in indices {
        if let Some(m) = caps.get(i) {
            if !m.as_str().is_empty() || caps.get(i).is_some() {
                return Some(m.as_str().to_owned());
            }
        }
    }
    None
}

// ---------------------------------------------------------------------------
// Public extraction API
// ---------------------------------------------------------------------------

/// Extract regex patterns from `source` written in `lang`.
/// `file` is recorded verbatim in each [`ExtractedRegex`].
pub fn extract_regexes(source: &str, lang: Language, file: &Path) -> Vec<ExtractedRegex> {
    match lang {
        Language::Python => extract_python(source, file),
        Language::JavaScript => extract_js(source, file, lang),
        Language::Rust => extract_rust(source, file),
        Language::Go => extract_go(source, file),
        Language::Java => extract_java(source, file),
        Language::Ruby => extract_ruby(source, file),
        // For languages without dedicated extractors return empty.
        _ => vec![],
    }
}

// ---------------------------------------------------------------------------
// Python
// ---------------------------------------------------------------------------

fn extract_python(source: &str, file: &Path) -> Vec<ExtractedRegex> {
    let pats = python_patterns();
    let mut out = Vec::new();

    for (line_idx, line) in source.lines().enumerate() {
        if is_comment_line(line, Language::Python) {
            continue;
        }
        for caps in pats.compile.captures_iter(line) {
            // Groups: 1 = r"...", 2 = "...", 3 = r'...', 4 = '...'
            if let Some(pattern) = first_capture(&caps, &[1, 2, 3, 4]) {
                out.push(ExtractedRegex {
                    pattern,
                    file: file.to_path_buf(),
                    line: (line_idx + 1) as u32,
                    language: Language::Python,
                    flags: vec![],
                });
            }
        }
    }
    out
}

// ---------------------------------------------------------------------------
// JavaScript / TypeScript
// ---------------------------------------------------------------------------

fn extract_js(source: &str, file: &Path, lang: Language) -> Vec<ExtractedRegex> {
    let pats = js_patterns();
    let mut out = Vec::new();

    for (line_idx, line) in source.lines().enumerate() {
        if is_comment_line(line, lang) {
            continue;
        }

        // Literal /pattern/flags
        for caps in pats.literal.captures_iter(line) {
            if let Some(pattern) = caps.get(1).map(|m| m.as_str().to_owned()) {
                let flags_str = caps.get(2).map(|m| m.as_str()).unwrap_or("");
                let flags: Vec<String> = flags_str.chars().map(|c| c.to_string()).collect();
                out.push(ExtractedRegex {
                    pattern,
                    file: file.to_path_buf(),
                    line: (line_idx + 1) as u32,
                    language: lang,
                    flags,
                });
            }
        }

        // RegExp constructor
        for caps in pats.constructor.captures_iter(line) {
            // Groups 1 = double-quote, 2 = single-quote
            if let Some(pattern) = first_capture(&caps, &[1, 2]) {
                out.push(ExtractedRegex {
                    pattern,
                    file: file.to_path_buf(),
                    line: (line_idx + 1) as u32,
                    language: lang,
                    flags: vec![],
                });
            }
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Rust
// ---------------------------------------------------------------------------

fn extract_rust(source: &str, file: &Path) -> Vec<ExtractedRegex> {
    let pats = rust_patterns();
    let mut out = Vec::new();

    for (line_idx, line) in source.lines().enumerate() {
        if is_comment_line(line, Language::Rust) {
            continue;
        }
        for caps in pats.new.captures_iter(line) {
            // Groups: 1 = r"...", 2 = "..."
            if let Some(pattern) = first_capture(&caps, &[1, 2]) {
                out.push(ExtractedRegex {
                    pattern,
                    file: file.to_path_buf(),
                    line: (line_idx + 1) as u32,
                    language: Language::Rust,
                    flags: vec![],
                });
            }
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Go
// ---------------------------------------------------------------------------

fn extract_go(source: &str, file: &Path) -> Vec<ExtractedRegex> {
    let pats = go_patterns();
    let mut out = Vec::new();

    for (line_idx, line) in source.lines().enumerate() {
        if is_comment_line(line, Language::Go) {
            continue;
        }
        for caps in pats.compile.captures_iter(line) {
            if let Some(pattern) = caps.get(1).map(|m| m.as_str().to_owned()) {
                out.push(ExtractedRegex {
                    pattern,
                    file: file.to_path_buf(),
                    line: (line_idx + 1) as u32,
                    language: Language::Go,
                    flags: vec![],
                });
            }
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Java
// ---------------------------------------------------------------------------

fn extract_java(source: &str, file: &Path) -> Vec<ExtractedRegex> {
    let pats = java_patterns();
    let mut out = Vec::new();

    for (line_idx, line) in source.lines().enumerate() {
        if is_comment_line(line, Language::Java) {
            continue;
        }
        for caps in pats.compile.captures_iter(line) {
            if let Some(pattern) = caps.get(1).map(|m| m.as_str().to_owned()) {
                out.push(ExtractedRegex {
                    pattern,
                    file: file.to_path_buf(),
                    line: (line_idx + 1) as u32,
                    language: Language::Java,
                    flags: vec![],
                });
            }
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Ruby
// ---------------------------------------------------------------------------

fn extract_ruby(source: &str, file: &Path) -> Vec<ExtractedRegex> {
    let pats = ruby_patterns();
    let mut out = Vec::new();

    for (line_idx, line) in source.lines().enumerate() {
        if is_comment_line(line, Language::Ruby) {
            continue;
        }

        // Literal /pattern/flags
        for caps in pats.literal.captures_iter(line) {
            if let Some(pattern) = caps.get(1).map(|m| m.as_str().to_owned()) {
                let flags_str = caps.get(2).map(|m| m.as_str()).unwrap_or("");
                let flags: Vec<String> = flags_str.chars().map(|c| c.to_string()).collect();
                out.push(ExtractedRegex {
                    pattern,
                    file: file.to_path_buf(),
                    line: (line_idx + 1) as u32,
                    language: Language::Ruby,
                    flags,
                });
            }
        }

        // Regexp.new constructor
        for caps in pats.constructor.captures_iter(line) {
            if let Some(pattern) = first_capture(&caps, &[1, 2]) {
                out.push(ExtractedRegex {
                    pattern,
                    file: file.to_path_buf(),
                    line: (line_idx + 1) as u32,
                    language: Language::Ruby,
                    flags: vec![],
                });
            }
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn fake_file() -> PathBuf {
        PathBuf::from("test_file.py")
    }

    // 1. Python raw string
    #[test]
    fn python_compile_raw_double_quote() {
        let src = r#"import re
x = re.compile(r"(a+)+$")
"#;
        let results = extract_regexes(src, Language::Python, &fake_file());
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].pattern, "(a+)+$");
        assert_eq!(results[0].line, 2);
        assert_eq!(results[0].flags, Vec::<String>::new());
    }

    // Python regular string (no r prefix)
    #[test]
    fn python_compile_regular_string() {
        let src = r#"x = re.compile("(a+)+$")"#;
        let results = extract_regexes(src, Language::Python, &fake_file());
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].pattern, "(a+)+$");
    }

    // Python single-quote raw string
    #[test]
    fn python_compile_raw_single_quote() {
        let src = "x = re.compile(r'(a+)+$')";
        let results = extract_regexes(src, Language::Python, &fake_file());
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].pattern, "(a+)+$");
    }

    // Python various re functions
    #[test]
    fn python_various_functions() {
        let src = r#"
re.match(r"^\d+", text)
re.search(r"foo\d", s)
re.findall(r"[a-z]+", s)
re.sub(r"bad", "good", s)
re.fullmatch(r".*", s)
"#;
        let results = extract_regexes(src, Language::Python, &fake_file());
        assert_eq!(results.len(), 5);
    }

    // 2. JavaScript literal regex with flags
    #[test]
    fn js_literal_with_flags() {
        let src = r#"const r = /(a+)+$/g;"#;
        let results = extract_regexes(src, Language::JavaScript, &PathBuf::from("t.js"));
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].pattern, "(a+)+$");
        assert_eq!(results[0].flags, vec!["g"]);
    }

    // 3. JavaScript RegExp constructor
    #[test]
    fn js_regexp_constructor() {
        let src = r#"const r = new RegExp("(a+)+$");"#;
        let results = extract_regexes(src, Language::JavaScript, &PathBuf::from("t.js"));
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].pattern, "(a+)+$");
        assert_eq!(results[0].flags, Vec::<String>::new());
    }

    // JavaScript RegExp without new
    #[test]
    fn js_regexp_no_new() {
        let src = r#"const r = RegExp('hello+');"#;
        let results = extract_regexes(src, Language::JavaScript, &PathBuf::from("t.js"));
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].pattern, "hello+");
    }

    // JavaScript literal in function call context
    #[test]
    fn js_literal_in_function_call() {
        let src = r#"str.match(/(foo|bar)+/);"#;
        let results = extract_regexes(src, Language::JavaScript, &PathBuf::from("t.js"));
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].pattern, "(foo|bar)+");
    }

    // 4. Rust Regex::new with raw string
    #[test]
    fn rust_regex_new_raw() {
        let src = r#"let re = Regex::new(r"(a+)+$").unwrap();"#;
        let results = extract_regexes(src, Language::Rust, &PathBuf::from("t.rs"));
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].pattern, "(a+)+$");
    }

    // Rust Regex::new with regular string
    #[test]
    fn rust_regex_new_regular() {
        let src = r#"let re = Regex::new("(a+)+$").unwrap();"#;
        let results = extract_regexes(src, Language::Rust, &PathBuf::from("t.rs"));
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].pattern, "(a+)+$");
    }

    // Rust RegexBuilder
    #[test]
    fn rust_regex_builder() {
        let src = r#"let re = RegexBuilder::new("foo+").build().unwrap();"#;
        let results = extract_regexes(src, Language::Rust, &PathBuf::from("t.rs"));
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].pattern, "foo+");
    }

    // 5. Go regexp.MustCompile
    #[test]
    fn go_must_compile() {
        let src = r#"re := regexp.MustCompile("(a+)+$")"#;
        let results = extract_regexes(src, Language::Go, &PathBuf::from("t.go"));
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].pattern, "(a+)+$");
    }

    // Go regexp.Compile
    #[test]
    fn go_compile() {
        let src = r#"re, err := regexp.Compile("(a+)+$")"#;
        let results = extract_regexes(src, Language::Go, &PathBuf::from("t.go"));
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].pattern, "(a+)+$");
    }

    // Java Pattern.compile
    #[test]
    fn java_pattern_compile() {
        let src = r#"Pattern p = Pattern.compile("(a+)+$");"#;
        let results = extract_regexes(src, Language::Java, &PathBuf::from("T.java"));
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].pattern, "(a+)+$");
    }

    // Java Pattern.compile with flags argument (second arg ignored, pattern still extracted)
    #[test]
    fn java_pattern_compile_with_flags() {
        let src = r#"Pattern p = Pattern.compile("(a+)+$", Pattern.CASE_INSENSITIVE);"#;
        let results = extract_regexes(src, Language::Java, &PathBuf::from("T.java"));
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].pattern, "(a+)+$");
    }

    // Ruby literal regex
    #[test]
    fn ruby_literal_regex() {
        let src = "r = /(a+)+$/im";
        let results = extract_regexes(src, Language::Ruby, &PathBuf::from("t.rb"));
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].pattern, "(a+)+$");
        assert!(results[0].flags.contains(&"i".to_owned()));
        assert!(results[0].flags.contains(&"m".to_owned()));
    }

    // Ruby Regexp.new
    #[test]
    fn ruby_regexp_new() {
        let src = r#"r = Regexp.new("(a+)+$")"#;
        let results = extract_regexes(src, Language::Ruby, &PathBuf::from("t.rb"));
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].pattern, "(a+)+$");
    }

    // 6. Multiple regexes in one file
    #[test]
    fn multiple_regexes_in_file() {
        let src = r#"
x = re.compile(r"\d+")
y = re.compile(r"[a-z]+")
z = re.search(r"foo.*bar", text)
"#;
        let results = extract_regexes(src, Language::Python, &fake_file());
        assert_eq!(results.len(), 3);
        assert_eq!(results[0].pattern, r"\d+");
        assert_eq!(results[1].pattern, "[a-z]+");
        assert_eq!(results[2].pattern, "foo.*bar");
    }

    // 7. Regex in comment — must NOT be extracted
    #[test]
    fn python_comment_not_extracted() {
        let src = r#"# re.compile(r"(a+)+$")
x = re.compile(r"safe")
"#;
        let results = extract_regexes(src, Language::Python, &fake_file());
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].pattern, "safe");
    }

    #[test]
    fn js_comment_not_extracted() {
        let src = r#"// const r = /pattern/g;
const x = /real/i;
"#;
        let results = extract_regexes(src, Language::JavaScript, &PathBuf::from("t.js"));
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].pattern, "real");
    }

    #[test]
    fn rust_comment_not_extracted() {
        let src = r#"// let re = Regex::new(r"(skip)+").unwrap();
let re = Regex::new(r"(keep)+").unwrap();
"#;
        let results = extract_regexes(src, Language::Rust, &PathBuf::from("t.rs"));
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].pattern, "(keep)+");
    }

    // Line number accuracy
    #[test]
    fn line_numbers_are_correct() {
        let src = "x = 1\ny = re.compile(r\"pat\")\nz = 3\n";
        let results = extract_regexes(src, Language::Python, &fake_file());
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].line, 2);
    }

    // File path is preserved
    #[test]
    fn file_path_preserved() {
        let src = r#"x = re.compile(r"pat")"#;
        let p = PathBuf::from("/some/project/main.py");
        let results = extract_regexes(src, Language::Python, &p);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].file, p);
    }

    // Unsupported language returns empty
    #[test]
    fn unsupported_language_returns_empty() {
        let src = "anything here";
        let results = extract_regexes(src, Language::Wasm, &fake_file());
        assert!(results.is_empty());
    }
}
