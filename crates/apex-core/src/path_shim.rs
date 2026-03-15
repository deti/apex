//! PATH shim test utility for intercepting external tool invocations.
//!
//! Creates temporary directories with executable shell scripts that stand in for
//! real tools (`pip`, `clang`, `cargo`, etc.). When the shim directory is prepended
//! to `$PATH`, any `Command::new("pip")` resolves to our shim instead of the real
//! binary. The shim:
//!
//! 1. Logs its invocation (program name + arguments) to a JSON-lines file
//! 2. Prints configurable stdout
//! 3. Exits with a configurable exit code
//!
//! This enables testing `install_deps()`, `ensure_compiled()`, and similar functions
//! without real tools installed — and without modifying any function signatures.
//!
//! # Example
//!
//! ```rust,no_run
//! use apex_core::path_shim::PathShimDir;
//! use apex_core::command::{CommandSpec, CommandOutput, RealCommandRunner, CommandRunner};
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! let shims = PathShimDir::new()?;
//! shims.add("pip", 0, "Successfully installed requests\n")?;
//! shims.add("clang", 1, "")?; // simulate clang not working
//!
//! // Build a CommandSpec that uses the shimmed PATH
//! let spec = CommandSpec::new("pip", "/tmp")
//!     .args(["install", "-r", "requirements.txt"])
//!     .env("PATH", &shims.path_prepended());
//!
//! let runner = RealCommandRunner;
//! let output = runner.run_command(&spec).await?;
//! assert_eq!(output.exit_code, 0);
//! assert!(String::from_utf8_lossy(&output.stdout).contains("Successfully installed"));
//!
//! // Check what was invoked
//! let calls = shims.invocations("pip")?;
//! assert_eq!(calls.len(), 1);
//! assert_eq!(calls[0].args, vec!["install", "-r", "requirements.txt"]);
//! # Ok(())
//! # }
//! ```

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// A temporary directory containing executable shim scripts.
///
/// Dropped automatically when the value goes out of scope (via `tempfile::TempDir`).
pub struct PathShimDir {
    dir: tempfile::TempDir,
    log_dir: PathBuf,
}

/// A recorded invocation of a shimmed tool.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShimInvocation {
    /// The program name (e.g. "pip").
    pub program: String,
    /// The arguments passed to the program.
    pub args: Vec<String>,
    /// The working directory at invocation time.
    pub cwd: String,
}

impl PathShimDir {
    /// Create a new shim directory.
    pub fn new() -> io::Result<Self> {
        let dir = tempfile::tempdir()?;
        let log_dir = dir.path().join(".logs");
        fs::create_dir_all(&log_dir)?;
        Ok(PathShimDir { dir, log_dir })
    }

    /// Add a shim for the given program name.
    ///
    /// - `exit_code`: the exit code the shim will return
    /// - `stdout`: text the shim will print to stdout
    pub fn add(&self, program: &str, exit_code: i32, stdout: &str) -> io::Result<()> {
        self.add_full(program, exit_code, stdout, "")
    }

    /// Add a shim with both stdout and stderr output.
    pub fn add_full(
        &self,
        program: &str,
        exit_code: i32,
        stdout: &str,
        stderr: &str,
    ) -> io::Result<()> {
        // Validate program name to prevent shell injection.
        if !program
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.')
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("program name contains invalid characters: {program:?}"),
            ));
        }

        let shim_path = self.dir.path().join(program);
        let log_file = self.log_dir.join(format!("{program}.jsonl"));

        // Shell script that logs invocation then produces output
        let script = format!(
            r#"#!/bin/sh
# APEX test shim for "{program}"
LOG_FILE='{log_file}'
ARGS=$(printf '%s\n' "$@" | sed 's/"/\\"/g' | sed 's/.*/"&"/' | paste -sd, -)
printf '{{"program":"{program}","args":[%s],"cwd":"%s"}}\n' "$ARGS" "$(pwd)" >> "$LOG_FILE"
printf '%s' '{stdout_escaped}'
printf '%s' '{stderr_escaped}' >&2
exit {exit_code}
"#,
            program = program,
            log_file = log_file.display(),
            exit_code = exit_code,
            stdout_escaped = shell_escape(stdout),
            stderr_escaped = shell_escape(stderr),
        );

        fs::write(&shim_path, script)?;

        // Make executable (Unix)
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&shim_path, fs::Permissions::from_mode(0o755))?;
        }

        Ok(())
    }

    /// The path to the shim directory.
    pub fn path(&self) -> &Path {
        self.dir.path()
    }

    /// Return a PATH value with the shim directory prepended to the current PATH.
    pub fn path_prepended(&self) -> String {
        let current = std::env::var("PATH").unwrap_or_default();
        format!("{}:{current}", self.dir.path().display())
    }

    /// Read all invocations of a given program.
    pub fn invocations(&self, program: &str) -> io::Result<Vec<ShimInvocation>> {
        let log_file = self.log_dir.join(format!("{program}.jsonl"));
        if !log_file.exists() {
            return Ok(Vec::new());
        }

        let content = fs::read_to_string(&log_file)?;
        let mut result = Vec::new();

        for line in content.lines() {
            if line.trim().is_empty() {
                continue;
            }
            // Parse minimal JSON manually to avoid serde dependency in this module.
            // Format: {"program":"pip","args":["install","-r","requirements.txt"],"cwd":"/tmp"}
            if let Some(inv) = parse_invocation_line(line) {
                result.push(inv);
            }
        }

        Ok(result)
    }

    /// Total number of invocations across all shimmed programs.
    pub fn total_invocations(&self) -> io::Result<usize> {
        let mut total = 0;
        for entry in fs::read_dir(&self.log_dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.extension().is_some_and(|e| e == "jsonl") {
                let content = fs::read_to_string(&path)?;
                total += content.lines().filter(|l| !l.trim().is_empty()).count();
            }
        }
        Ok(total)
    }
}

/// Escape a string for embedding in a shell script's printf argument.
fn shell_escape(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('\'', "'\\''")
        .replace('%', "%%")
}

/// Parse a single JSONL invocation line.
/// Uses serde_json since apex-core already depends on it.
fn parse_invocation_line(line: &str) -> Option<ShimInvocation> {
    let v: serde_json::Value = serde_json::from_str(line).ok()?;
    let program = v.get("program")?.as_str()?.to_string();
    let args = v
        .get("args")?
        .as_array()?
        .iter()
        .filter_map(|a| a.as_str().map(String::from))
        .collect();
    let cwd = v
        .get("cwd")
        .and_then(|c| c.as_str())
        .unwrap_or("")
        .to_string();
    Some(ShimInvocation { program, args, cwd })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_shim_dir() {
        let shims = PathShimDir::new().unwrap();
        assert!(shims.path().exists());
        assert!(shims.path().join(".logs").exists());
    }

    #[test]
    fn add_shim_creates_executable() {
        let shims = PathShimDir::new().unwrap();
        shims.add("pip", 0, "ok\n").unwrap();
        let shim_path = shims.path().join("pip");
        assert!(shim_path.exists());

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let perms = fs::metadata(&shim_path).unwrap().permissions();
            assert!(perms.mode() & 0o111 != 0, "shim should be executable");
        }
    }

    #[test]
    fn shim_script_content() {
        let shims = PathShimDir::new().unwrap();
        shims.add("clang", 1, "error output").unwrap();
        let content = fs::read_to_string(shims.path().join("clang")).unwrap();
        assert!(content.starts_with("#!/bin/sh"));
        assert!(content.contains("exit 1"));
        assert!(content.contains("error output"));
    }

    #[test]
    fn path_prepended_contains_shim_dir() {
        let shims = PathShimDir::new().unwrap();
        let path = shims.path_prepended();
        assert!(path.starts_with(&shims.path().display().to_string()));
        assert!(path.contains(':'));
    }

    #[test]
    fn no_invocations_for_uncalled_program() {
        let shims = PathShimDir::new().unwrap();
        shims.add("pip", 0, "").unwrap();
        let calls = shims.invocations("pip").unwrap();
        assert!(calls.is_empty());
    }

    #[test]
    fn no_invocations_for_unknown_program() {
        let shims = PathShimDir::new().unwrap();
        let calls = shims.invocations("nonexistent").unwrap();
        assert!(calls.is_empty());
    }

    #[test]
    fn parse_invocation_json() {
        let line = r#"{"program":"pip","args":["install","-r","requirements.txt"],"cwd":"/tmp"}"#;
        let inv = parse_invocation_line(line).unwrap();
        assert_eq!(inv.program, "pip");
        assert_eq!(inv.args, vec!["install", "-r", "requirements.txt"]);
        assert_eq!(inv.cwd, "/tmp");
    }

    #[test]
    fn parse_invocation_empty_args() {
        let line = r#"{"program":"clang","args":[],"cwd":"/"}"#;
        let inv = parse_invocation_line(line).unwrap();
        assert_eq!(inv.program, "clang");
        assert!(inv.args.is_empty());
    }

    #[test]
    fn parse_invocation_malformed() {
        assert!(parse_invocation_line("not json").is_none());
        assert!(parse_invocation_line("{}").is_none());
    }

    #[tokio::test]
    async fn shim_executes_and_logs() {
        use crate::command::{CommandRunner, CommandSpec, RealCommandRunner};

        let shims = PathShimDir::new().unwrap();
        shims
            .add("pip", 0, "Successfully installed requests\n")
            .unwrap();

        let spec = CommandSpec::new("pip", shims.path())
            .args(["install", "-r", "requirements.txt"])
            .env("PATH", &shims.path_prepended());

        let runner = RealCommandRunner;
        let output = runner.run_command(&spec).await.unwrap();
        assert_eq!(output.exit_code, 0);
        assert!(
            String::from_utf8_lossy(&output.stdout).contains("Successfully installed"),
            "stdout: {}",
            String::from_utf8_lossy(&output.stdout)
        );

        let calls = shims.invocations("pip").unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].program, "pip");
        assert_eq!(calls[0].args, vec!["install", "-r", "requirements.txt"]);
    }

    #[tokio::test]
    async fn shim_returns_failure_exit_code() {
        use crate::command::{CommandRunner, CommandSpec, RealCommandRunner};

        let shims = PathShimDir::new().unwrap();
        shims.add("clang", 1, "").unwrap();

        let spec = CommandSpec::new("clang", shims.path())
            .args(["-shared", "-o", "libtest.so"])
            .env("PATH", &shims.path_prepended());

        let runner = RealCommandRunner;
        let output = runner.run_command(&spec).await.unwrap();
        assert_eq!(output.exit_code, 1);

        let calls = shims.invocations("clang").unwrap();
        assert_eq!(calls.len(), 1);
    }

    #[tokio::test]
    async fn shim_stderr_output() {
        use crate::command::{CommandRunner, CommandSpec, RealCommandRunner};

        let shims = PathShimDir::new().unwrap();
        shims
            .add_full("cargo", 1, "", "error: could not compile")
            .unwrap();

        let spec = CommandSpec::new("cargo", shims.path())
            .args(["test"])
            .env("PATH", &shims.path_prepended());

        let runner = RealCommandRunner;
        let output = runner.run_command(&spec).await.unwrap();
        assert_eq!(output.exit_code, 1);
        assert!(
            String::from_utf8_lossy(&output.stderr).contains("could not compile"),
            "stderr: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    #[tokio::test]
    async fn multiple_shims_independent() {
        use crate::command::{CommandRunner, CommandSpec, RealCommandRunner};

        let shims = PathShimDir::new().unwrap();
        shims.add("pip", 0, "pip ok\n").unwrap();
        shims.add("npm", 0, "npm ok\n").unwrap();

        let runner = RealCommandRunner;

        let spec1 = CommandSpec::new("pip", shims.path())
            .args(["install"])
            .env("PATH", &shims.path_prepended());
        runner.run_command(&spec1).await.unwrap();

        let spec2 = CommandSpec::new("npm", shims.path())
            .args(["install"])
            .env("PATH", &shims.path_prepended());
        runner.run_command(&spec2).await.unwrap();

        assert_eq!(shims.invocations("pip").unwrap().len(), 1);
        assert_eq!(shims.invocations("npm").unwrap().len(), 1);
        assert_eq!(shims.total_invocations().unwrap(), 2);
    }

    #[tokio::test]
    async fn shim_called_multiple_times() {
        use crate::command::{CommandRunner, CommandSpec, RealCommandRunner};

        let shims = PathShimDir::new().unwrap();
        shims.add("python3", 0, "3.11.0\n").unwrap();

        let runner = RealCommandRunner;

        let arg_sets: Vec<Vec<&str>> =
            vec![vec!["--version"], vec!["-c", "pass"], vec!["script.py"]];
        for args in &arg_sets {
            let spec = CommandSpec::new("python3", shims.path())
                .args(args.iter().copied())
                .env("PATH", &shims.path_prepended());
            runner.run_command(&spec).await.unwrap();
        }

        let calls = shims.invocations("python3").unwrap();
        assert_eq!(calls.len(), 3);
        assert_eq!(calls[0].args, vec!["--version"]);
        assert_eq!(calls[1].args, vec!["-c", "pass"]);
        assert_eq!(calls[2].args, vec!["script.py"]);
    }

    #[test]
    fn shell_escape_special_chars() {
        assert_eq!(shell_escape("hello"), "hello");
        assert_eq!(shell_escape("100%"), "100%%");
        assert_eq!(shell_escape("it's"), "it'\\''s");
        assert_eq!(shell_escape("a\\b"), "a\\\\b");
    }
}
