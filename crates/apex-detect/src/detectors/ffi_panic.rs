/// CWE-248 — Uncaught Exception / Undefined Behavior: panic across FFI boundary.
///
/// A Rust `panic!` (or any macro/method that panics) inside an `extern "C" fn`
/// or `extern "C-unwind" fn` body is Undefined Behavior.  The C ABI has no
/// mechanism for unwinding a Rust panic; the runtime will abort the process
/// (or, with `panic = "unwind"`, produce UB).
///
/// Flagged panic sources:
///   - `panic!(`
///   - `.unwrap()` or `unwrap_or_else` that panics
///   - `.expect(`
///   - `todo!()`
///   - `unimplemented!()`
///   - `unreachable!()` (when not in match arm with proof)
///
/// Safe alternatives:
///   - Return a sentinel value / null pointer and set a thread-local error.
///   - Use `std::panic::catch_unwind` around the body.
///   - Change function to `extern "C-unwind"` AND ensure the caller handles unwinds.
///
/// Severity: Critical
/// Languages: Rust only
use apex_core::error::Result;
use apex_core::types::Language;
use async_trait::async_trait;
use regex::Regex;
use std::sync::LazyLock;
use uuid::Uuid;

use super::util::{in_test_block, is_comment};
use crate::context::AnalysisContext;
use crate::finding::{Finding, FindingCategory, Severity};
use crate::Detector;

pub struct FfiPanicDetector;

/// Matches `extern "C" fn` or `extern "C-unwind" fn` — the start of an FFI-exported function.
static EXTERN_C_FN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"extern\s+"C(?:-unwind)?"\s+fn\s+"#).expect("extern C fn regex must compile")
});

/// Matches panic-inducing expressions.
static PANIC_EXPR: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r#"(?x)
        \bpanic!\s*\(
        | \.unwrap\s*\(\s*\)
        | \.expect\s*\(
        | \btodo!\s*\(\s*\)
        | \bunimplemented!\s*\(\s*\)
        | \bunreachable!\s*\(\s*\)
        "#,
    )
    .expect("panic expr regex must compile")
});

/// Matches safe patterns that involve `.unwrap()` on something that cannot fail:
/// e.g., `Regex::new(...).unwrap()` in a `LazyLock` initialiser — static context,
/// not inside an extern fn body at runtime.
/// We keep detection simple and rely on scope tracking; this is noted in tests.

#[async_trait]
impl Detector for FfiPanicDetector {
    fn name(&self) -> &str {
        "ffi-panic"
    }

    async fn analyze(&self, ctx: &AnalysisContext) -> Result<Vec<Finding>> {
        if ctx.language != Language::Rust {
            return Ok(vec![]);
        }

        let mut findings = Vec::new();

        for (path, source) in &ctx.source_cache {
            let lines: Vec<&str> = source.lines().collect();
            let n = lines.len();
            let mut i = 0usize;

            while i < n {
                let line = lines[i];
                let trimmed = line.trim();

                // Look for the start of an extern "C" fn.
                if EXTERN_C_FN.is_match(trimmed) {
                    // Scan forward to find the opening brace.
                    let mut brace_depth: i32 = 0;
                    let mut fn_body_start: Option<usize> = None;
                    let mut fn_body_end: Option<usize> = None;
                    let mut j = i;

                    'scan: while j < n {
                        let scan_line = lines[j];
                        for ch in scan_line.chars() {
                            match ch {
                                '{' => {
                                    if brace_depth == 0 {
                                        fn_body_start = Some(j);
                                    }
                                    brace_depth += 1;
                                }
                                '}' => {
                                    brace_depth -= 1;
                                    if brace_depth == 0 && fn_body_start.is_some() {
                                        fn_body_end = Some(j);
                                        break 'scan;
                                    }
                                }
                                _ => {}
                            }
                        }
                        j += 1;
                    }

                    if let (Some(start), Some(end)) = (fn_body_start, fn_body_end) {
                        // Scan the fn body for panic-inducing patterns.
                        for (body_idx, body_line) in lines[(start + 1)..end]
                            .iter()
                            .enumerate()
                            .map(|(rel, l)| (start + 1 + rel, *l))
                        {
                            let body_trimmed = body_line.trim();

                            if body_trimmed.is_empty() || is_comment(body_trimmed, Language::Rust) {
                                continue;
                            }

                            // Skip test blocks inside extern fns (unusual but possible).
                            if in_test_block(source, body_idx) {
                                continue;
                            }

                            if PANIC_EXPR.is_match(body_trimmed) {
                                let line_1based = (body_idx + 1) as u32;
                                let extern_fn_line = (i + 1) as u32;
                                findings.push(Finding {
                                    id: Uuid::new_v4(),
                                    detector: self.name().into(),
                                    severity: Severity::Critical,
                                    category: FindingCategory::UndefinedBehavior,
                                    file: path.clone(),
                                    line: Some(line_1based),
                                    title: format!(
                                        "Panic-inducing expression inside extern \"C\" fn \
                                         (defined at line {})",
                                        extern_fn_line
                                    ),
                                    description: format!(
                                        "Line {} in {} contains a panic-inducing expression inside \
                                         an `extern \"C\"` function (declared at line {}). \
                                         A panic crossing an FFI boundary is Undefined Behavior \
                                         in Rust — the process will abort or produce UB. \
                                         Use std::panic::catch_unwind or return an error sentinel \
                                         instead.",
                                        line_1based,
                                        path.display(),
                                        extern_fn_line
                                    ),
                                    evidence: vec![],
                                    covered: false,
                                    suggestion: "Wrap the body with std::panic::catch_unwind, or \
                                                 propagate errors as a return value / out-pointer \
                                                 rather than panicking."
                                        .into(),
                                    explanation: None,
                                    fix: None,
                                    cwe_ids: vec![248],
                    noisy: false,
                                });
                            }
                        }
                        i = end + 1;
                    } else {
                        i += 1;
                    }
                } else {
                    i += 1;
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
    use std::collections::HashMap;
    use std::path::PathBuf;

    fn make_ctx(files: HashMap<PathBuf, String>) -> AnalysisContext {
        AnalysisContext {
            language: Language::Rust,
            source_cache: files,
            ..AnalysisContext::test_default()
        }
    }

    // ---- True positives ----

    #[tokio::test]
    async fn detects_unwrap_in_extern_c_fn() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/ffi.rs"),
            r#"
#[no_mangle]
pub extern "C" fn process_data(ptr: *const u8, len: usize) -> i32 {
    let slice = unsafe { std::slice::from_raw_parts(ptr, len) };
    let s = std::str::from_utf8(slice).unwrap(); // DANGER: panic in FFI
    s.len() as i32
}
"#
            .into(),
        );
        let ctx = make_ctx(files);
        let findings = FfiPanicDetector.analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, Severity::Critical);
        assert_eq!(findings[0].category, FindingCategory::UndefinedBehavior);
        assert!(findings[0].cwe_ids.contains(&248));
    }

    #[tokio::test]
    async fn detects_explicit_panic_in_extern_c_fn() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/ffi.rs"),
            r#"
pub extern "C" fn must_succeed() {
    panic!("this should never fail");
}
"#
            .into(),
        );
        let ctx = make_ctx(files);
        let findings = FfiPanicDetector.analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 1);
    }

    #[tokio::test]
    async fn detects_todo_in_extern_c_fn() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/ffi.rs"),
            r#"
extern "C" fn not_implemented_yet() -> i32 {
    todo!()
}
"#
            .into(),
        );
        let ctx = make_ctx(files);
        let findings = FfiPanicDetector.analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 1);
    }

    #[tokio::test]
    async fn detects_expect_in_extern_c_fn() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/ffi.rs"),
            r#"
extern "C" fn parse_input(raw: *const u8, len: usize) -> i32 {
    let bytes = unsafe { std::slice::from_raw_parts(raw, len) };
    let val: i32 = std::str::from_utf8(bytes).expect("valid utf8").parse().expect("valid int");
    val
}
"#
            .into(),
        );
        let ctx = make_ctx(files);
        let findings = FfiPanicDetector.analyze(&ctx).await.unwrap();
        assert!(!findings.is_empty());
    }

    // ---- True negatives ----

    #[tokio::test]
    async fn no_finding_unwrap_in_regular_fn() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/lib.rs"),
            r#"
fn process(s: &str) -> usize {
    s.parse::<usize>().unwrap()
}
"#
            .into(),
        );
        let ctx = make_ctx(files);
        let findings = FfiPanicDetector.analyze(&ctx).await.unwrap();
        assert!(
            findings.is_empty(),
            "regular fn with unwrap should not flag"
        );
    }

    #[tokio::test]
    async fn no_finding_extern_c_fn_with_result_handling() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/ffi.rs"),
            r#"
extern "C" fn safe_parse(ptr: *const u8, len: usize) -> i32 {
    let bytes = unsafe { std::slice::from_raw_parts(ptr, len) };
    match std::str::from_utf8(bytes) {
        Ok(s) => s.len() as i32,
        Err(_) => -1,
    }
}
"#
            .into(),
        );
        let ctx = make_ctx(files);
        let findings = FfiPanicDetector.analyze(&ctx).await.unwrap();
        assert!(
            findings.is_empty(),
            "Result-handled extern fn should not flag"
        );
    }

    #[tokio::test]
    async fn no_finding_python_language() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/ffi.py"),
            r#"extern "C" fn foo(): panic!("oops")"#.into(),
        );
        let ctx = AnalysisContext {
            language: Language::Python,
            source_cache: files,
            ..AnalysisContext::test_default()
        };
        let findings = FfiPanicDetector.analyze(&ctx).await.unwrap();
        assert!(findings.is_empty());
    }

    #[test]
    fn does_not_use_cargo_subprocess() {
        assert!(!FfiPanicDetector.uses_cargo_subprocess());
    }
}
