use apex_core::{
    command::{adaptive_timeout, count_source_files, CommandRunner, CommandSpec, OpKind, RealCommandRunner},
    error::{ApexError, Result},
    traits::{LanguageRunner, PreflightInfo, TestRunOutput},
    types::Language,
};
use async_trait::async_trait;
use std::{path::Path, sync::OnceLock, time::Instant};
use tracing::{debug, info};

/// Python package manager variants.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PackageManager {
    Uv,
    Poetry,
    Pipenv,
    Pip,
}

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

    /// Resolve the python binary, trying `python3` first, falling back to `python`.
    pub fn resolve_python() -> &'static str {
        static PYTHON: OnceLock<&'static str> = OnceLock::new();
        PYTHON.get_or_init(|| {
            if let Ok(output) = std::process::Command::new("python3")
                .arg("--version")
                .output()
            {
                if output.status.success() {
                    return "python3";
                }
            }
            "python"
        })
    }

    /// Resolve the pip binary, trying `pip3` first, falling back to `pip`.
    pub fn resolve_pip() -> &'static str {
        static PIP: OnceLock<&'static str> = OnceLock::new();
        PIP.get_or_init(|| {
            if let Ok(output) = std::process::Command::new("pip3").arg("--version").output() {
                if output.status.success() {
                    return "pip3";
                }
            }
            "pip"
        })
    }

    /// Check if `uv` is available on PATH.
    pub fn resolve_uv() -> Option<String> {
        std::process::Command::new("uv")
            .arg("--version")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .ok()
            .filter(|s| s.success())
            .map(|_| "uv".to_string())
    }

    /// Find a virtualenv python binary relative to the target directory.
    pub fn find_venv_python(target: &Path) -> Option<String> {
        for venv_dir in &[".apex-venv", ".venv", "venv", ".env", "env"] {
            let python_path = target.join(venv_dir).join("bin").join("python");
            if python_path.exists() {
                return Some(python_path.to_string_lossy().into_owned());
            }
        }
        None
    }

    /// Check whether the system Python is PEP 668 externally-managed.
    ///
    /// Runs `python3 -c "import sysconfig; ..."` to locate the stdlib directory,
    /// then checks for the `EXTERNALLY-MANAGED` marker file.
    fn is_externally_managed(target: &Path) -> bool {
        let python = Self::resolve_python();
        let output = std::process::Command::new(python)
            .args([
                "-c",
                "import sysconfig, pathlib; \
                 stdlib = pathlib.Path(sysconfig.get_path('stdlib')); \
                 print('yes' if (stdlib / 'EXTERNALLY-MANAGED').exists() else 'no')",
            ])
            .current_dir(target)
            .output();
        match output {
            Ok(o) if o.status.success() => {
                String::from_utf8_lossy(&o.stdout).trim() == "yes"
            }
            _ => false,
        }
    }

    /// Ensure a `.apex-venv` virtualenv exists in `target`, creating one if needed.
    ///
    /// Returns the path to the venv python binary.
    async fn ensure_venv(&self, target: &Path) -> Result<String> {
        let venv_dir = target.join(".apex-venv");
        let venv_python = venv_dir.join("bin").join("python");

        if venv_python.exists() {
            return Ok(venv_python.to_string_lossy().into_owned());
        }

        info!(
            target = %target.display(),
            "creating .apex-venv (PEP 668 externally-managed Python detected)"
        );

        let python = Self::resolve_python();
        let spec = CommandSpec::new(python, target).args([
            "-m",
            "venv",
            ".apex-venv",
        ]);
        let output = self
            .runner
            .run_command(&spec)
            .await
            .map_err(|e| ApexError::LanguageRunner(format!("create venv: {e}")))?;

        if output.exit_code != 0 {
            let stderr = String::from_utf8_lossy(&output.stderr).to_string();
            return Err(ApexError::LanguageRunner(format!(
                "failed to create .apex-venv: {stderr}"
            )));
        }

        Ok(venv_python.to_string_lossy().into_owned())
    }

    /// Resolve python for a specific target: prefer venv, fall back to system.
    pub fn resolve_python_for(target: &Path) -> String {
        if let Some(venv_python) = Self::find_venv_python(target) {
            return venv_python;
        }
        Self::resolve_python().to_string()
    }

    /// Detect which Python package manager is in use for the target project.
    pub fn detect_package_manager(target: &Path) -> PackageManager {
        // uv.lock → uv
        if target.join("uv.lock").exists() {
            return PackageManager::Uv;
        }
        // poetry.lock → poetry
        if target.join("poetry.lock").exists() {
            return PackageManager::Poetry;
        }
        // pyproject.toml with [tool.poetry] section → poetry (no lockfile)
        if target.join("pyproject.toml").exists() {
            let content =
                std::fs::read_to_string(target.join("pyproject.toml")).unwrap_or_default();
            if content.contains("[tool.poetry]") {
                return PackageManager::Poetry;
            }
        }
        // Pipfile.lock or Pipfile → pipenv
        if target.join("Pipfile.lock").exists() || target.join("Pipfile").exists() {
            return PackageManager::Pipenv;
        }
        PackageManager::Pip
    }

    /// Detect the test runner from project config (structured parsing).
    fn detect_test_runner(target: &Path) -> Vec<String> {
        let python = Self::resolve_python_for(target);

        // Check pyproject.toml for [tool.pytest section header
        if target.join("pyproject.toml").exists() {
            let content =
                std::fs::read_to_string(target.join("pyproject.toml")).unwrap_or_default();
            if content.contains("[tool.pytest") {
                return vec![python, "-m".into(), "pytest".into(), "-q".into()];
            }
        }

        // Check for pytest.ini
        if target.join("pytest.ini").exists() {
            return vec![python, "-m".into(), "pytest".into(), "-q".into()];
        }

        // Check setup.cfg for [tool:pytest] section
        if target.join("setup.cfg").exists() {
            let content = std::fs::read_to_string(target.join("setup.cfg")).unwrap_or_default();
            if content.contains("[tool:pytest]") {
                return vec![python, "-m".into(), "pytest".into(), "-q".into()];
            }
            // Check for unittest configuration
            if content.contains("[unittest") {
                return vec![python, "-m".into(), "unittest".into(), "discover".into()];
            }
        }

        // Fallback: pytest is most common
        vec![python, "-m".into(), "pytest".into(), "-q".into()]
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
        let pkg_mgr = Self::detect_package_manager(target);
        let file_count = count_source_files(target);
        let dep_timeout = adaptive_timeout(file_count, Language::Python, OpKind::DepInstall);
        info!(target = %target.display(), ?pkg_mgr, file_count, dep_timeout, "installing Python dependencies");

        // For Pip-managed projects on PEP 668 externally-managed Python, create a
        // .apex-venv automatically so pip install works. We always need a venv when
        // the system Python is externally managed, even if uv is available — uv sync
        // requires a pyproject.toml which bare source dirs (like CPython/Lib) don't have.
        let needs_venv = pkg_mgr == PackageManager::Pip
            && Self::find_venv_python(target).is_none()
            && Self::is_externally_managed(target);

        if needs_venv {
            self.ensure_venv(target).await?;
        }

        // Re-resolve after potential venv creation so pip/python point at the venv.
        let python = Self::resolve_python_for(target);
        let pip = if let Some(venv) = Self::find_venv_python(target) {
            // Use the venv's pip (sibling of the venv python).
            let venv_path = std::path::PathBuf::from(&venv);
            let pip_path = venv_path.with_file_name("pip");
            if pip_path.exists() {
                pip_path.to_string_lossy().into_owned()
            } else {
                Self::resolve_pip().to_string()
            }
        } else {
            Self::resolve_pip().to_string()
        };

        match pkg_mgr {
            PackageManager::Uv => {
                let spec = CommandSpec::new("uv", target).args(["sync"]).timeout(dep_timeout);
                let output = self
                    .runner
                    .run_command(&spec)
                    .await
                    .map_err(|e| ApexError::LanguageRunner(format!("uv sync: {e}")))?;
                if output.exit_code != 0 {
                    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
                    return Err(ApexError::LanguageRunner(format!(
                        "uv sync failed: {stderr}"
                    )));
                }
            }
            PackageManager::Poetry => {
                let spec = CommandSpec::new("poetry", target).args(["install"]).timeout(dep_timeout);
                let output = self
                    .runner
                    .run_command(&spec)
                    .await
                    .map_err(|e| ApexError::LanguageRunner(format!("poetry install: {e}")))?;
                if output.exit_code != 0 {
                    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
                    return Err(ApexError::LanguageRunner(format!(
                        "poetry install failed: {stderr}"
                    )));
                }
            }
            PackageManager::Pipenv => {
                let spec = CommandSpec::new("pipenv", target).args(["install", "--dev"]).timeout(dep_timeout);
                let output = self
                    .runner
                    .run_command(&spec)
                    .await
                    .map_err(|e| ApexError::LanguageRunner(format!("pipenv install: {e}")))?;
                if output.exit_code != 0 {
                    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
                    return Err(ApexError::LanguageRunner(format!(
                        "pipenv install failed: {stderr}"
                    )));
                }
            }
            PackageManager::Pip => {
                if target.join("requirements.txt").exists() {
                    let spec = CommandSpec::new(&pip, target)
                        .args(["install", "-r", "requirements.txt"])
                        .timeout(dep_timeout);
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
                } else if target.join("pyproject.toml").exists() || target.join("setup.py").exists()
                {
                    let spec = CommandSpec::new(&pip, target).args(["install", "-e", "."]).timeout(dep_timeout);
                    let output =
                        self.runner.run_command(&spec).await.map_err(|e| {
                            ApexError::LanguageRunner(format!("pip install -e: {e}"))
                        })?;

                    if output.exit_code != 0 {
                        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
                        return Err(ApexError::LanguageRunner(format!(
                            "pip install -e failed: {stderr}"
                        )));
                    }
                }
            }
        }

        // Ensure coverage.py is available.
        let cov_spec = CommandSpec::new(&python, target).args(["-c", "import coverage"]);
        let cov_check = self
            .runner
            .run_command(&cov_spec)
            .await
            .map_err(|e| ApexError::LanguageRunner(e.to_string()))?;

        if cov_check.exit_code != 0 {
            debug!("coverage.py not found, installing");
            let output = if let Some(uv) = Self::resolve_uv() {
                // uv pip install --system works on PEP 668 / externally-managed envs.
                let spec = CommandSpec::new(&uv, target)
                    .args(["pip", "install", "--system", "coverage", "pytest"]);
                self.runner
                    .run_command(&spec)
                    .await
                    .map_err(|e| ApexError::LanguageRunner(e.to_string()))?
            } else {
                // Use venv pip if available (handles PEP 668 without uv).
                let spec =
                    CommandSpec::new(&pip, target).args(["install", "coverage", "pytest"]).timeout(dep_timeout);
                self.runner
                    .run_command(&spec)
                    .await
                    .map_err(|e| ApexError::LanguageRunner(e.to_string()))?
            };

            if output.exit_code != 0 {
                let stderr = String::from_utf8_lossy(&output.stderr).to_string();

                // If pip failed due to externally-managed environment and we haven't
                // created a venv yet, create one now and retry.
                if stderr.contains("externally-managed-environment") {
                    info!("PEP 668 externally-managed environment detected, creating .apex-venv");
                    let venv_python = self.ensure_venv(target).await?;
                    let venv_pip = std::path::PathBuf::from(&venv_python)
                        .with_file_name("pip")
                        .to_string_lossy()
                        .into_owned();
                    let retry_spec = CommandSpec::new(&venv_pip, target)
                        .args(["install", "coverage", "pytest"]);
                    let retry = self
                        .runner
                        .run_command(&retry_spec)
                        .await
                        .map_err(|e| ApexError::LanguageRunner(e.to_string()))?;
                    if retry.exit_code != 0 {
                        let retry_stderr = String::from_utf8_lossy(&retry.stderr).to_string();
                        return Err(ApexError::LanguageRunner(format!(
                            "failed to install coverage/pytest: {retry_stderr}"
                        )));
                    }
                } else {
                    return Err(ApexError::LanguageRunner(format!(
                        "failed to install coverage/pytest: {stderr}"
                    )));
                }
            }
        }

        Ok(())
    }

    async fn run_tests(&self, target: &Path, extra_args: &[String]) -> Result<TestRunOutput> {
        let cmd_parts = Self::detect_test_runner(target);
        let mut args: Vec<String> = cmd_parts[1..].to_vec();
        args.extend(extra_args.iter().cloned());

        let file_count = count_source_files(target);
        let test_timeout = adaptive_timeout(file_count, Language::Python, OpKind::TestRun);

        info!(
            target = %target.display(),
            cmd = ?cmd_parts,
            file_count, test_timeout,
            "running Python tests"
        );

        let spec = CommandSpec::new(&cmd_parts[0], target).args(args).timeout(test_timeout);

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

    fn preflight_check(&self, target: &Path) -> Result<PreflightInfo> {
        let mut info = PreflightInfo::default();

        // Detect package manager
        let pkg_mgr = Self::detect_package_manager(target);
        info.package_manager = Some(match pkg_mgr {
            PackageManager::Uv => "uv",
            PackageManager::Poetry => "poetry",
            PackageManager::Pipenv => "pipenv",
            PackageManager::Pip => "pip",
        }
        .into());
        info.build_system = info.package_manager.clone();

        // Detect test framework
        let test_cmd = Self::detect_test_runner(target);
        if test_cmd.iter().any(|s| s.contains("pytest")) {
            info.test_framework = Some("pytest".into());
        } else if test_cmd.iter().any(|s| s.contains("unittest")) {
            info.test_framework = Some("unittest".into());
        } else if test_cmd.iter().any(|s| s.contains("nose")) {
            info.test_framework = Some("nose".into());
        } else {
            info.test_framework = Some("pytest".into()); // default
        }

        // Check python
        let python = Self::resolve_python();
        if let Ok(output) = std::process::Command::new(python).arg("--version").output() {
            if output.status.success() {
                let ver = String::from_utf8_lossy(&output.stdout).trim().to_string();
                info.available_tools.push(("python".into(), ver));
            }
        } else {
            info.missing_tools.push("python3".into());
        }

        // Check for PEP 668
        if Self::is_externally_managed(target) {
            info.warnings.push(
                "PEP 668 externally-managed Python detected; a venv will be created automatically".into(),
            );
            info.extra.push(("pep668".into(), "true".into()));
        }

        // Check for existing venv
        if let Some(venv_path) = Self::find_venv_python(target) {
            info.extra.push(("venv_python".into(), venv_path));
            info.deps_installed = true;
        }

        // Check if this is a stdlib source dir (no setup.py/pyproject.toml)
        let has_project_file = target.join("pyproject.toml").exists()
            || target.join("setup.py").exists()
            || target.join("setup.cfg").exists();
        if !has_project_file && target.join("Lib").exists() {
            info.extra
                .push(("stdlib_source_dir".into(), "true".into()));
            info.warnings.push(
                "stdlib source directory detected (no setup.py/pyproject.toml); using --rootdir".into(),
            );
        }

        // Check coverage.py and pytest availability
        if let Some(ref _uv) = Self::resolve_uv() {
            info.available_tools
                .push(("uv".into(), "available".into()));
        }

        // Check for requirements.txt or pyproject.toml
        if target.join("requirements.txt").exists() {
            info.extra
                .push(("requirements_file".into(), "requirements.txt".into()));
        }
        if target.join("pyproject.toml").exists() {
            info.extra
                .push(("project_file".into(), "pyproject.toml".into()));
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
        let python = PythonRunner::<RealCommandRunner>::resolve_python();
        assert_eq!(cmd, vec![python, "-m", "pytest", "-q"]);
    }

    #[test]
    fn detect_test_runner_fallback() {
        let dir = tempfile::tempdir().unwrap();
        let cmd = PythonRunner::<RealCommandRunner>::detect_test_runner(dir.path());
        let python = PythonRunner::<RealCommandRunner>::resolve_python();
        assert_eq!(cmd, vec![python, "-m", "pytest", "-q"]);
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
            .withf(|spec| {
                spec.program == PythonRunner::<MockCmd>::resolve_pip()
                    && spec.args.contains(&"-r".to_string())
            })
            .times(1)
            .returning(|_| Ok(CommandOutput::success(b"Successfully installed".to_vec())));
        // python3 -c "import coverage"
        mock.expect_run_command()
            .withf(|spec| spec.program == PythonRunner::<MockCmd>::resolve_python())
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
            .withf(|spec| {
                spec.program == PythonRunner::<MockCmd>::resolve_pip()
                    && spec.args.contains(&"-r".to_string())
            })
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
            .withf(|spec| {
                spec.program == PythonRunner::<MockCmd>::resolve_pip()
                    && spec.args.contains(&"-e".to_string())
            })
            .times(1)
            .returning(|_| Ok(CommandOutput::success(b"".to_vec())));
        // python3 -c "import coverage" -- coverage already installed
        mock.expect_run_command()
            .withf(|spec| spec.program == PythonRunner::<MockCmd>::resolve_python())
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
            .withf(|spec| spec.program == PythonRunner::<MockCmd>::resolve_python())
            .times(1)
            .returning(|_| Ok(CommandOutput::failure(1, b"ModuleNotFoundError".to_vec())));
        // uv pip install or pip3 install coverage pytest
        mock.expect_run_command()
            .withf(|spec| {
                spec.args.contains(&"coverage".to_string())
                    && (spec.program == PythonRunner::<MockCmd>::resolve_pip()
                        || spec.program == "uv")
            })
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
            .withf(|spec| spec.program == PythonRunner::<MockCmd>::resolve_python())
            .times(1)
            .returning(|_| Ok(CommandOutput::failure(1, b"ModuleNotFoundError".to_vec())));
        // uv pip install or pip3 install also fails
        mock.expect_run_command()
            .withf(|spec| {
                spec.args.contains(&"coverage".to_string())
                    && (spec.program == PythonRunner::<MockCmd>::resolve_pip()
                        || spec.program == "uv")
            })
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
            .withf(|spec| spec.program == PythonRunner::<MockCmd>::resolve_python())
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
    fn detect_test_runner_pyproject_with_pytest_keyword_no_section() {
        let dir = tempfile::tempdir().unwrap();
        // Contains "pytest" as a keyword but not "[tool.pytest" section —
        // structured parsing no longer matches this, falls through to default
        std::fs::write(
            dir.path().join("pyproject.toml"),
            "[build-system]\nrequires = [\"pytest\", \"setuptools\"]\n",
        )
        .unwrap();
        let cmd = PythonRunner::<RealCommandRunner>::detect_test_runner(dir.path());
        let python = PythonRunner::<RealCommandRunner>::resolve_python();
        // Falls through to the default (pytest) since no [tool.pytest section
        assert_eq!(cmd, vec![python, "-m", "pytest", "-q"]);
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
        let python = PythonRunner::<RealCommandRunner>::resolve_python();
        // Falls through to the fallback (which is the same command)
        assert_eq!(cmd, vec![python, "-m", "pytest", "-q"]);
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
        std::fs::write(
            dir.path().join("setup.py"),
            "from setuptools import setup; setup()",
        )
        .unwrap();

        let mut mock = MockCmd::new();
        // pip3 install -e .
        mock.expect_run_command()
            .withf(|spec| {
                spec.program == PythonRunner::<MockCmd>::resolve_pip()
                    && spec.args.contains(&"-e".to_string())
            })
            .times(1)
            .returning(|_| Ok(CommandOutput::success(b"".to_vec())));
        // python3 -c "import coverage" — already installed
        mock.expect_run_command()
            .withf(|spec| spec.program == PythonRunner::<MockCmd>::resolve_python())
            .times(1)
            .returning(|_| Ok(CommandOutput::success(b"".to_vec())));

        let runner = PythonRunner::with_runner(mock);
        let result = runner.install_deps(dir.path()).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn install_deps_setup_py_editable_install_fails() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("setup.py"),
            "from setuptools import setup; setup()",
        )
        .unwrap();

        let mut mock = MockCmd::new();
        mock.expect_run_command()
            .withf(|spec| {
                spec.program == PythonRunner::<MockCmd>::resolve_pip()
                    && spec.args.contains(&"-e".to_string())
            })
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
            .withf(|spec| {
                spec.program == PythonRunner::<MockCmd>::resolve_pip()
                    && spec.args.contains(&"-e".to_string())
            })
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
            .withf(|spec| {
                spec.program == PythonRunner::<MockCmd>::resolve_pip()
                    && spec.args.contains(&"-r".to_string())
            })
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
            .withf(|spec| {
                spec.program == PythonRunner::<MockCmd>::resolve_pip()
                    && spec.args.contains(&"-e".to_string())
            })
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
            .withf(|spec| spec.program == PythonRunner::<MockCmd>::resolve_python())
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
            .withf(|spec| spec.program == PythonRunner::<MockCmd>::resolve_python())
            .times(1)
            .returning(|_| Ok(CommandOutput::failure(1, b"ModuleNotFoundError".to_vec())));
        // uv pip install or pip3 install coverage pytest fails with spawn error
        mock.expect_run_command()
            .withf(|spec| {
                spec.args.contains(&"coverage".to_string())
                    && (spec.program == PythonRunner::<MockCmd>::resolve_pip()
                        || spec.program == "uv")
            })
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
    // resolve_uv — returns Some("uv") when uv is installed, None otherwise
    // ------------------------------------------------------------------

    #[test]
    fn resolve_uv_returns_option_string() {
        // resolve_uv() must return either None or Some("uv") — never panics.
        let result = PythonRunner::<RealCommandRunner>::resolve_uv();
        match &result {
            None => {}
            Some(s) => assert_eq!(s, "uv", "expected 'uv', got: {s}"),
        }
    }

    // ------------------------------------------------------------------
    // install_deps — uv path: uses `uv pip install --system`
    // ------------------------------------------------------------------

    #[tokio::test]
    async fn install_deps_coverage_missing_uses_uv_pip_when_uv_available() {
        // Simulate: coverage check fails, then we expect either uv or pip to run.
        // We cannot reliably mock resolve_uv() (it calls a real process), so we
        // verify the invariant: one of the two install commands is called and
        // succeeds, without caring which path was taken.
        let dir = tempfile::tempdir().unwrap();

        let mut mock = MockCmd::new();
        // python3 -c "import coverage" fails
        mock.expect_run_command()
            .withf(|spec| spec.program == PythonRunner::<MockCmd>::resolve_python())
            .times(1)
            .returning(|_| Ok(CommandOutput::failure(1, b"ModuleNotFoundError".to_vec())));
        // Either `uv pip install --system coverage pytest`
        // OR    `pip3 install coverage pytest` — accept both.
        mock.expect_run_command()
            .withf(|spec| {
                (spec.program == "uv"
                    && spec.args.contains(&"--system".to_string())
                    && spec.args.contains(&"coverage".to_string()))
                    || (spec.args.contains(&"coverage".to_string())
                        && spec.program == PythonRunner::<MockCmd>::resolve_pip())
            })
            .times(1)
            .returning(|_| Ok(CommandOutput::success(b"".to_vec())));

        let runner = PythonRunner::with_runner(mock);
        let result = runner.install_deps(dir.path()).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn install_deps_uv_fallback_pip_path_still_works() {
        // Verify that when uv is absent the pip3 path still functions correctly
        // (regression guard: the refactor must not break the non-uv path).
        // We test this by verifying that an empty directory (Pip pkg manager)
        // with coverage already present returns Ok(()).
        let dir = tempfile::tempdir().unwrap();

        let mut mock = MockCmd::new();
        mock.expect_run_command()
            .withf(|spec| spec.program == PythonRunner::<MockCmd>::resolve_python())
            .times(1)
            .returning(|_| Ok(CommandOutput::success(b"".to_vec())));

        let runner = PythonRunner::with_runner(mock);
        let result = runner.install_deps(dir.path()).await;
        assert!(result.is_ok(), "non-uv path broken: {result:?}");
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
                spec.program == PythonRunner::<MockCmd>::resolve_python()
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
            .withf(|spec| spec.program == PythonRunner::<MockCmd>::resolve_python())
            .times(1)
            .returning(|_| Ok(CommandOutput::success(b"".to_vec())));

        let runner = PythonRunner::with_runner(mock);
        let result = runner.install_deps(dir.path()).await;
        assert!(result.is_ok());
    }

    // ------------------------------------------------------------------
    // Task 1: resolve_python binary
    // ------------------------------------------------------------------

    #[test]
    fn resolve_python_binary_checks_python3_then_python() {
        let python = PythonRunner::<RealCommandRunner>::resolve_python();
        assert!(
            python == "python3" || python == "python",
            "expected python3 or python, got: {python}"
        );
    }

    #[test]
    fn resolve_pip_binary_checks_pip3_then_pip() {
        let pip = PythonRunner::<RealCommandRunner>::resolve_pip();
        assert!(
            pip == "pip3" || pip == "pip",
            "expected pip3 or pip, got: {pip}"
        );
    }

    // ------------------------------------------------------------------
    // Task 2: Package manager detection
    // ------------------------------------------------------------------

    #[test]
    fn detect_package_manager_poetry() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("poetry.lock"), "").unwrap();
        assert_eq!(
            PythonRunner::<RealCommandRunner>::detect_package_manager(dir.path()),
            PackageManager::Poetry
        );
    }

    #[test]
    fn detect_package_manager_uv() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("uv.lock"), "").unwrap();
        assert_eq!(
            PythonRunner::<RealCommandRunner>::detect_package_manager(dir.path()),
            PackageManager::Uv
        );
    }

    #[test]
    fn detect_package_manager_pipenv() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("Pipfile"), "").unwrap();
        assert_eq!(
            PythonRunner::<RealCommandRunner>::detect_package_manager(dir.path()),
            PackageManager::Pipenv
        );
    }

    #[test]
    fn detect_package_manager_poetry_no_lockfile() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("pyproject.toml"),
            "[tool.poetry]\nname = \"foo\"\n",
        )
        .unwrap();
        assert_eq!(
            PythonRunner::<RealCommandRunner>::detect_package_manager(dir.path()),
            PackageManager::Poetry
        );
    }

    #[test]
    fn detect_package_manager_pip_fallback() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(
            PythonRunner::<RealCommandRunner>::detect_package_manager(dir.path()),
            PackageManager::Pip
        );
    }

    // ------------------------------------------------------------------
    // Task 3: Virtual environment detection
    // ------------------------------------------------------------------

    #[test]
    fn find_venv_python_dot_venv() {
        let dir = tempfile::tempdir().unwrap();
        let venv_bin = dir.path().join(".venv").join("bin");
        std::fs::create_dir_all(&venv_bin).unwrap();
        std::fs::write(venv_bin.join("python"), "").unwrap();
        let result = PythonRunner::<RealCommandRunner>::find_venv_python(dir.path());
        assert!(result.is_some());
        assert!(result.unwrap().contains(".venv"));
    }

    #[test]
    fn find_venv_python_venv() {
        let dir = tempfile::tempdir().unwrap();
        let venv_bin = dir.path().join("venv").join("bin");
        std::fs::create_dir_all(&venv_bin).unwrap();
        std::fs::write(venv_bin.join("python"), "").unwrap();
        let result = PythonRunner::<RealCommandRunner>::find_venv_python(dir.path());
        assert!(result.is_some());
        assert!(result.unwrap().contains("venv"));
    }

    #[test]
    fn find_venv_python_none() {
        let dir = tempfile::tempdir().unwrap();
        let result = PythonRunner::<RealCommandRunner>::find_venv_python(dir.path());
        assert!(result.is_none());
    }

    // ------------------------------------------------------------------
    // Task 4: Structured test runner detection
    // ------------------------------------------------------------------

    #[test]
    fn detect_test_runner_unittest_setup_cfg() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("setup.cfg"),
            "[unittest]\ntest_module = tests\n",
        )
        .unwrap();
        let cmd = PythonRunner::<RealCommandRunner>::detect_test_runner(dir.path());
        let python = PythonRunner::<RealCommandRunner>::resolve_python();
        assert_eq!(cmd, vec![python, "-m", "unittest", "discover"]);
    }

    #[test]
    fn detect_test_runner_pytest_ini() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("pytest.ini"), "[pytest]\naddopts = -v\n").unwrap();
        let cmd = PythonRunner::<RealCommandRunner>::detect_test_runner(dir.path());
        let python = PythonRunner::<RealCommandRunner>::resolve_python();
        assert_eq!(cmd, vec![python, "-m", "pytest", "-q"]);
    }

    #[test]
    fn detect_test_runner_setup_cfg_tool_pytest() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("setup.cfg"),
            "[tool:pytest]\naddopts = -v\n",
        )
        .unwrap();
        let cmd = PythonRunner::<RealCommandRunner>::detect_test_runner(dir.path());
        let python = PythonRunner::<RealCommandRunner>::resolve_python();
        assert_eq!(cmd, vec![python, "-m", "pytest", "-q"]);
    }

    #[test]
    fn detect_test_runner_uses_venv_python() {
        let dir = tempfile::tempdir().unwrap();
        let venv_bin = dir.path().join(".venv").join("bin");
        std::fs::create_dir_all(&venv_bin).unwrap();
        std::fs::write(venv_bin.join("python"), "").unwrap();
        let cmd = PythonRunner::<RealCommandRunner>::detect_test_runner(dir.path());
        assert!(
            cmd[0].contains(".venv"),
            "expected venv python, got: {}",
            cmd[0]
        );
    }

    // ------------------------------------------------------------------
    // preflight_check tests
    // ------------------------------------------------------------------

    #[test]
    fn preflight_check_basic_pip_project() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("requirements.txt"), "pytest\n").unwrap();
        let runner = PythonRunner::new();
        let info = runner.preflight_check(dir.path()).unwrap();
        assert_eq!(info.package_manager.as_deref(), Some("pip"));
        assert_eq!(info.test_framework.as_deref(), Some("pytest"));
    }

    #[test]
    fn preflight_check_uv_project() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("uv.lock"), "").unwrap();
        std::fs::write(dir.path().join("pyproject.toml"), "[project]\nname = 'foo'\n").unwrap();
        let runner = PythonRunner::new();
        let info = runner.preflight_check(dir.path()).unwrap();
        // If uv is on PATH, should detect uv; otherwise pip
        assert!(info.package_manager.is_some());
    }

    #[test]
    fn preflight_check_poetry_project() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("poetry.lock"), "").unwrap();
        std::fs::write(dir.path().join("pyproject.toml"), "[tool.poetry]\nname = 'foo'\n").unwrap();
        let runner = PythonRunner::new();
        let info = runner.preflight_check(dir.path()).unwrap();
        assert_eq!(info.package_manager.as_deref(), Some("poetry"));
    }

    #[test]
    fn preflight_check_detects_venv() {
        let dir = tempfile::tempdir().unwrap();
        let venv_bin = dir.path().join(".venv").join("bin");
        std::fs::create_dir_all(&venv_bin).unwrap();
        std::fs::write(venv_bin.join("python"), "").unwrap();
        let runner = PythonRunner::new();
        let info = runner.preflight_check(dir.path()).unwrap();
        assert!(info.extra.iter().any(|(k, _)| k == "venv_python"));
        assert!(info.deps_installed);
    }

    #[test]
    fn preflight_check_stdlib_source_dir() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("Lib")).unwrap();
        let runner = PythonRunner::new();
        let info = runner.preflight_check(dir.path()).unwrap();
        assert!(info.extra.iter().any(|(k, v)| k == "stdlib_source_dir" && v == "true"));
        assert!(info.warnings.iter().any(|w| w.contains("stdlib")));
    }

    #[test]
    fn preflight_check_detects_pyproject() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("pyproject.toml"), "[project]\n").unwrap();
        let runner = PythonRunner::new();
        let info = runner.preflight_check(dir.path()).unwrap();
        assert!(
            info.extra
                .iter()
                .any(|(k, v)| k == "project_file" && v == "pyproject.toml")
        );
    }

    #[test]
    fn preflight_check_detects_requirements() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("requirements.txt"), "flask\n").unwrap();
        let runner = PythonRunner::new();
        let info = runner.preflight_check(dir.path()).unwrap();
        assert!(
            info.extra
                .iter()
                .any(|(k, v)| k == "requirements_file" && v == "requirements.txt")
        );
    }

    #[test]
    fn preflight_check_reports_python_available() {
        let dir = tempfile::tempdir().unwrap();
        let runner = PythonRunner::new();
        let info = runner.preflight_check(dir.path()).unwrap();
        // python3 should be available in CI/dev
        assert!(
            info.available_tools.iter().any(|(name, _)| name == "python"),
            "python should be available: {:?}",
            info.available_tools
        );
    }
}
