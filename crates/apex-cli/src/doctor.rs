//! `apex doctor` — prerequisite checker.
//!
//! Verifies that all external tools APEX needs are available on the current
//! system, grouped by language.  Exits with code 1 if any required tool is
//! missing; optional tools that are absent are shown as warnings only.

use apex_core::command::{CommandOutput, CommandRunner, CommandSpec, RealCommandRunner};

// ---------------------------------------------------------------------------
// Check primitives
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
enum Status {
    Ok(String),   // found, version string
    Warn(String), // optional, not found
    Fail(String), // required, not found
}

struct Check {
    name: &'static str,
    description: &'static str,
    status: Status,
}

async fn version_of(runner: &dyn CommandRunner, bin: &str, args: &[&str]) -> Option<String> {
    let spec = CommandSpec::new(bin, ".")
        .args(args.iter().copied())
        .timeout(10_000);

    let output: CommandOutput = runner.run_command(&spec).await.ok()?;

    if output.exit_code != 0 {
        return None;
    }

    // Some tools (java, javac) print version to stderr even on success.
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    let raw = if stdout.trim().is_empty() {
        stderr
    } else {
        stdout
    };

    raw.lines()
        .find(|l| !l.trim().is_empty())
        .map(|l| l.trim().to_string())
}

async fn check_required(
    runner: &dyn CommandRunner,
    name: &'static str,
    desc: &'static str,
    bin: &str,
    ver_args: &[&str],
) -> Check {
    match version_of(runner, bin, ver_args).await {
        Some(v) => Check {
            name,
            description: desc,
            status: Status::Ok(v),
        },
        None => Check {
            name,
            description: desc,
            status: Status::Fail(format!("{name} not found in PATH")),
        },
    }
}

async fn check_optional(
    runner: &dyn CommandRunner,
    name: &'static str,
    desc: &'static str,
    bin: &str,
    ver_args: &[&str],
) -> Check {
    match version_of(runner, bin, ver_args).await {
        Some(v) => Check {
            name,
            description: desc,
            status: Status::Ok(v),
        },
        None => Check {
            name,
            description: desc,
            status: Status::Warn(format!("{name} not found (optional)")),
        },
    }
}

async fn check_env_optional(name: &'static str, desc: &'static str, var: &str) -> Check {
    match std::env::var(var) {
        Ok(v) if !v.is_empty() => Check {
            name,
            description: desc,
            status: Status::Ok(format!("set ({} chars)", v.len())),
        },
        _ => Check {
            name,
            description: desc,
            status: Status::Warn(format!("{var} not set (needed for --strategy agent)")),
        },
    }
}

// ---------------------------------------------------------------------------
// Non-command checks (TCP, filesystem, etc.)
// ---------------------------------------------------------------------------

fn check_tcp_basics() -> Check {
    match std::net::TcpListener::bind("127.0.0.1:0") {
        Ok(listener) => {
            let port = listener.local_addr().map(|a| a.port()).unwrap_or(0);
            drop(listener);
            Check {
                name: "tcp",
                description: "TCP/IP networking",
                status: Status::Ok(format!("localhost TCP available (tested port {port})")),
            }
        }
        Err(e) => Check {
            name: "tcp",
            description: "TCP/IP networking",
            status: Status::Warn(format!(
                "cannot bind TCP on localhost: {e}. RPC features may not work."
            )),
        },
    }
}

// ---------------------------------------------------------------------------
// Check groups
// ---------------------------------------------------------------------------

async fn checks_core(runner: &dyn CommandRunner) -> Vec<Check> {
    vec![
        check_required(runner, "rust", "Rust compiler", "rustc", &["--version"]).await,
        check_required(runner, "cargo", "Cargo build tool", "cargo", &["--version"]).await,
        check_optional(
            runner,
            "cargo-llvm-cov",
            "Rust branch coverage (--lang rust)",
            "cargo",
            &["llvm-cov", "--version"],
        )
        .await,
        check_env_optional(
            "ANTHROPIC_API_KEY",
            "Claude API key (--strategy agent)",
            "ANTHROPIC_API_KEY",
        )
        .await,
        check_tcp_basics(),
    ]
}

async fn checks_python(runner: &dyn CommandRunner) -> Vec<Check> {
    vec![
        check_required(
            runner,
            "python3",
            "Python 3 interpreter",
            "python3",
            &["--version"],
        )
        .await,
        check_required(
            runner,
            "pip3",
            "pip package manager",
            "pip3",
            &["--version"],
        )
        .await,
        check_optional(
            runner,
            "pytest",
            "Python test runner",
            "python3",
            &["-m", "pytest", "--version"],
        )
        .await,
        check_optional(
            runner,
            "coverage.py",
            "Python coverage tool",
            "python3",
            &["-m", "coverage", "--version"],
        )
        .await,
    ]
}

async fn checks_javascript(runner: &dyn CommandRunner) -> Vec<Check> {
    vec![
        check_required(runner, "node", "Node.js runtime", "node", &["--version"]).await,
        check_required(runner, "npm", "npm package manager", "npm", &["--version"]).await,
        check_optional(
            runner,
            "npx",
            "npx tool runner (for nyc)",
            "npx",
            &["--version"],
        )
        .await,
    ]
}

async fn checks_java(runner: &dyn CommandRunner) -> Vec<Check> {
    vec![
        check_required(runner, "java", "JVM runtime", "java", &["-version"]).await,
        check_required(runner, "javac", "Java compiler", "javac", &["-version"]).await,
        check_optional(runner, "mvn", "Apache Maven", "mvn", &["--version"]).await,
        check_optional(
            runner,
            "gradle",
            "Gradle build tool",
            "gradle",
            &["--version"],
        )
        .await,
    ]
}

async fn checks_c(runner: &dyn CommandRunner) -> Vec<Check> {
    vec![
        check_required(runner, "clang", "LLVM C compiler", "clang", &["--version"]).await,
        check_optional(
            runner,
            "llvm-cov",
            "LLVM coverage tool",
            "llvm-cov",
            &["--version"],
        )
        .await,
        check_optional(
            runner,
            "llvm-profdata",
            "LLVM profile data tool",
            "llvm-profdata",
            &["--version"],
        )
        .await,
        check_optional(runner, "make", "Make build tool", "make", &["--version"]).await,
        check_optional(
            runner,
            "cmake",
            "CMake build system",
            "cmake",
            &["--version"],
        )
        .await,
    ]
}

async fn checks_wasm(runner: &dyn CommandRunner) -> Vec<Check> {
    vec![
        check_optional(
            runner,
            "wasmtime",
            "WASM runtime",
            "wasmtime",
            &["--version"],
        )
        .await,
        check_optional(
            runner,
            "wasm-opt",
            "Binaryen optimizer/instrumentor",
            "wasm-opt",
            &["--version"],
        )
        .await,
    ]
}

async fn checks_firecracker(runner: &dyn CommandRunner) -> Vec<Check> {
    vec![
        check_optional(
            runner,
            "firecracker",
            "Firecracker microVM",
            "firecracker",
            &["--version"],
        )
        .await,
        check_optional(
            runner,
            "jailer",
            "Firecracker jailer",
            "jailer",
            &["--version"],
        )
        .await,
        {
            // /dev/kvm
            let ok = std::path::Path::new("/dev/kvm").exists();
            Check {
                name: "/dev/kvm",
                description: "KVM device (required for Firecracker)",
                status: if ok {
                    Status::Ok("/dev/kvm present".into())
                } else {
                    Status::Warn("/dev/kvm not found (Firecracker sandbox unavailable)".into())
                },
            }
        },
    ]
}

// ---------------------------------------------------------------------------
// Printer
// ---------------------------------------------------------------------------

fn print_group(title: &str, checks: &[Check]) -> usize {
    let green = "\x1b[32m";
    let yellow = "\x1b[33m";
    let red = "\x1b[31m";
    let bold = "\x1b[1m";
    let reset = "\x1b[0m";

    println!("\n{bold}{title}{reset}");
    let bar: String = "─".repeat(title.len());
    println!("{bar}");

    let mut fail_count = 0;

    for c in checks {
        let (icon, color, detail) = match &c.status {
            Status::Ok(v) => ("✓", green, v.as_str()),
            Status::Warn(v) => ("⚠", yellow, v.as_str()),
            Status::Fail(v) => {
                fail_count += 1;
                ("✗", red, v.as_str())
            }
        };
        println!("  {color}{icon}{reset} {:<24} {}", c.name, detail);
        if !c.description.is_empty() {
            println!("    {}", c.description);
        }
    }

    fail_count
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

pub async fn run_doctor() -> color_eyre::Result<()> {
    let runner = RealCommandRunner;
    run_doctor_with_runner(&runner).await
}

async fn run_doctor_with_runner(runner: &dyn CommandRunner) -> color_eyre::Result<()> {
    println!("APEX prerequisite check\n");

    let mut total_failures = 0;

    // Run all check groups sequentially (could be parallel but order matters for display).
    let core_checks = checks_core(runner).await;
    let py_checks = checks_python(runner).await;
    let js_checks = checks_javascript(runner).await;
    let java_checks = checks_java(runner).await;
    let c_checks = checks_c(runner).await;
    let wasm_checks = checks_wasm(runner).await;
    let fc_checks = checks_firecracker(runner).await;

    total_failures += print_group("Core", &core_checks);
    total_failures += print_group("Python  (--lang python)", &py_checks);
    total_failures += print_group("JavaScript  (--lang js)", &js_checks);
    total_failures += print_group("Java  (--lang java)", &java_checks);
    total_failures += print_group("C  (--lang c)", &c_checks);
    total_failures += print_group("WASM  (--lang wasm)", &wasm_checks);
    total_failures += print_group("Firecracker  (--strategy fuzz with VMs)", &fc_checks);

    println!();

    if total_failures == 0 {
        println!("\x1b[32m\x1b[1mAll required tools present.\x1b[0m");
    } else {
        return Err(color_eyre::eyre::eyre!(
            "{total_failures} required tool(s) missing. Install them and re-run `apex doctor`."
        ));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use apex_core::command::CommandSpec;
    use apex_core::error::ApexError;
    use async_trait::async_trait;
    use std::sync::Arc;
    use tokio::sync::Mutex;

    // -----------------------------------------------------------------
    // Test helper: configurable mock CommandRunner
    // -----------------------------------------------------------------

    /// A mock command runner that returns results based on a closure.
    struct MockRunner {
        handler: Arc<dyn Fn(&CommandSpec) -> apex_core::Result<CommandOutput> + Send + Sync>,
    }

    impl MockRunner {
        /// Creates a runner where every command succeeds with the given stdout.
        fn all_succeed(stdout: &'static str) -> Self {
            MockRunner {
                handler: Arc::new(move |_| Ok(CommandOutput::success(stdout.as_bytes().to_vec()))),
            }
        }

        /// Creates a runner where every command fails (spawn error).
        fn all_fail() -> Self {
            MockRunner {
                handler: Arc::new(|_| {
                    Err(ApexError::Subprocess {
                        exit_code: 127,
                        stderr: "command not found".into(),
                    })
                }),
            }
        }

        /// Creates a runner where every command exits with a non-zero code.
        fn all_exit_nonzero() -> Self {
            MockRunner {
                handler: Arc::new(|_| Ok(CommandOutput::failure(1, b"error".to_vec()))),
            }
        }

        /// Creates a runner where every command succeeds with empty stdout and stderr.
        fn all_empty_output() -> Self {
            MockRunner {
                handler: Arc::new(|_| {
                    Ok(CommandOutput {
                        exit_code: 0,
                        stdout: Vec::new(),
                        stderr: Vec::new(),
                    })
                }),
            }
        }

        /// Creates a runner where every command succeeds with output on stderr only.
        fn all_stderr(stderr: &'static str) -> Self {
            MockRunner {
                handler: Arc::new(move |_| {
                    Ok(CommandOutput {
                        exit_code: 0,
                        stdout: Vec::new(),
                        stderr: stderr.as_bytes().to_vec(),
                    })
                }),
            }
        }

        /// Creates a runner that delegates to a closure for per-command logic.
        fn with_handler<F>(f: F) -> Self
        where
            F: Fn(&CommandSpec) -> apex_core::Result<CommandOutput> + Send + Sync + 'static,
        {
            MockRunner {
                handler: Arc::new(f),
            }
        }
    }

    #[async_trait]
    impl CommandRunner for MockRunner {
        async fn run_command(&self, spec: &CommandSpec) -> apex_core::Result<CommandOutput> {
            (self.handler)(spec)
        }
    }

    // A mock that records which commands were called.
    struct RecordingRunner {
        handler: Arc<dyn Fn(&CommandSpec) -> apex_core::Result<CommandOutput> + Send + Sync>,
        calls: Arc<Mutex<Vec<(String, Vec<String>)>>>,
    }

    impl RecordingRunner {
        fn new_all_succeed(stdout: &'static str) -> Self {
            RecordingRunner {
                handler: Arc::new(move |_| Ok(CommandOutput::success(stdout.as_bytes().to_vec()))),
                calls: Arc::new(Mutex::new(Vec::new())),
            }
        }

        async fn calls(&self) -> Vec<(String, Vec<String>)> {
            self.calls.lock().await.clone()
        }
    }

    #[async_trait]
    impl CommandRunner for RecordingRunner {
        async fn run_command(&self, spec: &CommandSpec) -> apex_core::Result<CommandOutput> {
            self.calls
                .lock()
                .await
                .push((spec.program.clone(), spec.args.clone()));
            (self.handler)(spec)
        }
    }

    // ==================================================================
    // Existing tests (unchanged)
    // ==================================================================

    #[test]
    fn status_ok_contains_version() {
        let s = Status::Ok("1.0.0".into());
        assert_eq!(s, Status::Ok("1.0.0".into()));
    }

    #[test]
    fn status_warn_contains_message() {
        let s = Status::Warn("not found".into());
        assert_eq!(s, Status::Warn("not found".into()));
    }

    #[test]
    fn status_fail_contains_message() {
        let s = Status::Fail("missing".into());
        assert_eq!(s, Status::Fail("missing".into()));
    }

    #[test]
    fn status_ne_different_variants() {
        assert_ne!(Status::Ok("v".into()), Status::Fail("v".into()));
        assert_ne!(Status::Ok("v".into()), Status::Warn("v".into()));
        assert_ne!(Status::Warn("v".into()), Status::Fail("v".into()));
    }

    #[test]
    fn status_clone() {
        let s = Status::Ok("1.0".into());
        let c = s.clone();
        assert_eq!(s, c);
    }

    #[test]
    fn status_debug() {
        let s = Status::Ok("1.0".into());
        let d = format!("{s:?}");
        assert!(d.contains("Ok"));
    }

    #[test]
    fn print_group_returns_zero_for_all_ok() {
        let checks = vec![
            Check {
                name: "rust",
                description: "Rust compiler",
                status: Status::Ok("1.75.0".into()),
            },
            Check {
                name: "cargo",
                description: "build tool",
                status: Status::Ok("1.75.0".into()),
            },
        ];
        let fails = print_group("Test", &checks);
        assert_eq!(fails, 0);
    }

    #[test]
    fn print_group_counts_failures() {
        let checks = vec![
            Check {
                name: "a",
                description: "",
                status: Status::Ok("ok".into()),
            },
            Check {
                name: "b",
                description: "",
                status: Status::Fail("missing".into()),
            },
            Check {
                name: "c",
                description: "",
                status: Status::Warn("optional".into()),
            },
            Check {
                name: "d",
                description: "",
                status: Status::Fail("also missing".into()),
            },
        ];
        let fails = print_group("Mixed", &checks);
        assert_eq!(fails, 2);
    }

    #[test]
    fn print_group_zero_for_empty() {
        let fails = print_group("Empty", &[]);
        assert_eq!(fails, 0);
    }

    #[test]
    fn print_group_warns_dont_count_as_failures() {
        let checks = vec![Check {
            name: "w",
            description: "",
            status: Status::Warn("not found".into()),
        }];
        let fails = print_group("Warnings", &checks);
        assert_eq!(fails, 0);
    }

    #[tokio::test]
    async fn check_env_optional_with_set_var() {
        // HOME is always set
        let c = check_env_optional("home", "Home dir", "HOME").await;
        assert_eq!(c.name, "home");
        assert!(matches!(c.status, Status::Ok(_)));
    }

    #[tokio::test]
    async fn check_env_optional_with_unset_var() {
        let c = check_env_optional("test", "Nonexistent", "APEX_NONEXISTENT_TEST_VAR_12345").await;
        assert!(matches!(c.status, Status::Warn(_)));
    }

    #[tokio::test]
    async fn version_of_existing_binary() {
        let runner = RealCommandRunner;
        let v = version_of(&runner, "echo", &["hello"]).await;
        assert!(v.is_some());
        assert!(v.unwrap().contains("hello"));
    }

    #[tokio::test]
    async fn version_of_nonexistent_binary() {
        let runner = RealCommandRunner;
        let v = version_of(&runner, "nonexistent_binary_12345", &["--version"]).await;
        assert!(v.is_none());
    }

    #[tokio::test]
    async fn check_required_existing() {
        let runner = RealCommandRunner;
        let c = check_required(&runner, "echo", "Echo", "echo", &["test"]).await;
        assert!(matches!(c.status, Status::Ok(_)));
    }

    #[tokio::test]
    async fn check_required_missing() {
        let runner = RealCommandRunner;
        let c = check_required(&runner, "fake", "Fake", "nonexistent_12345", &[]).await;
        assert!(matches!(c.status, Status::Fail(_)));
    }

    #[tokio::test]
    async fn check_optional_existing() {
        let runner = RealCommandRunner;
        let c = check_optional(&runner, "echo", "Echo", "echo", &["test"]).await;
        assert!(matches!(c.status, Status::Ok(_)));
    }

    #[tokio::test]
    async fn check_optional_missing() {
        let runner = RealCommandRunner;
        let c = check_optional(&runner, "fake", "Fake", "nonexistent_12345", &[]).await;
        assert!(matches!(c.status, Status::Warn(_)));
    }

    // ------------------------------------------------------------------
    // version_of edge cases
    // ------------------------------------------------------------------

    #[tokio::test]
    async fn version_of_with_stderr_output() {
        let runner = RealCommandRunner;
        // Use a command that outputs to stderr: `sh -c 'echo VERSION >&2'`
        let v = version_of(&runner, "sh", &["-c", "echo VERSION >&2"]).await;
        // stdout is empty → falls back to stderr
        assert!(v.is_some());
        assert!(v.unwrap().contains("VERSION"));
    }

    #[tokio::test]
    async fn version_of_failing_command_returns_none() {
        let runner = RealCommandRunner;
        // `false` exits with 1 — version_of should return None
        let v = version_of(&runner, "false", &[]).await;
        assert!(v.is_none());
    }

    #[tokio::test]
    async fn version_of_with_multiple_lines() {
        let runner = RealCommandRunner;
        let v = version_of(&runner, "sh", &["-c", "echo 'line1\nline2\nline3'"]).await;
        assert!(v.is_some());
        // Should return first non-empty line
        assert_eq!(v.unwrap(), "line1");
    }

    #[tokio::test]
    async fn version_of_empty_stdout_and_stderr() {
        let runner = RealCommandRunner;
        // `true` produces no output
        let v = version_of(&runner, "true", &[]).await;
        // Both stdout and stderr are empty — should return None (no non-empty line)
        assert!(v.is_none());
    }

    // ------------------------------------------------------------------
    // Check group functions (using RealCommandRunner)
    // ------------------------------------------------------------------

    #[tokio::test]
    async fn checks_core_runs_without_panic() {
        let runner = RealCommandRunner;
        let checks = checks_core(&runner).await;
        // Should contain at least rustc and cargo checks
        assert!(checks.len() >= 4);
        // rustc and cargo should be present on a dev machine
        assert!(checks.iter().any(|c| c.name == "rust"));
        assert!(checks.iter().any(|c| c.name == "cargo"));
        assert!(checks.iter().any(|c| c.name == "ANTHROPIC_API_KEY"));
    }

    #[tokio::test]
    async fn checks_python_runs_without_panic() {
        let runner = RealCommandRunner;
        let checks = checks_python(&runner).await;
        assert!(checks.len() >= 4);
        assert!(checks.iter().any(|c| c.name == "python3"));
        assert!(checks.iter().any(|c| c.name == "pip3"));
    }

    #[tokio::test]
    async fn checks_javascript_runs_without_panic() {
        let runner = RealCommandRunner;
        let checks = checks_javascript(&runner).await;
        assert!(checks.len() >= 3);
        assert!(checks.iter().any(|c| c.name == "node"));
        assert!(checks.iter().any(|c| c.name == "npm"));
    }

    #[tokio::test]
    async fn checks_java_runs_without_panic() {
        let runner = RealCommandRunner;
        let checks = checks_java(&runner).await;
        assert!(checks.len() >= 4);
        assert!(checks.iter().any(|c| c.name == "java"));
        assert!(checks.iter().any(|c| c.name == "javac"));
    }

    #[tokio::test]
    async fn checks_c_runs_without_panic() {
        let runner = RealCommandRunner;
        let checks = checks_c(&runner).await;
        assert!(checks.len() >= 5);
        assert!(checks.iter().any(|c| c.name == "clang"));
    }

    #[tokio::test]
    async fn checks_wasm_runs_without_panic() {
        let runner = RealCommandRunner;
        let checks = checks_wasm(&runner).await;
        assert!(checks.len() >= 2);
        assert!(checks.iter().any(|c| c.name == "wasmtime"));
        assert!(checks.iter().any(|c| c.name == "wasm-opt"));
    }

    #[tokio::test]
    async fn checks_firecracker_runs_without_panic() {
        let runner = RealCommandRunner;
        let checks = checks_firecracker(&runner).await;
        assert!(checks.len() >= 3);
        assert!(checks.iter().any(|c| c.name == "firecracker"));
        assert!(checks.iter().any(|c| c.name == "jailer"));
        assert!(checks.iter().any(|c| c.name == "/dev/kvm"));
    }

    #[tokio::test]
    async fn checks_firecracker_kvm_status() {
        let runner = RealCommandRunner;
        let checks = checks_firecracker(&runner).await;
        let kvm = checks.iter().find(|c| c.name == "/dev/kvm").unwrap();
        // On macOS, /dev/kvm doesn't exist → Warn
        // On Linux with KVM, it exists → Ok
        match &kvm.status {
            Status::Ok(msg) => assert!(msg.contains("present")),
            Status::Warn(msg) => assert!(msg.contains("not found")),
            Status::Fail(_) => panic!("KVM check should not be Fail (it's optional)"),
        }
    }

    // ------------------------------------------------------------------
    // Check result messages
    // ------------------------------------------------------------------

    #[tokio::test]
    async fn check_required_fail_message_contains_name() {
        let runner = RealCommandRunner;
        let c = check_required(&runner, "mytool", "My tool", "nonexistent_99", &[]).await;
        match c.status {
            Status::Fail(msg) => assert!(msg.contains("mytool")),
            other => panic!("expected Fail, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn check_optional_warn_message_contains_name() {
        let runner = RealCommandRunner;
        let c = check_optional(&runner, "mytool", "My tool", "nonexistent_99", &[]).await;
        match c.status {
            Status::Warn(msg) => assert!(msg.contains("mytool")),
            other => panic!("expected Warn, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn check_env_optional_warn_message_contains_var() {
        let c = check_env_optional("test", "Test", "APEX_NOT_SET_XYZ").await;
        match c.status {
            Status::Warn(msg) => assert!(msg.contains("APEX_NOT_SET_XYZ")),
            other => panic!("expected Warn, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn check_env_optional_ok_shows_char_count() {
        let c = check_env_optional("home", "Home", "HOME").await;
        match c.status {
            Status::Ok(msg) => assert!(msg.contains("chars")),
            other => panic!("expected Ok, got {other:?}"),
        }
    }

    // ------------------------------------------------------------------
    // print_group edge cases
    // ------------------------------------------------------------------

    #[test]
    fn print_group_with_description() {
        let checks = vec![Check {
            name: "tool",
            description: "A useful tool for things",
            status: Status::Ok("v1.0".into()),
        }];
        let fails = print_group("Tools", &checks);
        assert_eq!(fails, 0);
    }

    #[test]
    fn print_group_all_failures() {
        let checks = vec![
            Check {
                name: "a",
                description: "",
                status: Status::Fail("missing".into()),
            },
            Check {
                name: "b",
                description: "",
                status: Status::Fail("also missing".into()),
            },
            Check {
                name: "c",
                description: "",
                status: Status::Fail("gone".into()),
            },
        ];
        let fails = print_group("All Fail", &checks);
        assert_eq!(fails, 3);
    }

    #[test]
    fn print_group_all_warns() {
        let checks = vec![
            Check {
                name: "x",
                description: "",
                status: Status::Warn("optional".into()),
            },
            Check {
                name: "y",
                description: "",
                status: Status::Warn("optional".into()),
            },
        ];
        let fails = print_group("All Warn", &checks);
        assert_eq!(fails, 0);
    }

    #[test]
    fn check_struct_stores_fields() {
        let c = Check {
            name: "test",
            description: "test description",
            status: Status::Ok("1.0".into()),
        };
        assert_eq!(c.name, "test");
        assert_eq!(c.description, "test description");
        assert_eq!(c.status, Status::Ok("1.0".into()));
    }

    // ==================================================================
    // Mock-based tests for version_of
    // ==================================================================

    #[tokio::test]
    async fn mock_version_of_success_returns_stdout() {
        let runner = MockRunner::all_succeed("rustc 1.75.0");
        let v = version_of(&runner, "rustc", &["--version"]).await;
        assert_eq!(v, Some("rustc 1.75.0".into()));
    }

    #[tokio::test]
    async fn mock_version_of_command_not_found_returns_none() {
        let runner = MockRunner::all_fail();
        let v = version_of(&runner, "rustc", &["--version"]).await;
        assert!(v.is_none());
    }

    #[tokio::test]
    async fn mock_version_of_nonzero_exit_returns_none() {
        let runner = MockRunner::all_exit_nonzero();
        let v = version_of(&runner, "rustc", &["--version"]).await;
        assert!(v.is_none());
    }

    #[tokio::test]
    async fn mock_version_of_empty_output_returns_none() {
        let runner = MockRunner::all_empty_output();
        let v = version_of(&runner, "rustc", &["--version"]).await;
        assert!(v.is_none());
    }

    #[tokio::test]
    async fn mock_version_of_stderr_fallback() {
        let runner = MockRunner::all_stderr("java version \"21.0.1\"");
        let v = version_of(&runner, "java", &["-version"]).await;
        assert_eq!(v, Some("java version \"21.0.1\"".into()));
    }

    #[tokio::test]
    async fn mock_version_of_multiline_returns_first() {
        let runner =
            MockRunner::all_succeed("clang version 17.0.0\nTarget: x86_64\nThread model: posix");
        let v = version_of(&runner, "clang", &["--version"]).await;
        assert_eq!(v, Some("clang version 17.0.0".into()));
    }

    #[tokio::test]
    async fn mock_version_of_leading_blank_lines_skipped() {
        let runner = MockRunner::all_succeed("\n\n  \nactual version 1.0");
        let v = version_of(&runner, "tool", &["--version"]).await;
        assert_eq!(v, Some("actual version 1.0".into()));
    }

    // ==================================================================
    // Mock-based tests for check_required / check_optional
    // ==================================================================

    #[tokio::test]
    async fn mock_check_required_found() {
        let runner = MockRunner::all_succeed("rustc 1.75.0");
        let c = check_required(&runner, "rust", "Rust compiler", "rustc", &["--version"]).await;
        assert_eq!(c.name, "rust");
        assert_eq!(c.description, "Rust compiler");
        match c.status {
            Status::Ok(v) => assert_eq!(v, "rustc 1.75.0"),
            other => panic!("expected Ok, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn mock_check_required_not_found() {
        let runner = MockRunner::all_fail();
        let c = check_required(&runner, "rust", "Rust compiler", "rustc", &["--version"]).await;
        match c.status {
            Status::Fail(msg) => assert!(msg.contains("rust")),
            other => panic!("expected Fail, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn mock_check_required_empty_output() {
        let runner = MockRunner::all_empty_output();
        let c = check_required(&runner, "rust", "Rust compiler", "rustc", &["--version"]).await;
        assert!(matches!(c.status, Status::Fail(_)));
    }

    #[tokio::test]
    async fn mock_check_optional_found() {
        let runner = MockRunner::all_succeed("v18.17.1");
        let c = check_optional(&runner, "node", "Node.js runtime", "node", &["--version"]).await;
        match c.status {
            Status::Ok(v) => assert_eq!(v, "v18.17.1"),
            other => panic!("expected Ok, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn mock_check_optional_not_found() {
        let runner = MockRunner::all_fail();
        let c = check_optional(&runner, "node", "Node.js runtime", "node", &["--version"]).await;
        match c.status {
            Status::Warn(msg) => assert!(msg.contains("node")),
            other => panic!("expected Warn, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn mock_check_optional_empty_output() {
        let runner = MockRunner::all_empty_output();
        let c = check_optional(&runner, "node", "Node.js runtime", "node", &["--version"]).await;
        assert!(matches!(c.status, Status::Warn(_)));
    }

    // ==================================================================
    // Mock-based tests for each tool check — found / not-found / empty
    // ==================================================================

    // --- rustc ---

    #[tokio::test]
    async fn mock_rustc_found() {
        let runner = MockRunner::with_handler(|spec| {
            if spec.program == "rustc" {
                Ok(CommandOutput::success(
                    b"rustc 1.75.0 (82e1608df 2023-12-21)".to_vec(),
                ))
            } else {
                Ok(CommandOutput::success(b"other tool".to_vec()))
            }
        });
        let checks = checks_core(&runner).await;
        let rust = checks.iter().find(|c| c.name == "rust").unwrap();
        assert!(matches!(&rust.status, Status::Ok(v) if v.contains("rustc 1.75.0")));
    }

    #[tokio::test]
    async fn mock_rustc_not_found() {
        let runner = MockRunner::all_fail();
        let checks = checks_core(&runner).await;
        let rust = checks.iter().find(|c| c.name == "rust").unwrap();
        assert!(matches!(&rust.status, Status::Fail(_)));
    }

    #[tokio::test]
    async fn mock_rustc_empty_output() {
        let runner = MockRunner::all_empty_output();
        let checks = checks_core(&runner).await;
        let rust = checks.iter().find(|c| c.name == "rust").unwrap();
        assert!(matches!(&rust.status, Status::Fail(_)));
    }

    // --- python3 ---

    #[tokio::test]
    async fn mock_python3_found() {
        let runner = MockRunner::all_succeed("Python 3.12.1");
        let checks = checks_python(&runner).await;
        let py = checks.iter().find(|c| c.name == "python3").unwrap();
        assert!(matches!(&py.status, Status::Ok(v) if v.contains("Python 3.12")));
    }

    #[tokio::test]
    async fn mock_python3_not_found() {
        let runner = MockRunner::all_fail();
        let checks = checks_python(&runner).await;
        let py = checks.iter().find(|c| c.name == "python3").unwrap();
        assert!(matches!(&py.status, Status::Fail(_)));
    }

    #[tokio::test]
    async fn mock_python3_empty_output() {
        let runner = MockRunner::all_empty_output();
        let checks = checks_python(&runner).await;
        let py = checks.iter().find(|c| c.name == "python3").unwrap();
        assert!(matches!(&py.status, Status::Fail(_)));
    }

    // --- node ---

    #[tokio::test]
    async fn mock_node_found() {
        let runner = MockRunner::all_succeed("v20.10.0");
        let checks = checks_javascript(&runner).await;
        let node = checks.iter().find(|c| c.name == "node").unwrap();
        assert!(matches!(&node.status, Status::Ok(v) if v == "v20.10.0"));
    }

    #[tokio::test]
    async fn mock_node_not_found() {
        let runner = MockRunner::all_fail();
        let checks = checks_javascript(&runner).await;
        let node = checks.iter().find(|c| c.name == "node").unwrap();
        assert!(matches!(&node.status, Status::Fail(_)));
    }

    #[tokio::test]
    async fn mock_node_empty_output() {
        let runner = MockRunner::all_empty_output();
        let checks = checks_javascript(&runner).await;
        let node = checks.iter().find(|c| c.name == "node").unwrap();
        assert!(matches!(&node.status, Status::Fail(_)));
    }

    // --- java ---

    #[tokio::test]
    async fn mock_java_found_via_stderr() {
        // java -version outputs to stderr
        let runner = MockRunner::all_stderr("openjdk version \"21.0.1\" 2023-10-17");
        let checks = checks_java(&runner).await;
        let java = checks.iter().find(|c| c.name == "java").unwrap();
        assert!(matches!(&java.status, Status::Ok(v) if v.contains("openjdk")));
    }

    #[tokio::test]
    async fn mock_java_not_found() {
        let runner = MockRunner::all_fail();
        let checks = checks_java(&runner).await;
        let java = checks.iter().find(|c| c.name == "java").unwrap();
        assert!(matches!(&java.status, Status::Fail(_)));
    }

    #[tokio::test]
    async fn mock_java_empty_output() {
        let runner = MockRunner::all_empty_output();
        let checks = checks_java(&runner).await;
        let java = checks.iter().find(|c| c.name == "java").unwrap();
        assert!(matches!(&java.status, Status::Fail(_)));
    }

    // --- clang ---

    #[tokio::test]
    async fn mock_clang_found() {
        let runner = MockRunner::all_succeed("Apple clang version 15.0.0");
        let checks = checks_c(&runner).await;
        let clang = checks.iter().find(|c| c.name == "clang").unwrap();
        assert!(matches!(&clang.status, Status::Ok(v) if v.contains("clang")));
    }

    #[tokio::test]
    async fn mock_clang_not_found() {
        let runner = MockRunner::all_fail();
        let checks = checks_c(&runner).await;
        let clang = checks.iter().find(|c| c.name == "clang").unwrap();
        assert!(matches!(&clang.status, Status::Fail(_)));
    }

    #[tokio::test]
    async fn mock_clang_empty_output() {
        let runner = MockRunner::all_empty_output();
        let checks = checks_c(&runner).await;
        let clang = checks.iter().find(|c| c.name == "clang").unwrap();
        assert!(matches!(&clang.status, Status::Fail(_)));
    }

    // --- wasm-opt ---

    #[tokio::test]
    async fn mock_wasm_opt_found() {
        let runner = MockRunner::all_succeed("wasm-opt version 116");
        let checks = checks_wasm(&runner).await;
        let wo = checks.iter().find(|c| c.name == "wasm-opt").unwrap();
        assert!(matches!(&wo.status, Status::Ok(v) if v.contains("wasm-opt")));
    }

    #[tokio::test]
    async fn mock_wasm_opt_not_found() {
        let runner = MockRunner::all_fail();
        let checks = checks_wasm(&runner).await;
        let wo = checks.iter().find(|c| c.name == "wasm-opt").unwrap();
        assert!(matches!(&wo.status, Status::Warn(_)));
    }

    #[tokio::test]
    async fn mock_wasm_opt_empty_output() {
        let runner = MockRunner::all_empty_output();
        let checks = checks_wasm(&runner).await;
        let wo = checks.iter().find(|c| c.name == "wasm-opt").unwrap();
        assert!(matches!(&wo.status, Status::Warn(_)));
    }

    // --- wasmtime ---

    #[tokio::test]
    async fn mock_wasmtime_found() {
        let runner = MockRunner::all_succeed("wasmtime-cli 16.0.0");
        let checks = checks_wasm(&runner).await;
        let wt = checks.iter().find(|c| c.name == "wasmtime").unwrap();
        assert!(matches!(&wt.status, Status::Ok(v) if v.contains("wasmtime")));
    }

    #[tokio::test]
    async fn mock_wasmtime_not_found() {
        let runner = MockRunner::all_fail();
        let checks = checks_wasm(&runner).await;
        let wt = checks.iter().find(|c| c.name == "wasmtime").unwrap();
        assert!(matches!(&wt.status, Status::Warn(_)));
    }

    #[tokio::test]
    async fn mock_wasmtime_empty_output() {
        let runner = MockRunner::all_empty_output();
        let checks = checks_wasm(&runner).await;
        let wt = checks.iter().find(|c| c.name == "wasmtime").unwrap();
        assert!(matches!(&wt.status, Status::Warn(_)));
    }

    // --- firecracker ---

    #[tokio::test]
    async fn mock_firecracker_found() {
        let runner = MockRunner::all_succeed("Firecracker v1.6.0");
        let checks = checks_firecracker(&runner).await;
        let fc = checks.iter().find(|c| c.name == "firecracker").unwrap();
        assert!(matches!(&fc.status, Status::Ok(v) if v.contains("Firecracker")));
    }

    #[tokio::test]
    async fn mock_firecracker_not_found() {
        let runner = MockRunner::all_fail();
        let checks = checks_firecracker(&runner).await;
        let fc = checks.iter().find(|c| c.name == "firecracker").unwrap();
        assert!(matches!(&fc.status, Status::Warn(_)));
    }

    #[tokio::test]
    async fn mock_firecracker_empty_output() {
        let runner = MockRunner::all_empty_output();
        let checks = checks_firecracker(&runner).await;
        let fc = checks.iter().find(|c| c.name == "firecracker").unwrap();
        assert!(matches!(&fc.status, Status::Warn(_)));
    }

    // ==================================================================
    // Mock-based tests for check groups — all found vs all missing
    // ==================================================================

    #[tokio::test]
    async fn mock_checks_core_all_found() {
        let runner = MockRunner::all_succeed("version 1.0.0");
        let checks = checks_core(&runner).await;
        for c in &checks {
            if c.name != "ANTHROPIC_API_KEY" {
                assert!(
                    matches!(&c.status, Status::Ok(_)),
                    "expected Ok for {}",
                    c.name
                );
            }
        }
    }

    #[tokio::test]
    async fn mock_checks_core_all_missing() {
        let runner = MockRunner::all_fail();
        let checks = checks_core(&runner).await;
        let rust = checks.iter().find(|c| c.name == "rust").unwrap();
        assert!(matches!(&rust.status, Status::Fail(_)));
        let cargo = checks.iter().find(|c| c.name == "cargo").unwrap();
        assert!(matches!(&cargo.status, Status::Fail(_)));
        // cargo-llvm-cov is optional
        let llvm = checks.iter().find(|c| c.name == "cargo-llvm-cov").unwrap();
        assert!(matches!(&llvm.status, Status::Warn(_)));
    }

    #[tokio::test]
    async fn mock_checks_python_all_found() {
        let runner = MockRunner::all_succeed("Python 3.12.0");
        let checks = checks_python(&runner).await;
        for c in &checks {
            assert!(
                matches!(&c.status, Status::Ok(_)),
                "expected Ok for {}",
                c.name
            );
        }
    }

    #[tokio::test]
    async fn mock_checks_python_all_missing() {
        let runner = MockRunner::all_fail();
        let checks = checks_python(&runner).await;
        let py = checks.iter().find(|c| c.name == "python3").unwrap();
        assert!(matches!(&py.status, Status::Fail(_)));
        let pip = checks.iter().find(|c| c.name == "pip3").unwrap();
        assert!(matches!(&pip.status, Status::Fail(_)));
        // optional ones should be Warn
        let pytest = checks.iter().find(|c| c.name == "pytest").unwrap();
        assert!(matches!(&pytest.status, Status::Warn(_)));
    }

    #[tokio::test]
    async fn mock_checks_javascript_all_found() {
        let runner = MockRunner::all_succeed("v20.0.0");
        let checks = checks_javascript(&runner).await;
        for c in &checks {
            assert!(
                matches!(&c.status, Status::Ok(_)),
                "expected Ok for {}",
                c.name
            );
        }
    }

    #[tokio::test]
    async fn mock_checks_javascript_all_missing() {
        let runner = MockRunner::all_fail();
        let checks = checks_javascript(&runner).await;
        let node = checks.iter().find(|c| c.name == "node").unwrap();
        assert!(matches!(&node.status, Status::Fail(_)));
        let npm = checks.iter().find(|c| c.name == "npm").unwrap();
        assert!(matches!(&npm.status, Status::Fail(_)));
        let npx = checks.iter().find(|c| c.name == "npx").unwrap();
        assert!(matches!(&npx.status, Status::Warn(_)));
    }

    #[tokio::test]
    async fn mock_checks_java_all_found() {
        let runner = MockRunner::all_succeed("openjdk 21.0.1");
        let checks = checks_java(&runner).await;
        for c in &checks {
            assert!(
                matches!(&c.status, Status::Ok(_)),
                "expected Ok for {}",
                c.name
            );
        }
    }

    #[tokio::test]
    async fn mock_checks_java_all_missing() {
        let runner = MockRunner::all_fail();
        let checks = checks_java(&runner).await;
        let java = checks.iter().find(|c| c.name == "java").unwrap();
        assert!(matches!(&java.status, Status::Fail(_)));
        let javac = checks.iter().find(|c| c.name == "javac").unwrap();
        assert!(matches!(&javac.status, Status::Fail(_)));
        let mvn = checks.iter().find(|c| c.name == "mvn").unwrap();
        assert!(matches!(&mvn.status, Status::Warn(_)));
        let gradle = checks.iter().find(|c| c.name == "gradle").unwrap();
        assert!(matches!(&gradle.status, Status::Warn(_)));
    }

    #[tokio::test]
    async fn mock_checks_c_all_found() {
        let runner = MockRunner::all_succeed("clang version 17.0");
        let checks = checks_c(&runner).await;
        for c in &checks {
            assert!(
                matches!(&c.status, Status::Ok(_)),
                "expected Ok for {}",
                c.name
            );
        }
    }

    #[tokio::test]
    async fn mock_checks_c_all_missing() {
        let runner = MockRunner::all_fail();
        let checks = checks_c(&runner).await;
        let clang = checks.iter().find(|c| c.name == "clang").unwrap();
        assert!(matches!(&clang.status, Status::Fail(_)));
        // optional ones should be Warn
        let llvm_cov = checks.iter().find(|c| c.name == "llvm-cov").unwrap();
        assert!(matches!(&llvm_cov.status, Status::Warn(_)));
    }

    #[tokio::test]
    async fn mock_checks_wasm_all_found() {
        let runner = MockRunner::all_succeed("wasmtime 16.0.0");
        let checks = checks_wasm(&runner).await;
        for c in &checks {
            assert!(
                matches!(&c.status, Status::Ok(_)),
                "expected Ok for {}",
                c.name
            );
        }
    }

    #[tokio::test]
    async fn mock_checks_wasm_all_missing() {
        let runner = MockRunner::all_fail();
        let checks = checks_wasm(&runner).await;
        for c in &checks {
            assert!(
                matches!(&c.status, Status::Warn(_)),
                "expected Warn for {}",
                c.name
            );
        }
    }

    #[tokio::test]
    async fn mock_checks_firecracker_all_found() {
        let runner = MockRunner::all_succeed("Firecracker v1.6.0");
        let checks = checks_firecracker(&runner).await;
        let fc = checks.iter().find(|c| c.name == "firecracker").unwrap();
        assert!(matches!(&fc.status, Status::Ok(_)));
        let jailer = checks.iter().find(|c| c.name == "jailer").unwrap();
        assert!(matches!(&jailer.status, Status::Ok(_)));
    }

    #[tokio::test]
    async fn mock_checks_firecracker_all_missing() {
        let runner = MockRunner::all_fail();
        let checks = checks_firecracker(&runner).await;
        let fc = checks.iter().find(|c| c.name == "firecracker").unwrap();
        assert!(matches!(&fc.status, Status::Warn(_)));
        let jailer = checks.iter().find(|c| c.name == "jailer").unwrap();
        assert!(matches!(&jailer.status, Status::Warn(_)));
    }

    // ==================================================================
    // Per-command routing: verify correct binary + args are called
    // ==================================================================

    #[tokio::test]
    async fn mock_version_of_passes_correct_spec() {
        let runner = RecordingRunner::new_all_succeed("v1.0");
        let _ = version_of(&runner, "rustc", &["--version"]).await;
        let calls = runner.calls().await;
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].0, "rustc");
        assert_eq!(calls[0].1, vec!["--version"]);
    }

    #[tokio::test]
    async fn mock_java_check_passes_dash_version() {
        let runner = RecordingRunner::new_all_succeed("openjdk 21");
        let _ = checks_java(&runner).await;
        let calls = runner.calls().await;
        // java and javac both use -version (not --version)
        let java_call = calls.iter().find(|c| c.0 == "java").unwrap();
        assert_eq!(java_call.1, vec!["-version"]);
        let javac_call = calls.iter().find(|c| c.0 == "javac").unwrap();
        assert_eq!(javac_call.1, vec!["-version"]);
    }

    // ==================================================================
    // run_doctor_with_runner integration
    // ==================================================================

    #[tokio::test]
    async fn mock_run_doctor_all_found() {
        let runner = MockRunner::all_succeed("v1.0.0");
        // Should not panic or exit; all tools "found"
        let result = run_doctor_with_runner(&runner).await;
        assert!(result.is_ok());
    }

    // ==================================================================
    // Additional coverage tests
    // ==================================================================

    // --- version_of edge cases with mocks ---

    #[tokio::test]
    async fn mock_version_of_whitespace_only_output_returns_none() {
        let runner = MockRunner::all_succeed("   \n  \n   ");
        let v = version_of(&runner, "tool", &["--version"]).await;
        assert!(v.is_none());
    }

    #[tokio::test]
    async fn mock_version_of_trims_leading_trailing_whitespace() {
        let runner = MockRunner::all_succeed("  rustc 1.75.0  ");
        let v = version_of(&runner, "rustc", &["--version"]).await;
        assert_eq!(v, Some("rustc 1.75.0".into()));
    }

    #[tokio::test]
    async fn mock_version_of_both_stdout_and_stderr() {
        // When both stdout and stderr have content, stdout is preferred
        let runner = MockRunner::with_handler(|_| {
            Ok(CommandOutput {
                exit_code: 0,
                stdout: b"stdout version".to_vec(),
                stderr: b"stderr version".to_vec(),
            })
        });
        let v = version_of(&runner, "tool", &["--version"]).await;
        assert_eq!(v, Some("stdout version".into()));
    }

    // --- check_env_optional with empty string ---

    #[tokio::test]
    async fn check_env_optional_with_empty_string() {
        // Set an env var to empty string — should be treated as unset
        std::env::set_var("APEX_TEST_EMPTY_VAR", "");
        let c = check_env_optional("test", "Test var", "APEX_TEST_EMPTY_VAR").await;
        assert!(matches!(c.status, Status::Warn(_)));
        std::env::remove_var("APEX_TEST_EMPTY_VAR");
    }

    #[tokio::test]
    async fn check_env_optional_fields_populated() {
        let c = check_env_optional("myvar", "My description", "APEX_NONEXISTENT_VAR_XYZ").await;
        assert_eq!(c.name, "myvar");
        assert_eq!(c.description, "My description");
    }

    // --- Individual tool checks within groups ---

    // pip3
    #[tokio::test]
    async fn mock_pip3_found() {
        let runner = MockRunner::all_succeed("pip 23.3.1 from /usr/lib/python3/dist-packages/pip");
        let checks = checks_python(&runner).await;
        let pip = checks.iter().find(|c| c.name == "pip3").unwrap();
        assert!(matches!(&pip.status, Status::Ok(v) if v.contains("pip")));
    }

    #[tokio::test]
    async fn mock_pip3_not_found() {
        let runner = MockRunner::all_fail();
        let checks = checks_python(&runner).await;
        let pip = checks.iter().find(|c| c.name == "pip3").unwrap();
        assert!(matches!(&pip.status, Status::Fail(_)));
    }

    // pytest
    #[tokio::test]
    async fn mock_pytest_found() {
        let runner = MockRunner::all_succeed("pytest 7.4.3");
        let checks = checks_python(&runner).await;
        let pytest = checks.iter().find(|c| c.name == "pytest").unwrap();
        assert!(matches!(&pytest.status, Status::Ok(v) if v.contains("pytest")));
    }

    #[tokio::test]
    async fn mock_pytest_not_found() {
        let runner = MockRunner::all_fail();
        let checks = checks_python(&runner).await;
        let pytest = checks.iter().find(|c| c.name == "pytest").unwrap();
        assert!(matches!(&pytest.status, Status::Warn(_)));
    }

    // coverage.py
    #[tokio::test]
    async fn mock_coverage_py_found() {
        let runner = MockRunner::all_succeed("Coverage.py, version 7.3.2");
        let checks = checks_python(&runner).await;
        let cov = checks.iter().find(|c| c.name == "coverage.py").unwrap();
        assert!(matches!(&cov.status, Status::Ok(v) if v.contains("Coverage")));
    }

    #[tokio::test]
    async fn mock_coverage_py_not_found() {
        let runner = MockRunner::all_fail();
        let checks = checks_python(&runner).await;
        let cov = checks.iter().find(|c| c.name == "coverage.py").unwrap();
        assert!(matches!(&cov.status, Status::Warn(_)));
    }

    // npm
    #[tokio::test]
    async fn mock_npm_found() {
        let runner = MockRunner::all_succeed("10.2.3");
        let checks = checks_javascript(&runner).await;
        let npm = checks.iter().find(|c| c.name == "npm").unwrap();
        assert!(matches!(&npm.status, Status::Ok(_)));
    }

    #[tokio::test]
    async fn mock_npm_not_found() {
        let runner = MockRunner::all_fail();
        let checks = checks_javascript(&runner).await;
        let npm = checks.iter().find(|c| c.name == "npm").unwrap();
        assert!(matches!(&npm.status, Status::Fail(_)));
    }

    // npx
    #[tokio::test]
    async fn mock_npx_found() {
        let runner = MockRunner::all_succeed("10.2.3");
        let checks = checks_javascript(&runner).await;
        let npx = checks.iter().find(|c| c.name == "npx").unwrap();
        assert!(matches!(&npx.status, Status::Ok(_)));
    }

    #[tokio::test]
    async fn mock_npx_not_found() {
        let runner = MockRunner::all_fail();
        let checks = checks_javascript(&runner).await;
        let npx = checks.iter().find(|c| c.name == "npx").unwrap();
        assert!(matches!(&npx.status, Status::Warn(_)));
    }

    // javac
    #[tokio::test]
    async fn mock_javac_found() {
        let runner = MockRunner::all_stderr("javac 21.0.1");
        let checks = checks_java(&runner).await;
        let javac = checks.iter().find(|c| c.name == "javac").unwrap();
        assert!(matches!(&javac.status, Status::Ok(v) if v.contains("javac")));
    }

    #[tokio::test]
    async fn mock_javac_not_found() {
        let runner = MockRunner::all_fail();
        let checks = checks_java(&runner).await;
        let javac = checks.iter().find(|c| c.name == "javac").unwrap();
        assert!(matches!(&javac.status, Status::Fail(_)));
    }

    // mvn
    #[tokio::test]
    async fn mock_mvn_found() {
        let runner = MockRunner::all_succeed("Apache Maven 3.9.6");
        let checks = checks_java(&runner).await;
        let mvn = checks.iter().find(|c| c.name == "mvn").unwrap();
        assert!(matches!(&mvn.status, Status::Ok(v) if v.contains("Maven")));
    }

    #[tokio::test]
    async fn mock_mvn_not_found() {
        let runner = MockRunner::all_fail();
        let checks = checks_java(&runner).await;
        let mvn = checks.iter().find(|c| c.name == "mvn").unwrap();
        assert!(matches!(&mvn.status, Status::Warn(_)));
    }

    // gradle
    #[tokio::test]
    async fn mock_gradle_found() {
        let runner = MockRunner::all_succeed("Gradle 8.5");
        let checks = checks_java(&runner).await;
        let gradle = checks.iter().find(|c| c.name == "gradle").unwrap();
        assert!(matches!(&gradle.status, Status::Ok(v) if v.contains("Gradle")));
    }

    #[tokio::test]
    async fn mock_gradle_not_found() {
        let runner = MockRunner::all_fail();
        let checks = checks_java(&runner).await;
        let gradle = checks.iter().find(|c| c.name == "gradle").unwrap();
        assert!(matches!(&gradle.status, Status::Warn(_)));
    }

    // llvm-cov
    #[tokio::test]
    async fn mock_llvm_cov_found() {
        let runner = MockRunner::all_succeed("LLVM version 17.0.0");
        let checks = checks_c(&runner).await;
        let lc = checks.iter().find(|c| c.name == "llvm-cov").unwrap();
        assert!(matches!(&lc.status, Status::Ok(v) if v.contains("LLVM")));
    }

    #[tokio::test]
    async fn mock_llvm_cov_not_found() {
        let runner = MockRunner::all_fail();
        let checks = checks_c(&runner).await;
        let lc = checks.iter().find(|c| c.name == "llvm-cov").unwrap();
        assert!(matches!(&lc.status, Status::Warn(_)));
    }

    // llvm-profdata
    #[tokio::test]
    async fn mock_llvm_profdata_found() {
        let runner = MockRunner::all_succeed("LLVM version 17.0.0");
        let checks = checks_c(&runner).await;
        let lp = checks.iter().find(|c| c.name == "llvm-profdata").unwrap();
        assert!(matches!(&lp.status, Status::Ok(_)));
    }

    #[tokio::test]
    async fn mock_llvm_profdata_not_found() {
        let runner = MockRunner::all_fail();
        let checks = checks_c(&runner).await;
        let lp = checks.iter().find(|c| c.name == "llvm-profdata").unwrap();
        assert!(matches!(&lp.status, Status::Warn(_)));
    }

    // make
    #[tokio::test]
    async fn mock_make_found() {
        let runner = MockRunner::all_succeed("GNU Make 4.3");
        let checks = checks_c(&runner).await;
        let make = checks.iter().find(|c| c.name == "make").unwrap();
        assert!(matches!(&make.status, Status::Ok(v) if v.contains("Make")));
    }

    #[tokio::test]
    async fn mock_make_not_found() {
        let runner = MockRunner::all_fail();
        let checks = checks_c(&runner).await;
        let make = checks.iter().find(|c| c.name == "make").unwrap();
        assert!(matches!(&make.status, Status::Warn(_)));
    }

    // cmake
    #[tokio::test]
    async fn mock_cmake_found() {
        let runner = MockRunner::all_succeed("cmake version 3.28.1");
        let checks = checks_c(&runner).await;
        let cmake = checks.iter().find(|c| c.name == "cmake").unwrap();
        assert!(matches!(&cmake.status, Status::Ok(v) if v.contains("cmake")));
    }

    #[tokio::test]
    async fn mock_cmake_not_found() {
        let runner = MockRunner::all_fail();
        let checks = checks_c(&runner).await;
        let cmake = checks.iter().find(|c| c.name == "cmake").unwrap();
        assert!(matches!(&cmake.status, Status::Warn(_)));
    }

    // jailer
    #[tokio::test]
    async fn mock_jailer_found() {
        let runner = MockRunner::all_succeed("Jailer v1.6.0");
        let checks = checks_firecracker(&runner).await;
        let jailer = checks.iter().find(|c| c.name == "jailer").unwrap();
        assert!(matches!(&jailer.status, Status::Ok(v) if v.contains("Jailer")));
    }

    #[tokio::test]
    async fn mock_jailer_not_found() {
        let runner = MockRunner::all_fail();
        let checks = checks_firecracker(&runner).await;
        let jailer = checks.iter().find(|c| c.name == "jailer").unwrap();
        assert!(matches!(&jailer.status, Status::Warn(_)));
    }

    // cargo-llvm-cov
    #[tokio::test]
    async fn mock_cargo_llvm_cov_found() {
        let runner = MockRunner::all_succeed("cargo-llvm-cov 0.5.36");
        let checks = checks_core(&runner).await;
        let llvm = checks.iter().find(|c| c.name == "cargo-llvm-cov").unwrap();
        assert!(matches!(&llvm.status, Status::Ok(v) if v.contains("cargo-llvm-cov")));
    }

    #[tokio::test]
    async fn mock_cargo_llvm_cov_not_found() {
        let runner = MockRunner::all_fail();
        let checks = checks_core(&runner).await;
        let llvm = checks.iter().find(|c| c.name == "cargo-llvm-cov").unwrap();
        assert!(matches!(&llvm.status, Status::Warn(_)));
    }

    // --- RecordingRunner tests ---

    #[tokio::test]
    async fn recording_runner_captures_all_calls() {
        let runner = RecordingRunner::new_all_succeed("v1.0");
        let _ = version_of(&runner, "tool1", &["--version"]).await;
        let _ = version_of(&runner, "tool2", &["-v"]).await;
        let calls = runner.calls().await;
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0].0, "tool1");
        assert_eq!(calls[0].1, vec!["--version"]);
        assert_eq!(calls[1].0, "tool2");
        assert_eq!(calls[1].1, vec!["-v"]);
    }

    #[tokio::test]
    async fn recording_runner_core_calls_correct_binaries() {
        let runner = RecordingRunner::new_all_succeed("v1.0");
        let _ = checks_core(&runner).await;
        let calls = runner.calls().await;
        let programs: Vec<&str> = calls.iter().map(|c| c.0.as_str()).collect();
        assert!(programs.contains(&"rustc"));
        assert!(programs.contains(&"cargo"));
    }

    #[tokio::test]
    async fn recording_runner_python_calls_correct_binaries() {
        let runner = RecordingRunner::new_all_succeed("v1.0");
        let _ = checks_python(&runner).await;
        let calls = runner.calls().await;
        let programs: Vec<&str> = calls.iter().map(|c| c.0.as_str()).collect();
        assert!(programs.contains(&"python3"));
        assert!(programs.contains(&"pip3"));
    }

    #[tokio::test]
    async fn recording_runner_js_calls_correct_binaries() {
        let runner = RecordingRunner::new_all_succeed("v1.0");
        let _ = checks_javascript(&runner).await;
        let calls = runner.calls().await;
        let programs: Vec<&str> = calls.iter().map(|c| c.0.as_str()).collect();
        assert!(programs.contains(&"node"));
        assert!(programs.contains(&"npm"));
        assert!(programs.contains(&"npx"));
    }

    #[tokio::test]
    async fn recording_runner_c_calls_correct_binaries() {
        let runner = RecordingRunner::new_all_succeed("v1.0");
        let _ = checks_c(&runner).await;
        let calls = runner.calls().await;
        let programs: Vec<&str> = calls.iter().map(|c| c.0.as_str()).collect();
        assert!(programs.contains(&"clang"));
        assert!(programs.contains(&"llvm-cov"));
        assert!(programs.contains(&"llvm-profdata"));
        assert!(programs.contains(&"make"));
        assert!(programs.contains(&"cmake"));
    }

    #[tokio::test]
    async fn recording_runner_wasm_calls_correct_binaries() {
        let runner = RecordingRunner::new_all_succeed("v1.0");
        let _ = checks_wasm(&runner).await;
        let calls = runner.calls().await;
        let programs: Vec<&str> = calls.iter().map(|c| c.0.as_str()).collect();
        assert!(programs.contains(&"wasmtime"));
        assert!(programs.contains(&"wasm-opt"));
    }

    #[tokio::test]
    async fn recording_runner_firecracker_calls_correct_binaries() {
        let runner = RecordingRunner::new_all_succeed("v1.0");
        let _ = checks_firecracker(&runner).await;
        let calls = runner.calls().await;
        let programs: Vec<&str> = calls.iter().map(|c| c.0.as_str()).collect();
        assert!(programs.contains(&"firecracker"));
        assert!(programs.contains(&"jailer"));
    }

    // --- Selective tool found/not-found with handler ---

    #[tokio::test]
    async fn mock_selective_some_tools_found_some_not() {
        let runner = MockRunner::with_handler(|spec| {
            if spec.program == "python3" {
                Ok(CommandOutput::success(b"Python 3.12.0".to_vec()))
            } else if spec.program == "pip3" {
                Err(ApexError::Subprocess {
                    exit_code: 127,
                    stderr: "not found".into(),
                })
            } else {
                Ok(CommandOutput::success(b"v1.0".to_vec()))
            }
        });
        let checks = checks_python(&runner).await;
        let py = checks.iter().find(|c| c.name == "python3").unwrap();
        assert!(matches!(&py.status, Status::Ok(_)));
        let pip = checks.iter().find(|c| c.name == "pip3").unwrap();
        assert!(matches!(&pip.status, Status::Fail(_)));
    }

    #[tokio::test]
    async fn mock_selective_js_node_found_npm_missing() {
        let runner = MockRunner::with_handler(|spec| {
            if spec.program == "node" {
                Ok(CommandOutput::success(b"v20.0.0".to_vec()))
            } else {
                Err(ApexError::Subprocess {
                    exit_code: 127,
                    stderr: "not found".into(),
                })
            }
        });
        let checks = checks_javascript(&runner).await;
        let node = checks.iter().find(|c| c.name == "node").unwrap();
        assert!(matches!(&node.status, Status::Ok(_)));
        let npm = checks.iter().find(|c| c.name == "npm").unwrap();
        assert!(matches!(&npm.status, Status::Fail(_)));
        let npx = checks.iter().find(|c| c.name == "npx").unwrap();
        assert!(matches!(&npx.status, Status::Warn(_)));
    }

    // --- print_group detailed coverage ---

    #[test]
    fn print_group_with_long_name() {
        let checks = vec![Check {
            name: "a-very-long-tool-name-here",
            description: "Some description",
            status: Status::Ok("v1.0.0".into()),
        }];
        let fails = print_group("Long Names", &checks);
        assert_eq!(fails, 0);
    }

    #[test]
    fn print_group_mixed_statuses_in_order() {
        let checks = vec![
            Check {
                name: "ok-tool",
                description: "ok desc",
                status: Status::Ok("v1.0".into()),
            },
            Check {
                name: "warn-tool",
                description: "warn desc",
                status: Status::Warn("optional".into()),
            },
            Check {
                name: "fail-tool",
                description: "fail desc",
                status: Status::Fail("missing".into()),
            },
            Check {
                name: "ok-tool-2",
                description: "",
                status: Status::Ok("v2.0".into()),
            },
        ];
        let fails = print_group("Mixed Order", &checks);
        assert_eq!(fails, 1);
    }

    #[test]
    fn print_group_single_fail() {
        let checks = vec![Check {
            name: "solo-fail",
            description: "this tool is critical",
            status: Status::Fail("not installed".into()),
        }];
        let fails = print_group("Single Fail", &checks);
        assert_eq!(fails, 1);
    }

    #[test]
    fn print_group_empty_description_still_works() {
        let checks = vec![
            Check {
                name: "tool",
                description: "",
                status: Status::Ok("v1".into()),
            },
            Check {
                name: "tool2",
                description: "",
                status: Status::Warn("warn".into()),
            },
            Check {
                name: "tool3",
                description: "",
                status: Status::Fail("fail".into()),
            },
        ];
        let fails = print_group("No Descriptions", &checks);
        assert_eq!(fails, 1);
    }

    // --- run_doctor_with_runner additional coverage ---

    #[tokio::test]
    async fn mock_run_doctor_checks_all_groups() {
        // Verify all groups are run by checking that the runner receives calls
        // for binaries from every group
        let runner = RecordingRunner::new_all_succeed("v1.0.0");
        let result = run_doctor_with_runner(&runner).await;
        assert!(result.is_ok());
        let calls = runner.calls().await;
        let programs: Vec<&str> = calls.iter().map(|c| c.0.as_str()).collect();
        // Core
        assert!(programs.contains(&"rustc"));
        assert!(programs.contains(&"cargo"));
        // Python
        assert!(programs.contains(&"python3"));
        assert!(programs.contains(&"pip3"));
        // JavaScript
        assert!(programs.contains(&"node"));
        assert!(programs.contains(&"npm"));
        assert!(programs.contains(&"npx"));
        // Java
        assert!(programs.contains(&"java"));
        assert!(programs.contains(&"javac"));
        // C
        assert!(programs.contains(&"clang"));
        // WASM
        assert!(programs.contains(&"wasmtime"));
        assert!(programs.contains(&"wasm-opt"));
        // Firecracker
        assert!(programs.contains(&"firecracker"));
        assert!(programs.contains(&"jailer"));
    }

    // --- Status equality/inequality edge cases ---

    #[test]
    fn status_ok_different_values() {
        assert_ne!(Status::Ok("1.0".into()), Status::Ok("2.0".into()));
    }

    #[test]
    fn status_warn_different_values() {
        assert_ne!(Status::Warn("a".into()), Status::Warn("b".into()));
    }

    #[test]
    fn status_fail_different_values() {
        assert_ne!(Status::Fail("a".into()), Status::Fail("b".into()));
    }

    #[test]
    fn status_clone_all_variants() {
        let ok = Status::Ok("v".into());
        assert_eq!(ok.clone(), ok);
        let warn = Status::Warn("w".into());
        assert_eq!(warn.clone(), warn);
        let fail = Status::Fail("f".into());
        assert_eq!(fail.clone(), fail);
    }

    #[test]
    fn status_debug_all_variants() {
        assert!(format!("{:?}", Status::Ok("v".into())).contains("Ok"));
        assert!(format!("{:?}", Status::Warn("w".into())).contains("Warn"));
        assert!(format!("{:?}", Status::Fail("f".into())).contains("Fail"));
    }

    // --- version_of with nonzero exit via mock ---

    #[tokio::test]
    async fn mock_version_of_nonzero_exit_with_stdout() {
        // Even if there's stdout, non-zero exit -> None
        let runner = MockRunner::with_handler(|_| {
            Ok(CommandOutput {
                exit_code: 1,
                stdout: b"some output".to_vec(),
                stderr: Vec::new(),
            })
        });
        let v = version_of(&runner, "tool", &["--version"]).await;
        assert!(v.is_none());
    }

    // --- check groups with nonzero exit ---

    #[tokio::test]
    async fn mock_checks_core_nonzero_exit() {
        let runner = MockRunner::all_exit_nonzero();
        let checks = checks_core(&runner).await;
        let rust = checks.iter().find(|c| c.name == "rust").unwrap();
        assert!(matches!(&rust.status, Status::Fail(_)));
        let cargo = checks.iter().find(|c| c.name == "cargo").unwrap();
        assert!(matches!(&cargo.status, Status::Fail(_)));
        let llvm = checks.iter().find(|c| c.name == "cargo-llvm-cov").unwrap();
        assert!(matches!(&llvm.status, Status::Warn(_)));
    }

    #[tokio::test]
    async fn mock_checks_python_nonzero_exit() {
        let runner = MockRunner::all_exit_nonzero();
        let checks = checks_python(&runner).await;
        let py = checks.iter().find(|c| c.name == "python3").unwrap();
        assert!(matches!(&py.status, Status::Fail(_)));
        let pip = checks.iter().find(|c| c.name == "pip3").unwrap();
        assert!(matches!(&pip.status, Status::Fail(_)));
        let pytest = checks.iter().find(|c| c.name == "pytest").unwrap();
        assert!(matches!(&pytest.status, Status::Warn(_)));
        let cov = checks.iter().find(|c| c.name == "coverage.py").unwrap();
        assert!(matches!(&cov.status, Status::Warn(_)));
    }

    #[tokio::test]
    async fn mock_checks_javascript_nonzero_exit() {
        let runner = MockRunner::all_exit_nonzero();
        let checks = checks_javascript(&runner).await;
        let node = checks.iter().find(|c| c.name == "node").unwrap();
        assert!(matches!(&node.status, Status::Fail(_)));
        let npm = checks.iter().find(|c| c.name == "npm").unwrap();
        assert!(matches!(&npm.status, Status::Fail(_)));
        let npx = checks.iter().find(|c| c.name == "npx").unwrap();
        assert!(matches!(&npx.status, Status::Warn(_)));
    }

    #[tokio::test]
    async fn mock_checks_java_nonzero_exit() {
        let runner = MockRunner::all_exit_nonzero();
        let checks = checks_java(&runner).await;
        let java = checks.iter().find(|c| c.name == "java").unwrap();
        assert!(matches!(&java.status, Status::Fail(_)));
        let javac = checks.iter().find(|c| c.name == "javac").unwrap();
        assert!(matches!(&javac.status, Status::Fail(_)));
        let mvn = checks.iter().find(|c| c.name == "mvn").unwrap();
        assert!(matches!(&mvn.status, Status::Warn(_)));
        let gradle = checks.iter().find(|c| c.name == "gradle").unwrap();
        assert!(matches!(&gradle.status, Status::Warn(_)));
    }

    #[tokio::test]
    async fn mock_checks_c_nonzero_exit() {
        let runner = MockRunner::all_exit_nonzero();
        let checks = checks_c(&runner).await;
        let clang = checks.iter().find(|c| c.name == "clang").unwrap();
        assert!(matches!(&clang.status, Status::Fail(_)));
        let lc = checks.iter().find(|c| c.name == "llvm-cov").unwrap();
        assert!(matches!(&lc.status, Status::Warn(_)));
        let lp = checks.iter().find(|c| c.name == "llvm-profdata").unwrap();
        assert!(matches!(&lp.status, Status::Warn(_)));
        let make = checks.iter().find(|c| c.name == "make").unwrap();
        assert!(matches!(&make.status, Status::Warn(_)));
        let cmake = checks.iter().find(|c| c.name == "cmake").unwrap();
        assert!(matches!(&cmake.status, Status::Warn(_)));
    }

    #[tokio::test]
    async fn mock_checks_wasm_nonzero_exit() {
        let runner = MockRunner::all_exit_nonzero();
        let checks = checks_wasm(&runner).await;
        for c in &checks {
            assert!(
                matches!(&c.status, Status::Warn(_)),
                "expected Warn for {}",
                c.name
            );
        }
    }

    #[tokio::test]
    async fn mock_checks_firecracker_nonzero_exit() {
        let runner = MockRunner::all_exit_nonzero();
        let checks = checks_firecracker(&runner).await;
        let fc = checks.iter().find(|c| c.name == "firecracker").unwrap();
        assert!(matches!(&fc.status, Status::Warn(_)));
        let jailer = checks.iter().find(|c| c.name == "jailer").unwrap();
        assert!(matches!(&jailer.status, Status::Warn(_)));
    }

    // --- Check groups with empty output ---

    #[tokio::test]
    async fn mock_checks_javascript_empty_output() {
        let runner = MockRunner::all_empty_output();
        let checks = checks_javascript(&runner).await;
        let node = checks.iter().find(|c| c.name == "node").unwrap();
        assert!(matches!(&node.status, Status::Fail(_)));
        let npm = checks.iter().find(|c| c.name == "npm").unwrap();
        assert!(matches!(&npm.status, Status::Fail(_)));
        let npx = checks.iter().find(|c| c.name == "npx").unwrap();
        assert!(matches!(&npx.status, Status::Warn(_)));
    }

    #[tokio::test]
    async fn mock_checks_java_empty_output() {
        let runner = MockRunner::all_empty_output();
        let checks = checks_java(&runner).await;
        let java = checks.iter().find(|c| c.name == "java").unwrap();
        assert!(matches!(&java.status, Status::Fail(_)));
        let javac = checks.iter().find(|c| c.name == "javac").unwrap();
        assert!(matches!(&javac.status, Status::Fail(_)));
        let mvn = checks.iter().find(|c| c.name == "mvn").unwrap();
        assert!(matches!(&mvn.status, Status::Warn(_)));
        let gradle = checks.iter().find(|c| c.name == "gradle").unwrap();
        assert!(matches!(&gradle.status, Status::Warn(_)));
    }

    #[tokio::test]
    async fn mock_checks_c_empty_output() {
        let runner = MockRunner::all_empty_output();
        let checks = checks_c(&runner).await;
        let clang = checks.iter().find(|c| c.name == "clang").unwrap();
        assert!(matches!(&clang.status, Status::Fail(_)));
        let lc = checks.iter().find(|c| c.name == "llvm-cov").unwrap();
        assert!(matches!(&lc.status, Status::Warn(_)));
        let lp = checks.iter().find(|c| c.name == "llvm-profdata").unwrap();
        assert!(matches!(&lp.status, Status::Warn(_)));
    }

    #[tokio::test]
    async fn mock_checks_wasm_empty_output() {
        let runner = MockRunner::all_empty_output();
        let checks = checks_wasm(&runner).await;
        for c in &checks {
            assert!(
                matches!(&c.status, Status::Warn(_)),
                "expected Warn for {}",
                c.name
            );
        }
    }

    #[tokio::test]
    async fn mock_checks_firecracker_empty_output() {
        let runner = MockRunner::all_empty_output();
        let checks = checks_firecracker(&runner).await;
        let fc = checks.iter().find(|c| c.name == "firecracker").unwrap();
        assert!(matches!(&fc.status, Status::Warn(_)));
        let jailer = checks.iter().find(|c| c.name == "jailer").unwrap();
        assert!(matches!(&jailer.status, Status::Warn(_)));
    }

    // --- Check group sizes ---

    #[tokio::test]
    async fn mock_checks_core_returns_five_checks() {
        let runner = MockRunner::all_succeed("v1.0");
        let checks = checks_core(&runner).await;
        assert_eq!(checks.len(), 5);
    }

    #[tokio::test]
    async fn mock_checks_python_returns_four_checks() {
        let runner = MockRunner::all_succeed("v1.0");
        let checks = checks_python(&runner).await;
        assert_eq!(checks.len(), 4);
    }

    #[tokio::test]
    async fn mock_checks_javascript_returns_three_checks() {
        let runner = MockRunner::all_succeed("v1.0");
        let checks = checks_javascript(&runner).await;
        assert_eq!(checks.len(), 3);
    }

    #[tokio::test]
    async fn mock_checks_java_returns_four_checks() {
        let runner = MockRunner::all_succeed("v1.0");
        let checks = checks_java(&runner).await;
        assert_eq!(checks.len(), 4);
    }

    #[tokio::test]
    async fn mock_checks_c_returns_five_checks() {
        let runner = MockRunner::all_succeed("v1.0");
        let checks = checks_c(&runner).await;
        assert_eq!(checks.len(), 5);
    }

    #[tokio::test]
    async fn mock_checks_wasm_returns_two_checks() {
        let runner = MockRunner::all_succeed("v1.0");
        let checks = checks_wasm(&runner).await;
        assert_eq!(checks.len(), 2);
    }

    #[tokio::test]
    async fn mock_checks_firecracker_returns_three_checks() {
        let runner = MockRunner::all_succeed("v1.0");
        let checks = checks_firecracker(&runner).await;
        assert_eq!(checks.len(), 3);
    }

    // --- Check descriptions are populated ---

    #[tokio::test]
    async fn mock_checks_core_descriptions_nonempty() {
        let runner = MockRunner::all_succeed("v1.0");
        let checks = checks_core(&runner).await;
        for c in &checks {
            assert!(
                !c.description.is_empty(),
                "description empty for {}",
                c.name
            );
        }
    }

    #[tokio::test]
    async fn mock_checks_python_descriptions_nonempty() {
        let runner = MockRunner::all_succeed("v1.0");
        let checks = checks_python(&runner).await;
        for c in &checks {
            assert!(
                !c.description.is_empty(),
                "description empty for {}",
                c.name
            );
        }
    }

    #[tokio::test]
    async fn mock_checks_c_descriptions_nonempty() {
        let runner = MockRunner::all_succeed("v1.0");
        let checks = checks_c(&runner).await;
        for c in &checks {
            assert!(
                !c.description.is_empty(),
                "description empty for {}",
                c.name
            );
        }
    }

    // --- version_of with various args ---

    #[tokio::test]
    async fn mock_version_of_with_multiple_args() {
        let runner = RecordingRunner::new_all_succeed("v1.0");
        let _ = version_of(&runner, "cargo", &["llvm-cov", "--version"]).await;
        let calls = runner.calls().await;
        assert_eq!(calls[0].1, vec!["llvm-cov", "--version"]);
    }

    #[tokio::test]
    async fn mock_version_of_with_no_args() {
        let runner = RecordingRunner::new_all_succeed("v1.0");
        let _ = version_of(&runner, "tool", &[]).await;
        let calls = runner.calls().await;
        assert!(calls[0].1.is_empty());
    }

    // --- python checks pass correct args ---

    #[tokio::test]
    async fn recording_runner_python_pytest_uses_dash_m() {
        let runner = RecordingRunner::new_all_succeed("v1.0");
        let _ = checks_python(&runner).await;
        let calls = runner.calls().await;
        // pytest is invoked as python3 -m pytest --version
        let pytest_call = calls
            .iter()
            .find(|c| c.1.contains(&"-m".to_string()) && c.1.contains(&"pytest".to_string()));
        assert!(pytest_call.is_some());
        let call = pytest_call.unwrap();
        assert_eq!(call.0, "python3");
        assert_eq!(call.1, vec!["-m", "pytest", "--version"]);
    }

    #[tokio::test]
    async fn recording_runner_python_coverage_uses_dash_m() {
        let runner = RecordingRunner::new_all_succeed("v1.0");
        let _ = checks_python(&runner).await;
        let calls = runner.calls().await;
        let cov_call = calls.iter().find(|c| c.1.contains(&"coverage".to_string()));
        assert!(cov_call.is_some());
        let call = cov_call.unwrap();
        assert_eq!(call.0, "python3");
        assert_eq!(call.1, vec!["-m", "coverage", "--version"]);
    }
}
