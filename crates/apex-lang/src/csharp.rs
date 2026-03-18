use apex_core::{
    command::{CommandRunner, CommandSpec, RealCommandRunner},
    config::InstrumentTimeouts,
    error::{ApexError, Result},
    traits::{LanguageRunner, PreflightInfo, TestRunOutput},
    types::Language,
};
use async_trait::async_trait;
use std::path::Path;
use std::time::Instant;
use tracing::{debug, info};

/// Augment `PATH` with common dotnet install locations so the binary is found
/// even when the user's shell profile hasn't been sourced (e.g. inside CI or
/// a sandboxed subprocess).
fn dotnet_path() -> String {
    let mut paths: Vec<String> = Vec::new();

    // ~/.dotnet (dotnet-install.sh default)
    if let Some(home) = std::env::var_os("HOME") {
        let home_dotnet = Path::new(&home).join(".dotnet");
        if home_dotnet.is_dir() {
            paths.push(home_dotnet.to_string_lossy().into_owned());
        }
    }

    // DOTNET_ROOT takes precedence when set explicitly.
    if let Ok(root) = std::env::var("DOTNET_ROOT") {
        if Path::new(&root).is_dir() {
            paths.push(root);
        }
    }

    // macOS .pkg installer location and common Linux paths.
    for candidate in ["/usr/local/share/dotnet", "/usr/share/dotnet"] {
        if Path::new(candidate).is_dir() {
            paths.push(candidate.to_string());
        }
    }

    // Append current PATH so existing entries are preserved.
    if let Ok(current) = std::env::var("PATH") {
        paths.push(current);
    }

    paths.join(":")
}

pub struct CSharpRunner<R: CommandRunner = RealCommandRunner> {
    runner: R,
    timeouts: InstrumentTimeouts,
}

impl CSharpRunner {
    pub fn new() -> Self {
        CSharpRunner {
            runner: RealCommandRunner,
            timeouts: InstrumentTimeouts::default(),
        }
    }
}

impl<R: CommandRunner> CSharpRunner<R> {
    pub fn with_runner(runner: R) -> Self {
        CSharpRunner {
            runner,
            timeouts: InstrumentTimeouts::default(),
        }
    }

    pub fn with_timeouts(mut self, timeouts: InstrumentTimeouts) -> Self {
        self.timeouts = timeouts;
        self
    }

    /// Check if a binary is on the augmented dotnet PATH and return its version string.
    fn dotnet_version() -> Option<String> {
        let output = std::process::Command::new("dotnet")
            .arg("--version")
            .env("PATH", dotnet_path())
            .output()
            .ok()?;
        if !output.status.success() {
            return None;
        }
        Some(
            String::from_utf8_lossy(&output.stdout)
                .trim()
                .to_string(),
        )
    }

    /// Detect whether the project has a solution file or just project files.
    fn detect_project_structure(target: &Path) -> (&'static str, Vec<String>) {
        let mut projects = Vec::new();
        if let Ok(entries) = std::fs::read_dir(target) {
            for entry in entries.flatten() {
                if let Some(ext) = entry.path().extension() {
                    if ext == "sln" {
                        return ("solution", vec![entry.file_name().to_string_lossy().into()]);
                    }
                    if ext == "csproj" {
                        projects.push(entry.file_name().to_string_lossy().into());
                    }
                }
            }
        }
        if projects.is_empty() {
            ("none", projects)
        } else {
            ("project", projects)
        }
    }

    /// Check if coverlet is referenced in any csproj file.
    fn has_coverlet(target: &Path) -> bool {
        if let Ok(entries) = std::fs::read_dir(target) {
            for entry in entries.flatten() {
                if entry.path().extension().and_then(|e| e.to_str()) == Some("csproj") {
                    if let Ok(content) = std::fs::read_to_string(entry.path()) {
                        if content.contains("coverlet") || content.contains("Coverlet") {
                            return true;
                        }
                    }
                }
            }
        }
        false
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

        let spec = CommandSpec::new("dotnet", target)
            .args(["restore"])
            .env("PATH", dotnet_path())
            .timeout(self.timeouts.csharp_restore_ms);
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

    async fn run_tests(&self, target: &Path, extra_args: &[String]) -> Result<TestRunOutput> {
        info!(target = %target.display(), "running C# tests");

        let start = Instant::now();
        let mut args: Vec<String> = vec!["test".into()];
        args.extend_from_slice(extra_args);
        let spec = CommandSpec::new("dotnet", target)
            .args(args)
            .env("PATH", dotnet_path())
            .timeout(self.timeouts.csharp_ms);
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

    fn preflight_check(&self, target: &Path) -> Result<PreflightInfo> {
        let mut info = PreflightInfo::default();
        info.build_system = Some("dotnet".into());
        info.package_manager = Some("nuget".into());

        // Check dotnet CLI
        if let Some(ver) = Self::dotnet_version() {
            info.available_tools.push(("dotnet".into(), ver));
        } else {
            info.missing_tools.push("dotnet".into());
            info.warnings.push(
                "dotnet not found on PATH (checked ~/.dotnet, DOTNET_ROOT, /usr/local/share/dotnet)".into(),
            );
        }

        // Detect project structure
        let (structure, projects) = Self::detect_project_structure(target);
        info.extra.push(("project_structure".into(), structure.into()));
        for p in &projects {
            info.extra.push(("project_file".into(), p.clone()));
        }

        // Detect test framework from csproj contents
        let mut test_framework = None;
        if let Ok(entries) = std::fs::read_dir(target) {
            for entry in entries.flatten() {
                if entry.path().extension().and_then(|e| e.to_str()) == Some("csproj") {
                    if let Ok(content) = std::fs::read_to_string(entry.path()) {
                        if content.contains("xunit") || content.contains("xUnit") {
                            test_framework = Some("xUnit");
                        } else if content.contains("NUnit") || content.contains("nunit") {
                            test_framework = Some("NUnit");
                        } else if content.contains("MSTest") {
                            test_framework = Some("MSTest");
                        }
                    }
                }
            }
        }
        info.test_framework = test_framework.map(|s| s.to_string());

        // Check coverlet
        if Self::has_coverlet(target) {
            info.extra.push(("coverlet".into(), "true".into()));
        } else {
            info.warnings.push(
                "coverlet not found in project dependencies; code coverage collection may fail".into(),
            );
        }

        // Check if obj/ or bin/ exist (project has been built)
        info.deps_installed = target.join("obj").exists() || target.join("bin").exists();

        Ok(info)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use apex_core::command::CommandOutput;
    use apex_core::config::InstrumentTimeouts;
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
            .withf(|spec| {
                spec.program == "dotnet"
                    && spec.args == ["restore"]
                    && spec.timeout_ms == InstrumentTimeouts::default().csharp_restore_ms
            })
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

    #[tokio::test]
    async fn run_tests_checks_timeout() {
        let mut mock = MockCmd::new();
        mock.expect_run_command()
            .withf(|spec| {
                spec.program == "dotnet"
                    && spec.args[0] == "test"
                    && spec.timeout_ms == InstrumentTimeouts::default().csharp_ms
            })
            .returning(|_| {
                Ok(CommandOutput {
                    exit_code: 0,
                    stdout: b"Passed!\n".to_vec(),
                    stderr: Vec::new(),
                })
            });
        let runner = CSharpRunner::with_runner(mock);
        let tmp = tempfile::tempdir().unwrap();
        let result = runner.run_tests(tmp.path(), &[]).await.unwrap();
        assert_eq!(result.exit_code, 0);
    }

    // ------------------------------------------------------------------
    // preflight_check tests
    // ------------------------------------------------------------------

    #[test]
    fn preflight_check_basic() {
        let dir = tempfile::tempdir().unwrap();
        let runner = CSharpRunner::new();
        let info = runner.preflight_check(dir.path()).unwrap();
        assert_eq!(info.build_system.as_deref(), Some("dotnet"));
        assert_eq!(info.package_manager.as_deref(), Some("nuget"));
    }

    #[test]
    fn preflight_check_detects_solution() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("MyApp.sln"), "").unwrap();
        let runner = CSharpRunner::new();
        let info = runner.preflight_check(dir.path()).unwrap();
        assert!(
            info.extra
                .iter()
                .any(|(k, v)| k == "project_structure" && v == "solution"),
            "extra: {:?}",
            info.extra
        );
    }

    #[test]
    fn preflight_check_detects_project() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("MyApp.csproj"), "<Project />").unwrap();
        let runner = CSharpRunner::new();
        let info = runner.preflight_check(dir.path()).unwrap();
        assert!(
            info.extra
                .iter()
                .any(|(k, v)| k == "project_structure" && v == "project"),
            "extra: {:?}",
            info.extra
        );
    }

    #[test]
    fn preflight_check_detects_xunit() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("Tests.csproj"),
            r#"<Project><ItemGroup><PackageReference Include="xunit" /></ItemGroup></Project>"#,
        )
        .unwrap();
        let runner = CSharpRunner::new();
        let info = runner.preflight_check(dir.path()).unwrap();
        assert_eq!(info.test_framework.as_deref(), Some("xUnit"));
    }

    #[test]
    fn preflight_check_detects_nunit() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("Tests.csproj"),
            r#"<Project><ItemGroup><PackageReference Include="NUnit" /></ItemGroup></Project>"#,
        )
        .unwrap();
        let runner = CSharpRunner::new();
        let info = runner.preflight_check(dir.path()).unwrap();
        assert_eq!(info.test_framework.as_deref(), Some("NUnit"));
    }

    #[test]
    fn preflight_check_detects_coverlet() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("Tests.csproj"),
            r#"<Project><ItemGroup><PackageReference Include="coverlet.msbuild" /></ItemGroup></Project>"#,
        )
        .unwrap();
        let runner = CSharpRunner::new();
        let info = runner.preflight_check(dir.path()).unwrap();
        assert!(info.extra.iter().any(|(k, v)| k == "coverlet" && v == "true"));
    }

    #[test]
    fn preflight_check_warns_no_coverlet() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("Tests.csproj"),
            r#"<Project><ItemGroup><PackageReference Include="xunit" /></ItemGroup></Project>"#,
        )
        .unwrap();
        let runner = CSharpRunner::new();
        let info = runner.preflight_check(dir.path()).unwrap();
        assert!(info.warnings.iter().any(|w| w.contains("coverlet")));
    }

    #[test]
    fn has_coverlet_true() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("Test.csproj"),
            "<PackageReference Include=\"coverlet.collector\" />",
        )
        .unwrap();
        assert!(CSharpRunner::<RealCommandRunner>::has_coverlet(dir.path()));
    }

    #[test]
    fn has_coverlet_false() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("Test.csproj"), "<Project />").unwrap();
        assert!(!CSharpRunner::<RealCommandRunner>::has_coverlet(dir.path()));
    }

    #[test]
    fn detect_project_structure_none() {
        let dir = tempfile::tempdir().unwrap();
        let (s, p) = CSharpRunner::<RealCommandRunner>::detect_project_structure(dir.path());
        assert_eq!(s, "none");
        assert!(p.is_empty());
    }
}
