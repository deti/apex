//! Multi-language path traversal detector (CWE-22).
//!
//! Catches unsanitized file path operations across all 11 supported languages
//! where user-controlled input may reach filesystem access functions.

use apex_core::config::ThreatModelType;
use apex_core::error::Result;
use apex_core::types::Language;
use async_trait::async_trait;
use regex::Regex;
use std::sync::LazyLock;
use uuid::Uuid;

use super::util::{is_comment, is_test_file};
use crate::context::AnalysisContext;
use crate::finding::{Finding, FindingCategory, Severity};
use crate::Detector;

pub struct MultiPathTraversalDetector;

struct LangPattern {
    lang: Language,
    name: &'static str,
    regex: Regex,
    description: &'static str,
}

/// Sanitization indicators that suggest path is validated.
const PATH_SANITIZATION: &[&str] = &[
    "resolve",
    "realpath",
    "abspath",
    "normpath",
    "canonicalize",
    "canonical",
    "clean",
    "sanitize",
    "validate",
];

static PATTERNS: LazyLock<Vec<LangPattern>> = LazyLock::new(|| {
    vec![
        // ── Python ──────────────────────────────────────────────────
        LangPattern {
            lang: Language::Python,
            name: "open with variable",
            regex: Regex::new(r#"open\(\s*[a-zA-Z_][a-zA-Z0-9_.]*\s*[,)]"#).unwrap(),
            description: "File open with potentially user-controlled path",
        },
        LangPattern {
            lang: Language::Python,
            name: "os.path.join",
            regex: Regex::new(r#"os\.path\.join\([^)]*[a-zA-Z_][a-zA-Z0-9_.]*[^)]*\)"#).unwrap(),
            description: "Path join with potentially user-controlled component",
        },
        LangPattern {
            lang: Language::Python,
            name: "pathlib Path",
            regex: Regex::new(r#"Path\(\s*[a-zA-Z_][a-zA-Z0-9_.]*\s*\)"#).unwrap(),
            description: "Path construction with potentially user-controlled input",
        },
        LangPattern {
            lang: Language::Python,
            name: "send_file/send_from_directory",
            regex: Regex::new(r#"send_(?:file|from_directory)\s*\(\s*[a-zA-Z_]"#).unwrap(),
            description: "Flask file serving with potentially user-controlled path",
        },
        // ── JavaScript ──────────────────────────────────────────────
        LangPattern {
            lang: Language::JavaScript,
            name: "fs.readFile",
            regex: Regex::new(r#"fs\.(?:readFile|writeFile|readFileSync|writeFileSync|createReadStream|createWriteStream)\s*\(\s*[a-zA-Z_]"#).unwrap(),
            description: "File operation with potentially user-controlled path",
        },
        LangPattern {
            lang: Language::JavaScript,
            name: "path.join with req",
            regex: Regex::new(r#"path\.(?:join|resolve)\s*\([^)]*(?:req\.|params|query|body)"#).unwrap(),
            description: "Path construction with request input",
        },
        LangPattern {
            lang: Language::JavaScript,
            name: "res.sendFile",
            regex: Regex::new(r#"res\.sendFile\s*\(\s*[a-zA-Z_]"#).unwrap(),
            description: "Express sendFile with potentially user-controlled path",
        },
        // ── Java ────────────────────────────────────────────────────
        LangPattern {
            lang: Language::Java,
            name: "new File",
            regex: Regex::new(r#"new\s+File\s*\(\s*[a-zA-Z_]"#).unwrap(),
            description: "File construction with potentially user-controlled path",
        },
        LangPattern {
            lang: Language::Java,
            name: "Paths.get",
            regex: Regex::new(r#"Paths\.get\s*\(\s*[a-zA-Z_]"#).unwrap(),
            description: "Path construction with potentially user-controlled input",
        },
        LangPattern {
            lang: Language::Java,
            name: "FileInputStream",
            regex: Regex::new(r#"new\s+FileInputStream\s*\(\s*[a-zA-Z_]"#).unwrap(),
            description: "FileInputStream with potentially user-controlled path",
        },
        // ── Go ──────────────────────────────────────────────────────
        LangPattern {
            lang: Language::Go,
            name: "os.Open/ReadFile",
            regex: Regex::new(r#"os\.(?:Open|ReadFile|Create|OpenFile)\s*\(\s*[a-zA-Z_]"#).unwrap(),
            description: "File operation with potentially user-controlled path",
        },
        LangPattern {
            lang: Language::Go,
            name: "filepath.Join",
            regex: Regex::new(r#"filepath\.Join\s*\([^)]*[a-zA-Z_]"#).unwrap(),
            description: "Path construction with potentially user-controlled component",
        },
        LangPattern {
            lang: Language::Go,
            name: "http.ServeFile",
            regex: Regex::new(r#"http\.ServeFile\s*\([^,]*,\s*[^,]*,\s*[a-zA-Z_]"#).unwrap(),
            description: "HTTP file serving with potentially user-controlled path",
        },
        // ── Ruby ────────────────────────────────────────────────────
        LangPattern {
            lang: Language::Ruby,
            name: "File.open/read",
            regex: Regex::new(r#"File\.(?:open|read|write|new)\s*\(\s*[a-zA-Z_]"#).unwrap(),
            description: "File operation with potentially user-controlled path",
        },
        LangPattern {
            lang: Language::Ruby,
            name: "send_file",
            regex: Regex::new(r#"send_file\s*\(\s*[a-zA-Z_]"#).unwrap(),
            description: "Rails send_file with potentially user-controlled path",
        },
        // ── C# ──────────────────────────────────────────────────────
        LangPattern {
            lang: Language::CSharp,
            name: "File.Read/Open",
            regex: Regex::new(r#"File\.(?:ReadAllText|ReadAllBytes|Open|OpenRead|OpenWrite|WriteAllText)\s*\(\s*[a-zA-Z_]"#).unwrap(),
            description: "File operation with potentially user-controlled path",
        },
        LangPattern {
            lang: Language::CSharp,
            name: "Path.Combine",
            regex: Regex::new(r#"Path\.Combine\s*\([^)]*[a-zA-Z_]"#).unwrap(),
            description: "Path construction with potentially user-controlled component",
        },
        // ── Rust ────────────────────────────────────────────────────
        LangPattern {
            lang: Language::Rust,
            name: "std::fs operations",
            regex: Regex::new(r#"(?:std::fs|fs)::(?:read_to_string|read|write|File::open|File::create)\s*\(\s*[a-zA-Z_]"#).unwrap(),
            description: "File operation with potentially user-controlled path",
        },
        LangPattern {
            lang: Language::Rust,
            name: "PathBuf::from",
            regex: Regex::new(r#"PathBuf::from\s*\(\s*[a-zA-Z_]"#).unwrap(),
            description: "Path construction with potentially user-controlled input",
        },
        // ── C ───────────────────────────────────────────────────────
        LangPattern {
            lang: Language::C,
            name: "fopen",
            regex: Regex::new(r#"fopen\s*\(\s*[a-zA-Z_]"#).unwrap(),
            description: "File open with potentially user-controlled path",
        },
        // ── C++ ─────────────────────────────────────────────────────
        LangPattern {
            lang: Language::Cpp,
            name: "fstream/fopen",
            regex: Regex::new(r#"(?:fopen|ifstream|ofstream|fstream)\s*[\(]\s*[a-zA-Z_]"#).unwrap(),
            description: "File open with potentially user-controlled path",
        },
        // ── Swift ───────────────────────────────────────────────────
        LangPattern {
            lang: Language::Swift,
            name: "FileManager",
            regex: Regex::new(r#"FileManager\.\w+\.contents\s*\(atPath:\s*[a-zA-Z_]"#).unwrap(),
            description: "FileManager operation with potentially user-controlled path",
        },
        // ── Kotlin ──────────────────────────────────────────────────
        LangPattern {
            lang: Language::Kotlin,
            name: "File constructor",
            regex: Regex::new(r#"File\s*\(\s*[a-zA-Z_]"#).unwrap(),
            description: "File construction with potentially user-controlled path",
        },
    ]
});

/// Returns true when the surrounding context has sanitization.
fn has_sanitization(line: &str) -> bool {
    let lower = line.to_lowercase();
    PATH_SANITIZATION.iter().any(|s| lower.contains(s))
}

/// Safe variable prefixes suggesting non-user-input.
fn is_safe_variable(line: &str) -> bool {
    const SAFE_PREFIXES: &[&str] = &[
        "self.", "config.", "settings.", "BASE_", "ROOT_", "APP_",
        "__file__", "__dirname",
    ];
    SAFE_PREFIXES.iter().any(|p| line.contains(p))
}

/// Returns true when the threat model indicates file-path arguments are
/// intentionally user-controlled (CLI tools, console tools, CI pipelines).
///
/// For these project types, reading a file from an argument the user passes
/// explicitly is the expected behaviour — not a path-traversal vulnerability.
/// Findings are still emitted but marked `noisy: true` so they can be
/// filtered in downstream reporting without being silently dropped.
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
/// almost always reading developer-controlled paths.
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

#[async_trait]
impl Detector for MultiPathTraversalDetector {
    fn name(&self) -> &str {
        "multi-path-traversal"
    }

    async fn analyze(&self, ctx: &AnalysisContext) -> Result<Vec<Finding>> {
        if ctx.language == Language::Wasm {
            return Ok(Vec::new());
        }

        // Determine the project-level noisiness once, before the file loop.
        //
        // CLI tools, console tools, and CI pipelines intentionally accept
        // file paths from the user; flagging every `fs::read_to_string(path)`
        // produces hundreds of spurious HIGH findings.  We still emit findings
        // (so the user sees them if they look) but mark them `noisy: true` so
        // that default report views can filter them out.
        let project_is_trusted_input = is_trusted_input_model(ctx);

        let mut findings = Vec::new();

        for (path, source) in &ctx.source_cache {
            if is_test_file(path) {
                continue;
            }

            // For Rust with no explicit threat model, fall back to a
            // heuristic: mark findings noisy when the file has no web-handler
            // annotations.  Almost every `fs::` call in a CLI crate is
            // reading a developer- or user-chosen config/target file, not
            // processing untrusted HTTP input.
            let file_noisy = project_is_trusted_input
                || (ctx.language == Language::Rust
                    && ctx.threat_model.model_type.is_none()
                    && !has_web_handler_context(source));

            for (line_num, line) in source.lines().enumerate() {
                let trimmed = line.trim();

                if is_comment(trimmed, ctx.language) {
                    continue;
                }

                if has_sanitization(trimmed) || is_safe_variable(trimmed) {
                    continue;
                }

                for pattern in PATTERNS.iter() {
                    if pattern.lang != ctx.language {
                        continue;
                    }

                    if pattern.regex.is_match(trimmed) {
                        let line_1based = (line_num + 1) as u32;

                        // Downgrade severity to Low for noisy findings so
                        // that severity-threshold filters suppress them even
                        // when the caller does not check the `noisy` flag.
                        let (severity, noisy) = if file_noisy {
                            (Severity::Low, true)
                        } else {
                            (Severity::High, false)
                        };

                        findings.push(Finding {
                            id: Uuid::new_v4(),
                            detector: self.name().into(),
                            severity,
                            category: FindingCategory::PathTraversal,
                            file: path.clone(),
                            line: Some(line_1based),
                            title: format!(
                                "{}: {} at line {}",
                                pattern.name, pattern.description, line_1based
                            ),
                            description: format!(
                                "{} pattern matched in {}:{}",
                                pattern.name,
                                path.display(),
                                line_1based
                            ),
                            evidence: super::util::reachability_evidence(ctx, path, line_1based),
                            covered: false,
                            suggestion:
                                "Validate and canonicalize file paths before use. \
                                 Ensure paths cannot escape the intended directory."
                                    .into(),
                            explanation: None,
                            fix: None,
                            cwe_ids: vec![22],
                            noisy,
                            base_severity: None,
                            coverage_confidence: None,
                        });
                        break;
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
    use crate::context::AnalysisContext;
    use apex_core::config::{ThreatModelConfig, ThreatModelType};
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

    fn make_ctx_with_threat_model(
        files: HashMap<PathBuf, String>,
        lang: Language,
        model_type: ThreatModelType,
    ) -> AnalysisContext {
        AnalysisContext {
            language: lang,
            source_cache: files,
            threat_model: ThreatModelConfig {
                model_type: Some(model_type),
                ..ThreatModelConfig::default()
            },
            ..AnalysisContext::test_default()
        }
    }

    fn single_file(name: &str, content: &str) -> HashMap<PathBuf, String> {
        let mut m = HashMap::new();
        m.insert(PathBuf::from(name), content.into());
        m
    }

    #[tokio::test]
    async fn detects_python_open_variable() {
        let files = single_file("src/app.py", "data = open(user_path, 'r')\n");
        let ctx = make_ctx(files, Language::Python);
        let findings = MultiPathTraversalDetector.analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].cwe_ids, vec![22]);
    }

    #[tokio::test]
    async fn detects_java_new_file() {
        let files = single_file("src/App.java", "File f = new File(userPath)\n");
        let ctx = make_ctx(files, Language::Java);
        let findings = MultiPathTraversalDetector.analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 1);
    }

    #[tokio::test]
    async fn detects_go_os_open() {
        let files = single_file("src/main.go", "f, err := os.Open(userPath)\n");
        let ctx = make_ctx(files, Language::Go);
        let findings = MultiPathTraversalDetector.analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 1);
    }

    // Rust without a threat model and without web handler context: finding
    // is emitted but flagged noisy + Low severity (heuristic suppression).
    #[tokio::test]
    async fn rust_fs_read_no_threat_model_is_noisy() {
        let files = single_file("src/main.rs", "let data = fs::read_to_string(user_path)?;\n");
        let ctx = make_ctx(files, Language::Rust);
        let findings = MultiPathTraversalDetector.analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 1);
        assert!(findings[0].noisy, "should be noisy for Rust CLI-like code");
        assert_eq!(findings[0].severity, Severity::Low);
    }

    // Rust with explicit WebService threat model: finding is High and not noisy.
    #[tokio::test]
    async fn rust_fs_read_web_service_threat_model_is_high() {
        let files = single_file("src/handler.rs", "let data = fs::read_to_string(user_path)?;\n");
        let ctx = make_ctx_with_threat_model(files, Language::Rust, ThreatModelType::WebService);
        let findings = MultiPathTraversalDetector.analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 1);
        assert!(!findings[0].noisy, "web service findings must not be noisy");
        assert_eq!(findings[0].severity, Severity::High);
    }

    // Rust with CliTool threat model: finding is noisy + Low.
    #[tokio::test]
    async fn rust_fs_read_cli_tool_threat_model_is_noisy() {
        let files = single_file("src/main.rs", "let data = fs::read_to_string(path)?;\n");
        let ctx = make_ctx_with_threat_model(files, Language::Rust, ThreatModelType::CliTool);
        let findings = MultiPathTraversalDetector.analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 1);
        assert!(findings[0].noisy);
        assert_eq!(findings[0].severity, Severity::Low);
    }

    // Rust with ConsoleTool threat model: also noisy.
    #[tokio::test]
    async fn rust_fs_read_console_tool_threat_model_is_noisy() {
        let files = single_file("src/main.rs", "let data = fs::read_to_string(path)?;\n");
        let ctx =
            make_ctx_with_threat_model(files, Language::Rust, ThreatModelType::ConsoleTool);
        let findings = MultiPathTraversalDetector.analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 1);
        assert!(findings[0].noisy);
        assert_eq!(findings[0].severity, Severity::Low);
    }

    // Rust with CiPipeline threat model: also noisy.
    #[tokio::test]
    async fn rust_fs_read_ci_pipeline_threat_model_is_noisy() {
        let files = single_file("src/main.rs", "let data = fs::read_to_string(path)?;\n");
        let ctx =
            make_ctx_with_threat_model(files, Language::Rust, ThreatModelType::CiPipeline);
        let findings = MultiPathTraversalDetector.analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 1);
        assert!(findings[0].noisy);
        assert_eq!(findings[0].severity, Severity::Low);
    }

    // Non-Rust languages (Python) with CliTool: also noisy + Low.
    #[tokio::test]
    async fn python_cli_tool_threat_model_is_noisy() {
        let files = single_file("src/cli.py", "data = open(user_path, 'r')\n");
        let ctx = make_ctx_with_threat_model(files, Language::Python, ThreatModelType::CliTool);
        let findings = MultiPathTraversalDetector.analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 1);
        assert!(findings[0].noisy);
        assert_eq!(findings[0].severity, Severity::Low);
    }

    // Non-Rust languages (Python) with WebService: High and not noisy.
    #[tokio::test]
    async fn python_web_service_threat_model_is_high() {
        let files = single_file("src/views.py", "data = open(user_path, 'r')\n");
        let ctx =
            make_ctx_with_threat_model(files, Language::Python, ThreatModelType::WebService);
        let findings = MultiPathTraversalDetector.analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 1);
        assert!(!findings[0].noisy);
        assert_eq!(findings[0].severity, Severity::High);
    }

    // Rust without threat model but with web-handler annotations: High, not noisy.
    #[tokio::test]
    async fn rust_web_handler_context_no_threat_model_is_high() {
        let source = "#[post(\"/upload\")]\nasync fn handler(req: HttpRequest) {\n    let data = fs::read_to_string(user_path)?;\n}\n";
        let files = single_file("src/routes.rs", source);
        let ctx = make_ctx(files, Language::Rust);
        let findings = MultiPathTraversalDetector.analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 1);
        assert!(!findings[0].noisy, "web handler context should not be noisy");
        assert_eq!(findings[0].severity, Severity::High);
    }

    // Library threat model: file ops are untrusted — High, not noisy.
    #[tokio::test]
    async fn rust_library_threat_model_is_high() {
        let files = single_file("src/lib.rs", "let data = fs::read_to_string(path)?;\n");
        let ctx = make_ctx_with_threat_model(files, Language::Rust, ThreatModelType::Library);
        let findings = MultiPathTraversalDetector.analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 1);
        assert!(!findings[0].noisy);
        assert_eq!(findings[0].severity, Severity::High);
    }

    #[tokio::test]
    async fn skips_sanitized_path() {
        let files = single_file("src/app.py", "safe = os.path.realpath(open(user_path))\n");
        let ctx = make_ctx(files, Language::Python);
        let findings = MultiPathTraversalDetector.analyze(&ctx).await.unwrap();
        assert!(findings.is_empty());
    }

    #[tokio::test]
    async fn skips_test_files() {
        let files = single_file("tests/test_path.py", "open(user_path, 'r')\n");
        let ctx = make_ctx(files, Language::Python);
        let findings = MultiPathTraversalDetector.analyze(&ctx).await.unwrap();
        assert!(findings.is_empty());
    }

    #[test]
    fn does_not_use_cargo_subprocess() {
        assert!(!MultiPathTraversalDetector.uses_cargo_subprocess());
    }
}
