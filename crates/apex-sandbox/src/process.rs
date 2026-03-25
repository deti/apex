use crate::shm::{ShmBitmap, SHM_ENV_VAR};
use apex_core::{
    command::{CommandRunner, CommandSpec, RealCommandRunner},
    error::{ApexError, Result},
    traits::Sandbox,
    types::{
        BranchId, ExecutionResult, ExecutionStatus, InputSeed, Language, ResourceMetrics,
        SnapshotId,
    },
};
use apex_coverage::CoverageOracle;
use async_trait::async_trait;
#[cfg(unix)]
use libc;
use std::{path::PathBuf, sync::Arc, time::Instant};
use tracing::{debug, error, instrument, warn};

// ---------------------------------------------------------------------------
// ProcessSandbox
// ---------------------------------------------------------------------------

/// Sandbox that forks a subprocess, feeds it input on stdin, and (optionally)
/// reads an AFL++-compatible coverage bitmap from POSIX SHM after exit.
///
/// Coverage feedback is enabled when `oracle` is `Some` — the sandbox creates
/// a SHM region, exports `__APEX_SHM_NAME`, and after the child exits maps
/// the bitmap bytes to `BranchId`s using the oracle's registered branch order.
///
/// Generic over `R: CommandRunner` so that tests can inject a mock runner.
/// The default (`RealCommandRunner`) spawns real subprocesses.
pub struct ProcessSandbox<R: CommandRunner = RealCommandRunner> {
    pub language: Language,
    pub target_dir: PathBuf,
    pub command: Vec<String>,
    pub timeout_ms: u64,
    /// When set, the sandbox wires SHM coverage feedback.
    oracle: Option<Arc<CoverageOracle>>,
    /// Ordered list of BranchIds mirroring the bitmap index expected from the
    /// target (set when oracle is used).
    branch_index: Vec<BranchId>,
    /// Path to LD_PRELOAD shim (optional).
    shim_path: Option<PathBuf>,
    /// Command runner (real or mock).
    runner: R,
}

impl ProcessSandbox<RealCommandRunner> {
    pub fn new(language: Language, target_dir: PathBuf, command: Vec<String>) -> Self {
        ProcessSandbox {
            language,
            target_dir,
            command,
            timeout_ms: 10_000,
            oracle: None,
            branch_index: Vec::new(),
            shim_path: None,
            runner: RealCommandRunner,
        }
    }
}

impl<R: CommandRunner> ProcessSandbox<R> {
    /// Create a ProcessSandbox with a custom command runner (useful for testing).
    pub fn with_runner(
        language: Language,
        target_dir: PathBuf,
        command: Vec<String>,
        runner: R,
    ) -> Self {
        ProcessSandbox {
            language,
            target_dir,
            command,
            timeout_ms: 10_000,
            oracle: None,
            branch_index: Vec::new(),
            shim_path: None,
            runner,
        }
    }

    /// Enable SHM coverage feedback. `branch_index` must list BranchIds in the
    /// same order the target's guard array is initialised (matches the oracle's
    /// registered order when targeting our shim).
    pub fn with_coverage(
        mut self,
        oracle: Arc<CoverageOracle>,
        branch_index: Vec<BranchId>,
    ) -> Self {
        self.oracle = Some(oracle);
        self.branch_index = branch_index;
        self
    }

    /// Inject the APEX coverage shim via LD_PRELOAD / DYLD_INSERT_LIBRARIES.
    pub fn with_shim(mut self, shim_path: PathBuf) -> Self {
        self.shim_path = Some(shim_path);
        self
    }

    pub fn with_timeout(mut self, ms: u64) -> Self {
        self.timeout_ms = ms;
        self
    }

    fn bitmap_to_new_branches(&self, bitmap: &[u8]) -> Vec<BranchId> {
        let oracle = match &self.oracle {
            Some(o) => o,
            None => return Vec::new(),
        };
        crate::bitmap::bitmap_to_new_branches(bitmap, &self.branch_index, oracle)
    }

    /// Build a [`CommandSpec`] from our configuration and the given input.
    fn build_spec(&self, input: &InputSeed, shm_name: Option<&str>) -> Result<CommandSpec> {
        let program = self
            .command
            .first()
            .ok_or_else(|| ApexError::Sandbox("empty command".into()))?;

        let mut spec = CommandSpec::new(program, &self.target_dir)
            .args(self.command[1..].iter().map(String::as_str))
            .stdin(input.data.to_vec())
            .timeout(self.timeout_ms);

        // Export SHM name so the target's shim can attach.
        if let Some(name) = shm_name {
            spec = spec.env(SHM_ENV_VAR, name);
        }

        // Inject coverage shim.
        if let Some(ref shim_path) = self.shim_path {
            let env_var = crate::shim::preload_env_var();
            spec = spec.env(env_var, shim_path.display().to_string());
        }

        Ok(spec)
    }
}

// ---------------------------------------------------------------------------
// Resource measurement helpers
// ---------------------------------------------------------------------------

/// Collect resource usage for child processes via `getrusage(RUSAGE_CHILDREN)`.
///
/// Called immediately after the child process exits so that the kernel's
/// accumulated child accounting reflects (at least) the just-reaped child.
#[cfg(unix)]
fn collect_rusage(wall_time_ms: u64) -> ResourceMetrics {
    // SAFETY: rusage is a plain C struct; zeroing it is a valid initialiser.
    let mut ru: libc::rusage = unsafe { std::mem::zeroed() };
    // SAFETY: RUSAGE_CHILDREN is a valid flag; &mut ru is a valid pointer.
    let rc = unsafe { libc::getrusage(libc::RUSAGE_CHILDREN, &mut ru) };
    if rc != 0 {
        // getrusage failed — return what we know.
        return ResourceMetrics {
            wall_time_ms,
            cpu_time_ms: 0,
            peak_memory_bytes: 0,
        };
    }

    // ru_utime: user CPU time as { tv_sec, tv_usec }
    let cpu_time_ms = ru.ru_utime.tv_sec as u64 * 1_000 + ru.ru_utime.tv_usec as u64 / 1_000;

    // ru_maxrss semantics differ by platform:
    //   Linux  — kilobytes  → multiply by 1024
    //   macOS  — bytes      → use directly
    #[cfg(target_os = "linux")]
    let peak_memory_bytes = ru.ru_maxrss as u64 * 1_024;
    #[cfg(not(target_os = "linux"))]
    let peak_memory_bytes = ru.ru_maxrss as u64;

    ResourceMetrics {
        wall_time_ms,
        cpu_time_ms,
        peak_memory_bytes,
    }
}

#[cfg(not(unix))]
fn collect_rusage(wall_time_ms: u64) -> ResourceMetrics {
    ResourceMetrics {
        wall_time_ms,
        ..Default::default()
    }
}

#[async_trait]
impl<R: CommandRunner> Sandbox for ProcessSandbox<R> {
    #[instrument(skip(self, input), fields(seed_id = ?input.id))]
    async fn run(&self, input: &InputSeed) -> Result<ExecutionResult> {
        let start = Instant::now();

        // Create SHM if coverage is enabled.
        let shm = if self.oracle.is_some() {
            match ShmBitmap::create() {
                Ok(s) => Some(s),
                Err(e) => {
                    warn!(error = %e, "failed to create SHM bitmap; running without coverage");
                    None
                }
            }
        } else {
            None
        };

        let shm_name = shm.as_ref().map(|s| s.name_str());
        let spec = self.build_spec(input, shm_name)?;

        let cmd_display = self.command.join(" ");
        debug!(cmd = %cmd_display, "Spawning sandbox process");
        let result = self.runner.run_command(&spec).await;
        let duration_ms = start.elapsed().as_millis() as u64;

        // Read bitmap before returning, regardless of exit status.
        let bitmap = shm.as_ref().map(|s| s.read());

        // Collect resource usage immediately after the child exits (before any
        // further work that could perturb the kernel's child accounting).
        let resource_metrics = Some(collect_rusage(duration_ms));

        match result {
            Err(ApexError::Timeout(_)) => {
                warn!(timeout_ms = self.timeout_ms, "Process timed out");
                Ok(ExecutionResult {
                    seed_id: input.id,
                    status: ExecutionStatus::Timeout,
                    new_branches: Vec::new(),
                    trace: None,
                    duration_ms,
                    stdout: String::new(),
                    stderr: String::new(),
                    input: None,
                    resource_metrics,
                })
            }
            Err(e) => {
                error!(cmd = %cmd_display, error = %e, "Failed to spawn sandbox process");
                Err(ApexError::Sandbox(format!("run_command: {e}")))
            }
            Ok(output) => {
                let status = match output.exit_code {
                    0 => ExecutionStatus::Pass,
                    c if c < 0 || (128..=159).contains(&c) => ExecutionStatus::Crash,
                    _ => ExecutionStatus::Fail,
                };

                match &status {
                    ExecutionStatus::Pass => {
                        debug!(exit_code = 0, "Process completed successfully")
                    }
                    ExecutionStatus::Fail => {
                        debug!(exit_code = output.exit_code, "Process exited with failure")
                    }
                    ExecutionStatus::Crash => {
                        warn!(exit_code = output.exit_code, "Process crashed")
                    }
                    _ => {} // Timeout handled above
                }

                let new_branches = bitmap
                    .as_deref()
                    .map(|bm| self.bitmap_to_new_branches(bm))
                    .unwrap_or_default();

                debug!(
                    status = ?status,
                    new_branches = new_branches.len(),
                    duration_ms,
                    "process sandbox result"
                );

                Ok(ExecutionResult {
                    seed_id: input.id,
                    status,
                    new_branches,
                    trace: None,
                    duration_ms,
                    stdout: String::from_utf8_lossy(&output.stdout).to_string(),
                    stderr: String::from_utf8_lossy(&output.stderr).to_string(),
                    input: None,
                    resource_metrics,
                })
            }
        }
    }

    async fn snapshot(&self) -> Result<SnapshotId> {
        Err(ApexError::NotSupported(
            "ProcessSandbox does not support snapshots".into(),
        ))
    }

    async fn restore(&self, _id: SnapshotId) -> Result<()> {
        Err(ApexError::NotSupported(
            "ProcessSandbox does not support restore".into(),
        ))
    }

    fn language(&self) -> Language {
        self.language
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use apex_core::command::CommandOutput;

    // -----------------------------------------------------------------------
    // Local mock for CommandRunner (cross-crate; the cfg_attr(test) automock
    // on the trait only generates MockCommandRunner inside apex-core tests).
    // -----------------------------------------------------------------------
    mockall::mock! {
        pub CmdRunner {}

        #[async_trait]
        impl CommandRunner for CmdRunner {
            async fn run_command(&self, spec: &CommandSpec) -> Result<CommandOutput>;
        }
    }

    fn make_input(data: &[u8]) -> InputSeed {
        InputSeed::new(data.to_vec(), apex_core::types::SeedOrigin::Corpus)
    }

    // -----------------------------------------------------------------------
    // Existing tests (unchanged, use default RealCommandRunner type)
    // -----------------------------------------------------------------------

    #[test]
    fn new_sets_defaults() {
        let sb = ProcessSandbox::new(
            Language::Python,
            PathBuf::from("/tmp/target"),
            vec!["python3".into(), "-c".into(), "pass".into()],
        );
        assert_eq!(sb.language, Language::Python);
        assert_eq!(sb.target_dir, PathBuf::from("/tmp/target"));
        assert_eq!(sb.command, vec!["python3", "-c", "pass"]);
        assert_eq!(sb.timeout_ms, 10_000);
        assert!(sb.oracle.is_none());
        assert!(sb.branch_index.is_empty());
        assert!(sb.shim_path.is_none());
    }

    #[test]
    fn with_timeout_overrides_default() {
        let sb = ProcessSandbox::new(Language::C, PathBuf::from("/tmp"), vec!["./a.out".into()])
            .with_timeout(5_000);
        assert_eq!(sb.timeout_ms, 5_000);
    }

    #[test]
    fn with_shim_sets_path() {
        let sb = ProcessSandbox::new(
            Language::Rust,
            PathBuf::from("/tmp"),
            vec!["./target".into()],
        )
        .with_shim(PathBuf::from("/usr/lib/libapex.so"));
        assert_eq!(sb.shim_path, Some(PathBuf::from("/usr/lib/libapex.so")));
    }

    #[test]
    fn with_coverage_sets_oracle_and_index() {
        let oracle = Arc::new(CoverageOracle::new());
        let b1 = BranchId::new(1, 10, 0, 0);
        let b2 = BranchId::new(1, 20, 0, 1);
        let index = vec![b1.clone(), b2.clone()];

        let sb = ProcessSandbox::new(Language::C, PathBuf::from("/tmp"), vec!["./target".into()])
            .with_coverage(oracle.clone(), index.clone());

        assert!(sb.oracle.is_some());
        assert_eq!(sb.branch_index.len(), 2);
        assert_eq!(sb.branch_index[0], b1);
        assert_eq!(sb.branch_index[1], b2);
    }

    #[test]
    fn bitmap_to_new_branches_without_oracle_returns_empty() {
        let sb = ProcessSandbox::new(
            Language::Python,
            PathBuf::from("/tmp"),
            vec!["python3".into()],
        );
        let bitmap = vec![1u8; 10];
        assert!(sb.bitmap_to_new_branches(&bitmap).is_empty());
    }

    #[test]
    fn bitmap_to_new_branches_finds_uncovered_hits() {
        let oracle = Arc::new(CoverageOracle::new());
        let b0 = BranchId::new(1, 1, 0, 0);
        let b1 = BranchId::new(1, 2, 0, 0);
        let b2 = BranchId::new(1, 3, 0, 0);
        oracle.register_branches([b0.clone(), b1.clone(), b2.clone()]);

        let index = vec![b0.clone(), b1.clone(), b2.clone()];
        let sb = ProcessSandbox::new(Language::C, PathBuf::from("/tmp"), vec!["./a.out".into()])
            .with_coverage(oracle, index);

        // bitmap: b0 hit, b1 not hit, b2 hit
        let bitmap = vec![5, 0, 1];
        let new = sb.bitmap_to_new_branches(&bitmap);
        assert_eq!(new.len(), 2);
        assert!(new.contains(&b0));
        assert!(new.contains(&b2));
    }

    #[test]
    fn bitmap_to_new_branches_skips_already_covered() {
        let oracle = Arc::new(CoverageOracle::new());
        let b0 = BranchId::new(1, 1, 0, 0);
        let b1 = BranchId::new(1, 2, 0, 0);
        oracle.register_branches([b0.clone(), b1.clone()]);
        // Mark b0 as already covered.
        oracle.mark_covered(&b0, apex_core::types::SeedId::new());

        let index = vec![b0.clone(), b1.clone()];
        let sb = ProcessSandbox::new(Language::C, PathBuf::from("/tmp"), vec!["./a.out".into()])
            .with_coverage(oracle, index);

        // Both hit in bitmap, but b0 is already covered.
        let bitmap = vec![1, 1];
        let new = sb.bitmap_to_new_branches(&bitmap);
        assert_eq!(new.len(), 1);
        assert_eq!(new[0], b1);
    }

    #[test]
    fn bitmap_to_new_branches_handles_short_bitmap() {
        let oracle = Arc::new(CoverageOracle::new());
        let b0 = BranchId::new(1, 1, 0, 0);
        let b1 = BranchId::new(1, 2, 0, 0);
        oracle.register_branches([b0.clone(), b1.clone()]);

        let index = vec![b0.clone(), b1.clone()];
        let sb = ProcessSandbox::new(Language::C, PathBuf::from("/tmp"), vec!["./a.out".into()])
            .with_coverage(oracle, index);

        // Bitmap shorter than branch_index — only first entry matches.
        let bitmap = vec![1u8];
        let new = sb.bitmap_to_new_branches(&bitmap);
        assert_eq!(new.len(), 1);
        assert_eq!(new[0], b0);
    }

    #[test]
    fn language_returns_configured_language() {
        use apex_core::traits::Sandbox;
        let sb = ProcessSandbox::new(
            Language::JavaScript,
            PathBuf::from("/tmp"),
            vec!["node".into()],
        );
        assert_eq!(sb.language(), Language::JavaScript);
    }

    #[tokio::test]
    async fn snapshot_returns_not_supported() {
        use apex_core::traits::Sandbox;
        let sb = ProcessSandbox::new(
            Language::Python,
            PathBuf::from("/tmp"),
            vec!["python3".into()],
        );
        let result = sb.snapshot().await;
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), ApexError::NotSupported(_)));
    }

    #[tokio::test]
    async fn restore_returns_not_supported() {
        use apex_core::traits::Sandbox;
        use apex_core::types::SnapshotId;
        let sb = ProcessSandbox::new(
            Language::Python,
            PathBuf::from("/tmp"),
            vec!["python3".into()],
        );
        let result = sb.restore(SnapshotId::new()).await;
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), ApexError::NotSupported(_)));
    }

    #[tokio::test]
    async fn snapshot_error_message() {
        use apex_core::traits::Sandbox;
        let sb = ProcessSandbox::new(
            Language::Python,
            PathBuf::from("/tmp"),
            vec!["python3".into()],
        );
        let err = sb.snapshot().await.unwrap_err();
        let msg = format!("{err}");
        assert!(
            msg.contains("does not support snapshots"),
            "expected 'does not support snapshots' in: {msg}"
        );
    }

    #[tokio::test]
    async fn restore_error_message() {
        use apex_core::traits::Sandbox;
        use apex_core::types::SnapshotId;
        let sb = ProcessSandbox::new(
            Language::Python,
            PathBuf::from("/tmp"),
            vec!["python3".into()],
        );
        let err = sb.restore(SnapshotId::new()).await.unwrap_err();
        let msg = format!("{err}");
        assert!(
            msg.contains("does not support restore"),
            "expected 'does not support restore' in: {msg}"
        );
    }

    #[test]
    fn bitmap_to_new_branches_empty_bitmap_with_oracle() {
        let oracle = Arc::new(CoverageOracle::new());
        let b0 = BranchId::new(1, 1, 0, 0);
        let b1 = BranchId::new(1, 2, 0, 0);
        oracle.register_branches([b0.clone(), b1.clone()]);

        let sb = ProcessSandbox::new(Language::C, PathBuf::from("/tmp"), vec!["./a.out".into()])
            .with_coverage(oracle, vec![b0, b1]);

        let new = sb.bitmap_to_new_branches(&[]);
        assert!(new.is_empty());
    }

    #[test]
    fn new_with_empty_command() {
        let sb = ProcessSandbox::new(Language::Python, PathBuf::from("/tmp"), vec![]);
        assert!(sb.command.is_empty());
        assert_eq!(sb.language, Language::Python);
    }

    // -----------------------------------------------------------------------
    // New mock-based tests for ProcessSandbox::run()
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn run_successful_execution() {
        let mut mock = MockCmdRunner::new();
        mock.expect_run_command().returning(|_spec| {
            Ok(CommandOutput {
                exit_code: 0,
                stdout: b"hello world".to_vec(),
                stderr: Vec::new(),
            })
        });

        let sb = ProcessSandbox::with_runner(
            Language::Python,
            PathBuf::from("/tmp"),
            vec!["python3".into(), "-c".into(), "print('hello')".into()],
            mock,
        );

        let input = make_input(b"test input");
        let result = sb.run(&input).await.unwrap();
        assert_eq!(result.status, ExecutionStatus::Pass);
        assert_eq!(result.stdout, "hello world");
        assert!(result.stderr.is_empty());
        assert_eq!(result.seed_id, input.id);
    }

    #[tokio::test]
    async fn run_timeout_returns_timeout_status() {
        let mut mock = MockCmdRunner::new();
        mock.expect_run_command()
            .returning(|_spec| Err(ApexError::Timeout(5000)));

        let sb = ProcessSandbox::with_runner(
            Language::Python,
            PathBuf::from("/tmp"),
            vec!["python3".into()],
            mock,
        )
        .with_timeout(5000);

        let input = make_input(b"input");
        let result = sb.run(&input).await.unwrap();
        assert_eq!(result.status, ExecutionStatus::Timeout);
        assert!(result.new_branches.is_empty());
        assert!(result.stdout.is_empty());
        assert!(result.stderr.is_empty());
    }

    #[tokio::test]
    async fn run_crash_nonzero_negative_exit_code() {
        let mut mock = MockCmdRunner::new();
        mock.expect_run_command().returning(|_spec| {
            Ok(CommandOutput {
                exit_code: -11, // SIGSEGV
                stdout: Vec::new(),
                stderr: b"Segmentation fault".to_vec(),
            })
        });

        let sb = ProcessSandbox::with_runner(
            Language::C,
            PathBuf::from("/tmp"),
            vec!["./a.out".into()],
            mock,
        );

        let input = make_input(b"crash input");
        let result = sb.run(&input).await.unwrap();
        assert_eq!(result.status, ExecutionStatus::Crash);
        assert_eq!(result.stderr, "Segmentation fault");
    }

    #[tokio::test]
    async fn run_fail_positive_nonzero_exit_code() {
        let mut mock = MockCmdRunner::new();
        mock.expect_run_command().returning(|_spec| {
            Ok(CommandOutput {
                exit_code: 1,
                stdout: Vec::new(),
                stderr: b"assertion failed".to_vec(),
            })
        });

        let sb = ProcessSandbox::with_runner(
            Language::Python,
            PathBuf::from("/tmp"),
            vec!["python3".into()],
            mock,
        );

        let input = make_input(b"bad input");
        let result = sb.run(&input).await.unwrap();
        assert_eq!(result.status, ExecutionStatus::Fail);
        assert_eq!(result.stderr, "assertion failed");
    }

    #[tokio::test]
    async fn run_empty_command_returns_error() {
        let mock = MockCmdRunner::new();
        let sb = ProcessSandbox::with_runner(Language::Python, PathBuf::from("/tmp"), vec![], mock);

        let input = make_input(b"test");
        let result = sb.run(&input).await;
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("empty command"), "got: {err_msg}");
    }

    #[tokio::test]
    async fn run_empty_stdout_and_stderr() {
        let mut mock = MockCmdRunner::new();
        mock.expect_run_command().returning(|_spec| {
            Ok(CommandOutput {
                exit_code: 0,
                stdout: Vec::new(),
                stderr: Vec::new(),
            })
        });

        let sb = ProcessSandbox::with_runner(
            Language::Python,
            PathBuf::from("/tmp"),
            vec!["python3".into()],
            mock,
        );

        let input = make_input(b"");
        let result = sb.run(&input).await.unwrap();
        assert_eq!(result.status, ExecutionStatus::Pass);
        assert!(result.stdout.is_empty());
        assert!(result.stderr.is_empty());
    }

    #[tokio::test]
    async fn run_subprocess_error_becomes_sandbox_error() {
        let mut mock = MockCmdRunner::new();
        mock.expect_run_command().returning(|_spec| {
            Err(ApexError::Subprocess {
                exit_code: -1,
                stderr: "spawn failed".into(),
            })
        });

        let sb = ProcessSandbox::with_runner(
            Language::Python,
            PathBuf::from("/tmp"),
            vec!["python3".into()],
            mock,
        );

        let input = make_input(b"test");
        let result = sb.run(&input).await;
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("run_command"), "got: {err_msg}");
    }

    #[tokio::test]
    async fn run_passes_stdin_in_spec() {
        let mut mock = MockCmdRunner::new();
        mock.expect_run_command()
            .withf(|spec: &CommandSpec| spec.stdin == Some(b"my input data".to_vec()))
            .returning(|_| Ok(CommandOutput::success(b"ok".to_vec())));

        let sb = ProcessSandbox::with_runner(
            Language::Python,
            PathBuf::from("/tmp"),
            vec!["python3".into()],
            mock,
        );

        let input = make_input(b"my input data");
        let result = sb.run(&input).await.unwrap();
        assert_eq!(result.status, ExecutionStatus::Pass);
    }

    #[tokio::test]
    async fn run_passes_correct_timeout_in_spec() {
        let mut mock = MockCmdRunner::new();
        mock.expect_run_command()
            .withf(|spec: &CommandSpec| spec.timeout_ms == 3_000)
            .returning(|_| Ok(CommandOutput::success(Vec::new())));

        let sb = ProcessSandbox::with_runner(
            Language::Python,
            PathBuf::from("/tmp"),
            vec!["python3".into()],
            mock,
        )
        .with_timeout(3_000);

        let input = make_input(b"");
        sb.run(&input).await.unwrap();
    }

    #[tokio::test]
    async fn run_passes_args_in_spec() {
        let mut mock = MockCmdRunner::new();
        mock.expect_run_command()
            .withf(|spec: &CommandSpec| {
                spec.program == "python3"
                    && spec.args == vec!["-c", "print(1)"]
                    && spec.working_dir == PathBuf::from("/work")
            })
            .returning(|_| Ok(CommandOutput::success(Vec::new())));

        let sb = ProcessSandbox::with_runner(
            Language::Python,
            PathBuf::from("/work"),
            vec!["python3".into(), "-c".into(), "print(1)".into()],
            mock,
        );

        let input = make_input(b"");
        sb.run(&input).await.unwrap();
    }

    #[tokio::test]
    async fn run_binary_stdout_is_lossy_converted() {
        let mut mock = MockCmdRunner::new();
        mock.expect_run_command().returning(|_spec| {
            Ok(CommandOutput {
                exit_code: 0,
                // Invalid UTF-8 bytes
                stdout: vec![0xFF, 0xFE, b'h', b'i'],
                stderr: vec![0x80, 0x81],
            })
        });

        let sb = ProcessSandbox::with_runner(
            Language::C,
            PathBuf::from("/tmp"),
            vec!["./a.out".into()],
            mock,
        );

        let input = make_input(b"bin");
        let result = sb.run(&input).await.unwrap();
        // Should not panic, and should contain the valid portion
        assert!(result.stdout.contains("hi"));
        assert!(!result.stderr.is_empty());
    }

    #[test]
    fn build_spec_includes_shim_env() {
        let mock = MockCmdRunner::new();
        let sb = ProcessSandbox::with_runner(
            Language::C,
            PathBuf::from("/tmp"),
            vec!["./a.out".into(), "--flag".into()],
            mock,
        )
        .with_shim(PathBuf::from("/usr/lib/libapex.so"));

        let input = make_input(b"test");
        let spec = sb.build_spec(&input, None).unwrap();

        let preload_var = crate::shim::preload_env_var();
        let has_shim_env = spec
            .env
            .iter()
            .any(|(k, v)| k == preload_var && v == "/usr/lib/libapex.so");
        assert!(has_shim_env, "spec.env = {:?}", spec.env);
    }

    #[test]
    fn build_spec_includes_shm_env() {
        let mock = MockCmdRunner::new();
        let sb = ProcessSandbox::with_runner(
            Language::C,
            PathBuf::from("/tmp"),
            vec!["./a.out".into()],
            mock,
        );

        let input = make_input(b"test");
        let spec = sb.build_spec(&input, Some("shm_test_name")).unwrap();

        let has_shm_env = spec
            .env
            .iter()
            .any(|(k, v)| k == SHM_ENV_VAR && v == "shm_test_name");
        assert!(has_shm_env, "spec.env = {:?}", spec.env);
    }

    #[test]
    fn build_spec_empty_command_errors() {
        let mock = MockCmdRunner::new();
        let sb = ProcessSandbox::with_runner(Language::Python, PathBuf::from("/tmp"), vec![], mock);

        let input = make_input(b"test");
        let result = sb.build_spec(&input, None);
        assert!(result.is_err());
    }

    #[test]
    fn with_runner_sets_all_fields() {
        let mock = MockCmdRunner::new();
        let sb = ProcessSandbox::with_runner(
            Language::Java,
            PathBuf::from("/work"),
            vec!["java".into(), "-jar".into(), "app.jar".into()],
            mock,
        );
        assert_eq!(sb.language, Language::Java);
        assert_eq!(sb.target_dir, PathBuf::from("/work"));
        assert_eq!(sb.command, vec!["java", "-jar", "app.jar"]);
        assert_eq!(sb.timeout_ms, 10_000);
        assert!(sb.oracle.is_none());
    }

    // ------------------------------------------------------------------
    // Additional branch-coverage tests
    // ------------------------------------------------------------------

    /// `build_spec` with no shim and no shm_name: neither env var is injected.
    #[test]
    fn build_spec_no_shim_no_shm_no_env() {
        let mock = MockCmdRunner::new();
        let sb = ProcessSandbox::with_runner(
            Language::Python,
            PathBuf::from("/tmp"),
            vec!["python3".into(), "test.py".into()],
            mock,
        );
        let input = make_input(b"hello");
        let spec = sb.build_spec(&input, None).unwrap();
        // No SHM or shim env vars should be present.
        let has_shm = spec.env.iter().any(|(k, _)| k == SHM_ENV_VAR);
        let has_preload = spec
            .env
            .iter()
            .any(|(k, _)| k == "LD_PRELOAD" || k == "DYLD_INSERT_LIBRARIES");
        assert!(!has_shm);
        assert!(!has_preload);
    }

    /// `build_spec` with both shim and shm_name: both env vars injected.
    #[test]
    fn build_spec_with_shim_and_shm() {
        let mock = MockCmdRunner::new();
        let sb = ProcessSandbox::with_runner(
            Language::C,
            PathBuf::from("/tmp"),
            vec!["./target".into()],
            mock,
        )
        .with_shim(PathBuf::from("/lib/libapex.so"));

        let input = make_input(b"data");
        let spec = sb.build_spec(&input, Some("/apx_deadbeef")).unwrap();

        let preload_var = crate::shim::preload_env_var();
        let has_shim = spec
            .env
            .iter()
            .any(|(k, v)| k == preload_var && v == "/lib/libapex.so");
        let has_shm = spec
            .env
            .iter()
            .any(|(k, v)| k == SHM_ENV_VAR && v == "/apx_deadbeef");
        assert!(has_shim, "shim env var missing: {:?}", spec.env);
        assert!(has_shm, "shm env var missing: {:?}", spec.env);
    }

    /// `language()` with each supported language.
    #[test]
    fn language_all_variants() {
        use apex_core::traits::Sandbox;
        for (lang, name) in [
            (Language::Python, "python"),
            (Language::Rust, "rust"),
            (Language::C, "c"),
            (Language::Java, "java"),
            (Language::JavaScript, "javascript"),
        ] {
            let sb = ProcessSandbox::new(lang, PathBuf::from("/tmp"), vec!["cmd".into()]);
            assert_eq!(sb.language(), lang, "language mismatch for {name}");
        }
    }

    /// `with_coverage` enables coverage feedback (oracle set, branch_index populated).
    #[test]
    fn with_coverage_oracle_present() {
        let oracle = Arc::new(CoverageOracle::new());
        let b = BranchId::new(1, 1, 0, 0);
        oracle.register_branches([b.clone()]);
        let index = vec![b.clone()];
        let mock = MockCmdRunner::new();
        let sb = ProcessSandbox::with_runner(
            Language::Rust,
            PathBuf::from("/tmp"),
            vec!["./target".into()],
            mock,
        )
        .with_coverage(oracle, index);

        assert!(sb.oracle.is_some());
        assert_eq!(sb.branch_index.len(), 1);
        assert_eq!(sb.branch_index[0], b);
    }

    /// `bitmap_to_new_branches` with oracle set but empty branch_index always empty.
    #[test]
    fn bitmap_to_new_branches_oracle_but_empty_index() {
        let oracle = Arc::new(CoverageOracle::new());
        let mock = MockCmdRunner::new();
        let sb = ProcessSandbox::with_runner(
            Language::C,
            PathBuf::from("/tmp"),
            vec!["./a.out".into()],
            mock,
        )
        .with_coverage(oracle, vec![]); // empty branch_index

        let bitmap = vec![1u8; 8];
        let result = sb.bitmap_to_new_branches(&bitmap);
        assert!(result.is_empty());
    }

    /// `run()` with exit code 2 (positive, non-zero, non-negative) → Fail.
    #[tokio::test]
    async fn run_positive_exit_code_two_is_fail() {
        let mut mock = MockCmdRunner::new();
        mock.expect_run_command().returning(|_spec| {
            Ok(apex_core::command::CommandOutput {
                exit_code: 2,
                stdout: Vec::new(),
                stderr: b"error".to_vec(),
            })
        });

        let sb = ProcessSandbox::with_runner(
            Language::Python,
            PathBuf::from("/tmp"),
            vec!["python3".into()],
            mock,
        );

        let input = make_input(b"test");
        let result = sb.run(&input).await.unwrap();
        assert_eq!(result.status, ExecutionStatus::Fail);
    }

    /// `run()` with exit code -1 → Crash (negative exit code arm).
    #[tokio::test]
    async fn run_minus_one_exit_is_crash() {
        let mut mock = MockCmdRunner::new();
        mock.expect_run_command().returning(|_spec| {
            Ok(apex_core::command::CommandOutput {
                exit_code: -1,
                stdout: Vec::new(),
                stderr: Vec::new(),
            })
        });

        let sb = ProcessSandbox::with_runner(
            Language::C,
            PathBuf::from("/tmp"),
            vec!["./a.out".into()],
            mock,
        );

        let input = make_input(b"crash");
        let result = sb.run(&input).await.unwrap();
        assert_eq!(result.status, ExecutionStatus::Crash);
    }

    /// `snapshot()` error variant is `NotSupported`.
    #[tokio::test]
    async fn snapshot_is_not_supported_variant() {
        use apex_core::traits::Sandbox;
        let mock = MockCmdRunner::new();
        let sb = ProcessSandbox::with_runner(
            Language::Python,
            PathBuf::from("/tmp"),
            vec!["python3".into()],
            mock,
        );
        let err = sb.snapshot().await.unwrap_err();
        assert!(matches!(err, ApexError::NotSupported(_)));
    }

    /// `restore()` error variant is `NotSupported`.
    #[tokio::test]
    async fn restore_is_not_supported_variant() {
        use apex_core::traits::Sandbox;
        use apex_core::types::SnapshotId;
        let mock = MockCmdRunner::new();
        let sb = ProcessSandbox::with_runner(
            Language::Python,
            PathBuf::from("/tmp"),
            vec!["python3".into()],
            mock,
        );
        let err = sb.restore(SnapshotId::new()).await.unwrap_err();
        assert!(matches!(err, ApexError::NotSupported(_)));
    }

    // -----------------------------------------------------------------------
    // Bug-exposing tests
    // -----------------------------------------------------------------------

    /// BUG: When a process times out, run() discards any coverage data the
    /// process wrote before the timeout. The bitmap is read (line 161) but
    /// the Timeout arm returns empty new_branches. Coverage from partial
    /// execution is silently lost.
    #[tokio::test]
    async fn bug_timeout_discards_coverage_data() {
        // This test documents that timeout always returns empty branches.
        // A timed-out process may have written valid coverage data to SHM
        // before being killed, but that data is thrown away.
        let mut mock = MockCmdRunner::new();
        mock.expect_run_command()
            .returning(|_spec| Err(ApexError::Timeout(5000)));

        let oracle = Arc::new(CoverageOracle::new());
        let b0 = BranchId::new(1, 1, 0, 0);
        oracle.register_branches([b0.clone()]);

        let sb = ProcessSandbox::with_runner(
            Language::C,
            PathBuf::from("/tmp"),
            vec!["./a.out".into()],
            mock,
        )
        .with_coverage(oracle, vec![b0])
        .with_timeout(5000);

        let input = make_input(b"timeout-input");
        let result = sb.run(&input).await.unwrap();
        assert_eq!(result.status, ExecutionStatus::Timeout);
        // BUG: new_branches is always empty on timeout, even if the
        // process wrote coverage data before being killed.
        assert!(
            result.new_branches.is_empty(),
            "BUG CONFIRMED: timeout discards all coverage data"
        );
    }

    /// BUG: run() silently degrades to no-coverage mode when SHM creation
    /// fails. The caller gets a successful ExecutionResult with empty
    /// new_branches, indistinguishable from "target didn't hit any branches".
    /// This can cause the fuzzer to miss all coverage feedback without
    /// any indication.
    #[tokio::test]
    async fn bug_shm_failure_silently_drops_coverage() {
        // When oracle is set but ShmBitmap::create() fails, run() logs a
        // warning and continues with None. The result has new_branches = [],
        // which is the same as "no new coverage found". The caller cannot
        // distinguish "SHM failed" from "target hit no new branches".
        let mut mock = MockCmdRunner::new();
        mock.expect_run_command().returning(|_spec| {
            Ok(CommandOutput {
                exit_code: 0,
                stdout: Vec::new(),
                stderr: Vec::new(),
            })
        });

        let oracle = Arc::new(CoverageOracle::new());
        let b0 = BranchId::new(1, 1, 0, 0);
        oracle.register_branches([b0.clone()]);

        let sb = ProcessSandbox::with_runner(
            Language::C,
            PathBuf::from("/tmp"),
            vec!["./a.out".into()],
            mock,
        )
        .with_coverage(oracle, vec![b0]);

        let input = make_input(b"test");
        let result = sb.run(&input).await.unwrap();
        // This test documents the behavior: even with oracle set, if SHM
        // creation succeeds (which it does here), we get empty branches
        // because the mock doesn't write to SHM. But the real bug is that
        // SHM failure is silently swallowed with just a warning log.
        assert!(
            result.new_branches.is_empty(),
            "Expected empty branches when process doesn't write to SHM"
        );
    }

    /// BUG: build_spec uses to_string_lossy() on the output path, which
    /// silently replaces non-UTF8 path components with the Unicode
    /// replacement character. This would cause the compiler to write to
    /// the wrong path or fail in a confusing way.
    #[test]
    fn bug_build_spec_shim_path_uses_lossy_conversion() {
        let mock = MockCmdRunner::new();
        let sb = ProcessSandbox::with_runner(
            Language::C,
            PathBuf::from("/tmp"),
            vec!["./a.out".into()],
            mock,
        )
        .with_shim(PathBuf::from("/path/with spaces/libapex.so"));

        let input = make_input(b"test");
        let spec = sb.build_spec(&input, None).unwrap();

        // Verify the shim path is passed through to the env var.
        // Paths with spaces work fine, but the underlying to_string_lossy()
        // in ensure_compiled() would corrupt non-UTF8 paths.
        let preload_var = crate::shim::preload_env_var();
        let shim_env = spec.env.iter().find(|(k, _)| k == preload_var);
        assert!(shim_env.is_some(), "shim env var should be set");
        assert_eq!(
            shim_env.unwrap().1,
            "/path/with spaces/libapex.so",
            "shim path should be preserved exactly (spaces are ok)"
        );
    }

    /// Verify that run() seed_id matches input seed_id for all exit statuses.
    #[tokio::test]
    async fn bug_seed_id_preserved_across_all_statuses() {
        for exit_code in [-11i32, 0, 1, 2, 127] {
            let mut mock = MockCmdRunner::new();
            mock.expect_run_command().returning(move |_spec| {
                Ok(CommandOutput {
                    exit_code,
                    stdout: Vec::new(),
                    stderr: Vec::new(),
                })
            });

            let sb = ProcessSandbox::with_runner(
                Language::C,
                PathBuf::from("/tmp"),
                vec!["./a.out".into()],
                mock,
            );

            let input = make_input(b"test");
            let expected_id = input.id;
            let result = sb.run(&input).await.unwrap();
            assert_eq!(
                result.seed_id, expected_id,
                "seed_id must be preserved for exit_code={exit_code}"
            );
        }
    }

    /// BUG: run() sets duration_ms from wall clock, but if the runner
    /// returns a Timeout error very quickly (mock), duration_ms can be 0.
    /// The duration_ms is measured from before SHM creation to after
    /// runner.run_command(), so it includes SHM setup overhead.
    #[tokio::test]
    async fn bug_duration_includes_shm_setup_overhead() {
        let mut mock = MockCmdRunner::new();
        mock.expect_run_command().returning(|_spec| {
            Ok(CommandOutput {
                exit_code: 0,
                stdout: Vec::new(),
                stderr: Vec::new(),
            })
        });

        let oracle = Arc::new(CoverageOracle::new());
        let b0 = BranchId::new(1, 1, 0, 0);
        oracle.register_branches([b0.clone()]);

        let sb = ProcessSandbox::with_runner(
            Language::C,
            PathBuf::from("/tmp"),
            vec!["./a.out".into()],
            mock,
        )
        .with_coverage(oracle, vec![b0]);

        let input = make_input(b"test");
        let result = sb.run(&input).await.unwrap();
        // duration_ms includes SHM creation + command execution.
        // For a fast mock, this should be very small but non-negative.
        // The issue is that duration_ms reports total wall time, not just
        // command execution time, which can be misleading for profiling.
        assert!(
            result.duration_ms < 5000,
            "duration should be reasonable for a mock: {}ms",
            result.duration_ms
        );
    }

    /// run() with coverage enabled but empty branch_index should return
    /// empty new_branches even if the bitmap has hits.
    #[tokio::test]
    async fn bug_coverage_with_empty_branch_index_always_empty() {
        let mut mock = MockCmdRunner::new();
        mock.expect_run_command().returning(|_spec| {
            Ok(CommandOutput {
                exit_code: 0,
                stdout: Vec::new(),
                stderr: Vec::new(),
            })
        });

        let oracle = Arc::new(CoverageOracle::new());
        // Enable coverage but with empty branch_index
        let sb = ProcessSandbox::with_runner(
            Language::C,
            PathBuf::from("/tmp"),
            vec!["./a.out".into()],
            mock,
        )
        .with_coverage(oracle, vec![]); // empty index = useless coverage

        let input = make_input(b"test");
        let result = sb.run(&input).await.unwrap();
        // BUG: Caller can accidentally enable coverage with empty branch_index,
        // which creates SHM overhead but can never report any branches.
        // There's no validation or warning for this case.
        assert!(
            result.new_branches.is_empty(),
            "empty branch_index means SHM was created for nothing"
        );
    }

    /// Verify trace is always None in ProcessSandbox results.
    #[tokio::test]
    async fn trace_always_none() {
        let mut mock = MockCmdRunner::new();
        mock.expect_run_command().returning(|_spec| {
            Ok(CommandOutput {
                exit_code: 0,
                stdout: b"output".to_vec(),
                stderr: Vec::new(),
            })
        });

        let sb = ProcessSandbox::with_runner(
            Language::Python,
            PathBuf::from("/tmp"),
            vec!["python3".into()],
            mock,
        );

        let input = make_input(b"test");
        let result = sb.run(&input).await.unwrap();
        assert!(
            result.trace.is_none(),
            "ProcessSandbox never produces traces"
        );
    }

    /// Verify input field is always None in ProcessSandbox results.
    #[tokio::test]
    async fn input_field_always_none() {
        let mut mock = MockCmdRunner::new();
        mock.expect_run_command().returning(|_spec| {
            Ok(CommandOutput {
                exit_code: 0,
                stdout: Vec::new(),
                stderr: Vec::new(),
            })
        });

        let sb = ProcessSandbox::with_runner(
            Language::Python,
            PathBuf::from("/tmp"),
            vec!["python3".into()],
            mock,
        );

        let input = make_input(b"some data");
        let result = sb.run(&input).await.unwrap();
        // BUG: The input field in ExecutionResult is always None.
        // This means the caller cannot correlate the result back to the
        // exact input bytes that were sent, which is needed for
        // crash reproduction.
        assert!(
            result.input.is_none(),
            "BUG: input is never populated in ExecutionResult"
        );
    }

    // -----------------------------------------------------------------------
    // resource_metrics tests
    // -----------------------------------------------------------------------

    /// After a successful run, resource_metrics must be Some and wall_time_ms
    /// must equal duration_ms (both derived from the same wall-clock elapsed time).
    #[tokio::test]
    async fn run_populates_resource_metrics_on_success() {
        let mut mock = MockCmdRunner::new();
        mock.expect_run_command().returning(|_spec| {
            Ok(CommandOutput {
                exit_code: 0,
                stdout: Vec::new(),
                stderr: Vec::new(),
            })
        });

        let sb = ProcessSandbox::with_runner(
            Language::Python,
            PathBuf::from("/tmp"),
            vec!["python3".into()],
            mock,
        );

        let input = make_input(b"test");
        let result = sb.run(&input).await.unwrap();

        let metrics = result
            .resource_metrics
            .expect("resource_metrics must be Some after execution");
        assert_eq!(
            metrics.wall_time_ms, result.duration_ms,
            "wall_time_ms must equal duration_ms"
        );
    }

    /// After a timeout, resource_metrics must also be Some with wall_time_ms > 0
    /// when the mock returns immediately (wall_time_ms can be 0 for near-instant
    /// mocks, but the field must be populated).
    #[tokio::test]
    async fn run_populates_resource_metrics_on_timeout() {
        let mut mock = MockCmdRunner::new();
        mock.expect_run_command()
            .returning(|_spec| Err(ApexError::Timeout(5000)));

        let sb = ProcessSandbox::with_runner(
            Language::Python,
            PathBuf::from("/tmp"),
            vec!["python3".into()],
            mock,
        )
        .with_timeout(5000);

        let input = make_input(b"input");
        let result = sb.run(&input).await.unwrap();
        assert_eq!(result.status, ExecutionStatus::Timeout);

        let metrics = result
            .resource_metrics
            .expect("resource_metrics must be Some even on timeout");
        assert_eq!(
            metrics.wall_time_ms, result.duration_ms,
            "wall_time_ms must equal duration_ms on timeout"
        );
    }

    /// resource_metrics.wall_time_ms is non-negative (u64 invariant) and
    /// peak_memory_bytes / cpu_time_ms are also non-negative (u64 invariant).
    /// This test uses a mock that returns immediately and verifies all three
    /// fields are present and have sensible types.
    #[tokio::test]
    async fn run_resource_metrics_fields_are_present() {
        let mut mock = MockCmdRunner::new();
        mock.expect_run_command().returning(|_spec| {
            Ok(CommandOutput {
                exit_code: 0,
                stdout: Vec::new(),
                stderr: Vec::new(),
            })
        });

        let sb = ProcessSandbox::with_runner(
            Language::C,
            PathBuf::from("/tmp"),
            vec!["./a.out".into()],
            mock,
        );

        let input = make_input(b"data");
        let result = sb.run(&input).await.unwrap();

        let metrics = result
            .resource_metrics
            .expect("resource_metrics must be populated");
        // All fields are u64 so always >= 0; just verify they are present and
        // wall_time_ms is consistent with the top-level duration_ms field.
        assert_eq!(metrics.wall_time_ms, result.duration_ms);
        // cpu_time_ms and peak_memory_bytes should be reasonable (not absurdly large).
        assert!(
            metrics.cpu_time_ms < 3_600_000,
            "cpu_time_ms should not be hours: {}",
            metrics.cpu_time_ms
        );
        assert!(
            metrics.peak_memory_bytes < 256 * 1024 * 1024 * 1024,
            "peak_memory_bytes should not be > 256 GiB: {}",
            metrics.peak_memory_bytes
        );
    }
}
