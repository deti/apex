use async_trait::async_trait;
use uuid::Uuid;

use super::util::{is_comment, is_test_file};
use crate::context::AnalysisContext;
use crate::finding::{Finding, FindingCategory, Severity};
use crate::Detector;
use apex_core::config::ThreatModelType;
use apex_core::error::Result;
use apex_core::types::Language;

pub struct PathNormalizationDetector;

// Function signature keywords for each language.
const PYTHON_FN_KEYWORDS: &[&str] = &["def "];
const JS_FN_KEYWORDS: &[&str] = &["function ", "=> {", "=> (", "const ", "let ", "var "];
const RUST_FN_KEYWORDS: &[&str] = &["fn "];
const JAVA_FN_KEYWORDS: &[&str] = &[
    "public ", "private ", "protected ", "static ", "void ", "String ", "File ",
];
const GO_FN_KEYWORDS: &[&str] = &["func "];
const C_FN_KEYWORDS: &[&str] = &["void ", "int ", "char ", "FILE ", "size_t ", "ssize_t "];
const CSHARP_FN_KEYWORDS: &[&str] = &[
    "public ", "private ", "protected ", "internal ", "static ", "void ", "string ", "async ",
];
const SWIFT_FN_KEYWORDS: &[&str] = &["func "];
const KOTLIN_FN_KEYWORDS: &[&str] = &["fun "];
const RUBY_FN_KEYWORDS: &[&str] = &["def "];

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

const JAVA_NORM_CALLS: &[&str] = &[
    ".normalize()",
    "getcanonicalpath()",
    "getcanonicalfile()",
    "torealpath(",
    "paths.get(",
];

const GO_NORM_CALLS: &[&str] = &[
    "filepath.clean(",
    "filepath.abs(",
    "filepath.evalsymlinks(",
];

const C_NORM_CALLS: &[&str] = &["realpath(", "canonicalize_file_name("];

const CSHARP_NORM_CALLS: &[&str] = &[
    "path.getfullpath(",
    "path.combine(",
    "getfullpath(",
];

const SWIFT_NORM_CALLS: &[&str] = &[
    "standardizedfileurl",
    "resolvingsymlinksinpath",
    "standardized",
];

const KOTLIN_NORM_CALLS: &[&str] = &[
    ".normalize()",
    "canonicalpath",
    "getcanonicalpath()",
    "torealpath(",
];

const RUBY_NORM_CALLS: &[&str] = &[
    "file.expand_path(",
    "pathname.new(",
    ".cleanpath",
    ".realpath",
];

// Validation checks that also count as safe — these protect without full normalisation.
const VALIDATION_PATTERNS: &[&str] = &[
    "\"..\"..", // Rust: contains("..")
    "\"..\"",   // any language string literal ".."
    "'..'",     // Python/JS single-quoted
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
        Language::Java => JAVA_FN_KEYWORDS,
        Language::Go => GO_FN_KEYWORDS,
        Language::C | Language::Cpp => C_FN_KEYWORDS,
        Language::CSharp => CSHARP_FN_KEYWORDS,
        Language::Swift => SWIFT_FN_KEYWORDS,
        Language::Kotlin => KOTLIN_FN_KEYWORDS,
        Language::Ruby => RUBY_FN_KEYWORDS,
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
                Language::Python | Language::Ruby => find_python_fn_end(&lines, i),
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

// File-operation sinks that take path arguments.
const PYTHON_SINKS: &[&str] = &[
    "open(",
    "os.remove(",
    "os.unlink(",
    "shutil.copy(",
    "pathlib.Path(",
];

const JS_SINKS: &[&str] = &[
    "fs.readFile(",
    "fs.readFileSync(",
    "fs.writeFile(",
    "fs.writeFileSync(",
    "fs.unlink(",
];

const RUST_SINKS: &[&str] = &["fs::read(", "fs::write(", "fs::remove_file(", "File::open("];

const JAVA_SINKS: &[&str] = &[
    "new File(",
    "Paths.get(",
    "FileInputStream(",
    "Files.readAllBytes(",
    "FileReader(",
    "FileOutputStream(",
];

const GO_SINKS: &[&str] = &[
    "os.Open(",
    "os.ReadFile(",
    "filepath.Join(",
    "os.Create(",
    "os.OpenFile(",
    "ioutil.ReadFile(",
];

const C_SINKS: &[&str] = &["fopen(", "open(", "stat(", "readdir(", "fread("];

const CSHARP_SINKS: &[&str] = &[
    "File.Open(",
    "File.ReadAllText(",
    "Path.Combine(",
    "StreamReader(",
    "File.ReadAllBytes(",
    "FileStream(",
];

const SWIFT_SINKS: &[&str] = &[
    "FileManager.default",
    "URL(fileURLWithPath:",
    "Data(contentsOf:",
    "String(contentsOfFile:",
];

const KOTLIN_SINKS: &[&str] = &[
    "File(",
    "Paths.get(",
    "FileInputStream(",
    "Files.readAllBytes(",
    "FileReader(",
];

const RUBY_SINKS: &[&str] = &[
    "File.open(",
    "File.read(",
    "FileUtils.",
    "IO.read(",
    "File.readlines(",
];

// User-input indicators per language.
const PYTHON_USER_INPUT: &[&str] = &[
    "request", "args", "form", "params", "query", "input", "argv", "sys.argv",
];
const JS_USER_INPUT: &[&str] = &["req.", "request", "params", "query", "body", "input"];
const RUST_USER_INPUT: &[&str] = &["user", "input", "request", "query", "args"];
const JAVA_USER_INPUT: &[&str] = &[
    "request", "getparameter", "getpathinfo", "getservletpath",
    "user_input", "args[", "params.",
];
const GO_USER_INPUT: &[&str] = &["r.url", "r.formvalue", "request", "user_input", "args[", "query.get"];
const C_USER_INPUT: &[&str] = &["argv", "user_input", "user", "buf", "request"];
const CSHARP_USER_INPUT: &[&str] = &[
    "request", "httpcontext", "user_input", "query[", "formcollection",
];
const SWIFT_USER_INPUT: &[&str] = &["request", "user_input", "params", "query", "urlcomponents"];
const KOTLIN_USER_INPUT: &[&str] = &[
    "request", "getparameter", "user_input", "args[", "params.",
];
const RUBY_USER_INPUT: &[&str] = &["params", "request", "input", "args", "ARGV"];

// Normalization calls for expression-level scanning (superset of the function-level ones).
const PYTHON_EXPR_NORM: &[&str] = &[
    "os.path.normpath",
    "os.path.realpath",
    "os.path.abspath",
    "pathlib.Path.resolve",
    ".resolve()",
    "secure_filename",
    "safe_join",
    "send_from_directory",
];

const JS_EXPR_NORM: &[&str] = &["path.normalize", "path.resolve", "sanitize", "basename"];

const RUST_EXPR_NORM: &[&str] = &["canonicalize", "normalize", "sanitize"];
const JAVA_EXPR_NORM: &[&str] = &[
    "normalize", "getcanonicalpath", "getcanonicalfile", "torealpath", "sanitize",
];
const GO_EXPR_NORM: &[&str] = &["filepath.clean", "filepath.abs", "filepath.evalsymlinks"];
const C_EXPR_NORM: &[&str] = &["realpath", "canonicalize_file_name", "sanitize"];
const CSHARP_EXPR_NORM: &[&str] = &["getfullpath", "path.getfullpath", "sanitize"];
const SWIFT_EXPR_NORM: &[&str] = &[
    "standardizedfileurl", "resolvingsymlinksinpath", "standardized", "sanitize",
];
const KOTLIN_EXPR_NORM: &[&str] = &[
    "normalize", "canonicalpath", "getcanonicalpath", "torealpath", "sanitize",
];
const RUBY_EXPR_NORM: &[&str] = &[
    "file.expand_path", "cleanpath", "realpath", "sanitize",
];

/// Returns true when the threat model indicates file-path arguments are
/// intentionally user-controlled (CLI tools, console tools, CI pipelines).
///
/// For these project types, reading a file from an argument the user passes
/// explicitly is the expected behaviour — not a path-traversal vulnerability.
/// Findings are still emitted but marked `noisy: true` so that default report
/// views can filter them out.
fn is_trusted_input_model(ctx: &AnalysisContext) -> bool {
    matches!(
        ctx.threat_model.model_type,
        Some(ThreatModelType::CliTool)
            | Some(ThreatModelType::ConsoleTool)
            | Some(ThreatModelType::CiPipeline)
    )
}

/// Returns true when `source` contains web-handler annotations or patterns
/// that indicate the file is part of a request-handling layer.
///
/// Used as a fallback when no threat model is configured: Rust `fs::` calls
/// inside web handler functions are genuinely suspicious; those outside are
/// almost always reading developer- or user-chosen config/target files.
fn has_web_handler_context(source: &str) -> bool {
    const WEB_MARKERS: &[&str] = &[
        "#[get(",
        "#[post(",
        "#[put(",
        "#[delete(",
        "#[patch(",
        "#[route(",
        "async fn handler",
        "async fn handle_request",
        "HttpRequest",
        "actix_web",
        "axum::",
        "warp::",
        "rocket::",
        "tide::",
    ];
    WEB_MARKERS.iter().any(|m| source.contains(m))
}

fn sinks_for(lang: Language) -> &'static [&'static str] {
    match lang {
        Language::Python => PYTHON_SINKS,
        Language::JavaScript => JS_SINKS,
        Language::Rust => RUST_SINKS,
        Language::Java => JAVA_SINKS,
        Language::Go => GO_SINKS,
        Language::C | Language::Cpp => C_SINKS,
        Language::CSharp => CSHARP_SINKS,
        Language::Swift => SWIFT_SINKS,
        Language::Kotlin => KOTLIN_SINKS,
        Language::Ruby => RUBY_SINKS,
        _ => &[],
    }
}

fn user_input_indicators(lang: Language) -> &'static [&'static str] {
    match lang {
        Language::Python => PYTHON_USER_INPUT,
        Language::JavaScript => JS_USER_INPUT,
        Language::Rust => RUST_USER_INPUT,
        Language::Java => JAVA_USER_INPUT,
        Language::Go => GO_USER_INPUT,
        Language::C | Language::Cpp => C_USER_INPUT,
        Language::CSharp => CSHARP_USER_INPUT,
        Language::Swift => SWIFT_USER_INPUT,
        Language::Kotlin => KOTLIN_USER_INPUT,
        Language::Ruby => RUBY_USER_INPUT,
        _ => &[],
    }
}

fn expr_norm_calls(lang: Language) -> &'static [&'static str] {
    match lang {
        Language::Python => PYTHON_EXPR_NORM,
        Language::JavaScript => JS_EXPR_NORM,
        Language::Rust => RUST_EXPR_NORM,
        Language::Java => JAVA_EXPR_NORM,
        Language::Go => GO_EXPR_NORM,
        Language::C | Language::Cpp => C_EXPR_NORM,
        Language::CSharp => CSHARP_EXPR_NORM,
        Language::Swift => SWIFT_EXPR_NORM,
        Language::Kotlin => KOTLIN_EXPR_NORM,
        Language::Ruby => RUBY_EXPR_NORM,
        _ => &[],
    }
}

/// Scan for file-operation sinks with user-input indicators in a window,
/// checking for normalization above the sink.
fn find_expression_sinks(
    lines: &[&str],
    lang: Language,
    path: &std::path::Path,
    detector_name: &str,
    noisy: bool,
) -> Vec<Finding> {
    let sinks = sinks_for(lang);
    let input_indicators = user_input_indicators(lang);
    let norm_calls = expr_norm_calls(lang);
    let mut findings = Vec::new();

    for (i, line) in lines.iter().enumerate() {
        let trimmed = line.trim();
        if is_comment(trimmed, lang) {
            continue;
        }

        // Check if any sink appears on this line.
        let has_sink = sinks.iter().any(|s| line.contains(s));
        if !has_sink {
            continue;
        }

        // Define a 5-line window around the sink (lines i-5..=i+5).
        let window_start = i.saturating_sub(5);
        let window_end = (i + 5).min(lines.len().saturating_sub(1));
        let window = &lines[window_start..=window_end];

        // Check for user-input indicators in the window.
        let has_user_input = window.iter().any(|wl| {
            let lower = wl.to_lowercase();
            input_indicators.iter().any(|ind| lower.contains(ind))
        });
        if !has_user_input {
            continue;
        }

        // Check for normalization in the 5 lines above the sink (inclusive).
        let norm_start = i.saturating_sub(5);
        let above = &lines[norm_start..=i];
        let has_norm = above.iter().any(|wl| {
            let lower = wl.to_lowercase();
            norm_calls.iter().any(|nc| lower.contains(nc))
                || VALIDATION_PATTERNS.iter().any(|vp| wl.contains(vp))
        });

        if has_norm {
            continue;
        }

        let line_1based = (i + 1) as u32;
        let (severity, finding_noisy) = if noisy {
            (Severity::Low, true)
        } else {
            (Severity::High, false)
        };
        findings.push(Finding {
            id: Uuid::new_v4(),
            detector: detector_name.into(),
            severity,
            category: FindingCategory::PathTraversal,
            file: path.to_path_buf(),
            line: Some(line_1based),
            title: format!("File operation with unsanitized input at line {line_1based}"),
            description: format!(
                "File operation at {}:{} uses a path that may come from user \
                 input without normalization or validation, risking path traversal.",
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
            cwe_ids: vec![22],
            noisy: finding_noisy,
            base_severity: None,
            coverage_confidence: None,
        });
    }

    findings
}

/// Check whether the given source slice contains any normalization or validation call.
fn has_normalization(body_lines: &[&str], lang: Language) -> bool {
    let norm_calls: &[&str] = match lang {
        Language::Python => PYTHON_NORM_CALLS,
        Language::JavaScript => JS_NORM_CALLS,
        Language::Rust => RUST_NORM_CALLS,
        Language::Java => JAVA_NORM_CALLS,
        Language::Go => GO_NORM_CALLS,
        Language::C | Language::Cpp => C_NORM_CALLS,
        Language::CSharp => CSHARP_NORM_CALLS,
        Language::Swift => SWIFT_NORM_CALLS,
        Language::Kotlin => KOTLIN_NORM_CALLS,
        Language::Ruby => RUBY_NORM_CALLS,
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

        // Bug 8: Skip path normalization checks for compiler/toolchain/sdk and vendor trees
        let root_str = ctx.target_root.to_string_lossy();
        if root_str.contains("compiler")
            || root_str.contains("toolchain")
            || root_str.contains("sdk")
        {
            return Ok(findings);
        }

        // Determine project-level noisiness once before the file loop.
        //
        // CLI tools, console tools, and CI pipelines intentionally accept file
        // paths from the user.  Flagging every path parameter produces hundreds
        // of spurious findings.  We still emit findings but mark them noisy so
        // default report views can filter them out.
        let project_is_trusted_input = is_trusted_input_model(ctx);

        for (path, source) in &ctx.source_cache {
            // Skip test files.
            if is_test_file(path) {
                continue;
            }

            // Bug 8: Skip vendor and third_party paths
            let path_str = path.to_string_lossy();
            if path_str.contains("vendor/")
                || path_str.contains("third_party/")
                || path_str.contains("vendor\\")
                || path_str.contains("third_party\\")
            {
                continue;
            }

            // Only scan languages we know about.
            let lang = ctx.language;
            if !matches!(
                lang,
                Language::Python
                    | Language::JavaScript
                    | Language::Rust
                    | Language::Java
                    | Language::Go
                    | Language::C
                    | Language::Cpp
                    | Language::CSharp
                    | Language::Swift
                    | Language::Kotlin
                    | Language::Ruby
            ) {
                continue;
            }

            // For Rust with no explicit threat model, fall back to a heuristic:
            // mark findings noisy when the file has no web-handler annotations.
            // Almost every path parameter in a CLI crate is a developer- or
            // user-chosen config/target file, not untrusted HTTP input.
            let file_noisy = project_is_trusted_input
                || (lang == Language::Rust
                    && ctx.threat_model.model_type.is_none()
                    && !has_web_handler_context(source));

            let lines: Vec<&str> = source.lines().collect();
            let ranges = collect_suspect_function_ranges(source, lang);

            for (fn_start, fn_end) in ranges {
                let body = &lines[fn_start..=fn_end.min(lines.len().saturating_sub(1))];

                if !has_normalization(body, lang) {
                    let line_1based = (fn_start + 1) as u32;
                    // Downgrade severity to Low for noisy findings so that
                    // severity-threshold filters suppress them even when the
                    // caller does not check the `noisy` flag.
                    let (severity, noisy) = if file_noisy {
                        (Severity::Low, true)
                    } else {
                        (Severity::Medium, false)
                    };
                    findings.push(Finding {
                        id: Uuid::new_v4(),
                        detector: self.name().into(),
                        severity,
                        category: FindingCategory::PathTraversal,
                        file: path.clone(),
                        line: Some(line_1based),
                        title: format!("Missing path/URL normalization at line {line_1based}"),
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
                        cwe_ids: vec![22],
                        noisy,
                        base_severity: None,
                        coverage_confidence: None,
                    });
                }
            }

            // Pass 2: expression-level file-operation sink scanning.
            let expr_findings =
                find_expression_sinks(&lines, lang, path, self.name(), file_noisy);
            findings.extend(expr_findings);
        }

        Ok(findings)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::DetectConfig;
    use apex_core::command::RealCommandRunner;
    use apex_core::config::{ThreatModelConfig, ThreatModelType};
    use apex_coverage::CoverageOracle;
    use std::collections::HashMap;
    use std::path::PathBuf;
    use std::sync::Arc;

    fn make_ctx_with_source(filename: &str, source: &str, lang: Language) -> AnalysisContext {
        let mut source_cache = HashMap::new();
        source_cache.insert(PathBuf::from(filename), source.to_string());

        AnalysisContext {
            language: lang,
            source_cache,
            ..AnalysisContext::test_default()
        }
    }

    fn make_ctx_with_threat_model(
        filename: &str,
        source: &str,
        lang: Language,
        model_type: ThreatModelType,
    ) -> AnalysisContext {
        let mut source_cache = HashMap::new();
        source_cache.insert(PathBuf::from(filename), source.to_string());

        AnalysisContext {
            language: lang,
            source_cache,
            threat_model: ThreatModelConfig {
                model_type: Some(model_type),
                ..ThreatModelConfig::default()
            },
            ..AnalysisContext::test_default()
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
        assert!(
            findings.is_empty(),
            "expected no findings, got: {findings:?}"
        );
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
        assert!(
            findings.is_empty(),
            "expected no findings, got: {findings:?}"
        );
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
        let ctx = make_ctx_with_source("src/server.js", src, Language::JavaScript);
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
        let ctx = make_ctx_with_source("src/server.js", src, Language::JavaScript);
        let findings = PathNormalizationDetector.analyze(&ctx).await.unwrap();
        assert!(
            findings.is_empty(),
            "expected no findings, got: {findings:?}"
        );
    }

    // ---- Expression-level path traversal tests ----

    // 8. Python: open() with user input nearby
    #[tokio::test]
    async fn detects_inline_open_with_request_input() {
        let src = "\
def download(request):
    path = request.args.get('file')
    data = open(path).read()
    return data
";
        let ctx = make_ctx_with_source("src/app.py", src, Language::Python);
        let findings = PathNormalizationDetector.analyze(&ctx).await.unwrap();
        assert!(
            !findings.is_empty(),
            "should detect open(path) with user input"
        );
    }

    // 9. Python: normpath suppresses expression-level finding
    #[tokio::test]
    async fn expr_no_finding_when_normpath_used() {
        let src = "\
def download(request):
    path = request.args.get('file')
    safe = os.path.normpath(path)
    data = open(safe).read()
    return data
";
        let ctx = make_ctx_with_source("src/app.py", src, Language::Python);
        let findings = PathNormalizationDetector.analyze(&ctx).await.unwrap();
        assert!(findings.is_empty(), "normpath should suppress finding");
    }

    // 10. JS: fs.readFileSync with req.params
    #[tokio::test]
    async fn detects_fs_readfile_with_req_params() {
        let src = "\
function serve(req, res) {
  const data = fs.readFileSync(req.params.path);
  res.send(data);
}
";
        let ctx = make_ctx_with_source("src/app.js", src, Language::JavaScript);
        let findings = PathNormalizationDetector.analyze(&ctx).await.unwrap();
        assert!(
            !findings.is_empty(),
            "should detect fs.readFileSync with req.params"
        );
    }

    // 11. JS: path.normalize suppresses
    #[tokio::test]
    async fn js_path_normalize_suppresses() {
        let src = "\
function serve(req, res) {
  const safe = path.normalize(req.params.file);
  const data = fs.readFileSync(safe);
  res.send(data);
}
";
        let ctx = make_ctx_with_source("src/app.js", src, Language::JavaScript);
        let findings = PathNormalizationDetector.analyze(&ctx).await.unwrap();
        assert!(findings.is_empty(), "path.normalize should suppress");
    }

    // 12. Rust: File::open with user input
    #[tokio::test]
    async fn detects_rust_file_open_with_user_input() {
        let src = "\
fn handle(input: &str) {
    let f = File::open(input);
}
";
        let ctx = make_ctx_with_source("src/handler.rs", src, Language::Rust);
        let findings = PathNormalizationDetector.analyze(&ctx).await.unwrap();
        assert!(
            !findings.is_empty(),
            "should detect File::open with user input"
        );
    }

    // 13. Test files should be skipped for expression-level too
    #[tokio::test]
    async fn no_expression_finding_in_test_file() {
        let src = "\
def test_open():
    path = request.args.get('file')
    open(path)
";
        let ctx = make_ctx_with_source("tests/test_app.py", src, Language::Python);
        let findings = PathNormalizationDetector.analyze(&ctx).await.unwrap();
        assert!(findings.is_empty());
    }

    // -----------------------------------------------------------------------
    // Bug 8: vendor/ and third_party/ paths should be skipped
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn skips_vendor_path() {
        let src = "\
def load(path):
    return open(path).read()
";
        let ctx = make_ctx_with_source("vendor/flask/app.py", src, Language::Python);
        let findings = PathNormalizationDetector.analyze(&ctx).await.unwrap();
        assert!(findings.is_empty(), "vendor/ files should be skipped");
    }

    #[tokio::test]
    async fn skips_third_party_path() {
        let src = "\
def load(path):
    return open(path).read()
";
        let ctx = make_ctx_with_source("third_party/lib/util.py", src, Language::Python);
        let findings = PathNormalizationDetector.analyze(&ctx).await.unwrap();
        assert!(findings.is_empty(), "third_party/ files should be skipped");
    }

    #[tokio::test]
    async fn non_vendor_path_still_detected() {
        let src = "\
def load(path):
    return open(path).read()
";
        let ctx = make_ctx_with_source("src/files.py", src, Language::Python);
        let findings = PathNormalizationDetector.analyze(&ctx).await.unwrap();
        assert!(
            !findings.is_empty(),
            "non-vendor production file should still be detected"
        );
    }

    // -----------------------------------------------------------------------
    // Java language tests
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn detects_java_file_read_without_normalization() {
        let src = "\
public void readFile(String path) {
    File f = new File(path);
    FileInputStream fis = new FileInputStream(f);
}
";
        let ctx = make_ctx_with_source("src/FileService.java", src, Language::Java);
        let findings = PathNormalizationDetector.analyze(&ctx).await.unwrap();
        assert!(
            !findings.is_empty(),
            "should detect Java File operation without normalization"
        );
    }

    #[tokio::test]
    async fn java_canonical_path_suppresses() {
        let src = "\
public void readFile(String path) {
    String safe = new File(path).getCanonicalPath();
    FileInputStream fis = new FileInputStream(safe);
}
";
        let ctx = make_ctx_with_source("src/FileService.java", src, Language::Java);
        let findings = PathNormalizationDetector.analyze(&ctx).await.unwrap();
        assert!(
            findings.is_empty(),
            "getCanonicalPath should suppress Java finding"
        );
    }

    // -----------------------------------------------------------------------
    // Go language tests
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn detects_go_file_open_without_normalization() {
        let src = "\
func readFile(path string) {
    f, err := os.Open(path)
    defer f.Close()
}
";
        let ctx = make_ctx_with_source("src/handler.go", src, Language::Go);
        let findings = PathNormalizationDetector.analyze(&ctx).await.unwrap();
        assert!(
            !findings.is_empty(),
            "should detect Go os.Open without normalization"
        );
    }

    #[tokio::test]
    async fn go_filepath_clean_suppresses() {
        let src = "\
func readFile(path string) {
    safe := filepath.Clean(path)
    f, err := os.Open(safe)
    defer f.Close()
}
";
        let ctx = make_ctx_with_source("src/handler.go", src, Language::Go);
        let findings = PathNormalizationDetector.analyze(&ctx).await.unwrap();
        assert!(
            findings.is_empty(),
            "filepath.Clean should suppress Go finding"
        );
    }

    // -----------------------------------------------------------------------
    // C/C++ language tests
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn detects_c_fopen_without_normalization() {
        let src = "\
void read_file(const char* path) {
    FILE* f = fopen(path, \"r\");
    fclose(f);
}
";
        let ctx = make_ctx_with_source("src/file.c", src, Language::C);
        let findings = PathNormalizationDetector.analyze(&ctx).await.unwrap();
        assert!(
            !findings.is_empty(),
            "should detect C fopen without normalization"
        );
    }

    #[tokio::test]
    async fn c_realpath_suppresses() {
        let src = "\
void read_file(const char* path) {
    char resolved[PATH_MAX];
    realpath(path, resolved);
    FILE* f = fopen(resolved, \"r\");
    fclose(f);
}
";
        let ctx = make_ctx_with_source("src/file.c", src, Language::C);
        let findings = PathNormalizationDetector.analyze(&ctx).await.unwrap();
        assert!(
            findings.is_empty(),
            "realpath should suppress C finding"
        );
    }

    #[tokio::test]
    async fn detects_cpp_fopen_without_normalization() {
        let src = "\
void read_file(const char* path) {
    FILE* f = fopen(path, \"r\");
}
";
        let ctx = make_ctx_with_source("src/file.cpp", src, Language::Cpp);
        let findings = PathNormalizationDetector.analyze(&ctx).await.unwrap();
        assert!(
            !findings.is_empty(),
            "should detect C++ fopen without normalization"
        );
    }

    // -----------------------------------------------------------------------
    // C# language tests
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn detects_csharp_file_read_without_normalization() {
        let src = "\
public void ReadFile(string path) {
    var content = File.ReadAllText(path);
}
";
        let ctx = make_ctx_with_source("src/FileService.cs", src, Language::CSharp);
        let findings = PathNormalizationDetector.analyze(&ctx).await.unwrap();
        assert!(
            !findings.is_empty(),
            "should detect C# File.ReadAllText without normalization"
        );
    }

    #[tokio::test]
    async fn csharp_get_full_path_suppresses() {
        let src = "\
public void ReadFile(string filePath) {
    var safe = Path.GetFullPath(filePath);
    var content = File.ReadAllText(safe);
}
";
        let ctx = make_ctx_with_source("src/FileService.cs", src, Language::CSharp);
        let findings = PathNormalizationDetector.analyze(&ctx).await.unwrap();
        assert!(
            findings.is_empty(),
            "Path.GetFullPath should suppress C# finding"
        );
    }

    // -----------------------------------------------------------------------
    // Swift language tests
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn detects_swift_file_read_without_normalization() {
        let src = "\
func readFile(path: String) {
    let url = URL(fileURLWithPath: path)
    let data = try Data(contentsOf: url)
}
";
        let ctx = make_ctx_with_source("Sources/FileService.swift", src, Language::Swift);
        let findings = PathNormalizationDetector.analyze(&ctx).await.unwrap();
        assert!(
            !findings.is_empty(),
            "should detect Swift file read without normalization"
        );
    }

    #[tokio::test]
    async fn swift_standardized_suppresses() {
        let src = "\
func readFile(path: String) {
    let url = URL(fileURLWithPath: path).standardizedFileURL
    let data = try Data(contentsOf: url)
}
";
        let ctx = make_ctx_with_source("Sources/FileService.swift", src, Language::Swift);
        let findings = PathNormalizationDetector.analyze(&ctx).await.unwrap();
        assert!(
            findings.is_empty(),
            "standardizedFileURL should suppress Swift finding"
        );
    }

    // -----------------------------------------------------------------------
    // Kotlin language tests
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn detects_kotlin_file_read_without_normalization() {
        let src = "\
fun readFile(path: String) {
    val f = File(path)
    val content = FileInputStream(f)
}
";
        let ctx = make_ctx_with_source("src/FileService.kt", src, Language::Kotlin);
        let findings = PathNormalizationDetector.analyze(&ctx).await.unwrap();
        assert!(
            !findings.is_empty(),
            "should detect Kotlin file read without normalization"
        );
    }

    #[tokio::test]
    async fn kotlin_canonical_path_suppresses() {
        let src = "\
fun readFile(path: String) {
    val f = File(path)
    val safe = f.canonicalPath
    val content = FileInputStream(safe)
}
";
        let ctx = make_ctx_with_source("src/FileService.kt", src, Language::Kotlin);
        let findings = PathNormalizationDetector.analyze(&ctx).await.unwrap();
        assert!(
            findings.is_empty(),
            "canonicalPath should suppress Kotlin finding"
        );
    }

    // -----------------------------------------------------------------------
    // Ruby language tests
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn detects_ruby_file_read_without_normalization() {
        let src = "\
def read_file(path)
  content = File.read(path)
  content
";
        let ctx = make_ctx_with_source("src/file_service.rb", src, Language::Ruby);
        let findings = PathNormalizationDetector.analyze(&ctx).await.unwrap();
        assert!(
            !findings.is_empty(),
            "should detect Ruby File.read without normalization"
        );
    }

    #[tokio::test]
    async fn ruby_expand_path_suppresses() {
        let src = "\
def read_file(path)
  safe = File.expand_path(path)
  content = File.read(safe)
  content
";
        let ctx = make_ctx_with_source("src/file_service.rb", src, Language::Ruby);
        let findings = PathNormalizationDetector.analyze(&ctx).await.unwrap();
        assert!(
            findings.is_empty(),
            "File.expand_path should suppress Ruby finding"
        );
    }

    // -----------------------------------------------------------------------
    // Expression-level tests for new languages
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn detects_java_expr_sink_with_user_input() {
        let src = "\
public void download(HttpServletRequest request) {
    String path = request.getParameter(\"file\");
    FileInputStream fis = new FileInputStream(path);
}
";
        let ctx = make_ctx_with_source("src/Servlet.java", src, Language::Java);
        let findings = PathNormalizationDetector.analyze(&ctx).await.unwrap();
        assert!(
            !findings.is_empty(),
            "should detect Java FileInputStream with request input"
        );
    }

    #[tokio::test]
    async fn detects_go_expr_sink_with_user_input() {
        let src = "\
func handler(w http.ResponseWriter, r *http.Request) {
    path := r.URL.Query().Get(\"file\")
    f, _ := os.Open(path)
}
";
        let ctx = make_ctx_with_source("src/handler.go", src, Language::Go);
        let findings = PathNormalizationDetector.analyze(&ctx).await.unwrap();
        assert!(
            !findings.is_empty(),
            "should detect Go os.Open with request input"
        );
    }

    #[tokio::test]
    async fn detects_csharp_expr_sink_with_user_input() {
        let src = "\
public void Download(HttpContext context) {
    var path = Request.Query[\"file\"];
    var content = File.ReadAllText(path);
}
";
        let ctx = make_ctx_with_source("src/Controller.cs", src, Language::CSharp);
        let findings = PathNormalizationDetector.analyze(&ctx).await.unwrap();
        assert!(
            !findings.is_empty(),
            "should detect C# File.ReadAllText with Request input"
        );
    }

    #[tokio::test]
    async fn detects_ruby_expr_sink_with_user_input() {
        let src = "\
def download
  path = params[:file]
  content = File.read(path)
  send_data content
";
        let ctx = make_ctx_with_source("src/controller.rb", src, Language::Ruby);
        let findings = PathNormalizationDetector.analyze(&ctx).await.unwrap();
        assert!(
            !findings.is_empty(),
            "should detect Ruby File.read with params input"
        );
    }

    // -----------------------------------------------------------------------
    // Threat-model awareness tests
    // -----------------------------------------------------------------------

    // CliTool threat model: path findings should be Low severity and noisy.
    #[tokio::test]
    async fn cli_tool_threat_model_findings_are_low_and_noisy() {
        let src = "\
fn read_file(path: &Path) -> Vec<u8> {
    fs::read(path).unwrap()
}
";
        let ctx = make_ctx_with_threat_model(
            "src/main.rs",
            src,
            Language::Rust,
            ThreatModelType::CliTool,
        );
        let findings = PathNormalizationDetector.analyze(&ctx).await.unwrap();
        assert!(!findings.is_empty(), "should still emit findings for CLI tools");
        for f in &findings {
            assert_eq!(f.severity, Severity::Low, "CLI tool findings must be Low");
            assert!(f.noisy, "CLI tool findings must be noisy");
        }
    }

    // WebService threat model: path findings should be High/Medium and not noisy.
    #[tokio::test]
    async fn web_service_threat_model_findings_are_high_and_not_noisy() {
        let src = "\
fn serve_file(path: &Path) -> Vec<u8> {
    fs::read(path).unwrap()
}
";
        let ctx = make_ctx_with_threat_model(
            "src/handler.rs",
            src,
            Language::Rust,
            ThreatModelType::WebService,
        );
        let findings = PathNormalizationDetector.analyze(&ctx).await.unwrap();
        assert!(!findings.is_empty(), "web service findings must not be suppressed");
        for f in &findings {
            assert!(!f.noisy, "web service findings must not be noisy");
            assert_ne!(
                f.severity,
                Severity::Low,
                "web service findings must not be downgraded to Low"
            );
        }
    }

    // No threat model, Rust code, no web handler annotations: Low + noisy.
    #[tokio::test]
    async fn rust_no_threat_model_no_web_handlers_is_noisy() {
        let src = "\
fn load_config(path: &Path) -> String {
    fs::read_to_string(path).unwrap()
}
";
        let ctx = make_ctx_with_source("src/config.rs", src, Language::Rust);
        let findings = PathNormalizationDetector.analyze(&ctx).await.unwrap();
        assert!(!findings.is_empty(), "should emit findings");
        for f in &findings {
            assert!(f.noisy, "Rust CLI-like code without threat model should be noisy");
            assert_eq!(f.severity, Severity::Low);
        }
    }

    // Python with no threat model: High (could be a web app).
    #[tokio::test]
    async fn python_no_threat_model_is_high_not_noisy() {
        let src = "\
def serve(path):
    data = open(path).read()
    return data
";
        let ctx = make_ctx_with_source("src/views.py", src, Language::Python);
        let findings = PathNormalizationDetector.analyze(&ctx).await.unwrap();
        assert!(!findings.is_empty(), "should emit findings for Python");
        // Python has no Rust heuristic fallback, so should not be noisy
        for f in &findings {
            assert!(!f.noisy, "Python with no threat model should not be noisy");
        }
    }
}
