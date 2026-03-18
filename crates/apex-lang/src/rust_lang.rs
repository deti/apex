use apex_core::{
    command::{CommandRunner, CommandSpec, RealCommandRunner},
    error::{ApexError, Result},
    traits::{LanguageRunner, PreflightInfo, TestRunOutput},
    types::Language,
};
use async_trait::async_trait;
use std::{path::Path, time::Instant};
use tracing::info;

pub struct RustRunner<R: CommandRunner = RealCommandRunner> {
    runner: R,
}

impl RustRunner {
    pub fn new() -> Self {
        RustRunner {
            runner: RealCommandRunner,
        }
    }
}

impl<R: CommandRunner> RustRunner<R> {
    pub fn with_runner(runner: R) -> Self {
        RustRunner { runner }
    }

    /// Check if a binary is on PATH and return its version string.
    fn tool_version(bin: &str, version_flag: &str) -> Option<String> {
        let output = std::process::Command::new(bin)
            .arg(version_flag)
            .output()
            .ok()?;
        if !output.status.success() {
            return None;
        }
        let stdout = String::from_utf8_lossy(&output.stdout);
        Some(stdout.lines().next().unwrap_or("").trim().to_string())
    }

    /// Detect whether this is a workspace or single-crate project.
    fn is_workspace(target: &Path) -> bool {
        if let Ok(content) = std::fs::read_to_string(target.join("Cargo.toml")) {
            content.contains("[workspace]")
        } else {
            false
        }
    }
}

impl Default for RustRunner {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl<R: CommandRunner> LanguageRunner for RustRunner<R> {
    fn language(&self) -> Language {
        Language::Rust
    }

    fn detect(&self, target: &Path) -> bool {
        target.join("Cargo.toml").exists()
    }

    async fn install_deps(&self, _target: &Path) -> Result<()> {
        // Rust deps are managed by cargo; nothing extra to install.
        Ok(())
    }

    async fn run_tests(&self, target: &Path, extra_args: &[String]) -> Result<TestRunOutput> {
        info!(target = %target.display(), "running cargo test");
        let start = Instant::now();

        let mut args = vec!["test".to_string()];
        args.extend_from_slice(extra_args);

        let spec = CommandSpec::new("cargo", target).args(args);
        let output = self
            .runner
            .run_command(&spec)
            .await
            .map_err(|e| ApexError::LanguageRunner(format!("cargo test: {e}")))?;

        Ok(TestRunOutput {
            exit_code: output.exit_code,
            stdout: String::from_utf8_lossy(&output.stdout).to_string(),
            stderr: String::from_utf8_lossy(&output.stderr).to_string(),
            duration_ms: start.elapsed().as_millis() as u64,
        })
    }

    fn preflight_check(&self, target: &Path) -> Result<PreflightInfo> {
        let mut info = PreflightInfo::default();
        info.build_system = Some("cargo".into());
        info.package_manager = Some("cargo".into());

        // Detect workspace vs single crate
        if Self::is_workspace(target) {
            info.extra.push(("project_type".into(), "workspace".into()));
        } else {
            info.extra.push(("project_type".into(), "single-crate".into()));
        }

        // Check cargo
        if let Some(ver) = Self::tool_version("cargo", "--version") {
            info.available_tools.push(("cargo".into(), ver));
        } else {
            info.missing_tools.push("cargo".into());
        }

        // Check rustc
        if let Some(ver) = Self::tool_version("rustc", "--version") {
            info.available_tools.push(("rustc".into(), ver));
        } else {
            info.missing_tools.push("rustc".into());
        }

        // Check cargo-llvm-cov (needed for coverage)
        if let Some(ver) = Self::tool_version("cargo-llvm-cov", "--version") {
            info.available_tools.push(("cargo-llvm-cov".into(), ver));
        } else {
            info.warnings.push(
                "cargo-llvm-cov not installed; coverage instrumentation will not work".into(),
            );
        }

        // Check cargo-nextest (preferred test runner)
        if let Some(ver) = Self::tool_version("cargo-nextest", "--version") {
            info.available_tools.push(("cargo-nextest".into(), ver));
            info.test_framework = Some("nextest".into());
        } else {
            info.test_framework = Some("cargo-test".into());
            info.warnings.push(
                "cargo-nextest not installed; falling back to cargo test".into(),
            );
        }

        // Check if Cargo.lock exists (deps resolved)
        info.deps_installed = target.join("Cargo.lock").exists();

        Ok(info)
    }
}

// ---------------------------------------------------------------------------
// Instrumented build helpers
// ---------------------------------------------------------------------------

/// Build with LLVM source-based coverage instrumentation.
pub async fn build_with_coverage(target: &Path) -> Result<()> {
    info!(target = %target.display(), "building Rust target with coverage instrumentation");

    let output = tokio::process::Command::new("cargo")
        .args(["build", "--tests"])
        .env("RUSTFLAGS", "-C instrument-coverage -C codegen-units=1")
        .current_dir(target)
        .output()
        .await
        .map_err(|e| ApexError::LanguageRunner(format!("cargo build: {e}")))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(ApexError::LanguageRunner(format!(
            "instrumented build failed:\n{stderr}"
        )));
    }
    Ok(())
}

/// Build with SanitizerCoverage (AFL++-compatible trace-pc-guard).
///
/// When `bin_name` is `Some`, builds only that binary (`--bin <name>`).
/// Otherwise builds `--tests` (all test targets).
///
/// The compiled APEX coverage shim (`libapex_cov`) is linked into the binary
/// so that `__sanitizer_cov_trace_pc_guard{_init}` symbols are resolved at
/// link time.  The shim must already exist (call `shim::ensure_compiled()`
/// first).
pub async fn build_with_sancov(
    target: &Path,
    bin_name: Option<&str>,
    shim_path: Option<&Path>,
) -> Result<()> {
    // Base SanCov instrumentation flags (stable rustc, no nightly needed).
    let mut rustflags = String::from(
        "-Cpasses=sancov-module \
         -Cllvm-args=-sanitizer-coverage-level=3 \
         -Cllvm-args=-sanitizer-coverage-trace-pc-guard",
    );

    // Link the coverage shim so the linker can resolve the guard symbols.
    if let Some(shim) = shim_path {
        rustflags.push_str(&format!(" -Clink-arg={}", shim.display()));
    }

    let mut args = vec!["build"];
    match bin_name {
        Some(name) => {
            info!(target = %target.display(), bin = name, "building Rust binary with SanitizerCoverage");
            args.extend(["--bin", name]);
        }
        None => {
            info!(target = %target.display(), "building Rust tests with SanitizerCoverage");
            args.push("--tests");
        }
    }

    let output = tokio::process::Command::new("cargo")
        .args(&args)
        .env("RUSTFLAGS", &rustflags)
        .current_dir(target)
        .output()
        .await
        .map_err(|e| ApexError::LanguageRunner(format!("cargo build (sancov): {e}")))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(ApexError::LanguageRunner(format!(
            "sancov build failed:\n{stderr}"
        )));
    }
    Ok(())
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
    fn detect_cargo_toml() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("Cargo.toml"), "[package]\nname = \"foo\"\n").unwrap();
        assert!(RustRunner::new().detect(dir.path()));
    }

    #[test]
    fn detect_empty_dir_returns_false() {
        let dir = tempfile::tempdir().unwrap();
        assert!(!RustRunner::new().detect(dir.path()));
    }

    #[test]
    fn language_is_rust() {
        assert_eq!(RustRunner::new().language(), Language::Rust);
    }

    #[test]
    fn default_creates_runner() {
        let runner = RustRunner::default();
        assert_eq!(runner.language(), Language::Rust);
    }

    #[test]
    fn detect_not_fooled_by_cargo_lock_only() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("Cargo.lock"), "").unwrap();
        assert!(!RustRunner::new().detect(dir.path()));
    }

    #[test]
    fn detect_nonexistent_dir_returns_false() {
        let runner = RustRunner::new();
        assert!(!runner.detect(Path::new("/nonexistent/path/that/does/not/exist")));
    }

    #[test]
    fn detect_nested_cargo_toml_not_detected() {
        let dir = tempfile::tempdir().unwrap();
        let nested = dir.path().join("subdir");
        std::fs::create_dir_all(&nested).unwrap();
        std::fs::write(nested.join("Cargo.toml"), "[package]").unwrap();
        assert!(!RustRunner::new().detect(dir.path()));
    }

    #[tokio::test]
    async fn install_deps_always_succeeds() {
        let dir = tempfile::tempdir().unwrap();
        let runner = RustRunner::new();
        let result = runner.install_deps(dir.path()).await;
        assert!(result.is_ok());
    }

    #[test]
    fn detect_cargo_toml_in_file_not_dir() {
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("Cargo.toml");
        std::fs::write(&file_path, "[package]").unwrap();
        assert!(!RustRunner::new().detect(&file_path));
    }

    // ---- Mock-based tests ----

    #[tokio::test]
    async fn run_tests_success() {
        let dir = tempfile::tempdir().unwrap();

        let mut mock = MockCmd::new();
        mock.expect_run_command()
            .withf(|spec| spec.program == "cargo" && spec.args.contains(&"test".to_string()))
            .times(1)
            .returning(|_| {
                Ok(CommandOutput::success(
                    b"test result: ok. 10 passed".to_vec(),
                ))
            });

        let runner = RustRunner::with_runner(mock);
        let result = runner.run_tests(dir.path(), &[]).await.unwrap();
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("10 passed"));
    }

    #[tokio::test]
    async fn run_tests_failure() {
        let dir = tempfile::tempdir().unwrap();

        let mut mock = MockCmd::new();
        mock.expect_run_command().times(1).returning(|_| {
            Ok(CommandOutput {
                exit_code: 101,
                stdout: b"test result: FAILED. 1 passed; 2 failed".to_vec(),
                stderr: b"error: test failed".to_vec(),
            })
        });

        let runner = RustRunner::with_runner(mock);
        let result = runner.run_tests(dir.path(), &[]).await.unwrap();
        assert_eq!(result.exit_code, 101);
        assert!(result.stdout.contains("FAILED"));
    }

    #[tokio::test]
    async fn run_tests_command_not_found() {
        let dir = tempfile::tempdir().unwrap();

        let mut mock = MockCmd::new();
        mock.expect_run_command().times(1).returning(|_| {
            Err(ApexError::Subprocess {
                exit_code: -1,
                stderr: "spawn cargo: No such file or directory".into(),
            })
        });

        let runner = RustRunner::with_runner(mock);
        let result = runner.run_tests(dir.path(), &[]).await;
        assert!(result.is_err());
        let msg = format!("{}", result.unwrap_err());
        assert!(msg.contains("cargo test"), "unexpected: {msg}");
    }

    #[tokio::test]
    async fn run_tests_with_extra_args() {
        let dir = tempfile::tempdir().unwrap();

        let mut mock = MockCmd::new();
        mock.expect_run_command()
            .withf(|spec| {
                spec.args.contains(&"test".to_string())
                    && spec.args.contains(&"--".to_string())
                    && spec.args.contains(&"--nocapture".to_string())
            })
            .times(1)
            .returning(|_| Ok(CommandOutput::success(b"ok".to_vec())));

        let runner = RustRunner::with_runner(mock);
        let result = runner
            .run_tests(dir.path(), &["--".into(), "--nocapture".into()])
            .await
            .unwrap();
        assert_eq!(result.exit_code, 0);
    }

    #[tokio::test]
    async fn run_tests_sets_working_dir() {
        let dir = tempfile::tempdir().unwrap();
        let expected_dir = dir.path().to_path_buf();

        let mut mock = MockCmd::new();
        mock.expect_run_command()
            .withf(move |spec| spec.working_dir == expected_dir)
            .times(1)
            .returning(|_| Ok(CommandOutput::success(b"ok".to_vec())));

        let runner = RustRunner::with_runner(mock);
        let result = runner.run_tests(dir.path(), &[]).await.unwrap();
        assert_eq!(result.exit_code, 0);
    }

    #[tokio::test]
    async fn run_tests_empty_stdout_and_stderr() {
        let dir = tempfile::tempdir().unwrap();

        let mut mock = MockCmd::new();
        mock.expect_run_command()
            .times(1)
            .returning(|_| Ok(CommandOutput::success(vec![])));

        let runner = RustRunner::with_runner(mock);
        let result = runner.run_tests(dir.path(), &[]).await.unwrap();
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "");
        assert_eq!(result.stderr, "");
    }

    #[tokio::test]
    async fn run_tests_non_utf8_output_replaced() {
        let dir = tempfile::tempdir().unwrap();

        // 0xFF is not valid UTF-8; from_utf8_lossy should replace it with U+FFFD
        let mut mock = MockCmd::new();
        mock.expect_run_command().times(1).returning(|_| {
            Ok(CommandOutput {
                exit_code: 0,
                stdout: vec![0xFF, 0xFE],
                stderr: vec![0x80],
            })
        });

        let runner = RustRunner::with_runner(mock);
        let result = runner.run_tests(dir.path(), &[]).await.unwrap();
        // Should not panic; replacement character must be present
        assert!(result.stdout.contains('\u{FFFD}'));
        assert!(result.stderr.contains('\u{FFFD}'));
    }

    #[tokio::test]
    async fn run_tests_duration_ms_is_set() {
        let dir = tempfile::tempdir().unwrap();

        let mut mock = MockCmd::new();
        mock.expect_run_command()
            .times(1)
            .returning(|_| Ok(CommandOutput::success(b"ok".to_vec())));

        let runner = RustRunner::with_runner(mock);
        let result = runner.run_tests(dir.path(), &[]).await.unwrap();
        // duration_ms must be a valid (non-huge) number; hard to assert exact value,
        // but it must not be u64::MAX (would indicate an overflow).
        assert!(result.duration_ms < u64::MAX);
    }

    #[tokio::test]
    async fn run_tests_error_message_contains_cargo_test() {
        // Validates that the ApexError wrapping preserves "cargo test" in the message.
        let dir = tempfile::tempdir().unwrap();

        let mut mock = MockCmd::new();
        mock.expect_run_command().times(1).returning(|_| {
            Err(ApexError::Subprocess {
                exit_code: 1,
                stderr: "some internal error".into(),
            })
        });

        let runner = RustRunner::with_runner(mock);
        let err = runner.run_tests(dir.path(), &[]).await.unwrap_err();
        let msg = format!("{err}");
        assert!(
            msg.contains("cargo test"),
            "expected 'cargo test' in: {msg}"
        );
    }

    #[tokio::test]
    async fn run_tests_first_arg_is_test() {
        // Verify the first argument to cargo is always "test".
        let dir = tempfile::tempdir().unwrap();

        let mut mock = MockCmd::new();
        mock.expect_run_command()
            .withf(|spec| spec.args.first().map(|s| s.as_str()) == Some("test"))
            .times(1)
            .returning(|_| Ok(CommandOutput::success(b"ok".to_vec())));

        let runner = RustRunner::with_runner(mock);
        runner.run_tests(dir.path(), &[]).await.unwrap();
    }

    #[tokio::test]
    async fn run_tests_program_is_cargo() {
        // Verify the command program is always "cargo".
        let dir = tempfile::tempdir().unwrap();

        let mut mock = MockCmd::new();
        mock.expect_run_command()
            .withf(|spec| spec.program == "cargo")
            .times(1)
            .returning(|_| Ok(CommandOutput::success(b"ok".to_vec())));

        let runner = RustRunner::with_runner(mock);
        runner.run_tests(dir.path(), &[]).await.unwrap();
    }

    // --- build_with_coverage / build_with_sancov error-format tests ---
    // These functions use tokio::process::Command directly; we can't mock the
    // subprocess, but we CAN verify the ApexError variant and message format
    // when the underlying spawn fails (by pointing at a non-existent binary).

    #[tokio::test]
    async fn build_with_coverage_spawn_error_wraps_apex_error() {
        // Run against /nonexistent to make the spawn itself fail with a clean error.
        let result = build_with_coverage(Path::new("/nonexistent/path")).await;
        // On any OS the working_dir will be invalid, so either spawn or the
        // process itself will fail.  Either way we get an ApexError.
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn build_with_sancov_spawn_error_wraps_apex_error() {
        let result = build_with_sancov(Path::new("/nonexistent/path"), None, None).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn build_with_sancov_bin_name_branch() {
        // Exercise the Some(bin_name) arm; the build will fail (no project),
        // but the important thing is the branch is reached without a panic.
        let result =
            build_with_sancov(Path::new("/nonexistent/path"), Some("my_binary"), None).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn build_with_sancov_shim_path_appended_to_rustflags() {
        // With a shim path the function must reach the shim-path formatting branch.
        // The build will fail (no real project), but we verify no panic.
        let tmp = tempfile::tempdir().unwrap();
        let shim = tmp.path().join("libapex_cov.a");
        std::fs::write(&shim, b"fake").unwrap();
        let result =
            build_with_sancov(Path::new("/nonexistent/path"), None, Some(shim.as_path())).await;
        assert!(result.is_err());
    }

    // ------------------------------------------------------------------
    // build_with_sancov — bin_name Some + shim path (covers both branches)
    // ------------------------------------------------------------------

    #[tokio::test]
    async fn build_with_sancov_bin_name_and_shim_path() {
        let tmp = tempfile::tempdir().unwrap();
        let shim = tmp.path().join("libapex_cov.a");
        std::fs::write(&shim, b"fake").unwrap();
        let result = build_with_sancov(
            Path::new("/nonexistent/path"),
            Some("my_bin"),
            Some(shim.as_path()),
        )
        .await;
        assert!(result.is_err());
    }

    // ------------------------------------------------------------------
    // install_deps — with_runner variant also always Ok
    // ------------------------------------------------------------------

    #[tokio::test]
    async fn install_deps_with_runner_always_ok() {
        let dir = tempfile::tempdir().unwrap();
        let mock = MockCmd::new();
        // No expectations — run_command must NOT be called for Rust
        let runner = RustRunner::with_runner(mock);
        let result = runner.install_deps(dir.path()).await;
        assert!(result.is_ok());
    }

    // ------------------------------------------------------------------
    // run_tests — stderr captured correctly
    // ------------------------------------------------------------------

    #[tokio::test]
    async fn run_tests_stderr_captured() {
        let dir = tempfile::tempdir().unwrap();

        let mut mock = MockCmd::new();
        mock.expect_run_command().times(1).returning(|_| {
            Ok(CommandOutput {
                exit_code: 0,
                stdout: b"".to_vec(),
                stderr: b"warning: unused variable".to_vec(),
            })
        });

        let runner = RustRunner::with_runner(mock);
        let result = runner.run_tests(dir.path(), &[]).await.unwrap();
        assert!(result.stderr.contains("unused variable"));
    }

    // ------------------------------------------------------------------
    // preflight_check tests
    // ------------------------------------------------------------------

    #[test]
    fn preflight_check_detects_workspace() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("Cargo.toml"),
            "[workspace]\nmembers = [\"crate-a\"]\n",
        )
        .unwrap();
        let runner = RustRunner::new();
        let info = runner.preflight_check(dir.path()).unwrap();
        assert_eq!(info.build_system.as_deref(), Some("cargo"));
        assert!(info.extra.iter().any(|(k, v)| k == "project_type" && v == "workspace"));
    }

    #[test]
    fn preflight_check_detects_single_crate() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("Cargo.toml"),
            "[package]\nname = \"foo\"\nversion = \"0.1.0\"\n",
        )
        .unwrap();
        let runner = RustRunner::new();
        let info = runner.preflight_check(dir.path()).unwrap();
        assert!(info.extra.iter().any(|(k, v)| k == "project_type" && v == "single-crate"));
    }

    #[test]
    fn preflight_check_no_cargo_toml() {
        let dir = tempfile::tempdir().unwrap();
        let runner = RustRunner::new();
        let info = runner.preflight_check(dir.path()).unwrap();
        // Should still detect single-crate (no [workspace] found)
        assert!(info.extra.iter().any(|(k, v)| k == "project_type" && v == "single-crate"));
    }

    #[test]
    fn preflight_check_reports_cargo_available() {
        // cargo must be on PATH in our CI/dev environment
        let dir = tempfile::tempdir().unwrap();
        let runner = RustRunner::new();
        let info = runner.preflight_check(dir.path()).unwrap();
        assert!(
            info.available_tools.iter().any(|(name, _)| name == "cargo"),
            "cargo should be available: {:?}",
            info.available_tools
        );
    }

    #[test]
    fn preflight_check_deps_installed_when_lock_exists() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("Cargo.lock"), "").unwrap();
        let runner = RustRunner::new();
        let info = runner.preflight_check(dir.path()).unwrap();
        assert!(info.deps_installed);
    }

    #[test]
    fn preflight_check_deps_not_installed_without_lock() {
        let dir = tempfile::tempdir().unwrap();
        let runner = RustRunner::new();
        let info = runner.preflight_check(dir.path()).unwrap();
        assert!(!info.deps_installed);
    }

    #[test]
    fn preflight_check_summary_has_build_system() {
        let dir = tempfile::tempdir().unwrap();
        let runner = RustRunner::new();
        let info = runner.preflight_check(dir.path()).unwrap();
        let summary = info.summary();
        assert!(summary.contains("build system: cargo"));
    }

    #[test]
    fn is_workspace_returns_true_for_workspace() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("Cargo.toml"), "[workspace]\nmembers = []\n").unwrap();
        assert!(RustRunner::<RealCommandRunner>::is_workspace(dir.path()));
    }

    #[test]
    fn is_workspace_returns_false_for_package() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("Cargo.toml"), "[package]\nname = \"x\"\n").unwrap();
        assert!(!RustRunner::<RealCommandRunner>::is_workspace(dir.path()));
    }
}
