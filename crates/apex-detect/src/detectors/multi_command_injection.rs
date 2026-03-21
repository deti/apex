//! Multi-language command injection detector (CWE-78).
//!
//! Catches shell command execution sinks across all 11 supported languages
//! where unsanitized input may reach the shell.

use apex_core::config::ThreatModelType;
use apex_core::error::Result;
use apex_core::types::Language;
use async_trait::async_trait;
use regex::Regex;
use std::sync::LazyLock;
use uuid::Uuid;

use super::util::{is_comment, is_test_file, taint_reaches_sink};
use crate::context::AnalysisContext;
use crate::finding::{Finding, FindingCategory, Severity};
use crate::Detector;

/// Returns true when the project is a CLI or console tool.
///
/// CLI/console tools deliberately spawn subprocesses — compilers, test runners,
/// linters, coverage tools, etc.  Every `Command::new()`-equivalent line in
/// such a project is intentional.  Findings are still emitted so the user can
/// see them, but they are marked `noisy: true` and downgraded to `Severity::Low`
/// so that default report views can filter them without silently dropping them.
fn is_cli_threat_model(ctx: &AnalysisContext) -> bool {
    matches!(
        ctx.threat_model.model_type,
        Some(ThreatModelType::CliTool) | Some(ThreatModelType::ConsoleTool)
    )
}

pub struct MultiCommandInjectionDetector;

struct LangPattern {
    lang: Language,
    name: &'static str,
    regex: Regex,
    description: &'static str,
}

static PATTERNS: LazyLock<Vec<LangPattern>> = LazyLock::new(|| {
    vec![
        // ── Python ──────────────────────────────────────────────────
        LangPattern {
            lang: Language::Python,
            name: "subprocess with shell",
            regex: Regex::new(r#"subprocess\.\w+\s*\(.*shell\s*=\s*True"#).unwrap(),
            description: "Subprocess call with shell=True may execute unsanitized input",
        },
        LangPattern {
            lang: Language::Python,
            name: "os.system",
            regex: Regex::new(r#"os\.system\s*\(\s*[a-zA-Z_\"]"#).unwrap(),
            description: "os.system passes command to shell",
        },
        LangPattern {
            lang: Language::Python,
            name: "os.popen",
            regex: Regex::new(r#"os\.popen\s*\("#).unwrap(),
            description: "os.popen passes command to shell",
        },
        LangPattern {
            lang: Language::Python,
            name: "subprocess.Popen shell",
            regex: Regex::new(r#"Popen\s*\(.*shell\s*=\s*True"#).unwrap(),
            description: "Popen with shell=True may execute unsanitized input",
        },
        LangPattern {
            lang: Language::Python,
            name: "subprocess call",
            regex: Regex::new(r#"subprocess\.(?:call|run)\s*\(\s*(?:f["']|[a-zA-Z_])"#).unwrap(),
            description: "Subprocess call with potentially unsanitized input",
        },
        // ── JavaScript ──────────────────────────────────────────────
        LangPattern {
            lang: Language::JavaScript,
            name: "child_process exec/execSync",
            regex: Regex::new(
                r#"(?:child_process\s*[\.\)]\s*)?(?:exec|execSync)\s*\(\s*(?:`[^`]*\$\{|[a-zA-Z_])"#,
            )
            .unwrap(),
            description: "Shell command executed with potentially untrusted input",
        },
        LangPattern {
            lang: Language::JavaScript,
            name: "require child_process exec",
            regex: Regex::new(
                r#"require\s*\(\s*['"]child_process['"]\s*\)\s*\.exec\s*\("#,
            )
            .unwrap(),
            description: "Direct child_process.exec call",
        },
        LangPattern {
            lang: Language::JavaScript,
            name: "shelljs exec",
            regex: Regex::new(r#"shelljs\.exec\s*\(\s*(?:`[^`]*\$\{|[a-zA-Z_])"#).unwrap(),
            description: "shelljs.exec with potentially untrusted input",
        },
        LangPattern {
            lang: Language::JavaScript,
            name: "template literal in exec",
            regex: Regex::new(r#"exec\s*\(\s*`[^`]*\$\{[^}]+\}[^`]*`"#).unwrap(),
            description: "Shell command built with template literal interpolation",
        },
        // ── Java ────────────────────────────────────────────────────
        LangPattern {
            lang: Language::Java,
            name: "Runtime.exec",
            regex: Regex::new(r#"Runtime\.getRuntime\s*\(\s*\)\s*\.exec\s*\(\s*[a-zA-Z_]"#)
                .unwrap(),
            description: "Runtime.exec with potentially unsanitized input",
        },
        LangPattern {
            lang: Language::Java,
            name: "ProcessBuilder",
            regex: Regex::new(r#"(?:new\s+)?ProcessBuilder\s*\(\s*[a-zA-Z_]"#).unwrap(),
            description: "ProcessBuilder with potentially unsanitized input",
        },
        // ── Go ──────────────────────────────────────────────────────
        LangPattern {
            lang: Language::Go,
            name: "exec.Command",
            regex: Regex::new(r#"exec\.Command(?:Context)?\s*\(\s*[a-zA-Z_]"#).unwrap(),
            description: "exec.Command with potentially unsanitized input",
        },
        LangPattern {
            lang: Language::Go,
            name: "syscall.Exec",
            regex: Regex::new(r#"syscall\.Exec\s*\("#).unwrap(),
            description: "syscall.Exec passes command directly to OS",
        },
        // ── Ruby ────────────────────────────────────────────────────
        LangPattern {
            lang: Language::Ruby,
            name: "system/exec/spawn",
            regex: Regex::new(r##"(?:system|exec|spawn)\s*\(\s*[a-zA-Z_"#]"##).unwrap(),
            description: "Shell command execution with potentially unsanitized input",
        },
        LangPattern {
            lang: Language::Ruby,
            name: "backtick/Open3",
            regex: Regex::new(r##"(?:`[^`]*#\{|%x\{|Open3\.)"##).unwrap(),
            description: "Shell command execution via backtick or Open3",
        },
        LangPattern {
            lang: Language::Ruby,
            name: "IO.popen",
            regex: Regex::new(r#"IO\.popen\s*\("#).unwrap(),
            description: "IO.popen passes command to shell",
        },
        // ── C# ──────────────────────────────────────────────────────
        LangPattern {
            lang: Language::CSharp,
            name: "Process.Start",
            regex: Regex::new(r#"Process\.Start\s*\(\s*[a-zA-Z_]"#).unwrap(),
            description: "Process.Start with potentially unsanitized input",
        },
        LangPattern {
            lang: Language::CSharp,
            name: "ProcessStartInfo",
            regex: Regex::new(r#"(?:new\s+)?ProcessStartInfo\s*\(\s*[a-zA-Z_]"#).unwrap(),
            description: "ProcessStartInfo with potentially unsanitized input",
        },
        // ── Swift ───────────────────────────────────────────────────
        LangPattern {
            lang: Language::Swift,
            name: "Process/NSTask",
            regex: Regex::new(r#"(?:Process\s*\(\)|NSTask\s*\(|Process\.launchedProcess\s*\()"#)
                .unwrap(),
            description: "Process/NSTask command execution",
        },
        LangPattern {
            lang: Language::Swift,
            name: "shell function",
            regex: Regex::new(r#"shell\s*\(\s*[a-zA-Z_""]"#).unwrap(),
            description: "Shell helper function with potentially unsanitized input",
        },
        // ── Kotlin ──────────────────────────────────────────────────
        LangPattern {
            lang: Language::Kotlin,
            name: "Runtime.exec",
            regex: Regex::new(r#"Runtime\.getRuntime\s*\(\s*\)\s*\.exec\s*\(\s*[a-zA-Z_]"#)
                .unwrap(),
            description: "Runtime.exec with potentially unsanitized input",
        },
        LangPattern {
            lang: Language::Kotlin,
            name: "ProcessBuilder",
            regex: Regex::new(r#"ProcessBuilder\s*\(\s*[a-zA-Z_]"#).unwrap(),
            description: "ProcessBuilder with potentially unsanitized input",
        },
        // ── C ───────────────────────────────────────────────────────
        LangPattern {
            lang: Language::C,
            name: "system/popen",
            regex: Regex::new(r#"(?:system|popen)\s*\(\s*[a-zA-Z_]"#).unwrap(),
            description: "Shell command execution with potentially unsanitized input",
        },
        LangPattern {
            lang: Language::C,
            name: "exec family",
            regex: Regex::new(r#"(?:execve|execvp|execl)\s*\("#).unwrap(),
            description: "Direct exec-family call",
        },
        // ── C++ ─────────────────────────────────────────────────────
        LangPattern {
            lang: Language::Cpp,
            name: "system/popen",
            regex: Regex::new(r#"(?:system|popen)\s*\(\s*[a-zA-Z_]"#).unwrap(),
            description: "Shell command execution with potentially unsanitized input",
        },
        LangPattern {
            lang: Language::Cpp,
            name: "exec family",
            regex: Regex::new(r#"(?:execve|execvp|execl)\s*\("#).unwrap(),
            description: "Direct exec-family call",
        },
        // ── Rust ────────────────────────────────────────────────────
        LangPattern {
            lang: Language::Rust,
            name: "Command::new",
            regex: Regex::new(r#"Command::new\s*\(\s*[a-zA-Z_]"#).unwrap(),
            description: "Command::new with potentially unsanitized input",
        },
        LangPattern {
            lang: Language::Rust,
            name: "std::process::Command",
            regex: Regex::new(r#"std::process::Command"#).unwrap(),
            description: "Direct use of std::process::Command",
        },
    ]
});

/// Safe patterns that suppress findings per language.
fn is_safe_pattern(line: &str, lang: Language) -> bool {
    match lang {
        Language::Python => {
            // subprocess.run([...]) without shell=True is safe
            (line.contains("subprocess.run([") || line.contains("subprocess.call(["))
                && !line.contains("shell=True")
        }
        Language::JavaScript => {
            line.contains("execFile(") || line.contains("execFileSync(") || line.contains("spawn(")
        }
        Language::Java | Language::Kotlin => {
            // ProcessBuilder with list constructor
            line.contains("ProcessBuilder(Arrays.asList(")
                || line.contains("ProcessBuilder(List.of(")
        }
        _ => false,
    }
}

/// Returns true when a command call uses only hardcoded string literals.
fn is_hardcoded_command(line: &str, lang: Language) -> bool {
    match lang {
        Language::JavaScript => {
            if let Some(pos) = line.find("exec(") {
                let after = &line[pos + 5..];
                if (after.starts_with('"') || after.starts_with('\''))
                    && !after.contains("${")
                    && !after.contains("\" +")
                    && !after.contains("' +")
                {
                    return true;
                }
            }
            if let Some(pos) = line.find("execSync(") {
                let after = &line[pos + 9..];
                if (after.starts_with('"') || after.starts_with('\''))
                    && !after.contains("${")
                    && !after.contains("\" +")
                    && !after.contains("' +")
                {
                    return true;
                }
            }
            false
        }
        Language::Python => {
            // os.system("literal") with no f-string or format
            if let Some(pos) = line.find("os.system(") {
                let after = &line[pos + 10..];
                if (after.starts_with('"') || after.starts_with('\''))
                    && !after.starts_with("f\"")
                    && !after.starts_with("f'")
                    && !after.contains(".format(")
                    && !after.contains("% ")
                {
                    return true;
                }
            }
            false
        }
        _ => false,
    }
}

#[async_trait]
impl Detector for MultiCommandInjectionDetector {
    fn name(&self) -> &str {
        "multi-command-injection"
    }

    async fn analyze(&self, ctx: &AnalysisContext) -> Result<Vec<Finding>> {
        // Skip Wasm — no command execution concept
        if ctx.language == Language::Wasm {
            return Ok(Vec::new());
        }

        // CLI/console tools spawn subprocesses by design.  Downgrade all
        // command-injection findings to noisy + Low so they don't flood
        // reports.  WebService and Library projects stay at High.
        let cli_tool = is_cli_threat_model(ctx);

        let mut findings = Vec::new();

        for (path, source) in &ctx.source_cache {
            if is_test_file(path) {
                continue;
            }

            for (line_num, line) in source.lines().enumerate() {
                let trimmed = line.trim();

                if is_comment(trimmed, ctx.language) {
                    continue;
                }

                if is_safe_pattern(trimmed, ctx.language) {
                    continue;
                }

                if is_hardcoded_command(trimmed, ctx.language) {
                    continue;
                }

                for pattern in PATTERNS.iter() {
                    if pattern.lang != ctx.language {
                        continue;
                    }

                    if pattern.regex.is_match(trimmed) {
                        let line_1based = (line_num + 1) as u32;

                        let (severity, noisy) = if cli_tool {
                            (Severity::Low, true)
                        } else {
                            (Severity::High, false)
                        };

                        let mut finding = Finding {
                            id: Uuid::new_v4(),
                            detector: self.name().into(),
                            severity,
                            category: FindingCategory::Injection,
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
                                "Use safe APIs with argument arrays instead of shell strings. \
                                 Never pass unsanitized user input to shell commands."
                                    .into(),
                            explanation: None,
                            fix: None,
                            cwe_ids: vec![78],
                            noisy,
                            base_severity: None,
                            coverage_confidence: None,
                        };

                        // Check taint flow if CPG is available — downgrade instead of discard.
                        if let Some(has_taint) = taint_reaches_sink(
                            ctx,
                            path,
                            line_1based,
                            &["user_input", "request", "args", "params", "stdin", "env"],
                        ) {
                            if !has_taint {
                                finding.noisy = true;
                                finding.severity = Severity::Low;
                                finding.description = format!(
                                    "{} (no taint flow detected — likely safe)",
                                    finding.description
                                );
                            }
                        }

                        findings.push(finding);
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

    // ── Python ──────────────────────────────────────────────────────

    #[tokio::test]
    async fn detects_python_os_system() {
        let files = single_file("src/app.py", "os.system(user_input)\n");
        let ctx = make_ctx(files, Language::Python);
        let findings = MultiCommandInjectionDetector.analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].cwe_ids, vec![78]);
    }

    // ── JavaScript ──────────────────────────────────────────────────

    #[tokio::test]
    async fn detects_js_exec_template_literal() {
        let files = single_file("src/run.js", "exec(`ls ${dir}`)\n");
        let ctx = make_ctx(files, Language::JavaScript);
        let findings = MultiCommandInjectionDetector.analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].cwe_ids, vec![78]);
    }

    // ── Java ────────────────────────────────────────────────────────

    #[tokio::test]
    async fn detects_java_runtime_exec() {
        let files = single_file("src/App.java", "Runtime.getRuntime().exec(cmd)\n");
        let ctx = make_ctx(files, Language::Java);
        let findings = MultiCommandInjectionDetector.analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].cwe_ids, vec![78]);
    }

    // ── Go ──────────────────────────────────────────────────────────

    #[tokio::test]
    async fn detects_go_exec_command() {
        let files = single_file("src/main.go", "exec.Command(userInput, args...)\n");
        let ctx = make_ctx(files, Language::Go);
        let findings = MultiCommandInjectionDetector.analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].cwe_ids, vec![78]);
    }

    // ── Ruby ────────────────────────────────────────────────────────

    #[tokio::test]
    async fn detects_ruby_system() {
        let files = single_file("src/app.rb", "system(user_input)\n");
        let ctx = make_ctx(files, Language::Ruby);
        let findings = MultiCommandInjectionDetector.analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].cwe_ids, vec![78]);
    }

    // ── C# ──────────────────────────────────────────────────────────

    #[tokio::test]
    async fn detects_csharp_process_start() {
        let files = single_file("src/App.cs", "Process.Start(userInput)\n");
        let ctx = make_ctx(files, Language::CSharp);
        let findings = MultiCommandInjectionDetector.analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].cwe_ids, vec![78]);
    }

    // ── Swift ───────────────────────────────────────────────────────

    #[tokio::test]
    async fn detects_swift_process() {
        let files = single_file("src/App.swift", "let p = Process()\n");
        let ctx = make_ctx(files, Language::Swift);
        let findings = MultiCommandInjectionDetector.analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].cwe_ids, vec![78]);
    }

    // ── Kotlin ──────────────────────────────────────────────────────

    #[tokio::test]
    async fn detects_kotlin_runtime_exec() {
        let files = single_file("src/App.kt", "Runtime.getRuntime().exec(cmd)\n");
        let ctx = make_ctx(files, Language::Kotlin);
        let findings = MultiCommandInjectionDetector.analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].cwe_ids, vec![78]);
    }

    // ── C ───────────────────────────────────────────────────────────

    #[tokio::test]
    async fn detects_c_system() {
        let files = single_file("src/main.c", "system(user_cmd)\n");
        let ctx = make_ctx(files, Language::C);
        let findings = MultiCommandInjectionDetector.analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].cwe_ids, vec![78]);
    }

    // ── C++ ─────────────────────────────────────────────────────────

    #[tokio::test]
    async fn detects_cpp_system() {
        let files = single_file("src/main.cpp", "system(user_cmd)\n");
        let ctx = make_ctx(files, Language::Cpp);
        let findings = MultiCommandInjectionDetector.analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].cwe_ids, vec![78]);
    }

    // ── Rust ────────────────────────────────────────────────────────

    #[tokio::test]
    async fn detects_rust_command_new() {
        let files = single_file("src/main.rs", "Command::new(user_input)\n");
        let ctx = make_ctx(files, Language::Rust);
        let findings = MultiCommandInjectionDetector.analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].cwe_ids, vec![78]);
    }

    // ── Negative tests ──────────────────────────────────────────────

    #[tokio::test]
    async fn skips_test_files() {
        let files = single_file("tests/test_cmd.py", "os.system(user_input)\n");
        let ctx = make_ctx(files, Language::Python);
        let findings = MultiCommandInjectionDetector.analyze(&ctx).await.unwrap();
        assert!(findings.is_empty());
    }

    #[tokio::test]
    async fn skips_comments() {
        let files = single_file("src/app.py", "# os.system(user_input)\n");
        let ctx = make_ctx(files, Language::Python);
        let findings = MultiCommandInjectionDetector.analyze(&ctx).await.unwrap();
        assert!(findings.is_empty());
    }

    #[tokio::test]
    async fn skips_safe_execfile_js() {
        let files = single_file("src/run.js", "execFile(\"ls\", [\"-la\"])\n");
        let ctx = make_ctx(files, Language::JavaScript);
        let findings = MultiCommandInjectionDetector.analyze(&ctx).await.unwrap();
        assert!(findings.is_empty());
    }

    #[test]
    fn does_not_use_cargo_subprocess() {
        assert!(!MultiCommandInjectionDetector.uses_cargo_subprocess());
    }

    // -----------------------------------------------------------------------
    // Task 3.2: threat-model suppression — CLI tools
    // -----------------------------------------------------------------------

    // CliTool → noisy + Low for all command-injection findings
    #[tokio::test]
    async fn cli_tool_rust_command_new_is_noisy_low() {
        let files = single_file("src/main.rs", "Command::new(user_input)\n");
        let ctx = make_ctx_with_threat_model(files, Language::Rust, ThreatModelType::CliTool);
        let findings = MultiCommandInjectionDetector.analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 1);
        assert!(findings[0].noisy, "CliTool finding should be noisy");
        assert_eq!(
            findings[0].severity,
            Severity::Low,
            "CliTool finding should be Low"
        );
    }

    // ConsoleTool → noisy + Low (same as CliTool)
    #[tokio::test]
    async fn console_tool_rust_command_new_is_noisy_low() {
        let files = single_file("src/main.rs", "Command::new(user_input)\n");
        let ctx =
            make_ctx_with_threat_model(files, Language::Rust, ThreatModelType::ConsoleTool);
        let findings = MultiCommandInjectionDetector.analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 1);
        assert!(findings[0].noisy, "ConsoleTool finding should be noisy");
        assert_eq!(
            findings[0].severity,
            Severity::Low,
            "ConsoleTool finding should be Low"
        );
    }

    // WebService → High and not noisy
    #[tokio::test]
    async fn web_service_python_os_system_is_high_not_noisy() {
        let files = single_file("src/app.py", "os.system(user_input)\n");
        let ctx =
            make_ctx_with_threat_model(files, Language::Python, ThreatModelType::WebService);
        let findings = MultiCommandInjectionDetector.analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 1);
        assert!(
            !findings[0].noisy,
            "WebService finding should not be noisy"
        );
        assert_eq!(
            findings[0].severity,
            Severity::High,
            "WebService finding should be High"
        );
    }

    // Library → High and not noisy
    #[tokio::test]
    async fn library_go_exec_command_is_high_not_noisy() {
        let files = single_file("src/main.go", "exec.Command(userInput, args...)\n");
        let ctx =
            make_ctx_with_threat_model(files, Language::Go, ThreatModelType::Library);
        let findings = MultiCommandInjectionDetector.analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 1);
        assert!(!findings[0].noisy, "Library finding should not be noisy");
        assert_eq!(
            findings[0].severity,
            Severity::High,
            "Library finding should be High"
        );
    }

    // No threat model → High (default behaviour unchanged)
    #[tokio::test]
    async fn no_threat_model_is_high_not_noisy() {
        let files = single_file("src/main.rs", "Command::new(user_input)\n");
        let ctx = make_ctx(files, Language::Rust);
        let findings = MultiCommandInjectionDetector.analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 1);
        assert!(!findings[0].noisy, "no threat model should not be noisy");
        assert_eq!(
            findings[0].severity,
            Severity::High,
            "no threat model should be High"
        );
    }

    // -----------------------------------------------------------------------
    // Taint flow integration via CPG
    // -----------------------------------------------------------------------

    fn make_ctx_with_cpg(
        files: HashMap<PathBuf, String>,
        lang: Language,
        cpg: apex_cpg::Cpg,
    ) -> AnalysisContext {
        use std::sync::Arc;
        AnalysisContext {
            language: lang,
            source_cache: files,
            cpg: Some(Arc::new(cpg)),
            ..AnalysisContext::test_default()
        }
    }

    // CPG with taint flow → finding stays at original severity (High)
    //
    // taint_reaches_sink filters sink candidates by whether their name matches one
    // of the source_indicators. For command injection the indicators include
    // "user_input". So we need an Identifier node named "user_input" on the sink
    // line, connected via ReachingDef from a Parameter (the taint source).
    #[tokio::test]
    async fn taint_flow_present_keeps_original_severity() {
        use apex_cpg::{EdgeKind, NodeKind};

        let mut cpg = apex_cpg::Cpg::new();
        // The Parameter is the taint source.
        let param = cpg.add_node(NodeKind::Parameter {
            name: "user_input".into(),
            index: 0,
        });
        // An Identifier "user_input" on line 1 is the sink candidate
        // (matches indicator "user_input").
        let sink_id = cpg.add_node(NodeKind::Identifier {
            name: "user_input".into(),
            line: 1,
        });
        cpg.add_edge(param, sink_id, EdgeKind::ReachingDef { variable: "user_input".into() });

        let files = single_file("src/app.py", "os.system(user_input)\n");
        let ctx = make_ctx_with_cpg(files, Language::Python, cpg);
        let findings = MultiCommandInjectionDetector.analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 1);
        assert!(!findings[0].noisy, "taint flow present — should not be noisy");
        assert_eq!(
            findings[0].severity,
            Severity::High,
            "taint flow present — should stay High"
        );
    }

    // CPG with no taint flow → finding downgraded to noisy + Low
    //
    // We put a matching identifier on line 1 but no ReachingDef edge from any
    // Parameter to it — so taint_reaches_sink returns Some(false).
    #[tokio::test]
    async fn no_taint_flow_downgrades_to_noisy_low() {
        use apex_cpg::NodeKind;

        let mut cpg = apex_cpg::Cpg::new();
        // A sink candidate (matches indicator) but no taint source connected.
        cpg.add_node(NodeKind::Identifier {
            name: "user_input".into(),
            line: 1,
        });

        let files = single_file("src/app.py", "os.system(user_input)\n");
        let ctx = make_ctx_with_cpg(files, Language::Python, cpg);
        let findings = MultiCommandInjectionDetector.analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 1);
        assert!(findings[0].noisy, "no taint flow — should be noisy");
        assert_eq!(
            findings[0].severity,
            Severity::Low,
            "no taint flow — should be downgraded to Low"
        );
        assert!(
            findings[0].description.contains("no taint flow"),
            "description should mention no taint flow"
        );
    }

    // No CPG → finding stays at original severity (fallback to pattern matching)
    #[tokio::test]
    async fn no_cpg_falls_back_to_pattern_severity() {
        let files = single_file("src/app.py", "os.system(user_input)\n");
        let ctx = make_ctx(files, Language::Python);
        let findings = MultiCommandInjectionDetector.analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 1);
        assert!(!findings[0].noisy, "no CPG — should not be noisy");
        assert_eq!(
            findings[0].severity,
            Severity::High,
            "no CPG — should stay at pattern severity"
        );
    }
}
