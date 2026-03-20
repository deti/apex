use apex_core::error::Result;
use apex_core::types::Language;
use async_trait::async_trait;
use regex::Regex;
use std::path::Path;
use std::sync::LazyLock;
use uuid::Uuid;

use super::util::is_comment;
use crate::context::AnalysisContext;
use crate::finding::{Finding, FindingCategory, Severity};
use crate::Detector;

pub struct ProcessExitInLibDetector;

// Rust: `std::process::exit(` or `process::exit(`
static RUST_EXIT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(std::)?process::exit\s*\(").unwrap());

// Python: `sys.exit(`, `os._exit(`, bare `exit(`
static PY_EXIT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\b(sys\.exit|os\._exit|exit)\s*\(").unwrap());

// JavaScript: `process.exit(`
static JS_EXIT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\bprocess\.exit\s*\(").unwrap());

/// Check whether a file is a "main" entry point for the given language.
///
/// Bug 17: Also matches cmd/, server.*, daemon.* patterns which are entry points
/// in many frameworks (Go cmd/, Python server.py, Node daemon.js).
fn is_main_file(path: &Path, language: Language, source: &str) -> bool {
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
    let path_str = path.to_string_lossy();

    // Cross-language: files under cmd/, or named server.* / daemon.* are entry points
    if path_str.contains("/cmd/")
        || path_str.contains("\\cmd\\")
        || name.starts_with("server.")
        || name.starts_with("daemon.")
    {
        return true;
    }

    match language {
        Language::Rust => {
            name == "main.rs" || path_str.contains("/bin/") || path_str.contains("\\bin\\")
        }
        Language::Python => {
            name == "__main__.py"
                || name == "manage.py"
                || name == "cli.py"
                || source.contains("if __name__")
        }
        Language::JavaScript => {
            name == "main.js"
                || name == "index.js"
                || name == "server.js"
                || name == "app.js"
                || name == "cli.js"
        }
        _ => false,
    }
}

fn matches_exit(line: &str, language: Language) -> bool {
    match language {
        Language::Rust => RUST_EXIT.is_match(line),
        Language::Python => PY_EXIT.is_match(line),
        Language::JavaScript => JS_EXIT.is_match(line),
        _ => false,
    }
}

#[async_trait]
impl Detector for ProcessExitInLibDetector {
    fn name(&self) -> &str {
        "process-exit-in-lib"
    }

    async fn analyze(&self, ctx: &AnalysisContext) -> Result<Vec<Finding>> {
        // Only supported for Rust, Python, JavaScript
        match ctx.language {
            Language::Rust | Language::Python | Language::JavaScript => {}
            _ => return Ok(vec![]),
        }

        let mut findings = Vec::new();

        for (path, source) in &ctx.source_cache {
            if is_main_file(path, ctx.language, source) {
                continue;
            }

            for (line_num, line) in source.lines().enumerate() {
                let trimmed = line.trim();
                if trimmed.is_empty() || is_comment(trimmed, ctx.language) {
                    continue;
                }

                if matches_exit(trimmed, ctx.language) {
                    let line_1based = (line_num + 1) as u32;
                    findings.push(Finding {
                        id: Uuid::new_v4(),
                        detector: self.name().into(),
                        severity: Severity::Medium,
                        category: FindingCategory::LogicBug,
                        file: path.clone(),
                        line: Some(line_1based),
                        title: format!("Process exit call in library code at line {}", line_1based),
                        description: format!(
                            "Line {} in {} calls process exit from library code. \
                             Libraries should return errors, not terminate the process.",
                            line_1based,
                            path.display()
                        ),
                        evidence: vec![],
                        covered: false,
                        suggestion:
                            "Return an error instead of calling process exit from library code"
                                .into(),
                        explanation: None,
                        fix: None,
                        cwe_ids: vec![],
                    noisy: false,
                    });
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

    fn make_ctx(files: HashMap<PathBuf, String>, lang: Language) -> AnalysisContext {
        AnalysisContext {
            language: lang,
            source_cache: files,
            ..AnalysisContext::test_default()
        }
    }

    // ---- Rust ----

    #[tokio::test]
    async fn detects_exit_in_rust_lib() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/lib.rs"),
            "fn shutdown() {\n    std::process::exit(1);\n}\n".into(),
        );
        let ctx = make_ctx(files, Language::Rust);
        let findings = ProcessExitInLibDetector.analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].line, Some(2));
    }

    #[tokio::test]
    async fn skips_rust_main_rs() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/main.rs"),
            "fn main() {\n    std::process::exit(0);\n}\n".into(),
        );
        let ctx = make_ctx(files, Language::Rust);
        let findings = ProcessExitInLibDetector.analyze(&ctx).await.unwrap();
        assert!(findings.is_empty());
    }

    #[tokio::test]
    async fn skips_rust_bin_dir() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/bin/cli.rs"),
            "fn main() {\n    process::exit(1);\n}\n".into(),
        );
        let ctx = make_ctx(files, Language::Rust);
        let findings = ProcessExitInLibDetector.analyze(&ctx).await.unwrap();
        assert!(findings.is_empty());
    }

    // ---- Python ----

    #[tokio::test]
    async fn detects_sys_exit_in_python_lib() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("mylib/utils.py"),
            "import sys\ndef fail():\n    sys.exit(1)\n".into(),
        );
        let ctx = make_ctx(files, Language::Python);
        let findings = ProcessExitInLibDetector.analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 1);
    }

    #[tokio::test]
    async fn detects_os_exit_in_python_lib() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("mylib/utils.py"),
            "import os\ndef fail():\n    os._exit(1)\n".into(),
        );
        let ctx = make_ctx(files, Language::Python);
        let findings = ProcessExitInLibDetector.analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 1);
    }

    #[tokio::test]
    async fn detects_bare_exit_in_python_lib() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("mylib/utils.py"),
            "def fail():\n    exit(1)\n".into(),
        );
        let ctx = make_ctx(files, Language::Python);
        let findings = ProcessExitInLibDetector.analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 1);
    }

    #[tokio::test]
    async fn skips_python_main_module() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("mylib/__main__.py"),
            "import sys\nsys.exit(0)\n".into(),
        );
        let ctx = make_ctx(files, Language::Python);
        let findings = ProcessExitInLibDetector.analyze(&ctx).await.unwrap();
        assert!(findings.is_empty());
    }

    #[tokio::test]
    async fn skips_python_file_with_name_guard() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("mylib/runner.py"),
            "import sys\nif __name__ == '__main__':\n    sys.exit(0)\n".into(),
        );
        let ctx = make_ctx(files, Language::Python);
        let findings = ProcessExitInLibDetector.analyze(&ctx).await.unwrap();
        assert!(findings.is_empty());
    }

    // ---- JavaScript ----

    #[tokio::test]
    async fn detects_process_exit_in_js_lib() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/utils.js"),
            "function fail() {\n    process.exit(1);\n}\n".into(),
        );
        let ctx = make_ctx(files, Language::JavaScript);
        let findings = ProcessExitInLibDetector.analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 1);
    }

    #[tokio::test]
    async fn skips_js_main_js() {
        let mut files = HashMap::new();
        files.insert(PathBuf::from("main.js"), "process.exit(0);\n".into());
        let ctx = make_ctx(files, Language::JavaScript);
        let findings = ProcessExitInLibDetector.analyze(&ctx).await.unwrap();
        assert!(findings.is_empty());
    }

    #[tokio::test]
    async fn skips_js_index_js() {
        let mut files = HashMap::new();
        files.insert(PathBuf::from("index.js"), "process.exit(0);\n".into());
        let ctx = make_ctx(files, Language::JavaScript);
        let findings = ProcessExitInLibDetector.analyze(&ctx).await.unwrap();
        assert!(findings.is_empty());
    }

    #[tokio::test]
    async fn skips_js_server_js() {
        let mut files = HashMap::new();
        files.insert(PathBuf::from("server.js"), "process.exit(0);\n".into());
        let ctx = make_ctx(files, Language::JavaScript);
        let findings = ProcessExitInLibDetector.analyze(&ctx).await.unwrap();
        assert!(findings.is_empty());
    }

    #[tokio::test]
    async fn skips_js_app_js() {
        let mut files = HashMap::new();
        files.insert(PathBuf::from("app.js"), "process.exit(0);\n".into());
        let ctx = make_ctx(files, Language::JavaScript);
        let findings = ProcessExitInLibDetector.analyze(&ctx).await.unwrap();
        assert!(findings.is_empty());
    }

    // ---- Unsupported language ----

    #[tokio::test]
    async fn skips_unsupported_language() {
        let mut files = HashMap::new();
        files.insert(PathBuf::from("src/Main.java"), "System.exit(0);\n".into());
        let ctx = make_ctx(files, Language::Java);
        let findings = ProcessExitInLibDetector.analyze(&ctx).await.unwrap();
        assert!(findings.is_empty());
    }

    #[test]
    fn does_not_use_cargo_subprocess() {
        assert!(!ProcessExitInLibDetector.uses_cargo_subprocess());
    }

    // -----------------------------------------------------------------------
    // Bug 17: cmd/, server.*, daemon.* files should be skipped
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn skips_go_cmd_dir() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("cmd/server/main.go"),
            "func main() {\n    os.Exit(1)\n}\n".into(),
        );
        let ctx = make_ctx(files, Language::Go);
        // Go is unsupported, so no findings anyway, but the path check still runs
        let findings = ProcessExitInLibDetector.analyze(&ctx).await.unwrap();
        assert!(findings.is_empty());
    }

    #[tokio::test]
    async fn skips_server_dot_py() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("server.py"),
            "import sys\nsys.exit(0)\n".into(),
        );
        let ctx = make_ctx(files, Language::Python);
        let findings = ProcessExitInLibDetector.analyze(&ctx).await.unwrap();
        assert!(
            findings.is_empty(),
            "server.py is an entry point and should be skipped"
        );
    }

    #[tokio::test]
    async fn skips_daemon_dot_py() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("daemon.py"),
            "import sys\nsys.exit(0)\n".into(),
        );
        let ctx = make_ctx(files, Language::Python);
        let findings = ProcessExitInLibDetector.analyze(&ctx).await.unwrap();
        assert!(
            findings.is_empty(),
            "daemon.py is an entry point and should be skipped"
        );
    }

    #[tokio::test]
    async fn skips_cmd_subdir_rust() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/cmd/migrate.rs"),
            "fn run() {\n    std::process::exit(0);\n}\n".into(),
        );
        let ctx = make_ctx(files, Language::Rust);
        let findings = ProcessExitInLibDetector.analyze(&ctx).await.unwrap();
        assert!(
            findings.is_empty(),
            "files in cmd/ subdirectory should be treated as entry points"
        );
    }

    #[tokio::test]
    async fn lib_file_outside_cmd_still_detected() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/utils.rs"),
            "fn shutdown() {\n    std::process::exit(1);\n}\n".into(),
        );
        let ctx = make_ctx(files, Language::Rust);
        let findings = ProcessExitInLibDetector.analyze(&ctx).await.unwrap();
        assert_eq!(
            findings.len(),
            1,
            "lib file outside cmd/ should still be flagged"
        );
    }
}
