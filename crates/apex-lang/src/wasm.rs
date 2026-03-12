//! WASM language runner — executes `.wasm` modules via `wasmtime`.
//!
//! Status: stub. Dependency discovery and test execution are not yet wired.

use apex_core::{
    command::{CommandRunner, CommandSpec, RealCommandRunner},
    error::{ApexError, Result},
    traits::{LanguageRunner, TestRunOutput},
    types::Language,
};
use async_trait::async_trait;
use std::{
    path::{Path, PathBuf},
    time::Instant,
};
use tracing::{info, warn};

/// Classification of a WASM execution outcome.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WasmExitKind {
    Pass,
    Fail,
    Crash,
    Timeout,
    OutOfMemory,
}

/// Classify a wasmtime execution by exit code and stderr content.
pub fn classify_wasm_exit(exit_code: i32, stderr: &str) -> WasmExitKind {
    if exit_code == 0 {
        return WasmExitKind::Pass;
    }

    let stderr_lower = stderr.to_lowercase();

    if stderr_lower.contains("out of memory") || stderr_lower.contains("oom") {
        return WasmExitKind::OutOfMemory;
    }

    if stderr_lower.contains("wasm trap")
        || stderr_lower.contains("unreachable")
        || exit_code >= 128
    {
        // SIGKILL (137) without OOM is timeout; trap messages are crashes
        if exit_code == 137 && !stderr_lower.contains("trap") {
            return WasmExitKind::Timeout;
        }
        return WasmExitKind::Crash;
    }

    WasmExitKind::Fail
}

pub struct WasmRunner<R: CommandRunner = RealCommandRunner> {
    runner: R,
}

impl WasmRunner {
    pub fn new() -> Self {
        WasmRunner {
            runner: RealCommandRunner,
        }
    }
}

impl<R: CommandRunner> WasmRunner<R> {
    pub fn with_runner(runner: R) -> Self {
        WasmRunner { runner }
    }

    /// Find the best `.wasm` file to run. Prefers `.inst.wasm` (instrumented)
    /// when the `wasm-instrument` feature is enabled. Returns `(path, is_instrumented)`.
    fn find_wasm_for_run(target: &Path) -> (Option<PathBuf>, bool) {
        let entries: Vec<_> = std::fs::read_dir(target)
            .ok()
            .map(|e| e.flatten().map(|e| e.path()).collect())
            .unwrap_or_default();

        // Look for instrumented binary first
        #[cfg(feature = "wasm-instrument")]
        {
            if let Some(inst) = entries
                .iter()
                .find(|p| p.to_string_lossy().ends_with(".inst.wasm"))
            {
                return (Some(inst.clone()), true);
            }
        }

        // Fall back to any .wasm file
        let wasm = entries
            .into_iter()
            .find(|p| p.extension().map(|x| x == "wasm").unwrap_or(false));
        (wasm, false)
    }
}

impl Default for WasmRunner {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl<R: CommandRunner> LanguageRunner for WasmRunner<R> {
    fn language(&self) -> Language {
        Language::Wasm
    }

    fn detect(&self, target: &Path) -> bool {
        // Any .wasm file in the directory tree.
        std::fs::read_dir(target)
            .map(|entries| {
                entries
                    .flatten()
                    .any(|e| e.path().extension().map(|x| x == "wasm").unwrap_or(false))
            })
            .unwrap_or(false)
    }

    async fn install_deps(&self, target: &Path) -> Result<()> {
        // Check for wasmtime in PATH.
        let spec = CommandSpec::new("wasmtime", target).args(["--version"]);
        let has_wasmtime = self
            .runner
            .run_command(&spec)
            .await
            .map(|o| o.exit_code == 0)
            .unwrap_or(false);

        if !has_wasmtime {
            warn!(
                "wasmtime not found in PATH. Install from https://wasmtime.dev or \
                 `cargo install wasmtime-cli`"
            );
        }

        info!(target = %target.display(), "WASM: no dep installation required (stub)");
        Ok(())
    }

    async fn run_tests(&self, target: &Path, extra_args: &[String]) -> Result<TestRunOutput> {
        // Prefer instrumented .inst.wasm files when available (wasm-instrument feature),
        // otherwise fall back to the first .wasm file.
        let (wasm_file, is_instrumented) = Self::find_wasm_for_run(target);

        let Some(wasm_path) = wasm_file else {
            return Err(ApexError::LanguageRunner(
                "no .wasm file found in target directory".into(),
            ));
        };

        info!(
            wasm = %wasm_path.display(),
            instrumented = is_instrumented,
            "running WASM with wasmtime"
        );

        let mut args: Vec<String> = vec!["run".to_string()];

        // When running an instrumented binary under coverage mode, pass the
        // shared-memory name via WASI env so the coverage runtime can write
        // guard hits to the bitmap.
        #[cfg(feature = "wasm-instrument")]
        if is_instrumented {
            if let Ok(shm_name) = std::env::var("__APEX_SHM_NAME") {
                info!(shm = %shm_name, "passing SHM name to WASM via --env");
                args.push("--env".to_string());
                args.push(format!("__APEX_SHM_NAME={shm_name}"));
            }
        }

        // Suppress unused variable warning when feature is off
        let _ = is_instrumented;

        args.push(wasm_path.to_string_lossy().to_string());
        for arg in extra_args {
            args.push(arg.clone());
        }

        let spec = CommandSpec::new("wasmtime", target).args(args);

        let start = Instant::now();
        let output = self
            .runner
            .run_command(&spec)
            .await
            .map_err(|e| ApexError::LanguageRunner(format!("wasmtime: {e}")))?;

        Ok(TestRunOutput {
            exit_code: output.exit_code,
            stdout: String::from_utf8_lossy(&output.stdout).to_string(),
            stderr: String::from_utf8_lossy(&output.stderr).to_string(),
            duration_ms: start.elapsed().as_millis() as u64,
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
    fn detect_wasm_file() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("module.wasm"), &[0x00, 0x61, 0x73, 0x6d]).unwrap();
        assert!(WasmRunner::new().detect(dir.path()));
    }

    #[test]
    fn detect_empty_dir_returns_false() {
        let dir = tempfile::tempdir().unwrap();
        assert!(!WasmRunner::new().detect(dir.path()));
    }

    #[test]
    fn detect_non_wasm_files_returns_false() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("main.rs"), "fn main() {}").unwrap();
        std::fs::write(dir.path().join("lib.js"), "").unwrap();
        assert!(!WasmRunner::new().detect(dir.path()));
    }

    #[test]
    fn detect_multiple_wasm_files() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.wasm"), &[0x00]).unwrap();
        std::fs::write(dir.path().join("b.wasm"), &[0x00]).unwrap();
        assert!(WasmRunner::new().detect(dir.path()));
    }

    #[test]
    fn language_is_wasm() {
        assert_eq!(WasmRunner::new().language(), Language::Wasm);
    }

    #[test]
    fn default_creates_runner() {
        let runner = WasmRunner::default();
        assert_eq!(runner.language(), Language::Wasm);
    }

    #[test]
    fn detect_nonexistent_dir_returns_false() {
        let runner = WasmRunner::new();
        assert!(!runner.detect(Path::new("/nonexistent/path/that/does/not/exist")));
    }

    #[test]
    fn detect_file_without_extension() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("noext"), &[0x00]).unwrap();
        assert!(!WasmRunner::new().detect(dir.path()));
    }

    #[test]
    fn detect_wasm_like_extension_no_match() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("module.wasmt"), &[0x00]).unwrap();
        assert!(!WasmRunner::new().detect(dir.path()));
    }

    #[test]
    fn detect_ignores_subdirectory_wasm_files() {
        let dir = tempfile::tempdir().unwrap();
        let sub = dir.path().join("subdir");
        std::fs::create_dir_all(&sub).unwrap();
        std::fs::write(sub.join("nested.wasm"), &[0x00]).unwrap();
        assert!(!WasmRunner::new().detect(dir.path()));
    }

    #[test]
    fn detect_mixed_files_with_one_wasm() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("readme.txt"), "hello").unwrap();
        std::fs::write(dir.path().join("app.js"), "").unwrap();
        std::fs::write(dir.path().join("mod.wasm"), &[0x00]).unwrap();
        assert!(WasmRunner::new().detect(dir.path()));
    }

    // ------------------------------------------------------------------
    // find_wasm_for_run tests
    // ------------------------------------------------------------------

    #[test]
    fn find_wasm_for_run_plain_wasm() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("app.wasm"), &[0x00]).unwrap();
        let (path, instrumented) = WasmRunner::<RealCommandRunner>::find_wasm_for_run(dir.path());
        assert!(path.is_some());
        assert!(!instrumented);
    }

    #[test]
    fn find_wasm_for_run_empty_dir() {
        let dir = tempfile::tempdir().unwrap();
        let (path, instrumented) = WasmRunner::<RealCommandRunner>::find_wasm_for_run(dir.path());
        assert!(path.is_none());
        assert!(!instrumented);
    }

    #[cfg(feature = "wasm-instrument")]
    #[test]
    fn find_wasm_for_run_prefers_instrumented() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("app.wasm"), &[0x00]).unwrap();
        std::fs::write(dir.path().join("app.inst.wasm"), &[0x00]).unwrap();
        let (path, instrumented) = WasmRunner::<RealCommandRunner>::find_wasm_for_run(dir.path());
        assert!(path.is_some());
        assert!(instrumented);
        let p = path.unwrap();
        assert!(
            p.to_string_lossy().contains("inst.wasm"),
            "should prefer .inst.wasm, got: {}",
            p.display()
        );
    }

    #[test]
    fn find_wasm_for_run_nonexistent_dir() {
        let (path, instrumented) =
            WasmRunner::<RealCommandRunner>::find_wasm_for_run(Path::new("/nonexistent/path"));
        assert!(path.is_none());
        assert!(!instrumented);
    }

    // ---- Mock-based tests ----

    #[tokio::test]
    async fn install_deps_wasmtime_found() {
        let dir = tempfile::tempdir().unwrap();

        let mut mock = MockCmd::new();
        mock.expect_run_command()
            .withf(|spec| {
                spec.program == "wasmtime" && spec.args.contains(&"--version".to_string())
            })
            .times(1)
            .returning(|_| Ok(CommandOutput::success(b"wasmtime-cli 15.0.0".to_vec())));

        let runner = WasmRunner::with_runner(mock);
        let result = runner.install_deps(dir.path()).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn install_deps_wasmtime_not_found() {
        let dir = tempfile::tempdir().unwrap();

        let mut mock = MockCmd::new();
        mock.expect_run_command().times(1).returning(|_| {
            Err(ApexError::Subprocess {
                exit_code: -1,
                stderr: "spawn wasmtime: not found".into(),
            })
        });

        let runner = WasmRunner::with_runner(mock);
        // Should still return Ok (just warns)
        let result = runner.install_deps(dir.path()).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn run_tests_no_wasm_file_errors() {
        let dir = tempfile::tempdir().unwrap();

        let mock = MockCmd::new();
        let runner = WasmRunner::with_runner(mock);
        let result = runner.run_tests(dir.path(), &[]).await;
        assert!(result.is_err());
        let msg = format!("{}", result.unwrap_err());
        assert!(msg.contains("no .wasm file"), "unexpected error: {msg}");
    }

    #[tokio::test]
    async fn run_tests_success() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("test.wasm"), &[0x00, 0x61, 0x73, 0x6d]).unwrap();

        let mut mock = MockCmd::new();
        mock.expect_run_command()
            .withf(|spec| spec.program == "wasmtime" && spec.args.contains(&"run".to_string()))
            .times(1)
            .returning(|_| Ok(CommandOutput::success(b"wasm output".to_vec())));

        let runner = WasmRunner::with_runner(mock);
        let result = runner.run_tests(dir.path(), &[]).await.unwrap();
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("wasm output"));
    }

    #[tokio::test]
    async fn run_tests_wasmtime_failure() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("bad.wasm"), &[0x00]).unwrap();

        let mut mock = MockCmd::new();
        mock.expect_run_command().times(1).returning(|_| {
            Ok(CommandOutput {
                exit_code: 1,
                stdout: Vec::new(),
                stderr: b"Error: failed to parse".to_vec(),
            })
        });

        let runner = WasmRunner::with_runner(mock);
        let result = runner.run_tests(dir.path(), &[]).await.unwrap();
        assert_eq!(result.exit_code, 1);
        assert!(result.stderr.contains("failed to parse"));
    }

    #[tokio::test]
    async fn run_tests_command_not_found() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("mod.wasm"), &[0x00]).unwrap();

        let mut mock = MockCmd::new();
        mock.expect_run_command().times(1).returning(|_| {
            Err(ApexError::Subprocess {
                exit_code: -1,
                stderr: "spawn wasmtime: not found".into(),
            })
        });

        let runner = WasmRunner::with_runner(mock);
        let result = runner.run_tests(dir.path(), &[]).await;
        assert!(result.is_err());
        let msg = format!("{}", result.unwrap_err());
        assert!(msg.contains("wasmtime"), "unexpected: {msg}");
    }

    #[tokio::test]
    async fn run_tests_with_extra_args() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("m.wasm"), &[0x00]).unwrap();

        let mut mock = MockCmd::new();
        mock.expect_run_command()
            .withf(|spec| spec.args.iter().any(|a| a == "--flag"))
            .times(1)
            .returning(|_| Ok(CommandOutput::success(b"ok".to_vec())));

        let runner = WasmRunner::with_runner(mock);
        let result = runner
            .run_tests(dir.path(), &["--flag".into()])
            .await
            .unwrap();
        assert_eq!(result.exit_code, 0);
    }

    // ------------------------------------------------------------------
    // classify_wasm_exit tests
    // ------------------------------------------------------------------

    #[test]
    fn classify_wasm_exit_normal() {
        assert_eq!(classify_wasm_exit(0, ""), WasmExitKind::Pass);
    }

    #[test]
    fn classify_wasm_exit_trap() {
        assert_eq!(
            classify_wasm_exit(128, "Error: wasm trap: unreachable"),
            WasmExitKind::Crash
        );
    }

    #[test]
    fn classify_wasm_exit_oom() {
        assert_eq!(
            classify_wasm_exit(1, "memory allocation failed: out of memory"),
            WasmExitKind::OutOfMemory
        );
    }

    #[test]
    fn classify_wasm_exit_timeout() {
        assert_eq!(classify_wasm_exit(137, ""), WasmExitKind::Timeout);
    }

    #[test]
    fn classify_wasm_exit_generic_fail() {
        assert_eq!(classify_wasm_exit(1, "some error"), WasmExitKind::Fail);
    }

    // ------------------------------------------------------------------
    // classify_wasm_exit — additional branch coverage
    // ------------------------------------------------------------------

    #[test]
    fn classify_wasm_exit_pass_with_nonempty_stderr() {
        // exit 0 should always be Pass, even when stderr has text
        assert_eq!(classify_wasm_exit(0, "some diagnostic"), WasmExitKind::Pass);
    }

    #[test]
    fn classify_wasm_exit_oom_via_short_keyword() {
        // "oom" branch of the OOM check
        assert_eq!(classify_wasm_exit(1, "oom"), WasmExitKind::OutOfMemory);
    }

    #[test]
    fn classify_wasm_exit_oom_case_insensitive() {
        // stderr is lowercased before comparison
        assert_eq!(
            classify_wasm_exit(1, "Process OOM killed"),
            WasmExitKind::OutOfMemory
        );
    }

    #[test]
    fn classify_wasm_exit_oom_out_of_memory_case_insensitive() {
        assert_eq!(
            classify_wasm_exit(1, "Out Of Memory error"),
            WasmExitKind::OutOfMemory
        );
    }

    #[test]
    fn classify_wasm_exit_unreachable_stderr_crash() {
        // "unreachable" in stderr triggers Crash (exit_code < 128, no "trap")
        assert_eq!(
            classify_wasm_exit(1, "error: unreachable executed"),
            WasmExitKind::Crash
        );
    }

    #[test]
    fn classify_wasm_exit_wasm_trap_uppercase() {
        // "wasm trap" check is case-insensitive via to_lowercase()
        assert_eq!(
            classify_wasm_exit(1, "WASM TRAP: integer divide by zero"),
            WasmExitKind::Crash
        );
    }

    #[test]
    fn classify_wasm_exit_exit_128_no_trap_crash() {
        // exit >= 128 without trap/unreachable/oom is Crash (not Timeout)
        assert_eq!(classify_wasm_exit(130, ""), WasmExitKind::Crash);
    }

    #[test]
    fn classify_wasm_exit_exit_137_with_trap_is_crash() {
        // exit 137 WITH "trap" in stderr should be Crash, not Timeout
        assert_eq!(
            classify_wasm_exit(137, "wasm trap: out of bounds memory access"),
            WasmExitKind::Crash
        );
    }

    #[test]
    fn classify_wasm_exit_exit_129_crash() {
        // exit 129 (>= 128) is Crash when no OOM/trap/unreachable keywords
        assert_eq!(classify_wasm_exit(129, "signal"), WasmExitKind::Crash);
    }

    #[test]
    fn classify_wasm_exit_fail_exit_127() {
        // exit 127 (< 128) with no keywords → Fail
        assert_eq!(
            classify_wasm_exit(127, "command not found"),
            WasmExitKind::Fail
        );
    }

    #[test]
    fn classify_wasm_exit_fail_zero_exit_code_is_pass() {
        // Boundary: exit 0 regardless of exit_code comparisons below
        assert_eq!(classify_wasm_exit(0, "trap"), WasmExitKind::Pass);
    }

    // ------------------------------------------------------------------
    // install_deps — wasmtime returns nonzero (not a spawn error)
    // ------------------------------------------------------------------

    #[tokio::test]
    async fn install_deps_wasmtime_returns_nonzero() {
        let dir = tempfile::tempdir().unwrap();

        let mut mock = MockCmd::new();
        // wasmtime --version returns exit code 1 (unhealthy but still responds)
        mock.expect_run_command()
            .withf(|spec| spec.program == "wasmtime")
            .times(1)
            .returning(|_| Ok(CommandOutput::failure(1, b"error".to_vec())));

        let runner = WasmRunner::with_runner(mock);
        // Should still return Ok (just warns that wasmtime is not found/functional)
        let result = runner.install_deps(dir.path()).await;
        assert!(result.is_ok());
    }

    // ------------------------------------------------------------------
    // find_wasm_for_run — multiple .wasm files, picks one
    // ------------------------------------------------------------------

    #[test]
    fn find_wasm_for_run_no_wasm_only_other_files() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("main.rs"), "fn main(){}").unwrap();
        std::fs::write(dir.path().join("lib.js"), "").unwrap();
        let (path, instrumented) = WasmRunner::<RealCommandRunner>::find_wasm_for_run(dir.path());
        assert!(path.is_none());
        assert!(!instrumented);
    }

    // ------------------------------------------------------------------
    // run_tests — wasm file path is passed to wasmtime
    // ------------------------------------------------------------------

    #[tokio::test]
    async fn run_tests_wasm_path_passed_as_arg() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("my_module.wasm"), &[0x00]).unwrap();

        let mut mock = MockCmd::new();
        mock.expect_run_command()
            .withf(|spec| {
                spec.program == "wasmtime"
                    && spec.args.iter().any(|a| a.contains("my_module.wasm"))
            })
            .times(1)
            .returning(|_| Ok(CommandOutput::success(b"ok".to_vec())));

        let runner = WasmRunner::with_runner(mock);
        let result = runner.run_tests(dir.path(), &[]).await.unwrap();
        assert_eq!(result.exit_code, 0);
    }
}
