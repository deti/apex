//! JavaScript/TypeScript command injection detector (CWE-78).
//!
//! Catches `exec`, `execSync`, `child_process.exec`, and `shelljs.exec` calls
//! that may pass unsanitized input to a shell.

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

pub struct JsCommandInjectionDetector;

struct CompiledPattern {
    name: &'static str,
    regex: Regex,
    description: &'static str,
}

static PATTERNS: LazyLock<Vec<CompiledPattern>> = LazyLock::new(|| {
    vec![
        CompiledPattern {
            name: "child_process exec/execSync",
            regex: Regex::new(
                r#"(?:child_process\s*[\.\)]\s*)?(?:exec|execSync)\s*\(\s*(?:`[^`]*\$\{|[a-zA-Z_])"#,
            )
            .expect("invalid regex"),
            description: "Shell command executed with potentially untrusted input",
        },
        CompiledPattern {
            name: "require child_process exec",
            regex: Regex::new(
                r#"require\s*\(\s*['"]child_process['"]\s*\)\s*\.exec\s*\("#,
            )
            .expect("invalid regex"),
            description: "Direct child_process.exec call",
        },
        CompiledPattern {
            name: "shelljs exec",
            regex: Regex::new(
                r#"shelljs\.exec\s*\(\s*(?:`[^`]*\$\{|[a-zA-Z_])"#,
            )
            .expect("invalid regex"),
            description: "shelljs.exec with potentially untrusted input",
        },
        CompiledPattern {
            name: "Template literal in exec",
            regex: Regex::new(
                r#"exec\s*\(\s*`[^`]*\$\{[^}]+\}[^`]*`"#,
            )
            .expect("invalid regex"),
            description: "Shell command built with template literal interpolation",
        },
    ]
});

/// Returns true when the call uses a hardcoded string literal (safe pattern).
fn is_hardcoded_exec(line: &str) -> bool {
    // exec("literal") or exec('literal') — no interpolation
    let trimmed = line.trim();
    if let Some(pos) = trimmed.find("exec(") {
        let after = &trimmed[pos + 5..];
        if (after.starts_with('"') || after.starts_with('\''))
            && !after.contains("${")
            && !after.contains("\" +")
            && !after.contains("' +")
        {
            return true;
        }
    }
    if let Some(pos) = trimmed.find("execSync(") {
        let after = &trimmed[pos + 9..];
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

/// Returns true when the call uses the safe `execFile` API.
fn is_safe_api(line: &str) -> bool {
    line.contains("execFile(") || line.contains("execFileSync(")
}

#[async_trait]
impl Detector for JsCommandInjectionDetector {
    fn name(&self) -> &str {
        "js-command-injection"
    }

    async fn analyze(&self, ctx: &AnalysisContext) -> Result<Vec<Finding>> {
        if ctx.language != Language::JavaScript {
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

                // Skip safe APIs (execFile).
                if is_safe_api(trimmed) {
                    continue;
                }

                // Skip hardcoded string literals (no user input).
                if is_hardcoded_exec(trimmed) {
                    continue;
                }

                for pattern in PATTERNS.iter() {
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
                                "Use execFile() or spawn() with an argument array instead of \
                                 exec(). Never pass unsanitized user input to shell commands."
                                    .into(),
                            explanation: None,
                            fix: None,
                            cwe_ids: vec![78],
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

    #[tokio::test]
    async fn detects_template_literal_in_exec() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/run.js"),
            "exec(`ls ${dir}`)\n".into(),
        );
        let ctx = make_ctx(files, Language::JavaScript);
        let findings = JsCommandInjectionDetector.analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, Severity::High);
        assert_eq!(findings[0].category, FindingCategory::Injection);
        assert_eq!(findings[0].cwe_ids, vec![78]);
    }

    #[tokio::test]
    async fn detects_child_process_exec_with_variable() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/run.js"),
            "child_process.exec(cmd)\n".into(),
        );
        let ctx = make_ctx(files, Language::JavaScript);
        let findings = JsCommandInjectionDetector.analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 1);
    }

    #[tokio::test]
    async fn detects_require_child_process_exec() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/run.js"),
            "require('child_process').exec(cmd)\n".into(),
        );
        let ctx = make_ctx(files, Language::JavaScript);
        let findings = JsCommandInjectionDetector.analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 1);
    }

    #[tokio::test]
    async fn detects_shelljs_exec() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/run.js"),
            "shelljs.exec(cmd)\n".into(),
        );
        let ctx = make_ctx(files, Language::JavaScript);
        let findings = JsCommandInjectionDetector.analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 1);
    }

    #[tokio::test]
    async fn skips_hardcoded_string_exec() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/run.js"),
            "exec(\"ls -la\")\n".into(),
        );
        let ctx = make_ctx(files, Language::JavaScript);
        let findings = JsCommandInjectionDetector.analyze(&ctx).await.unwrap();
        assert!(findings.is_empty(), "hardcoded string should not trigger");
    }

    #[tokio::test]
    async fn skips_exec_file_safe_api() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/run.js"),
            "execFile(\"ls\", [\"-la\"])\n".into(),
        );
        let ctx = make_ctx(files, Language::JavaScript);
        let findings = JsCommandInjectionDetector.analyze(&ctx).await.unwrap();
        assert!(findings.is_empty(), "execFile is a safe API");
    }

    #[tokio::test]
    async fn skips_non_javascript_language() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/run.py"),
            "exec(`ls ${dir}`)\n".into(),
        );
        let ctx = make_ctx(files, Language::Python);
        let findings = JsCommandInjectionDetector.analyze(&ctx).await.unwrap();
        assert!(findings.is_empty(), "should not fire for Python");
    }

    #[tokio::test]
    async fn skips_test_files() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("test/run.test.js"),
            "exec(`ls ${dir}`)\n".into(),
        );
        let ctx = make_ctx(files, Language::JavaScript);
        let findings = JsCommandInjectionDetector.analyze(&ctx).await.unwrap();
        assert!(findings.is_empty());
    }

    #[tokio::test]
    async fn skips_comments() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/run.js"),
            "// exec(`ls ${dir}`)\n".into(),
        );
        let ctx = make_ctx(files, Language::JavaScript);
        let findings = JsCommandInjectionDetector.analyze(&ctx).await.unwrap();
        assert!(findings.is_empty());
    }

    #[test]
    fn does_not_use_cargo_subprocess() {
        assert!(!JsCommandInjectionDetector.uses_cargo_subprocess());
    }
}
