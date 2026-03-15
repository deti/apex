use apex_core::{
    command::{CommandRunner, CommandSpec, RealCommandRunner},
    error::{ApexError, Result},
    traits::{LanguageRunner, TestRunOutput},
    types::Language,
};
use async_trait::async_trait;
use std::{path::Path, time::Instant};
use tracing::{debug, info};

pub struct SwiftRunner<R: CommandRunner = RealCommandRunner> {
    runner: R,
}

impl SwiftRunner {
    pub fn new() -> Self {
        SwiftRunner {
            runner: RealCommandRunner,
        }
    }
}

impl<R: CommandRunner> SwiftRunner<R> {
    pub fn with_runner(runner: R) -> Self {
        SwiftRunner { runner }
    }
}

impl Default for SwiftRunner {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl<R: CommandRunner> LanguageRunner for SwiftRunner<R> {
    fn language(&self) -> Language {
        Language::Swift
    }

    fn detect(&self, target: &Path) -> bool {
        target.join("Package.swift").exists()
    }

    async fn install_deps(&self, target: &Path) -> Result<()> {
        info!(target = %target.display(), "resolving Swift dependencies");

        let spec = CommandSpec::new("swift", target).args(["package", "resolve"]);
        let output = self
            .runner
            .run_command(&spec)
            .await
            .map_err(|e| ApexError::LanguageRunner(format!("swift package resolve: {e}")))?;

        if output.exit_code != 0 {
            let stderr = String::from_utf8_lossy(&output.stderr).to_string();
            return Err(ApexError::LanguageRunner(format!(
                "swift package resolve failed: {stderr}"
            )));
        }

        debug!("Swift dependencies resolved");
        Ok(())
    }

    async fn run_tests(
        &self,
        target: &Path,
        extra_args: &[String],
    ) -> Result<TestRunOutput> {
        info!(target = %target.display(), "running Swift tests");

        let start = Instant::now();
        let mut args: Vec<String> = vec!["test".into()];
        args.extend_from_slice(extra_args);
        let spec = CommandSpec::new("swift", target).args(args);
        let output = self
            .runner
            .run_command(&spec)
            .await
            .map_err(|e| ApexError::LanguageRunner(format!("swift test: {e}")))?;

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
    fn language_is_swift() {
        let runner = SwiftRunner::new();
        assert_eq!(runner.language(), Language::Swift);
    }

    #[test]
    fn detect_with_package_swift() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("Package.swift"), "// swift-tools-version:5.9")
            .unwrap();
        let runner = SwiftRunner::new();
        assert!(runner.detect(tmp.path()));
    }

    #[test]
    fn detect_without_package_swift() {
        let tmp = tempfile::tempdir().unwrap();
        let runner = SwiftRunner::new();
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
        let runner = SwiftRunner::with_runner(mock);
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
                stderr: b"error: package not found".to_vec(),
            })
        });
        let runner = SwiftRunner::with_runner(mock);
        let tmp = tempfile::tempdir().unwrap();
        let result = runner.install_deps(tmp.path()).await;
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("swift package resolve failed"),
            "got: {err}"
        );
    }

    #[tokio::test]
    async fn run_tests_success() {
        let mut mock = MockCmd::new();
        mock.expect_run_command().returning(|_| {
            Ok(CommandOutput {
                exit_code: 0,
                stdout: b"Test Suite 'All tests' passed.\n".to_vec(),
                stderr: Vec::new(),
            })
        });
        let runner = SwiftRunner::with_runner(mock);
        let tmp = tempfile::tempdir().unwrap();
        let result = runner.run_tests(tmp.path(), &[]).await.unwrap();
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("passed"));
    }

    #[tokio::test]
    async fn run_tests_failure() {
        let mut mock = MockCmd::new();
        mock.expect_run_command().returning(|_| {
            Ok(CommandOutput {
                exit_code: 1,
                stdout: Vec::new(),
                stderr: b"Test Suite 'All tests' failed.\n".to_vec(),
            })
        });
        let runner = SwiftRunner::with_runner(mock);
        let tmp = tempfile::tempdir().unwrap();
        let result = runner.run_tests(tmp.path(), &[]).await.unwrap();
        assert_eq!(result.exit_code, 1);
    }

    #[test]
    fn default_creates_runner() {
        let runner = SwiftRunner::default();
        assert_eq!(runner.language(), Language::Swift);
    }

    #[tokio::test]
    async fn install_deps_checks_command_spec() {
        let mut mock = MockCmd::new();
        mock.expect_run_command()
            .withf(|spec| spec.program == "swift" && spec.args == ["package", "resolve"])
            .returning(|_| {
                Ok(CommandOutput {
                    exit_code: 0,
                    stdout: Vec::new(),
                    stderr: Vec::new(),
                })
            });
        let runner = SwiftRunner::with_runner(mock);
        let tmp = tempfile::tempdir().unwrap();
        runner.install_deps(tmp.path()).await.unwrap();
    }
}
