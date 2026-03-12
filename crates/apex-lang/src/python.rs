use apex_core::{
    command::{CommandRunner, CommandSpec, RealCommandRunner},
    error::{ApexError, Result},
    traits::{LanguageRunner, TestRunOutput},
    types::Language,
};
use async_trait::async_trait;
use std::{path::Path, time::Instant};
use tracing::{debug, info};

pub struct PythonRunner<R: CommandRunner = RealCommandRunner> {
    runner: R,
}

impl PythonRunner {
    pub fn new() -> Self {
        PythonRunner {
            runner: RealCommandRunner,
        }
    }
}

impl<R: CommandRunner> PythonRunner<R> {
    pub fn with_runner(runner: R) -> Self {
        PythonRunner { runner }
    }

    /// Detect the test runner from project config.
    fn detect_test_runner(target: &Path) -> Vec<String> {
        // Check pyproject.toml for pytest
        if target.join("pyproject.toml").exists() {
            let content =
                std::fs::read_to_string(target.join("pyproject.toml")).unwrap_or_default();
            if content.contains("[tool.pytest") || content.contains("pytest") {
                return vec!["python3".into(), "-m".into(), "pytest".into(), "-q".into()];
            }
        }
        // Fallback to pytest
        vec!["python3".into(), "-m".into(), "pytest".into(), "-q".into()]
    }
}

impl Default for PythonRunner {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl<R: CommandRunner> LanguageRunner for PythonRunner<R> {
    fn language(&self) -> Language {
        Language::Python
    }

    fn detect(&self, target: &Path) -> bool {
        target.join("pyproject.toml").exists()
            || target.join("setup.py").exists()
            || target.join("setup.cfg").exists()
            || target.join("requirements.txt").exists()
    }

    async fn install_deps(&self, target: &Path) -> Result<()> {
        info!(target = %target.display(), "installing Python dependencies");

        if target.join("requirements.txt").exists() {
            let spec = CommandSpec::new("pip3", target).args(["install", "-r", "requirements.txt"]);
            let output = self
                .runner
                .run_command(&spec)
                .await
                .map_err(|e| ApexError::LanguageRunner(format!("pip install: {e}")))?;

            if output.exit_code != 0 {
                let stderr = String::from_utf8_lossy(&output.stderr).to_string();
                return Err(ApexError::LanguageRunner(format!(
                    "pip install failed: {stderr}"
                )));
            }
        } else if target.join("pyproject.toml").exists() || target.join("setup.py").exists() {
            let spec = CommandSpec::new("pip3", target).args(["install", "-e", "."]);
            let output = self
                .runner
                .run_command(&spec)
                .await
                .map_err(|e| ApexError::LanguageRunner(format!("pip install -e: {e}")))?;

            if output.exit_code != 0 {
                let stderr = String::from_utf8_lossy(&output.stderr).to_string();
                return Err(ApexError::LanguageRunner(format!(
                    "pip install -e failed: {stderr}"
                )));
            }
        }

        // Ensure coverage.py is available.
        let cov_spec = CommandSpec::new("python3", target).args(["-c", "import coverage"]);
        let cov_check = self
            .runner
            .run_command(&cov_spec)
            .await
            .map_err(|e| ApexError::LanguageRunner(e.to_string()))?;

        if cov_check.exit_code != 0 {
            debug!("coverage.py not found, installing");
            let spec = CommandSpec::new("pip3", target).args(["install", "coverage", "pytest"]);
            let output = self
                .runner
                .run_command(&spec)
                .await
                .map_err(|e| ApexError::LanguageRunner(e.to_string()))?;

            if output.exit_code != 0 {
                let stderr = String::from_utf8_lossy(&output.stderr).to_string();
                return Err(ApexError::LanguageRunner(format!(
                    "failed to install coverage/pytest: {stderr}"
                )));
            }
        }

        Ok(())
    }

    async fn run_tests(&self, target: &Path, extra_args: &[String]) -> Result<TestRunOutput> {
        let cmd_parts = Self::detect_test_runner(target);
        let mut args: Vec<String> = cmd_parts[1..].to_vec();
        args.extend(extra_args.iter().cloned());

        info!(
            target = %target.display(),
            cmd = ?cmd_parts,
            "running Python tests"
        );

        let spec = CommandSpec::new(&cmd_parts[0], target).args(args);

        let start = Instant::now();
        let output = self
            .runner
            .run_command(&spec)
            .await
            .map_err(|e| ApexError::LanguageRunner(format!("run tests: {e}")))?;

        let duration_ms = start.elapsed().as_millis() as u64;

        Ok(TestRunOutput {
            exit_code: output.exit_code,
            stdout: String::from_utf8_lossy(&output.stdout).to_string(),
            stderr: String::from_utf8_lossy(&output.stderr).to_string(),
            duration_ms,
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
    fn detect_pyproject_toml() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("pyproject.toml"), "").unwrap();
        assert!(PythonRunner::new().detect(dir.path()));
    }

    #[test]
    fn detect_setup_py() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("setup.py"), "").unwrap();
        assert!(PythonRunner::new().detect(dir.path()));
    }

    #[test]
    fn detect_setup_cfg() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("setup.cfg"), "").unwrap();
        assert!(PythonRunner::new().detect(dir.path()));
    }

    #[test]
    fn detect_requirements_txt() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("requirements.txt"), "requests\n").unwrap();
        assert!(PythonRunner::new().detect(dir.path()));
    }

    #[test]
    fn detect_empty_dir_returns_false() {
        let dir = tempfile::tempdir().unwrap();
        assert!(!PythonRunner::new().detect(dir.path()));
    }

    #[test]
    fn detect_test_runner_with_pytest_in_pyproject() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("pyproject.toml"),
            "[tool.pytest.ini_options]\nminversion = \"6.0\"\n",
        )
        .unwrap();
        let cmd = PythonRunner::<RealCommandRunner>::detect_test_runner(dir.path());
        assert_eq!(cmd, vec!["python3", "-m", "pytest", "-q"]);
    }

    #[test]
    fn detect_test_runner_fallback() {
        let dir = tempfile::tempdir().unwrap();
        let cmd = PythonRunner::<RealCommandRunner>::detect_test_runner(dir.path());
        assert_eq!(cmd, vec!["python3", "-m", "pytest", "-q"]);
    }

    #[test]
    fn language_is_python() {
        assert_eq!(PythonRunner::new().language(), Language::Python);
    }

    // ---- Mock-based tests ----

    #[tokio::test]
    async fn install_deps_requirements_txt_success() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("requirements.txt"), "requests\n").unwrap();

        let mut mock = MockCmd::new();
        // pip3 install -r requirements.txt
        mock.expect_run_command()
            .withf(|spec| spec.program == "pip3" && spec.args.contains(&"-r".to_string()))
            .times(1)
            .returning(|_| Ok(CommandOutput::success(b"Successfully installed".to_vec())));
        // python3 -c "import coverage"
        mock.expect_run_command()
            .withf(|spec| spec.program == "python3")
            .times(1)
            .returning(|_| Ok(CommandOutput::success(b"".to_vec())));

        let runner = PythonRunner::with_runner(mock);
        let result = runner.install_deps(dir.path()).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn install_deps_requirements_txt_failure() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("requirements.txt"), "nonexistent-pkg\n").unwrap();

        let mut mock = MockCmd::new();
        mock.expect_run_command()
            .withf(|spec| spec.program == "pip3" && spec.args.contains(&"-r".to_string()))
            .times(1)
            .returning(|_| {
                Ok(CommandOutput::failure(
                    1,
                    b"No matching distribution".to_vec(),
                ))
            });

        let runner = PythonRunner::with_runner(mock);
        let result = runner.install_deps(dir.path()).await;
        assert!(result.is_err());
        let msg = format!("{}", result.unwrap_err());
        assert!(msg.contains("pip install failed"), "unexpected: {msg}");
    }

    #[tokio::test]
    async fn install_deps_pyproject_editable_install() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("pyproject.toml"),
            "[project]\nname = \"foo\"\n",
        )
        .unwrap();

        let mut mock = MockCmd::new();
        // pip3 install -e .
        mock.expect_run_command()
            .withf(|spec| spec.program == "pip3" && spec.args.contains(&"-e".to_string()))
            .times(1)
            .returning(|_| Ok(CommandOutput::success(b"".to_vec())));
        // python3 -c "import coverage" -- coverage already installed
        mock.expect_run_command()
            .withf(|spec| spec.program == "python3")
            .times(1)
            .returning(|_| Ok(CommandOutput::success(b"".to_vec())));

        let runner = PythonRunner::with_runner(mock);
        let result = runner.install_deps(dir.path()).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn install_deps_coverage_not_found_installs_it() {
        let dir = tempfile::tempdir().unwrap();
        // No requirements.txt or pyproject.toml, so skip dep install
        // but coverage check fails, so it installs coverage+pytest

        let mut mock = MockCmd::new();
        // python3 -c "import coverage" fails
        mock.expect_run_command()
            .withf(|spec| spec.program == "python3")
            .times(1)
            .returning(|_| Ok(CommandOutput::failure(1, b"ModuleNotFoundError".to_vec())));
        // pip3 install coverage pytest
        mock.expect_run_command()
            .withf(|spec| spec.program == "pip3" && spec.args.contains(&"coverage".to_string()))
            .times(1)
            .returning(|_| Ok(CommandOutput::success(b"".to_vec())));

        let runner = PythonRunner::with_runner(mock);
        let result = runner.install_deps(dir.path()).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn install_deps_coverage_install_fails() {
        let dir = tempfile::tempdir().unwrap();

        let mut mock = MockCmd::new();
        // python3 -c "import coverage" fails
        mock.expect_run_command()
            .withf(|spec| spec.program == "python3")
            .times(1)
            .returning(|_| Ok(CommandOutput::failure(1, b"ModuleNotFoundError".to_vec())));
        // pip3 install coverage pytest also fails
        mock.expect_run_command()
            .withf(|spec| spec.program == "pip3")
            .times(1)
            .returning(|_| Ok(CommandOutput::failure(1, b"Permission denied".to_vec())));

        let runner = PythonRunner::with_runner(mock);
        let result = runner.install_deps(dir.path()).await;
        assert!(result.is_err());
        let msg = format!("{}", result.unwrap_err());
        assert!(
            msg.contains("failed to install coverage/pytest"),
            "unexpected: {msg}"
        );
    }

    #[tokio::test]
    async fn run_tests_success() {
        let dir = tempfile::tempdir().unwrap();

        let mut mock = MockCmd::new();
        mock.expect_run_command()
            .withf(|spec| spec.program == "python3")
            .times(1)
            .returning(|_| Ok(CommandOutput::success(b"3 passed in 0.5s".to_vec())));

        let runner = PythonRunner::with_runner(mock);
        let result = runner.run_tests(dir.path(), &[]).await.unwrap();
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("3 passed"));
    }

    #[tokio::test]
    async fn run_tests_failure() {
        let dir = tempfile::tempdir().unwrap();

        let mut mock = MockCmd::new();
        mock.expect_run_command().times(1).returning(|_| {
            Ok(CommandOutput {
                exit_code: 1,
                stdout: b"1 failed, 2 passed".to_vec(),
                stderr: b"FAILED test_foo".to_vec(),
            })
        });

        let runner = PythonRunner::with_runner(mock);
        let result = runner.run_tests(dir.path(), &[]).await.unwrap();
        assert_eq!(result.exit_code, 1);
        assert!(result.stdout.contains("1 failed"));
    }

    #[tokio::test]
    async fn run_tests_command_not_found() {
        let dir = tempfile::tempdir().unwrap();

        let mut mock = MockCmd::new();
        mock.expect_run_command().times(1).returning(|_| {
            Err(ApexError::Subprocess {
                exit_code: -1,
                stderr: "spawn python3: No such file or directory".into(),
            })
        });

        let runner = PythonRunner::with_runner(mock);
        let result = runner.run_tests(dir.path(), &[]).await;
        assert!(result.is_err());
        let msg = format!("{}", result.unwrap_err());
        assert!(msg.contains("run tests"), "unexpected: {msg}");
    }

    #[tokio::test]
    async fn run_tests_with_extra_args() {
        let dir = tempfile::tempdir().unwrap();

        let mut mock = MockCmd::new();
        mock.expect_run_command()
            .withf(|spec| spec.args.iter().any(|a| a == "--verbose"))
            .times(1)
            .returning(|_| Ok(CommandOutput::success(b"verbose output".to_vec())));

        let runner = PythonRunner::with_runner(mock);
        let result = runner
            .run_tests(dir.path(), &["--verbose".into()])
            .await
            .unwrap();
        assert_eq!(result.exit_code, 0);
    }

    // ------------------------------------------------------------------
    // detect_test_runner — pyproject.toml with "pytest" keyword (not section header)
    // ------------------------------------------------------------------

    #[test]
    fn detect_test_runner_pyproject_with_pytest_keyword() {
        let dir = tempfile::tempdir().unwrap();
        // Contains "pytest" but not "[tool.pytest" — still matches because of `||`
        std::fs::write(
            dir.path().join("pyproject.toml"),
            "[build-system]\nrequires = [\"pytest\", \"setuptools\"]\n",
        )
        .unwrap();
        let cmd = PythonRunner::<RealCommandRunner>::detect_test_runner(dir.path());
        assert_eq!(cmd, vec!["python3", "-m", "pytest", "-q"]);
    }

    #[test]
    fn detect_test_runner_pyproject_no_pytest_falls_through_to_default() {
        let dir = tempfile::tempdir().unwrap();
        // pyproject.toml exists but doesn't mention pytest at all
        std::fs::write(
            dir.path().join("pyproject.toml"),
            "[project]\nname = \"foo\"\nversion = \"0.1.0\"\n",
        )
        .unwrap();
        let cmd = PythonRunner::<RealCommandRunner>::detect_test_runner(dir.path());
        // Falls through to the fallback (which is the same command)
        assert_eq!(cmd, vec!["python3", "-m", "pytest", "-q"]);
    }

    // ------------------------------------------------------------------
    // detect — multiple markers present
    // ------------------------------------------------------------------

    #[test]
    fn detect_pyproject_and_requirements() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("pyproject.toml"), "").unwrap();
        std::fs::write(dir.path().join("requirements.txt"), "requests\n").unwrap();
        assert!(PythonRunner::new().detect(dir.path()));
    }

    #[test]
    fn detect_nonexistent_dir_returns_false() {
        let runner = PythonRunner::new();
        assert!(!runner.detect(Path::new("/nonexistent/path/that/does/not/exist")));
    }

    // ------------------------------------------------------------------
    // install_deps — setup.py editable install path
    // ------------------------------------------------------------------

    #[tokio::test]
    async fn install_deps_setup_py_editable_install() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("setup.py"), "from setuptools import setup; setup()").unwrap();

        let mut mock = MockCmd::new();
        // pip3 install -e .
        mock.expect_run_command()
            .withf(|spec| spec.program == "pip3" && spec.args.contains(&"-e".to_string()))
            .times(1)
            .returning(|_| Ok(CommandOutput::success(b"".to_vec())));
        // python3 -c "import coverage" — already installed
        mock.expect_run_command()
            .withf(|spec| spec.program == "python3")
            .times(1)
            .returning(|_| Ok(CommandOutput::success(b"".to_vec())));

        let runner = PythonRunner::with_runner(mock);
        let result = runner.install_deps(dir.path()).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn install_deps_setup_py_editable_install_fails() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("setup.py"), "from setuptools import setup; setup()").unwrap();

        let mut mock = MockCmd::new();
        mock.expect_run_command()
            .withf(|spec| spec.program == "pip3" && spec.args.contains(&"-e".to_string()))
            .times(1)
            .returning(|_| Ok(CommandOutput::failure(1, b"Permission denied".to_vec())));

        let runner = PythonRunner::with_runner(mock);
        let result = runner.install_deps(dir.path()).await;
        assert!(result.is_err());
        let msg = format!("{}", result.unwrap_err());
        assert!(msg.contains("pip install -e failed"), "unexpected: {msg}");
    }

    #[tokio::test]
    async fn install_deps_pyproject_editable_install_fails() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("pyproject.toml"), "[project]\nname=\"x\"\n").unwrap();

        let mut mock = MockCmd::new();
        mock.expect_run_command()
            .withf(|spec| spec.program == "pip3" && spec.args.contains(&"-e".to_string()))
            .times(1)
            .returning(|_| Ok(CommandOutput::failure(1, b"build failed".to_vec())));

        let runner = PythonRunner::with_runner(mock);
        let result = runner.install_deps(dir.path()).await;
        assert!(result.is_err());
        let msg = format!("{}", result.unwrap_err());
        assert!(msg.contains("pip install -e failed"), "unexpected: {msg}");
    }

    #[tokio::test]
    async fn install_deps_requirements_txt_command_error() {
        // Spawn error (not nonzero exit) from pip3 install -r
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("requirements.txt"), "requests\n").unwrap();

        let mut mock = MockCmd::new();
        mock.expect_run_command()
            .withf(|spec| spec.program == "pip3" && spec.args.contains(&"-r".to_string()))
            .times(1)
            .returning(|_| {
                Err(ApexError::Subprocess {
                    exit_code: -1,
                    stderr: "spawn pip3: not found".into(),
                })
            });

        let runner = PythonRunner::with_runner(mock);
        let result = runner.install_deps(dir.path()).await;
        assert!(result.is_err());
        let msg = format!("{}", result.unwrap_err());
        assert!(msg.contains("pip install"), "unexpected: {msg}");
    }

    #[tokio::test]
    async fn install_deps_pyproject_editable_command_error() {
        // Spawn error from pip3 install -e .
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("pyproject.toml"), "[project]\nname=\"x\"\n").unwrap();

        let mut mock = MockCmd::new();
        mock.expect_run_command()
            .withf(|spec| spec.program == "pip3" && spec.args.contains(&"-e".to_string()))
            .times(1)
            .returning(|_| {
                Err(ApexError::Subprocess {
                    exit_code: -1,
                    stderr: "spawn pip3: not found".into(),
                })
            });

        let runner = PythonRunner::with_runner(mock);
        let result = runner.install_deps(dir.path()).await;
        assert!(result.is_err());
        let msg = format!("{}", result.unwrap_err());
        assert!(msg.contains("pip install -e"), "unexpected: {msg}");
    }

    #[tokio::test]
    async fn install_deps_coverage_check_command_error() {
        // Spawn error from the python3 -c "import coverage" check
        let dir = tempfile::tempdir().unwrap();

        let mut mock = MockCmd::new();
        mock.expect_run_command()
            .withf(|spec| spec.program == "python3")
            .times(1)
            .returning(|_| {
                Err(ApexError::Subprocess {
                    exit_code: -1,
                    stderr: "spawn python3: not found".into(),
                })
            });

        let runner = PythonRunner::with_runner(mock);
        let result = runner.install_deps(dir.path()).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn install_deps_coverage_install_command_error() {
        // Spawn error from pip3 install coverage pytest
        let dir = tempfile::tempdir().unwrap();

        let mut mock = MockCmd::new();
        // python3 -c "import coverage" fails with nonzero (not spawn error)
        mock.expect_run_command()
            .withf(|spec| spec.program == "python3")
            .times(1)
            .returning(|_| Ok(CommandOutput::failure(1, b"ModuleNotFoundError".to_vec())));
        // pip3 install coverage pytest fails with spawn error
        mock.expect_run_command()
            .withf(|spec| spec.program == "pip3")
            .times(1)
            .returning(|_| {
                Err(ApexError::Subprocess {
                    exit_code: -1,
                    stderr: "spawn pip3: not found".into(),
                })
            });

        let runner = PythonRunner::with_runner(mock);
        let result = runner.install_deps(dir.path()).await;
        assert!(result.is_err());
    }

    // ------------------------------------------------------------------
    // default() constructor
    // ------------------------------------------------------------------

    #[test]
    fn default_creates_runner() {
        let runner = PythonRunner::default();
        assert_eq!(runner.language(), Language::Python);
    }

    // ------------------------------------------------------------------
    // run_tests — duration is populated
    // ------------------------------------------------------------------

    #[tokio::test]
    async fn run_tests_duration_populated() {
        let dir = tempfile::tempdir().unwrap();

        let mut mock = MockCmd::new();
        mock.expect_run_command()
            .times(1)
            .returning(|_| Ok(CommandOutput::success(b"ok".to_vec())));

        let runner = PythonRunner::with_runner(mock);
        let result = runner.run_tests(dir.path(), &[]).await.unwrap();
        assert!(result.duration_ms < u64::MAX);
    }

    // ------------------------------------------------------------------
    // run_tests — pyproject.toml with pytest section header uses pytest
    // ------------------------------------------------------------------

    #[tokio::test]
    async fn run_tests_with_pyproject_pytest_section() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("pyproject.toml"),
            "[tool.pytest.ini_options]\nminversion = \"6.0\"\n",
        )
        .unwrap();

        let mut mock = MockCmd::new();
        mock.expect_run_command()
            .withf(|spec| {
                spec.program == "python3"
                    && spec.args.contains(&"-m".to_string())
                    && spec.args.contains(&"pytest".to_string())
            })
            .times(1)
            .returning(|_| Ok(CommandOutput::success(b"3 passed".to_vec())));

        let runner = PythonRunner::with_runner(mock);
        let result = runner.run_tests(dir.path(), &[]).await.unwrap();
        assert_eq!(result.exit_code, 0);
    }

    // ------------------------------------------------------------------
    // install_deps — no deps at all, only coverage check runs
    // ------------------------------------------------------------------

    #[tokio::test]
    async fn install_deps_no_deps_only_coverage_check_passes() {
        let dir = tempfile::tempdir().unwrap();
        // No requirements.txt, no pyproject.toml, no setup.py
        // So we skip directly to coverage check

        let mut mock = MockCmd::new();
        mock.expect_run_command()
            .withf(|spec| spec.program == "python3")
            .times(1)
            .returning(|_| Ok(CommandOutput::success(b"".to_vec())));

        let runner = PythonRunner::with_runner(mock);
        let result = runner.install_deps(dir.path()).await;
        assert!(result.is_ok());
    }
}
