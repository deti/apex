//! Multi-language command injection detector (CWE-78).
//!
//! Catches shell command execution sinks across all 11 supported languages
//! where unsanitized input may reach the shell.

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

                        findings.push(Finding {
                            id: Uuid::new_v4(),
                            detector: self.name().into(),
                            severity: Severity::High,
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
    use crate::context::AnalysisContext;
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
}
