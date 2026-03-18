//! Trait abstraction over subprocess execution.
//!
//! Production code uses [`RealCommandRunner`] which delegates to
//! `tokio::process::Command`. Tests inject [`MockCommandRunner`] (via mockall)
//! to control subprocess outputs without spawning real processes.

use crate::error::{ApexError, Result};
use async_trait::async_trait;
use std::path::PathBuf;

/// Specification for a command to execute.
#[derive(Debug, Clone)]
pub struct CommandSpec {
    pub program: String,
    pub args: Vec<String>,
    pub working_dir: PathBuf,
    pub stdin: Option<Vec<u8>>,
    pub env: Vec<(String, String)>,
    pub timeout_ms: u64,
}

impl CommandSpec {
    pub fn new(program: impl Into<String>, working_dir: impl Into<PathBuf>) -> Self {
        CommandSpec {
            program: program.into(),
            args: Vec::new(),
            working_dir: working_dir.into(),
            stdin: None,
            env: Vec::new(),
            timeout_ms: 30_000,
        }
    }

    pub fn args(mut self, args: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.args = args.into_iter().map(Into::into).collect();
        self
    }

    pub fn stdin(mut self, data: Vec<u8>) -> Self {
        self.stdin = Some(data);
        self
    }

    pub fn env(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.env.push((key.into(), value.into()));
        self
    }

    pub fn timeout(mut self, ms: u64) -> Self {
        self.timeout_ms = ms;
        self
    }
}

// ---------------------------------------------------------------------------
// Adaptive timeouts — scale with project size
// ---------------------------------------------------------------------------

/// What kind of subprocess operation is being timed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpKind {
    /// Downloading dependencies (npm install, go mod download, bundle install).
    DepInstall,
    /// Compiling / instrumenting (cargo build, gcc, dotnet build).
    Compile,
    /// Running test suite under coverage.
    TestRun,
    /// Post-processing (gcov, llvm-profdata merge, coverage report).
    PostProcess,
}

/// Compute a timeout in milliseconds that scales with project size.
///
/// Formula: `clamp(base + file_count * per_file, min, max)`
///
/// | OpKind      | base  | per_file | min   | max     |
/// |-------------|------:|---------:|------:|--------:|
/// | DepInstall  |  60s  |     0    |  60s  |  300s   |
/// | Compile     |  60s  |    50ms  |  60s  |  600s   |
/// | TestRun     |  60s  |   100ms  |  60s  |  600s   |
/// | PostProcess |  30s  |    10ms  |  30s  |  120s   |
///
/// `lang_multiplier` adjusts for language speed:
/// - C/C++: 2.0 (slow compilation)
/// - Rust: 1.5, Java/Kotlin: 1.5, Swift: 1.5
/// - Go: 1.0
/// - Python/JS/Ruby: 0.5 (interpreted — fast startup, deps are the bottleneck)
pub fn adaptive_timeout(file_count: usize, lang: crate::types::Language, op: OpKind) -> u64 {
    use crate::types::Language;

    let (base_ms, per_file_ms, min_ms, max_ms): (u64, u64, u64, u64) = match op {
        OpKind::DepInstall  => (60_000,   0, 60_000, 300_000),
        OpKind::Compile     => (60_000,  50, 60_000, 600_000),
        OpKind::TestRun     => (60_000, 100, 60_000, 600_000),
        OpKind::PostProcess => (30_000,  10, 30_000, 120_000),
    };

    let multiplier: f64 = match lang {
        Language::C | Language::Cpp     => 2.0,
        Language::Rust                  => 1.5,
        Language::Java | Language::Kotlin => 1.5,
        Language::Swift                 => 1.5,
        Language::Go                    => 1.0,
        Language::CSharp                => 1.2,
        Language::Python | Language::JavaScript | Language::Ruby => 0.5,
        Language::Wasm                  => 1.0,
    };

    let raw = base_ms + (file_count as u64) * per_file_ms;
    let scaled = (raw as f64 * multiplier) as u64;
    scaled.clamp(min_ms, max_ms)
}

/// Count source files in a directory (non-recursive quick scan for timeout estimation).
/// Counts files with common source extensions, skipping build/vendor dirs.
pub fn count_source_files(dir: &std::path::Path) -> usize {
    let skip = ["target", "node_modules", "vendor", ".git", "build", "dist", "__pycache__", ".tox", "venv", ".venv"];
    let mut count = 0;
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                if skip.contains(&name) { continue; }
            }
            if path.is_dir() {
                count += count_source_files(&path);
            } else if path.is_file() {
                let is_source = path.extension().and_then(|e| e.to_str()).is_some_and(|ext| {
                    matches!(ext, "rs" | "py" | "js" | "ts" | "jsx" | "tsx" | "go" | "java" | "kt"
                        | "c" | "h" | "cpp" | "hpp" | "cs" | "swift" | "rb" | "wasm")
                });
                if is_source { count += 1; }
            }
        }
    }
    count
}

/// Output from a command execution.
#[derive(Debug, Clone)]
pub struct CommandOutput {
    pub exit_code: i32,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}

impl CommandOutput {
    pub fn success(stdout: impl Into<Vec<u8>>) -> Self {
        CommandOutput {
            exit_code: 0,
            stdout: stdout.into(),
            stderr: Vec::new(),
        }
    }

    pub fn failure(exit_code: i32, stderr: impl Into<Vec<u8>>) -> Self {
        CommandOutput {
            exit_code,
            stdout: Vec::new(),
            stderr: stderr.into(),
        }
    }
}

/// Abstraction over subprocess execution.
#[cfg_attr(test, mockall::automock)]
#[async_trait]
pub trait CommandRunner: Send + Sync {
    async fn run_command(&self, spec: &CommandSpec) -> Result<CommandOutput>;
}

/// Real subprocess runner using `tokio::process::Command`.
pub struct RealCommandRunner;

#[async_trait]
impl CommandRunner for RealCommandRunner {
    async fn run_command(&self, spec: &CommandSpec) -> Result<CommandOutput> {
        let mut cmd = tokio::process::Command::new(&spec.program);
        cmd.args(&spec.args)
            .current_dir(&spec.working_dir)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());

        for (k, v) in &spec.env {
            cmd.env(k, v);
        }

        let mut child = cmd.spawn().map_err(|e| ApexError::Subprocess {
            exit_code: -1,
            stderr: format!("spawn {}: {e}", spec.program),
        })?;

        // Write stdin if provided.
        if let Some(ref data) = spec.stdin {
            if let Some(mut stdin) = child.stdin.take() {
                use tokio::io::AsyncWriteExt;
                let _ =
                    tokio::time::timeout(std::time::Duration::from_secs(30), stdin.write_all(data))
                        .await;
                // stdin is dropped here, closing the pipe
            }
        }

        let deadline = std::time::Duration::from_millis(spec.timeout_ms);
        let result = tokio::time::timeout(deadline, child.wait_with_output()).await;

        match result {
            // On timeout, the `child` future is dropped. tokio's `Child` Drop impl
            // sends SIGKILL on Unix (and terminates on Windows), so the child process
            // is cleaned up automatically — no orphaned processes.
            Err(_) => Err(ApexError::Timeout(spec.timeout_ms)),
            Ok(Err(e)) => Err(ApexError::Subprocess {
                exit_code: -1,
                stderr: format!("wait: {e}"),
            }),
            Ok(Ok(output)) => Ok(CommandOutput {
                exit_code: output.status.code().unwrap_or(-1),
                stdout: output.stdout,
                stderr: output.stderr,
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_spec_new_defaults() {
        let spec = CommandSpec::new("echo", "/tmp");
        assert_eq!(spec.program, "echo");
        assert_eq!(spec.working_dir, PathBuf::from("/tmp"));
        assert!(spec.args.is_empty());
        assert!(spec.stdin.is_none());
        assert!(spec.env.is_empty());
        assert_eq!(spec.timeout_ms, 30_000);
    }

    #[test]
    fn command_spec_builder_methods() {
        let spec = CommandSpec::new("test", "/work")
            .args(["--flag", "value"])
            .stdin(b"input".to_vec())
            .env("KEY", "VAL")
            .timeout(5_000);

        assert_eq!(spec.args, vec!["--flag", "value"]);
        assert_eq!(spec.stdin, Some(b"input".to_vec()));
        assert_eq!(spec.env, vec![("KEY".to_string(), "VAL".to_string())]);
        assert_eq!(spec.timeout_ms, 5_000);
    }

    #[test]
    fn command_spec_multiple_env() {
        let spec = CommandSpec::new("x", "/").env("A", "1").env("B", "2");
        assert_eq!(spec.env.len(), 2);
    }

    #[test]
    fn command_output_success() {
        let out = CommandOutput::success(b"hello".to_vec());
        assert_eq!(out.exit_code, 0);
        assert_eq!(out.stdout, b"hello");
        assert!(out.stderr.is_empty());
    }

    #[test]
    fn command_output_failure() {
        let out = CommandOutput::failure(1, b"error".to_vec());
        assert_eq!(out.exit_code, 1);
        assert!(out.stdout.is_empty());
        assert_eq!(out.stderr, b"error");
    }

    #[tokio::test]
    async fn real_runner_echo() {
        let runner = RealCommandRunner;
        let spec = CommandSpec::new("echo", "/tmp").args(["hello", "world"]);
        let result = runner.run_command(&spec).await.unwrap();
        assert_eq!(result.exit_code, 0);
        let stdout = String::from_utf8_lossy(&result.stdout);
        assert!(stdout.contains("hello world"));
    }

    #[tokio::test]
    async fn real_runner_nonexistent_binary() {
        let runner = RealCommandRunner;
        let spec = CommandSpec::new("nonexistent_binary_xyz_12345", "/tmp");
        let result = runner.run_command(&spec).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn real_runner_failing_command() {
        let runner = RealCommandRunner;
        let spec = CommandSpec::new("false", "/tmp");
        let result = runner.run_command(&spec).await.unwrap();
        assert_ne!(result.exit_code, 0);
    }

    #[tokio::test]
    async fn real_runner_with_stdin() {
        let runner = RealCommandRunner;
        let spec = CommandSpec::new("cat", "/tmp").stdin(b"piped input".to_vec());
        let result = runner.run_command(&spec).await.unwrap();
        assert_eq!(result.exit_code, 0);
        assert_eq!(String::from_utf8_lossy(&result.stdout), "piped input");
    }

    #[tokio::test]
    async fn real_runner_with_env() {
        let runner = RealCommandRunner;
        let spec = CommandSpec::new("sh", "/tmp")
            .args(["-c", "echo $APEX_TEST_VAR"])
            .env("APEX_TEST_VAR", "test_value");
        let result = runner.run_command(&spec).await.unwrap();
        assert_eq!(result.exit_code, 0);
        let stdout = String::from_utf8_lossy(&result.stdout);
        assert!(stdout.contains("test_value"));
    }

    #[tokio::test]
    async fn real_runner_timeout() {
        let runner = RealCommandRunner;
        let spec = CommandSpec::new("sleep", "/tmp").args(["10"]).timeout(100); // 100ms timeout, sleep 10s
        let result = runner.run_command(&spec).await;
        assert!(result.is_err());
        match result.unwrap_err() {
            ApexError::Timeout(ms) => assert_eq!(ms, 100),
            other => panic!("expected Timeout, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn mock_runner() {
        let mut mock = MockCommandRunner::new();
        mock.expect_run_command().returning(|_spec| {
            Ok(CommandOutput {
                exit_code: 0,
                stdout: b"mocked output".to_vec(),
                stderr: Vec::new(),
            })
        });

        let spec = CommandSpec::new("anything", "/tmp");
        let result = mock.run_command(&spec).await.unwrap();
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, b"mocked output");
    }

    #[tokio::test]
    async fn mock_runner_error() {
        let mut mock = MockCommandRunner::new();
        mock.expect_run_command().returning(|_| {
            Err(ApexError::Subprocess {
                exit_code: 127,
                stderr: "command not found".into(),
            })
        });

        let spec = CommandSpec::new("missing", "/tmp");
        let result = mock.run_command(&spec).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn timeout_kills_child_process() {
        let runner = RealCommandRunner;
        let spec = CommandSpec::new("sleep", "/tmp").args(["10"]).timeout(100);
        let start = std::time::Instant::now();
        let result = runner.run_command(&spec).await;
        let elapsed = start.elapsed();
        assert!(result.is_err());
        match result.unwrap_err() {
            ApexError::Timeout(ms) => assert_eq!(ms, 100),
            other => panic!("expected Timeout, got {other:?}"),
        }
        // Should return promptly (well under the 10s sleep).
        assert!(
            elapsed.as_secs() < 2,
            "timeout took too long: {elapsed:?}, child may not have been killed"
        );
    }

    #[test]
    fn command_spec_clone() {
        let spec = CommandSpec::new("echo", "/tmp")
            .args(["hello"])
            .env("K", "V");
        let cloned = spec.clone();
        assert_eq!(cloned.program, "echo");
        assert_eq!(cloned.args, vec!["hello"]);
    }

    #[test]
    fn command_output_clone() {
        let out = CommandOutput::success(b"data".to_vec());
        let cloned = out.clone();
        assert_eq!(cloned.exit_code, 0);
        assert_eq!(cloned.stdout, b"data");
    }

    // -----------------------------------------------------------------------
    // Adaptive timeout tests
    // -----------------------------------------------------------------------

    #[test]
    fn adaptive_timeout_small_python_project() {
        use crate::types::Language;
        // 50 files * 100ms/file * 0.5 (Python) = 2,500ms + 60s base * 0.5 = 32,500ms
        // Clamped to min 60,000
        let t = adaptive_timeout(50, Language::Python, OpKind::TestRun);
        assert_eq!(t, 60_000, "small Python project should hit minimum");
    }

    #[test]
    fn adaptive_timeout_large_c_project() {
        use crate::types::Language;
        // 5000 files * 50ms/file * 2.0 (C) = 500,000ms + 60s base * 2.0 = 620,000ms
        // Clamped to max 600,000
        let t = adaptive_timeout(5000, Language::C, OpKind::Compile);
        assert_eq!(t, 600_000, "large C project should hit maximum");
    }

    #[test]
    fn adaptive_timeout_medium_go_project() {
        use crate::types::Language;
        // 1000 files * 100ms/file * 1.0 (Go) = 100,000ms + 60s base = 160,000ms
        let t = adaptive_timeout(1000, Language::Go, OpKind::TestRun);
        assert_eq!(t, 160_000);
    }

    #[test]
    fn adaptive_timeout_dep_install_ignores_file_count() {
        use crate::types::Language;
        // DepInstall has per_file=0, so file_count doesn't matter
        let t1 = adaptive_timeout(10, Language::Rust, OpKind::DepInstall);
        let t2 = adaptive_timeout(10000, Language::Rust, OpKind::DepInstall);
        // Both should be base * multiplier = 60,000 * 1.5 = 90,000
        assert_eq!(t1, t2);
        assert_eq!(t1, 90_000);
    }

    #[test]
    fn adaptive_timeout_post_process_quick() {
        use crate::types::Language;
        // PostProcess: base 30s, per_file 10ms, min 30s, max 120s
        // 100 files * 10ms * 1.0 = 1,000ms + 30s = 31,000ms
        let t = adaptive_timeout(100, Language::Go, OpKind::PostProcess);
        assert_eq!(t, 31_000);
    }

    #[test]
    fn adaptive_timeout_zero_files_returns_min() {
        use crate::types::Language;
        let t = adaptive_timeout(0, Language::Python, OpKind::TestRun);
        // base * 0.5 = 30,000ms, clamped to min 60,000
        assert_eq!(t, 60_000);
    }

    #[test]
    fn adaptive_timeout_jvm_multiplier() {
        use crate::types::Language;
        // 2000 files * 50ms * 1.5 = 150,000 + 60,000 * 1.5 = 240,000
        let t = adaptive_timeout(2000, Language::Java, OpKind::Compile);
        assert_eq!(t, 240_000);
        // Kotlin same multiplier
        let t2 = adaptive_timeout(2000, Language::Kotlin, OpKind::Compile);
        assert_eq!(t, t2);
    }
}
