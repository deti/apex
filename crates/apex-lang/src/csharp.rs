use apex_core::{
    command::{CommandRunner, CommandSpec, RealCommandRunner},
    error::{ApexError, Result},
    traits::{LanguageRunner, TestRunOutput},
    types::Language,
};
use async_trait::async_trait;
use std::path::Path;
use std::time::Instant;
use tracing::{debug, info};

pub struct CSharpRunner<R: CommandRunner = RealCommandRunner> {
    runner: R,
}

impl CSharpRunner {
    pub fn new() -> Self {
        CSharpRunner {
            runner: RealCommandRunner,
        }
    }
}

impl<R: CommandRunner> CSharpRunner<R> {
    pub fn with_runner(runner: R) -> Self {
        CSharpRunner { runner }
    }
}

impl Default for CSharpRunner {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl<R: CommandRunner> LanguageRunner for CSharpRunner<R> {
    fn language(&self) -> Language {
        Language::CSharp
    }

    fn detect(&self, target: &Path) -> bool {
        // Check for *.csproj or *.sln files in the target directory.
        if let Ok(entries) = std::fs::read_dir(target) {
            for entry in entries.flatten() {
                if let Some(ext) = entry.path().extension() {
                    if ext == "csproj" || ext == "sln" {
                        return true;
                    }
                }
            }
        }
        false
    }

    async fn install_deps(&self, target: &Path) -> Result<()> {
        info!(target = %target.display(), "restoring C# dependencies");

        let spec = CommandSpec::new("dotnet", target).args(["restore"]);
        let output = self
            .runner
            .run_command(&spec)
            .await
            .map_err(|e| ApexError::LanguageRunner(format!("dotnet restore: {e}")))?;

        if output.exit_code != 0 {
            let stderr = String::from_utf8_lossy(&output.stderr).to_string();
            return Err(ApexError::LanguageRunner(format!(
                "dotnet restore failed: {stderr}"
            )));
        }

        debug!("C# dependencies restored");
        Ok(())
    }

    async fn run_tests(
        &self,
        target: &Path,
        extra_args: &[String],
    ) -> Result<TestRunOutput> {
        info!(target = %target.display(), "running C# tests");

        let start = Instant::now();
        let mut args: Vec<String> = vec!["test".into()];
        args.extend_from_slice(extra_args);
        let spec = CommandSpec::new("dotnet", target).args(args);
        let output = self
            .runner
            .run_command(&spec)
            .await
            .map_err(|e| ApexError::LanguageRunner(format!("dotnet test: {e}")))?;

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
    fn language_is_csharp() {
        let runner = CSharpRunner::new();
        assert_eq!(runner.language(), Language::CSharp);
    }

    #[test]
    fn detect_with_csproj() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("MyApp.csproj"), "<Project />").unwrap();
        let runner = CSharpRunner::new();
        assert!(runner.detect(tmp.path()));
    }

    #[test]
    fn detect_with_sln() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("MyApp.sln"), "").unwrap();
        let runner = CSharpRunner::new();
        assert!(runner.detect(tmp.path()));
    }

    #[test]
    fn detect_without_dotnet_files() {
        let tmp = tempfile::tempdir().unwrap();
        let runner = CSharpRunner::new();
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
        let runner = CSharpRunner::with_runner(mock);
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
        let runner = CSharpRunner::with_runner(mock);
        let tmp = tempfile::tempdir().unwrap();
        let result = runner.install_deps(tmp.path()).await;
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("dotnet restore failed"), "got: {err}");
    }

    #[tokio::test]
    async fn run_tests_success() {
        let mut mock = MockCmd::new();
        mock.expect_run_command().returning(|_| {
            Ok(CommandOutput {
                exit_code: 0,
                stdout: b"Passed!  - Failed: 0\n".to_vec(),
                stderr: Vec::new(),
            })
        });
        let runner = CSharpRunner::with_runner(mock);
        let tmp = tempfile::tempdir().unwrap();
        let result = runner.run_tests(tmp.path(), &[]).await.unwrap();
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("Passed"));
    }

    #[tokio::test]
    async fn run_tests_failure() {
        let mut mock = MockCmd::new();
        mock.expect_run_command().returning(|_| {
            Ok(CommandOutput {
                exit_code: 1,
                stdout: Vec::new(),
                stderr: b"Failed!  - Failed: 3\n".to_vec(),
            })
        });
        let runner = CSharpRunner::with_runner(mock);
        let tmp = tempfile::tempdir().unwrap();
        let result = runner.run_tests(tmp.path(), &[]).await.unwrap();
        assert_eq!(result.exit_code, 1);
    }

    #[test]
    fn default_creates_runner() {
        let runner = CSharpRunner::default();
        assert_eq!(runner.language(), Language::CSharp);
    }

    #[tokio::test]
    async fn install_deps_checks_command_spec() {
        let mut mock = MockCmd::new();
        mock.expect_run_command()
            .withf(|spec| spec.program == "dotnet" && spec.args == ["restore"])
            .returning(|_| {
                Ok(CommandOutput {
                    exit_code: 0,
                    stdout: Vec::new(),
                    stderr: Vec::new(),
                })
            });
        let runner = CSharpRunner::with_runner(mock);
        let tmp = tempfile::tempdir().unwrap();
        runner.install_deps(tmp.path()).await.unwrap();
    }
}
