use apex_core::error::Result;
use apex_core::types::Language;
use async_trait::async_trait;
use uuid::Uuid;

use super::util::{in_test_block, is_comment, is_test_file, strip_string_literals};
use crate::context::AnalysisContext;
use crate::finding::{Finding, FindingCategory, Severity};
use crate::Detector;

pub struct PanicPatternDetector;

/// Rust-specific panic patterns.
const RUST_PANIC_PATTERNS: &[(&str, &str)] = &[
    (".unwrap()", "unwrap() call — panics on None/Err"),
    (
        ".expect(",
        "expect() call — panics on None/Err with message",
    ),
    ("panic!(", "panic!() macro — explicit panic"),
    ("todo!(", "todo!() macro — unimplemented code"),
    (
        "unreachable!(",
        "unreachable!() macro — should-not-reach path",
    ),
    ("unimplemented!(", "unimplemented!() macro"),
];

/// Python-specific panic patterns.
const PYTHON_PANIC_PATTERNS: &[(&str, &str)] = &[
    ("assert ", "bare assert — disabled with python -O"),
    ("sys.exit(", "sys.exit() call — abrupt termination"),
    ("os._exit(", "os._exit() call — immediate termination"),
    ("raise SystemExit", "SystemExit — abrupt termination"),
];

/// JS/TS-specific panic patterns.
const JS_PANIC_PATTERNS: &[(&str, &str)] = &[
    ("process.exit(", "process.exit() — abrupt termination"),
    ("throw new Error(", "throw new Error — unhandled may crash"),
];

/// C-specific panic patterns.
const C_PANIC_PATTERNS: &[(&str, &str)] = &[
    ("abort()", "abort() call — immediate termination"),
    ("exit(", "exit() call — abrupt termination"),
    ("assert(", "assert() macro — disabled in release builds"),
];

/// Ruby-specific panic patterns.
const RUBY_PANIC_PATTERNS: &[(&str, &str)] = &[
    ("raise ", "raise — exception that may crash if unhandled"),
    ("abort", "abort — immediate termination"),
    ("exit!", "exit! — immediate termination (skips at_exit)"),
    ("exit(", "exit() — process termination"),
    ("Kernel.exit", "Kernel.exit — process termination"),
    ("fail ", "fail — alias for raise"),
];

/// Patterns that are always Medium+ regardless of context.
const HARD_PANIC_PATTERNS: &[&str] = &[
    "panic!(",
    "todo!(",
    "unimplemented!(",
    "abort()",
    "abort",
    "os._exit(",
    "process.exit(",
    "exit!",
];

/// Select patterns based on target language.
fn patterns_for_language(lang: Language) -> &'static [(&'static str, &'static str)] {
    match lang {
        Language::Rust => RUST_PANIC_PATTERNS,
        Language::Python => PYTHON_PANIC_PATTERNS,
        Language::JavaScript => JS_PANIC_PATTERNS,
        Language::C => C_PANIC_PATTERNS,
        Language::Ruby => RUBY_PANIC_PATTERNS,
        _ => RUST_PANIC_PATTERNS, // fallback (Java, Wasm)
    }
}

/// Determine severity for a panic pattern.
/// - Hard panics (panic!, todo!, unimplemented!) are always Medium.
/// - unwrap()/expect() in non-test production code are Low.
fn classify_severity(pattern: &str) -> Severity {
    for hard in HARD_PANIC_PATTERNS {
        if pattern == *hard {
            return Severity::Medium;
        }
    }
    // unwrap() and expect() are Low — they're common in Rust and usually
    // handled by callers or in non-critical paths.
    Severity::Low
}

#[async_trait]
impl Detector for PanicPatternDetector {
    fn name(&self) -> &str {
        "panic-pattern"
    }

    async fn analyze(&self, ctx: &AnalysisContext) -> Result<Vec<Finding>> {
        let mut findings = Vec::new();
        let patterns = patterns_for_language(ctx.language);

        for (path, source) in &ctx.source_cache {
            // Skip test files entirely
            if is_test_file(path) {
                continue;
            }

            for (line_num, line) in source.lines().enumerate() {
                let trimmed = line.trim();

                // Skip comments
                if is_comment(trimmed, ctx.language) {
                    continue;
                }

                // Skip #[test] function attributes
                if trimmed == "#[test]" || trimmed == "#[tokio::test]" {
                    continue;
                }

                let stripped = strip_string_literals(trimmed);
                for (pattern, description) in patterns {
                    if stripped.contains(pattern) {
                        let line_1based = (line_num + 1) as u32;

                        // Skip patterns inside #[cfg(test)] blocks
                        if in_test_block(source, line_num) {
                            break;
                        }

                        let severity = classify_severity(pattern);

                        findings.push(Finding {
                            id: Uuid::new_v4(),
                            detector: self.name().into(),
                            severity,
                            category: FindingCategory::PanicPath,
                            file: path.clone(),
                            line: Some(line_1based),
                            title: format!("{description} at line {line_1based}"),
                            description: format!(
                                "Pattern `{pattern}` found in {}:{}",
                                path.display(),
                                line_1based
                            ),
                            evidence: vec![],
                            covered: false,
                            suggestion: "Handle error explicitly or add test for panic path".into(),
                            explanation: None,
                            fix: None,
                            cwe_ids: vec![248],
                    noisy: false,
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
    use crate::config::DetectConfig;
    use crate::context::AnalysisContext;
    use crate::finding::FindingCategory;
    use apex_core::types::Language;
    use apex_coverage::CoverageOracle;
    use std::collections::HashMap;
    use std::path::PathBuf;
    use std::sync::Arc;

    fn make_ctx(source_files: HashMap<PathBuf, String>) -> AnalysisContext {
        AnalysisContext {
            source_cache: source_files,
            ..AnalysisContext::test_default()
        }
    }

    #[tokio::test]
    async fn detects_unwrap() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/main.rs"),
            "fn foo() {\n    let x = bar().unwrap();\n}\n".into(),
        );
        let ctx = make_ctx(files);
        let findings = PanicPatternDetector.analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].category, FindingCategory::PanicPath);
        assert!(findings[0].title.contains("unwrap"));
        assert_eq!(findings[0].line, Some(2));
    }

    #[tokio::test]
    async fn unwrap_is_low_severity() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/lib.rs"),
            "fn foo() {\n    bar().unwrap();\n}\n".into(),
        );
        let ctx = make_ctx(files);
        let findings = PanicPatternDetector.analyze(&ctx).await.unwrap();
        assert_eq!(findings[0].severity, Severity::Low);
    }

    #[tokio::test]
    async fn expect_is_low_severity() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/lib.rs"),
            "fn foo() {\n    x.expect(\"oops\");\n}\n".into(),
        );
        let ctx = make_ctx(files);
        let findings = PanicPatternDetector.analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, Severity::Low);
    }

    #[tokio::test]
    async fn panic_macro_is_medium() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/lib.rs"),
            "fn foo() {\n    panic!(\"boom\");\n}\n".into(),
        );
        let ctx = make_ctx(files);
        let findings = PanicPatternDetector.analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, Severity::Medium);
    }

    #[tokio::test]
    async fn todo_is_medium() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/lib.rs"),
            "fn foo() {\n    todo!();\n}\n".into(),
        );
        let ctx = make_ctx(files);
        let findings = PanicPatternDetector.analyze(&ctx).await.unwrap();
        assert_eq!(findings[0].severity, Severity::Medium);
    }

    #[tokio::test]
    async fn detects_todo_and_unreachable() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/lib.rs"),
            "fn foo() {\n    todo!();\n}\nfn bar() {\n    unreachable!();\n}\n".into(),
        );
        let ctx = make_ctx(files);
        let findings = PanicPatternDetector.analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 2);
    }

    #[tokio::test]
    async fn ignores_comments() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/lib.rs"),
            "fn foo() {\n    // x.unwrap() is bad\n    let y = 1;\n}\n".into(),
        );
        let ctx = make_ctx(files);
        let findings = PanicPatternDetector.analyze(&ctx).await.unwrap();
        assert!(findings.is_empty());
    }

    #[tokio::test]
    async fn skips_test_files() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("tests/integration.rs"),
            "fn test_foo() {\n    bar().unwrap();\n    panic!(\"expected\");\n}\n".into(),
        );
        files.insert(
            PathBuf::from("src/main.rs"),
            "fn real() {\n    bar().unwrap();\n}\n".into(),
        );
        let ctx = make_ctx(files);
        let findings = PanicPatternDetector.analyze(&ctx).await.unwrap();
        // Only the src/ file finding, not the tests/ file
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].file, PathBuf::from("src/main.rs"));
    }

    #[tokio::test]
    async fn skips_cfg_test_blocks() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/lib.rs"),
            "fn real() {\n    panic!(\"real bug\");\n}\n\n#[cfg(test)]\nmod tests {\n    fn test_thing() {\n        panic!(\"test ok\");\n    }\n}\n".into(),
        );
        let ctx = make_ctx(files);
        let findings = PanicPatternDetector.analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].line, Some(2)); // Only the real panic, not the test one
    }

    #[tokio::test]
    async fn skips_bench_files() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("benches/perf.rs"),
            "fn bench() {\n    x.unwrap();\n}\n".into(),
        );
        let ctx = make_ctx(files);
        let findings = PanicPatternDetector.analyze(&ctx).await.unwrap();
        assert!(findings.is_empty());
    }

    #[tokio::test]
    async fn empty_source_cache_produces_no_findings() {
        let ctx = make_ctx(HashMap::new());
        let findings = PanicPatternDetector.analyze(&ctx).await.unwrap();
        assert!(findings.is_empty());
    }

    #[test]
    fn does_not_use_cargo_subprocess() {
        assert!(!PanicPatternDetector.uses_cargo_subprocess());
    }

    #[test]
    fn is_test_file_checks() {
        // Rust conventions
        assert!(is_test_file(std::path::Path::new("tests/foo.rs")));
        assert!(is_test_file(std::path::Path::new(
            "tests/integration/bar.rs"
        )));
        assert!(is_test_file(std::path::Path::new("benches/perf.rs")));
        assert!(is_test_file(std::path::Path::new("src/foo_test.rs")));
        assert!(is_test_file(std::path::Path::new("src/foo_tests.rs")));
        assert!(is_test_file(std::path::Path::new(
            "crates/searcher/src/testutil.rs"
        )));
        assert!(is_test_file(std::path::Path::new("src/testutils.rs")));
        assert!(is_test_file(std::path::Path::new("src/test_helpers.rs")));
        assert!(is_test_file(std::path::Path::new("tests/conftest.py")));
        // JS conventions
        assert!(is_test_file(std::path::Path::new("test/app.all.js")));
        assert!(is_test_file(std::path::Path::new("__tests__/foo.js")));
        assert!(is_test_file(std::path::Path::new("src/__tests__/bar.tsx")));
        assert!(is_test_file(std::path::Path::new("src/foo.test.js")));
        assert!(is_test_file(std::path::Path::new("src/bar.spec.ts")));
        assert!(is_test_file(std::path::Path::new("spec/helper.rb")));
        // Non-test files
        assert!(!is_test_file(std::path::Path::new("src/lib.rs")));
        assert!(!is_test_file(std::path::Path::new("src/main.rs")));
        assert!(!is_test_file(std::path::Path::new("src/index.js")));
    }

    #[test]
    fn in_test_block_detects_cfg_test() {
        let source = "fn real() {\n    panic!(\"real\");\n}\n\n#[cfg(test)]\nmod tests {\n    fn t() {\n        panic!(\"test\");\n    }\n}\n";
        assert!(!in_test_block(source, 1)); // real panic
        assert!(in_test_block(source, 7)); // test panic
    }

    #[test]
    fn in_test_block_false_when_no_test_module() {
        let source = "fn foo() {\n    bar();\n}\n";
        assert!(!in_test_block(source, 1));
    }

    #[tokio::test]
    async fn ignores_patterns_inside_string_literals() {
        let mut files = HashMap::new();
        // Simulate const array definitions and other string-literal-only occurrences
        files.insert(
            PathBuf::from("src/detector.rs"),
            concat!(
                "const PATTERNS: &[(&str, &str)] = &[\n",
                "    (\".unwrap()\", \"unwrap() call\"),\n",
                "    (\"panic!(\", \"explicit panic\"),\n",
                "    \"todo!(\",\n",
                "];\n",
                "fn real() {\n",
                "    bar().unwrap();\n",
                "}\n",
            )
            .into(),
        );
        let ctx = make_ctx(files);
        let findings = PanicPatternDetector.analyze(&ctx).await.unwrap();
        // Only the real unwrap on line 7, not the string literal definitions
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].line, Some(7));
        assert!(findings[0].title.contains("unwrap"));
    }

    #[test]
    fn strip_string_literals_works() {
        assert_eq!(
            strip_string_literals(r#"foo "bar.unwrap()" baz"#),
            r#"foo "" baz"#
        );
        assert_eq!(strip_string_literals(r#"x.unwrap()"#), r#"x.unwrap()"#);
        assert_eq!(
            strip_string_literals(r#"("panic!(", "desc")"#),
            r#"("", "")"#
        );
        // Escaped quote inside string
        assert_eq!(
            strip_string_literals(r#""he said \"hi\"" done"#),
            r#""" done"#
        );
    }

    #[test]
    fn classify_severity_values() {
        assert_eq!(classify_severity("panic!("), Severity::Medium);
        assert_eq!(classify_severity("todo!("), Severity::Medium);
        assert_eq!(classify_severity("unimplemented!("), Severity::Medium);
        assert_eq!(classify_severity("abort()"), Severity::Medium);
        assert_eq!(classify_severity("os._exit("), Severity::Medium);
        assert_eq!(classify_severity("process.exit("), Severity::Medium);
        assert_eq!(classify_severity(".unwrap()"), Severity::Low);
        assert_eq!(classify_severity(".expect("), Severity::Low);
        assert_eq!(classify_severity("unreachable!("), Severity::Low);
    }

    fn make_ctx_lang(source_files: HashMap<PathBuf, String>, lang: Language) -> AnalysisContext {
        AnalysisContext {
            language: lang,
            source_cache: source_files,
            ..AnalysisContext::test_default()
        }
    }

    #[tokio::test]
    async fn detects_js_process_exit() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/index.js"),
            "function shutdown() {\n    process.exit(1);\n}\n".into(),
        );
        let ctx = make_ctx_lang(files, Language::JavaScript);
        let findings = PanicPatternDetector.analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 1);
        assert!(findings[0].title.contains("process.exit"));
        assert_eq!(findings[0].severity, Severity::Medium);
    }

    #[tokio::test]
    async fn js_skips_test_dir() {
        let mut files = HashMap::new();
        files.insert(PathBuf::from("test/app.js"), "process.exit(0);\n".into());
        files.insert(PathBuf::from("src/app.js"), "process.exit(0);\n".into());
        let ctx = make_ctx_lang(files, Language::JavaScript);
        let findings = PanicPatternDetector.analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].file, PathBuf::from("src/app.js"));
    }

    #[tokio::test]
    async fn js_does_not_match_rust_patterns() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/utils.js"),
            "const x = foo.expect(200);\nbar.unwrap();\n".into(),
        );
        let ctx = make_ctx_lang(files, Language::JavaScript);
        let findings = PanicPatternDetector.analyze(&ctx).await.unwrap();
        // .expect() and .unwrap() are Rust patterns, not JS
        assert!(findings.is_empty());
    }

    #[tokio::test]
    async fn detects_python_sys_exit() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/main.py"),
            "import sys\ndef stop():\n    sys.exit(1)\n".into(),
        );
        let ctx = make_ctx_lang(files, Language::Python);
        let findings = PanicPatternDetector.analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 1);
        assert!(findings[0].title.contains("sys.exit"));
    }

    #[tokio::test]
    async fn detects_ruby_raise_and_exit() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("lib/app.rb"),
            "def stop\n  raise \"oops\"\nend\ndef quit\n  exit!\nend\n".into(),
        );
        let ctx = make_ctx_lang(files, Language::Ruby);
        let findings = PanicPatternDetector.analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 2);
        assert!(findings[0].title.contains("raise"));
        assert_eq!(findings[1].severity, Severity::Medium); // exit! is hard panic
    }

    #[tokio::test]
    async fn ruby_skips_spec_dir() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("spec/foo_spec.rb"),
            "raise \"test error\"\n".into(),
        );
        files.insert(PathBuf::from("lib/bar.rb"), "raise \"real error\"\n".into());
        let ctx = make_ctx_lang(files, Language::Ruby);
        let findings = PanicPatternDetector.analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].file, PathBuf::from("lib/bar.rb"));
    }

    #[tokio::test]
    async fn ruby_skips_comments() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("lib/app.rb"),
            "# raise 'commented out'\ndef foo\n  raise 'real'\nend\n".into(),
        );
        let ctx = make_ctx_lang(files, Language::Ruby);
        let findings = PanicPatternDetector.analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].line, Some(3));
    }

    #[tokio::test]
    async fn detects_c_abort() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/main.c"),
            "void fail() {\n    abort();\n}\n".into(),
        );
        let ctx = make_ctx_lang(files, Language::C);
        let findings = PanicPatternDetector.analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 1);
        assert!(findings[0].title.contains("abort"));
        assert_eq!(findings[0].severity, Severity::Medium);
    }

    #[test]
    fn patterns_for_java_falls_back_to_rust() {
        let pats = patterns_for_language(Language::Java);
        // Java falls through to the _ arm → RUST_PANIC_PATTERNS
        assert!(pats.iter().any(|(p, _)| *p == ".unwrap()"));
    }

    #[test]
    fn patterns_for_wasm_falls_back_to_rust() {
        let pats = patterns_for_language(Language::Wasm);
        assert!(pats.iter().any(|(p, _)| *p == "panic!("));
    }

    #[test]
    fn classify_severity_abort_bare() {
        // "abort" (without parens) is also a hard panic pattern
        assert_eq!(classify_severity("abort"), Severity::Medium);
    }

    #[test]
    fn classify_severity_exit_bang() {
        assert_eq!(classify_severity("exit!"), Severity::Medium);
    }

    #[test]
    fn classify_severity_unknown_pattern() {
        assert_eq!(classify_severity("some_random_pattern"), Severity::Low);
    }

    #[tokio::test]
    async fn skips_test_and_tokio_test_attributes() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/lib.rs"),
            "#[test]\nfn test_foo() {}\n#[tokio::test]\nasync fn test_bar() {}\nfn real() { panic!(\"boom\"); }\n".into(),
        );
        let ctx = make_ctx(files);
        let findings = PanicPatternDetector.analyze(&ctx).await.unwrap();
        // Only the real panic, not the #[test]/#[tokio::test] attributes
        assert_eq!(findings.len(), 1);
        assert!(findings[0].title.contains("panic"));
    }

    #[tokio::test]
    async fn detects_python_os_exit() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/worker.py"),
            "import os\ndef crash():\n    os._exit(1)\n".into(),
        );
        let ctx = make_ctx_lang(files, Language::Python);
        let findings = PanicPatternDetector.analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 1);
        assert!(findings[0].title.contains("os._exit"));
        assert_eq!(findings[0].severity, Severity::Medium);
    }

    #[tokio::test]
    async fn detects_python_bare_assert() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/check.py"),
            "def validate(x):\n    assert x > 0\n".into(),
        );
        let ctx = make_ctx_lang(files, Language::Python);
        let findings = PanicPatternDetector.analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 1);
        assert!(findings[0].title.contains("assert"));
    }

    #[tokio::test]
    async fn detects_c_exit_and_assert() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/main.c"),
            "void stop() {\n    exit(1);\n}\nvoid check() {\n    assert(x > 0);\n}\n".into(),
        );
        let ctx = make_ctx_lang(files, Language::C);
        let findings = PanicPatternDetector.analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 2);
    }

    #[tokio::test]
    async fn detects_js_throw_error() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/api.js"),
            "function validate() {\n    throw new Error(\"invalid\");\n}\n".into(),
        );
        let ctx = make_ctx_lang(files, Language::JavaScript);
        let findings = PanicPatternDetector.analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 1);
        assert!(findings[0].title.contains("throw new Error"));
    }

    #[tokio::test]
    async fn detects_ruby_kernel_exit_and_fail() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("lib/runner.rb"),
            "def go\n  Kernel.exit\nend\ndef bad\n  fail \"oops\"\nend\n".into(),
        );
        let ctx = make_ctx_lang(files, Language::Ruby);
        let findings = PanicPatternDetector.analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 2);
    }

    #[tokio::test]
    async fn detects_rust_unimplemented() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/lib.rs"),
            "fn stub() {\n    unimplemented!();\n}\n".into(),
        );
        let ctx = make_ctx(files);
        let findings = PanicPatternDetector.analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, Severity::Medium);
    }

    #[tokio::test]
    async fn detects_python_raise_systemexit() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/app.py"),
            "def quit():\n    raise SystemExit\n".into(),
        );
        let ctx = make_ctx_lang(files, Language::Python);
        let findings = PanicPatternDetector.analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 1);
    }
}
