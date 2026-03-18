use apex_core::{
    command::{CommandRunner, CommandSpec, RealCommandRunner},
    error::{ApexError, Result},
    traits::{LanguageRunner, PreflightInfo, TestRunOutput},
    types::Language,
};
use async_trait::async_trait;
use std::{path::Path, time::Instant};
use tracing::{debug, info};

pub struct GoRunner<R: CommandRunner = RealCommandRunner> {
    runner: R,
}

impl GoRunner {
    pub fn new() -> Self {
        GoRunner {
            runner: RealCommandRunner,
        }
    }
}

impl<R: CommandRunner> GoRunner<R> {
    pub fn with_runner(runner: R) -> Self {
        GoRunner { runner }
    }

    /// Check if a binary is on PATH and return its version string.
    fn tool_version(bin: &str, args: &[&str]) -> Option<String> {
        let output = std::process::Command::new(bin).args(args).output().ok()?;
        if !output.status.success() {
            return None;
        }
        let stdout = String::from_utf8_lossy(&output.stdout);
        Some(stdout.lines().next().unwrap_or("").trim().to_string())
    }

    /// Parse the module path from go.mod.
    fn parse_module_path(target: &Path) -> Option<String> {
        let content = std::fs::read_to_string(target.join("go.mod")).ok()?;
        for line in content.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with("module ") {
                return Some(trimmed["module ".len()..].trim().to_string());
            }
        }
        None
    }

    /// Detect if this is a monorepo with multiple Go modules.
    fn is_monorepo(target: &Path) -> bool {
        // Look for go.mod files in immediate subdirectories
        let Ok(entries) = std::fs::read_dir(target) else {
            return false;
        };
        let submodule_count = entries
            .flatten()
            .filter(|e| e.path().is_dir() && e.path().join("go.mod").exists())
            .count();
        submodule_count > 0
    }
}

impl Default for GoRunner {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl<R: CommandRunner> LanguageRunner for GoRunner<R> {
    fn language(&self) -> Language {
        Language::Go
    }

    fn detect(&self, target: &Path) -> bool {
        target.join("go.mod").exists()
    }

    async fn install_deps(&self, target: &Path) -> Result<()> {
        info!(target = %target.display(), "installing Go dependencies");

        let spec = CommandSpec::new("go", target)
            .args(["mod", "download"])
            .timeout(300_000); // 5 min — large Go projects (e.g. Kubernetes) have 100+ deps
        let output = self
            .runner
            .run_command(&spec)
            .await
            .map_err(|e| ApexError::LanguageRunner(format!("go mod download: {e}")))?;

        if output.exit_code != 0 {
            let stderr = String::from_utf8_lossy(&output.stderr).to_string();
            return Err(ApexError::LanguageRunner(format!(
                "go mod download failed: {stderr}"
            )));
        }

        debug!("Go dependencies installed");
        Ok(())
    }

    async fn run_tests(&self, target: &Path, extra_args: &[String]) -> Result<TestRunOutput> {
        info!(target = %target.display(), "running Go tests");

        let start = Instant::now();
        let mut args: Vec<String> = vec!["test".into(), "./...".into()];
        args.extend_from_slice(extra_args);
        let spec = CommandSpec::new("go", target)
            .args(args)
            .timeout(600_000); // 10 min — Go test suites can be slow on large projects
        let output = self
            .runner
            .run_command(&spec)
            .await
            .map_err(|e| ApexError::LanguageRunner(format!("go test: {e}")))?;

        let duration = start.elapsed();
        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();

        Ok(TestRunOutput {
            exit_code: output.exit_code,
            stdout,
            stderr,
            duration_ms: duration.as_millis() as u64,
        })
    }

    fn preflight_check(&self, target: &Path) -> Result<PreflightInfo> {
        let mut info = PreflightInfo::default();
        info.build_system = Some("go".into());
        info.package_manager = Some("go-modules".into());
        info.test_framework = Some("go-test".into());

        // Check go binary
        if let Some(ver) = Self::tool_version("go", &["version"]) {
            info.available_tools.push(("go".into(), ver));
        } else {
            info.missing_tools.push("go".into());
        }

        // Parse module path from go.mod
        if let Some(mod_path) = Self::parse_module_path(target) {
            info.extra.push(("module_path".into(), mod_path));
        }

        // Detect monorepo
        if Self::is_monorepo(target) {
            info.extra.push(("monorepo".into(), "true".into()));
            info.warnings.push(
                "monorepo detected: multiple go.mod files in subdirectories".into(),
            );
        }

        // Check if go.sum exists (deps resolved)
        info.deps_installed = target.join("go.sum").exists();

        // Check for govulncheck
        if let Some(ver) = Self::tool_version("govulncheck", &["-version"]) {
            info.available_tools.push(("govulncheck".into(), ver));
        }

        Ok(info)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use apex_core::command::CommandOutput;
    use apex_core::traits::LanguageRunner;

    mockall::mock! {
        Cmd {}
        #[async_trait]
        impl CommandRunner for Cmd {
            async fn run_command(&self, spec: &CommandSpec) -> Result<CommandOutput>;
        }
    }

    #[test]
    fn language_is_go() {
        let runner = GoRunner::new();
        assert_eq!(runner.language(), Language::Go);
    }

    #[test]
    fn detect_with_go_mod() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("go.mod"), "module example.com/foo").unwrap();
        let runner = GoRunner::new();
        assert!(runner.detect(tmp.path()));
    }

    #[test]
    fn detect_without_go_mod() {
        let tmp = tempfile::tempdir().unwrap();
        let runner = GoRunner::new();
        assert!(!runner.detect(tmp.path()));
    }

    #[tokio::test]
    async fn install_deps_success() {
        let mut mock = MockCmd::new();
        mock.expect_run_command().returning(|_| {
            Ok(CommandOutput {
                exit_code: 0,
                stdout: Vec::new(),
                stderr: Vec::new(),
            })
        });
        let runner = GoRunner::with_runner(mock);
        let tmp = tempfile::tempdir().unwrap();
        let result = runner.install_deps(tmp.path()).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn install_deps_failure() {
        let mut mock = MockCmd::new();
        mock.expect_run_command().returning(|_| {
            Ok(CommandOutput {
                exit_code: 1,
                stdout: Vec::new(),
                stderr: b"go: module not found".to_vec(),
            })
        });
        let runner = GoRunner::with_runner(mock);
        let tmp = tempfile::tempdir().unwrap();
        let result = runner.install_deps(tmp.path()).await;
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("go mod download failed"), "got: {err}");
    }

    #[tokio::test]
    async fn run_tests_success() {
        let mut mock = MockCmd::new();
        mock.expect_run_command().returning(|_| {
            Ok(CommandOutput {
                exit_code: 0,
                stdout: b"ok  \texample.com/foo\t0.003s\n".to_vec(),
                stderr: Vec::new(),
            })
        });
        let runner = GoRunner::with_runner(mock);
        let tmp = tempfile::tempdir().unwrap();
        let result = runner.run_tests(tmp.path(), &[]).await.unwrap();
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("ok"));
    }

    #[tokio::test]
    async fn run_tests_failure() {
        let mut mock = MockCmd::new();
        mock.expect_run_command().returning(|_| {
            Ok(CommandOutput {
                exit_code: 1,
                stdout: Vec::new(),
                stderr: b"FAIL\texample.com/foo\n".to_vec(),
            })
        });
        let runner = GoRunner::with_runner(mock);
        let tmp = tempfile::tempdir().unwrap();
        let result = runner.run_tests(tmp.path(), &[]).await.unwrap();
        assert_eq!(result.exit_code, 1);
    }

    #[test]
    fn default_creates_runner() {
        let runner = GoRunner::default();
        assert_eq!(runner.language(), Language::Go);
    }

    #[tokio::test]
    async fn run_tests_captures_stderr() {
        let mut mock = MockCmd::new();
        mock.expect_run_command().returning(|_| {
            Ok(CommandOutput {
                exit_code: 0,
                stdout: b"ok\n".to_vec(),
                stderr: b"some warning\n".to_vec(),
            })
        });
        let runner = GoRunner::with_runner(mock);
        let tmp = tempfile::tempdir().unwrap();
        let result = runner.run_tests(tmp.path(), &[]).await.unwrap();
        assert!(result.stderr.contains("some warning"));
    }

    #[tokio::test]
    async fn install_deps_checks_command_spec() {
        let mut mock = MockCmd::new();
        mock.expect_run_command()
            .withf(|spec| spec.program == "go" && spec.args == ["mod", "download"])
            .returning(|_| {
                Ok(CommandOutput {
                    exit_code: 0,
                    stdout: Vec::new(),
                    stderr: Vec::new(),
                })
            });
        let runner = GoRunner::with_runner(mock);
        let tmp = tempfile::tempdir().unwrap();
        runner.install_deps(tmp.path()).await.unwrap();
    }

    // ------------------------------------------------------------------
    // preflight_check tests
    // ------------------------------------------------------------------

    #[test]
    fn preflight_check_basic() {
        let dir = tempfile::tempdir().unwrap();
        let runner = GoRunner::new();
        let info = runner.preflight_check(dir.path()).unwrap();
        assert_eq!(info.build_system.as_deref(), Some("go"));
        assert_eq!(info.test_framework.as_deref(), Some("go-test"));
    }

    #[test]
    fn preflight_check_parses_module_path() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("go.mod"),
            "module github.com/example/mymod\n\ngo 1.21\n",
        )
        .unwrap();
        let runner = GoRunner::new();
        let info = runner.preflight_check(dir.path()).unwrap();
        assert!(
            info.extra
                .iter()
                .any(|(k, v)| k == "module_path" && v == "github.com/example/mymod"),
            "extra: {:?}",
            info.extra
        );
    }

    #[test]
    fn preflight_check_detects_monorepo() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("go.mod"), "module example.com/root\n").unwrap();
        let sub = dir.path().join("submod");
        std::fs::create_dir(&sub).unwrap();
        std::fs::write(sub.join("go.mod"), "module example.com/root/submod\n").unwrap();
        let runner = GoRunner::new();
        let info = runner.preflight_check(dir.path()).unwrap();
        assert!(info.extra.iter().any(|(k, v)| k == "monorepo" && v == "true"));
        assert!(info.warnings.iter().any(|w| w.contains("monorepo")));
    }

    #[test]
    fn preflight_check_deps_installed_with_go_sum() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("go.sum"), "").unwrap();
        let runner = GoRunner::new();
        let info = runner.preflight_check(dir.path()).unwrap();
        assert!(info.deps_installed);
    }

    #[test]
    fn preflight_check_deps_not_installed_without_go_sum() {
        let dir = tempfile::tempdir().unwrap();
        let runner = GoRunner::new();
        let info = runner.preflight_check(dir.path()).unwrap();
        assert!(!info.deps_installed);
    }

    #[test]
    fn parse_module_path_valid() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("go.mod"), "module example.com/foo\n\ngo 1.21\n").unwrap();
        let path = GoRunner::<RealCommandRunner>::parse_module_path(dir.path());
        assert_eq!(path.as_deref(), Some("example.com/foo"));
    }

    #[test]
    fn parse_module_path_no_go_mod() {
        let dir = tempfile::tempdir().unwrap();
        let path = GoRunner::<RealCommandRunner>::parse_module_path(dir.path());
        assert!(path.is_none());
    }

    #[test]
    fn is_monorepo_false_no_subdirs() {
        let dir = tempfile::tempdir().unwrap();
        assert!(!GoRunner::<RealCommandRunner>::is_monorepo(dir.path()));
    }
}
