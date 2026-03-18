use apex_core::{
    command::{CommandRunner, CommandSpec, RealCommandRunner},
    config::InstrumentTimeouts,
    error::{ApexError, Result},
    traits::{LanguageRunner, PreflightInfo, TestRunOutput},
    types::Language,
};
use async_trait::async_trait;
use std::{path::Path, time::Instant};
use tracing::{debug, info};

pub struct SwiftRunner<R: CommandRunner = RealCommandRunner> {
    runner: R,
    timeouts: InstrumentTimeouts,
}

impl SwiftRunner {
    pub fn new() -> Self {
        SwiftRunner {
            runner: RealCommandRunner,
            timeouts: InstrumentTimeouts::default(),
        }
    }
}

impl<R: CommandRunner> SwiftRunner<R> {
    pub fn with_runner(runner: R) -> Self {
        SwiftRunner {
            runner,
            timeouts: InstrumentTimeouts::default(),
        }
    }

    pub fn with_timeouts(mut self, timeouts: InstrumentTimeouts) -> Self {
        self.timeouts = timeouts;
        self
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

    /// Parse swift-tools-version from Package.swift.
    fn parse_tools_version(target: &Path) -> Option<String> {
        let content = std::fs::read_to_string(target.join("Package.swift")).ok()?;
        // First line is typically: // swift-tools-version:5.9
        let first_line = content.lines().next()?;
        if let Some(idx) = first_line.find("swift-tools-version") {
            let after = &first_line[idx + "swift-tools-version".len()..];
            let version = after.trim_start_matches(':').trim_start_matches('=').trim();
            if !version.is_empty() {
                return Some(version.to_string());
            }
        }
        None
    }

    /// Detect whether Xcode or CommandLineTools is the active toolchain.
    fn detect_toolchain() -> &'static str {
        if let Ok(output) = std::process::Command::new("xcode-select")
            .arg("-p")
            .output()
        {
            if output.status.success() {
                let path = String::from_utf8_lossy(&output.stdout);
                if path.contains("CommandLineTools") {
                    return "CommandLineTools";
                }
                if path.contains("Xcode") {
                    return "Xcode";
                }
            }
        }
        "unknown"
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

        // Use a project-local SPM cache so we never fail on a read-only global
        // cache directory (CI images, sandboxed runners, etc.).
        let spm_cache = target.join(".build").join("spm-cache");
        let spec = CommandSpec::new("swift", target)
            .args(["package", "resolve"])
            .env("SWIFTPM_CACHE_DIR", spm_cache.to_string_lossy())
            .timeout(self.timeouts.swift_resolve_ms);
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

    async fn run_tests(&self, target: &Path, extra_args: &[String]) -> Result<TestRunOutput> {
        info!(target = %target.display(), "running Swift tests");

        let start = Instant::now();
        let mut args: Vec<String> = vec!["test".into()];
        args.extend_from_slice(extra_args);
        let spec = CommandSpec::new("swift", target)
            .args(args)
            .timeout(self.timeouts.swift_test_ms);
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

    fn preflight_check(&self, target: &Path) -> Result<PreflightInfo> {
        let mut info = PreflightInfo::default();
        info.build_system = Some("swift-package-manager".into());
        info.package_manager = Some("swift-package-manager".into());
        info.test_framework = Some("XCTest".into());

        // Check swift binary
        if let Some(ver) = Self::tool_version("swift", &["--version"]) {
            info.available_tools.push(("swift".into(), ver));
        } else {
            info.missing_tools.push("swift".into());
        }

        // Detect toolchain
        let toolchain = Self::detect_toolchain();
        info.extra.push(("toolchain".into(), toolchain.into()));

        // Parse swift-tools-version
        if let Some(tools_ver) = Self::parse_tools_version(target) {
            info.extra.push(("swift-tools-version".into(), tools_ver));
        }

        // Check if Package.resolved exists (deps resolved)
        info.deps_installed = target.join("Package.resolved").exists()
            || target.join(".build").exists();

        // Check for code coverage support
        if let Some(ver) = Self::tool_version("xcrun", &["llvm-cov", "--version"]) {
            info.available_tools.push(("llvm-cov".into(), ver));
        } else {
            info.warnings.push(
                "llvm-cov not found via xcrun; code coverage may not work".into(),
            );
        }

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
    fn language_is_swift() {
        let runner = SwiftRunner::new();
        assert_eq!(runner.language(), Language::Swift);
    }

    #[test]
    fn detect_with_package_swift() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(
            tmp.path().join("Package.swift"),
            "// swift-tools-version:5.9",
        )
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
        assert!(err.contains("swift package resolve failed"), "got: {err}");
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
            .withf(|spec| {
                spec.program == "swift"
                    && spec.args == ["package", "resolve"]
                    && spec.timeout_ms == InstrumentTimeouts::default().swift_resolve_ms
                    && spec
                        .env
                        .iter()
                        .any(|(k, _)| k == "SWIFTPM_CACHE_DIR")
            })
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

    #[tokio::test]
    async fn run_tests_uses_extended_timeout() {
        let mut mock = MockCmd::new();
        mock.expect_run_command()
            .withf(|spec| {
                spec.program == "swift"
                    && spec.args == ["test"]
                    && spec.timeout_ms == InstrumentTimeouts::default().swift_test_ms
            })
            .returning(|_| {
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
    }

    // ------------------------------------------------------------------
    // preflight_check tests
    // ------------------------------------------------------------------

    #[test]
    fn preflight_check_basic() {
        let dir = tempfile::tempdir().unwrap();
        let runner = SwiftRunner::new();
        let info = runner.preflight_check(dir.path()).unwrap();
        assert_eq!(info.build_system.as_deref(), Some("swift-package-manager"));
        assert_eq!(info.test_framework.as_deref(), Some("XCTest"));
    }

    #[test]
    fn preflight_check_parses_tools_version() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("Package.swift"),
            "// swift-tools-version:5.9\nimport PackageDescription\n",
        )
        .unwrap();
        let runner = SwiftRunner::new();
        let info = runner.preflight_check(dir.path()).unwrap();
        assert!(
            info.extra
                .iter()
                .any(|(k, v)| k == "swift-tools-version" && v == "5.9"),
            "extra: {:?}",
            info.extra
        );
    }

    #[test]
    fn preflight_check_deps_installed_with_resolved() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("Package.resolved"), "{}").unwrap();
        let runner = SwiftRunner::new();
        let info = runner.preflight_check(dir.path()).unwrap();
        assert!(info.deps_installed);
    }

    #[test]
    fn preflight_check_deps_not_installed() {
        let dir = tempfile::tempdir().unwrap();
        let runner = SwiftRunner::new();
        let info = runner.preflight_check(dir.path()).unwrap();
        assert!(!info.deps_installed);
    }

    #[test]
    fn parse_tools_version_valid() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("Package.swift"),
            "// swift-tools-version:5.10\n",
        )
        .unwrap();
        let ver = SwiftRunner::<RealCommandRunner>::parse_tools_version(dir.path());
        assert_eq!(ver.as_deref(), Some("5.10"));
    }

    #[test]
    fn parse_tools_version_no_file() {
        let dir = tempfile::tempdir().unwrap();
        let ver = SwiftRunner::<RealCommandRunner>::parse_tools_version(dir.path());
        assert!(ver.is_none());
    }

    #[test]
    fn detect_toolchain_returns_string() {
        let toolchain = SwiftRunner::<RealCommandRunner>::detect_toolchain();
        assert!(!toolchain.is_empty());
    }
}
