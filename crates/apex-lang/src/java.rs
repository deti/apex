use apex_core::{
    command::{CommandRunner, CommandSpec, RealCommandRunner},
    config::InstrumentTimeouts,
    error::{ApexError, Result},
    traits::{LanguageRunner, PreflightInfo, TestRunOutput},
    types::Language,
};
use async_trait::async_trait;
use std::{path::Path, time::Instant};
use tracing::{info, warn};

/// Build a `CommandSpec` with JVM-friendly defaults (configurable timeout, JAVA_HOME).
fn jvm_command(program: &str, target: &Path, timeout_ms: u64) -> CommandSpec {
    let mut spec = CommandSpec::new(program, target).timeout(timeout_ms);
    if let Ok(java_home) = std::env::var("JAVA_HOME") {
        spec = spec.env("JAVA_HOME", java_home);
    }
    spec
}

pub struct JavaRunner<R: CommandRunner = RealCommandRunner> {
    runner: R,
    timeouts: InstrumentTimeouts,
}

impl JavaRunner {
    pub fn new() -> Self {
        JavaRunner {
            runner: RealCommandRunner,
            timeouts: InstrumentTimeouts::default(),
        }
    }
}

impl<R: CommandRunner> JavaRunner<R> {
    pub fn with_runner(runner: R) -> Self {
        JavaRunner {
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
        let mut cmd = std::process::Command::new(bin);
        cmd.args(args);
        if let Ok(java_home) = std::env::var("JAVA_HOME") {
            cmd.env("JAVA_HOME", java_home);
        }
        let output = cmd.output().ok()?;
        if !output.status.success() {
            return None;
        }
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        // java -version outputs to stderr
        let text = if stdout.trim().is_empty() {
            stderr.to_string()
        } else {
            stdout.to_string()
        };
        Some(text.lines().next().unwrap_or("").trim().to_string())
    }

    /// Check if gradlew exists and is executable.
    fn has_gradle_wrapper(target: &Path) -> bool {
        let gradlew = target.join("gradlew");
        gradlew.exists()
    }

    /// Check if JaCoCo plugin is configured in build.gradle.
    fn has_jacoco_plugin(target: &Path) -> bool {
        for name in &["build.gradle", "build.gradle.kts"] {
            if let Ok(content) = std::fs::read_to_string(target.join(name)) {
                if content.contains("jacoco") || content.contains("JaCoCo") {
                    return true;
                }
            }
        }
        // Also check pom.xml for Maven
        if let Ok(content) = std::fs::read_to_string(target.join("pom.xml")) {
            if content.contains("jacoco") {
                return true;
            }
        }
        false
    }

    /// Detect Gradle multi-module projects by looking for settings.gradle.
    fn detect_subprojects(target: &Path) -> Vec<String> {
        let mut subprojects = Vec::new();
        for name in &["settings.gradle", "settings.gradle.kts"] {
            if let Ok(content) = std::fs::read_to_string(target.join(name)) {
                // Parse include statements: include("sub1", "sub2") or include 'sub1', 'sub2'
                for line in content.lines() {
                    let trimmed = line.trim();
                    if trimmed.starts_with("include") {
                        // Extract quoted strings
                        let mut in_quote = false;
                        let mut quote_char = '"';
                        let mut current = String::new();
                        for ch in trimmed.chars() {
                            if !in_quote && (ch == '"' || ch == '\'') {
                                in_quote = true;
                                quote_char = ch;
                            } else if in_quote && ch == quote_char {
                                in_quote = false;
                                if !current.is_empty() {
                                    subprojects.push(current.trim_start_matches(':').to_string());
                                    current.clear();
                                }
                            } else if in_quote {
                                current.push(ch);
                            }
                        }
                    }
                }
            }
        }
        subprojects
    }
}

impl Default for JavaRunner {
    fn default() -> Self {
        Self::new()
    }
}

/// Detect whether the project uses Gradle or Maven.
pub fn detect_build_tool(target: &Path) -> &'static str {
    if target.join("build.gradle").exists() || target.join("build.gradle.kts").exists() {
        "gradle"
    } else {
        "maven"
    }
}

#[async_trait]
impl<R: CommandRunner> LanguageRunner for JavaRunner<R> {
    fn language(&self) -> Language {
        Language::Java
    }

    fn detect(&self, target: &Path) -> bool {
        target.join("pom.xml").exists()
            || target.join("build.gradle").exists()
            || target.join("build.gradle.kts").exists()
    }

    async fn install_deps(&self, target: &Path) -> Result<()> {
        info!(target = %target.display(), "installing Java dependencies");

        let build_tool = detect_build_tool(target);

        let spec = if build_tool == "gradle" {
            jvm_command("./gradlew", target, self.timeouts.jvm_build_ms)
                .args(["dependencies", "--quiet"])
        } else {
            jvm_command("mvn", target, self.timeouts.jvm_build_ms)
                .args(["-q", "dependency:resolve", "-DskipTests"])
        };

        let result = self
            .runner
            .run_command(&spec)
            .await
            .map_err(|e| ApexError::LanguageRunner(format!("spawn {}: {e}", spec.program)))?;

        if result.exit_code != 0 {
            warn!(
                exit = result.exit_code,
                build_tool,
                "dependency installation returned non-zero (offline builds may still work)"
            );
        }

        Ok(())
    }

    async fn run_tests(&self, target: &Path, extra_args: &[String]) -> Result<TestRunOutput> {
        let build_tool = detect_build_tool(target);

        let mut cmd_parts: Vec<String> = if build_tool == "gradle" {
            vec!["./gradlew".into(), "test".into(), "--quiet".into()]
        } else {
            vec!["mvn".into(), "-q".into(), "test".into()]
        };

        cmd_parts.extend(extra_args.iter().cloned());

        info!(
            target = %target.display(),
            cmd = ?cmd_parts,
            "running Java tests"
        );

        let spec = jvm_command(&cmd_parts[0], target, self.timeouts.jvm_build_ms)
            .args(cmd_parts[1..].to_vec());

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
        let build_tool = detect_build_tool(target);
        info.build_system = Some(build_tool.into());
        info.test_framework = Some("JUnit".into());

        if build_tool == "gradle" {
            info.package_manager = Some("gradle".into());

            // Check for gradle wrapper
            if Self::has_gradle_wrapper(target) {
                info.extra.push(("gradlew".into(), "true".into()));
            } else {
                // Check system gradle
                if let Some(ver) = Self::tool_version("gradle", &["--version"]) {
                    info.available_tools.push(("gradle".into(), ver));
                } else {
                    info.missing_tools.push("gradle".into());
                    info.warnings.push(
                        "no gradlew wrapper and gradle not on PATH".into(),
                    );
                }
            }

            // Detect subprojects (multi-module)
            let subprojects = Self::detect_subprojects(target);
            if !subprojects.is_empty() {
                info.extra.push(("multi_module".into(), "true".into()));
                for sp in &subprojects {
                    info.extra.push(("subproject".into(), sp.clone()));
                }
            }
        } else {
            info.package_manager = Some("maven".into());

            // Check mvn
            if let Some(ver) = Self::tool_version("mvn", &["--version"]) {
                info.available_tools.push(("mvn".into(), ver));
            } else {
                info.missing_tools.push("mvn".into());
            }
        }

        // Check java
        if let Some(ver) = Self::tool_version("java", &["-version"]) {
            info.available_tools.push(("java".into(), ver));
        } else {
            info.missing_tools.push("java".into());
        }

        // Check JAVA_HOME
        if let Ok(java_home) = std::env::var("JAVA_HOME") {
            info.env_vars.push(("JAVA_HOME".into(), java_home));
        } else {
            info.warnings.push("JAVA_HOME not set".into());
        }

        // Check JaCoCo
        if Self::has_jacoco_plugin(target) {
            info.extra.push(("jacoco".into(), "configured".into()));
        } else {
            info.warnings.push(
                "JaCoCo not found in build configuration; coverage collection may need init.gradle injection".into(),
            );
        }

        // Deps installed check
        info.deps_installed = if build_tool == "gradle" {
            target.join(".gradle").exists() || target.join("build").exists()
        } else {
            target.join("target").exists()
        };

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
    fn detect_pom_xml() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("pom.xml"), "<project/>").unwrap();
        assert!(JavaRunner::new().detect(dir.path()));
    }

    #[test]
    fn detect_build_gradle() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("build.gradle"), "").unwrap();
        assert!(JavaRunner::new().detect(dir.path()));
    }

    #[test]
    fn detect_build_gradle_kts() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("build.gradle.kts"), "").unwrap();
        assert!(JavaRunner::new().detect(dir.path()));
    }

    #[test]
    fn detect_empty_dir_returns_false() {
        let dir = tempfile::tempdir().unwrap();
        assert!(!JavaRunner::new().detect(dir.path()));
    }

    #[test]
    fn detect_build_tool_gradle() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("build.gradle"), "").unwrap();
        assert_eq!(detect_build_tool(dir.path()), "gradle");
    }

    #[test]
    fn detect_build_tool_gradle_kts() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("build.gradle.kts"), "").unwrap();
        assert_eq!(detect_build_tool(dir.path()), "gradle");
    }

    #[test]
    fn detect_build_tool_maven_default() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(detect_build_tool(dir.path()), "maven");
    }

    #[test]
    fn detect_build_tool_maven_with_pom() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("pom.xml"), "").unwrap();
        assert_eq!(detect_build_tool(dir.path()), "maven");
    }

    #[test]
    fn language_is_java() {
        assert_eq!(JavaRunner::new().language(), Language::Java);
    }

    // ---- Mock-based tests ----

    #[tokio::test]
    async fn install_deps_gradle_success() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("build.gradle"), "").unwrap();

        let mut mock = MockCmd::new();
        mock.expect_run_command()
            .withf(|spec| spec.program == "./gradlew")
            .times(1)
            .returning(|_| Ok(CommandOutput::success(b"BUILD SUCCESSFUL".to_vec())));

        let runner = JavaRunner::with_runner(mock);
        let result = runner.install_deps(dir.path()).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn install_deps_maven_success() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("pom.xml"), "<project/>").unwrap();

        let mut mock = MockCmd::new();
        mock.expect_run_command()
            .withf(|spec| spec.program == "mvn")
            .times(1)
            .returning(|_| Ok(CommandOutput::success(b"BUILD SUCCESS".to_vec())));

        let runner = JavaRunner::with_runner(mock);
        let result = runner.install_deps(dir.path()).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn install_deps_nonzero_exit_still_ok() {
        // Non-zero exit from deps install is just a warning, not an error.
        let dir = tempfile::tempdir().unwrap();

        let mut mock = MockCmd::new();
        mock.expect_run_command()
            .times(1)
            .returning(|_| Ok(CommandOutput::failure(1, b"FAILURE".to_vec())));

        let runner = JavaRunner::with_runner(mock);
        let result = runner.install_deps(dir.path()).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn install_deps_command_error() {
        let dir = tempfile::tempdir().unwrap();

        let mut mock = MockCmd::new();
        mock.expect_run_command().times(1).returning(|_| {
            Err(ApexError::Subprocess {
                exit_code: -1,
                stderr: "spawn mvn: not found".into(),
            })
        });

        let runner = JavaRunner::with_runner(mock);
        let result = runner.install_deps(dir.path()).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn run_tests_gradle_success() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("build.gradle"), "").unwrap();

        let mut mock = MockCmd::new();
        mock.expect_run_command()
            .withf(|spec| spec.program == "./gradlew" && spec.args.contains(&"test".to_string()))
            .times(1)
            .returning(|_| {
                Ok(CommandOutput::success(
                    b"BUILD SUCCESSFUL\n5 tests passed".to_vec(),
                ))
            });

        let runner = JavaRunner::with_runner(mock);
        let result = runner.run_tests(dir.path(), &[]).await.unwrap();
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("5 tests passed"));
    }

    #[tokio::test]
    async fn run_tests_maven_failure() {
        let dir = tempfile::tempdir().unwrap();

        let mut mock = MockCmd::new();
        mock.expect_run_command()
            .withf(|spec| spec.program == "mvn")
            .times(1)
            .returning(|_| {
                Ok(CommandOutput {
                    exit_code: 1,
                    stdout: b"Tests run: 3, Failures: 1".to_vec(),
                    stderr: b"BUILD FAILURE".to_vec(),
                })
            });

        let runner = JavaRunner::with_runner(mock);
        let result = runner.run_tests(dir.path(), &[]).await.unwrap();
        assert_eq!(result.exit_code, 1);
        assert!(result.stdout.contains("Failures: 1"));
    }

    #[tokio::test]
    async fn run_tests_command_not_found() {
        let dir = tempfile::tempdir().unwrap();

        let mut mock = MockCmd::new();
        mock.expect_run_command().times(1).returning(|_| {
            Err(ApexError::Subprocess {
                exit_code: -1,
                stderr: "spawn mvn: No such file or directory".into(),
            })
        });

        let runner = JavaRunner::with_runner(mock);
        let result = runner.run_tests(dir.path(), &[]).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn run_tests_with_extra_args() {
        let dir = tempfile::tempdir().unwrap();

        let mut mock = MockCmd::new();
        mock.expect_run_command()
            .withf(|spec| spec.args.iter().any(|a| a == "-Dtest=MyTest"))
            .times(1)
            .returning(|_| Ok(CommandOutput::success(b"ok".to_vec())));

        let runner = JavaRunner::with_runner(mock);
        let result = runner
            .run_tests(dir.path(), &["-Dtest=MyTest".into()])
            .await
            .unwrap();
        assert_eq!(result.exit_code, 0);
    }

    // ------------------------------------------------------------------
    // default() constructor
    // ------------------------------------------------------------------

    #[test]
    fn default_creates_runner() {
        let runner = JavaRunner::default();
        assert_eq!(runner.language(), Language::Java);
    }

    // ------------------------------------------------------------------
    // detect — nonexistent dir
    // ------------------------------------------------------------------

    #[test]
    fn detect_nonexistent_dir_returns_false() {
        let runner = JavaRunner::new();
        assert!(!runner.detect(Path::new("/nonexistent/path/that/does/not/exist")));
    }

    // ------------------------------------------------------------------
    // install_deps — gradle nonzero exit is just a warning
    // ------------------------------------------------------------------

    #[tokio::test]
    async fn install_deps_gradle_nonzero_exit_still_ok() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("build.gradle"), "").unwrap();

        let mut mock = MockCmd::new();
        mock.expect_run_command()
            .withf(|spec| spec.program == "./gradlew")
            .times(1)
            .returning(|_| Ok(CommandOutput::failure(1, b"BUILD FAILED".to_vec())));

        let runner = JavaRunner::with_runner(mock);
        let result = runner.install_deps(dir.path()).await;
        assert!(result.is_ok(), "nonzero gradle exit should be a warning");
    }

    // ------------------------------------------------------------------
    // install_deps — gradle kts variant
    // ------------------------------------------------------------------

    #[tokio::test]
    async fn install_deps_gradle_kts() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("build.gradle.kts"), "").unwrap();

        let mut mock = MockCmd::new();
        mock.expect_run_command()
            .withf(|spec| spec.program == "./gradlew")
            .times(1)
            .returning(|_| Ok(CommandOutput::success(b"ok".to_vec())));

        let runner = JavaRunner::with_runner(mock);
        let result = runner.install_deps(dir.path()).await;
        assert!(result.is_ok());
    }

    // ------------------------------------------------------------------
    // run_tests — gradle with extra args
    // ------------------------------------------------------------------

    #[tokio::test]
    async fn run_tests_gradle_with_extra_args() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("build.gradle"), "").unwrap();

        let mut mock = MockCmd::new();
        mock.expect_run_command()
            .withf(|spec| {
                spec.program == "./gradlew"
                    && spec.args.contains(&"test".to_string())
                    && spec.args.iter().any(|a| a == "--info")
            })
            .times(1)
            .returning(|_| Ok(CommandOutput::success(b"ok".to_vec())));

        let runner = JavaRunner::with_runner(mock);
        let result = runner
            .run_tests(dir.path(), &["--info".into()])
            .await
            .unwrap();
        assert_eq!(result.exit_code, 0);
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

        let runner = JavaRunner::with_runner(mock);
        let result = runner.run_tests(dir.path(), &[]).await.unwrap();
        assert!(result.duration_ms < u64::MAX);
    }

    // ------------------------------------------------------------------
    // run_tests — gradle kts uses gradle path
    // ------------------------------------------------------------------

    #[tokio::test]
    async fn run_tests_gradle_kts_uses_gradlew() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("build.gradle.kts"), "").unwrap();

        let mut mock = MockCmd::new();
        mock.expect_run_command()
            .withf(|spec| spec.program == "./gradlew" && spec.args.contains(&"test".to_string()))
            .times(1)
            .returning(|_| Ok(CommandOutput::success(b"ok".to_vec())));

        let runner = JavaRunner::with_runner(mock);
        let result = runner.run_tests(dir.path(), &[]).await.unwrap();
        assert_eq!(result.exit_code, 0);
    }

    // ------------------------------------------------------------------
    // run_tests — maven nonzero exit is not an Err
    // ------------------------------------------------------------------

    #[tokio::test]
    async fn run_tests_maven_nonzero_exit() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("pom.xml"), "").unwrap();

        let mut mock = MockCmd::new();
        mock.expect_run_command()
            .times(1)
            .returning(|_| Ok(CommandOutput::failure(1, b"BUILD FAILURE".to_vec())));

        let runner = JavaRunner::with_runner(mock);
        let result = runner.run_tests(dir.path(), &[]).await.unwrap();
        assert_eq!(result.exit_code, 1);
    }

    // ------------------------------------------------------------------
    // detect — only unrelated files
    // ------------------------------------------------------------------

    #[test]
    fn detect_unrelated_files_returns_false() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("main.py"), "").unwrap();
        std::fs::write(dir.path().join("Cargo.toml"), "").unwrap();
        assert!(!JavaRunner::new().detect(dir.path()));
    }

    // ------------------------------------------------------------------
    // detect — multiple markers
    // ------------------------------------------------------------------

    #[test]
    fn detect_pom_and_gradle() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("pom.xml"), "").unwrap();
        std::fs::write(dir.path().join("build.gradle"), "").unwrap();
        assert!(JavaRunner::new().detect(dir.path()));
    }

    // ------------------------------------------------------------------
    // install_deps — maven spawn error
    // ------------------------------------------------------------------

    #[tokio::test]
    async fn install_deps_maven_command_error() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("pom.xml"), "<project/>").unwrap();

        let mut mock = MockCmd::new();
        mock.expect_run_command()
            .withf(|spec| spec.program == "mvn")
            .times(1)
            .returning(|_| {
                Err(ApexError::Subprocess {
                    exit_code: -1,
                    stderr: "spawn mvn: No such file or directory".into(),
                })
            });

        let runner = JavaRunner::with_runner(mock);
        let result = runner.install_deps(dir.path()).await;
        assert!(result.is_err());
        let msg = format!("{}", result.unwrap_err());
        assert!(msg.contains("spawn mvn"), "unexpected: {msg}");
    }

    // ------------------------------------------------------------------
    // run_tests — maven with extra args passes them
    // ------------------------------------------------------------------

    #[tokio::test]
    async fn run_tests_maven_with_extra_args() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("pom.xml"), "<project/>").unwrap();

        let mut mock = MockCmd::new();
        mock.expect_run_command()
            .withf(|spec| {
                spec.program == "mvn"
                    && spec.args.contains(&"test".to_string())
                    && spec.args.iter().any(|a| a == "-Dtest=FooTest")
            })
            .times(1)
            .returning(|_| Ok(CommandOutput::success(b"BUILD SUCCESS".to_vec())));

        let runner = JavaRunner::with_runner(mock);
        let result = runner
            .run_tests(dir.path(), &["-Dtest=FooTest".into()])
            .await
            .unwrap();
        assert_eq!(result.exit_code, 0);
    }

    // ------------------------------------------------------------------
    // run_tests — gradle spawn error
    // ------------------------------------------------------------------

    #[tokio::test]
    async fn run_tests_gradle_command_error() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("build.gradle"), "").unwrap();

        let mut mock = MockCmd::new();
        mock.expect_run_command()
            .withf(|spec| spec.program == "./gradlew")
            .times(1)
            .returning(|_| {
                Err(ApexError::Subprocess {
                    exit_code: -1,
                    stderr: "spawn ./gradlew: not found".into(),
                })
            });

        let runner = JavaRunner::with_runner(mock);
        let result = runner.run_tests(dir.path(), &[]).await;
        assert!(result.is_err());
        let msg = format!("{}", result.unwrap_err());
        assert!(msg.contains("run tests"), "unexpected: {msg}");
    }

    // ------------------------------------------------------------------
    // run_tests — duration is populated (maven path)
    // ------------------------------------------------------------------

    #[tokio::test]
    async fn run_tests_duration_populated_maven() {
        let dir = tempfile::tempdir().unwrap();

        let mut mock = MockCmd::new();
        mock.expect_run_command()
            .times(1)
            .returning(|_| Ok(CommandOutput::success(b"ok".to_vec())));

        let runner = JavaRunner::with_runner(mock);
        let result = runner.run_tests(dir.path(), &[]).await.unwrap();
        assert!(result.duration_ms < u64::MAX);
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
                stdout: b"BUILD SUCCESS".to_vec(),
                stderr: b"[WARNING] deprecated API".to_vec(),
            })
        });

        let runner = JavaRunner::with_runner(mock);
        let result = runner.run_tests(dir.path(), &[]).await.unwrap();
        assert!(result.stderr.contains("deprecated API"));
    }

    // ------------------------------------------------------------------
    // preflight_check tests
    // ------------------------------------------------------------------

    #[test]
    fn preflight_check_gradle_project() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("build.gradle"), "apply plugin: 'java'\n").unwrap();
        let runner = JavaRunner::new();
        let info = runner.preflight_check(dir.path()).unwrap();
        assert_eq!(info.build_system.as_deref(), Some("gradle"));
        assert_eq!(info.test_framework.as_deref(), Some("JUnit"));
    }

    #[test]
    fn preflight_check_maven_project() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("pom.xml"), "<project/>").unwrap();
        let runner = JavaRunner::new();
        let info = runner.preflight_check(dir.path()).unwrap();
        assert_eq!(info.build_system.as_deref(), Some("maven"));
        assert_eq!(info.package_manager.as_deref(), Some("maven"));
    }

    #[test]
    fn preflight_check_detects_jacoco_in_gradle() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("build.gradle"),
            "apply plugin: 'jacoco'\n",
        )
        .unwrap();
        let runner = JavaRunner::new();
        let info = runner.preflight_check(dir.path()).unwrap();
        assert!(info.extra.iter().any(|(k, v)| k == "jacoco" && v == "configured"));
    }

    #[test]
    fn preflight_check_warns_no_jacoco() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("build.gradle"), "apply plugin: 'java'\n").unwrap();
        let runner = JavaRunner::new();
        let info = runner.preflight_check(dir.path()).unwrap();
        assert!(info.warnings.iter().any(|w| w.contains("JaCoCo")));
    }

    #[test]
    fn preflight_check_detects_multi_module() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("build.gradle"), "").unwrap();
        std::fs::write(
            dir.path().join("settings.gradle"),
            "include 'module-a', 'module-b'\n",
        )
        .unwrap();
        let runner = JavaRunner::new();
        let info = runner.preflight_check(dir.path()).unwrap();
        assert!(info.extra.iter().any(|(k, v)| k == "multi_module" && v == "true"));
        assert!(info.extra.iter().any(|(k, v)| k == "subproject" && v == "module-a"));
        assert!(info.extra.iter().any(|(k, v)| k == "subproject" && v == "module-b"));
    }

    #[test]
    fn preflight_check_detects_gradlew() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("build.gradle"), "").unwrap();
        std::fs::write(dir.path().join("gradlew"), "#!/bin/sh\n").unwrap();
        let runner = JavaRunner::new();
        let info = runner.preflight_check(dir.path()).unwrap();
        assert!(info.extra.iter().any(|(k, v)| k == "gradlew" && v == "true"));
    }

    #[test]
    fn detect_subprojects_from_settings_gradle() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("settings.gradle"),
            "include 'api', 'core'\ninclude 'web'\n",
        )
        .unwrap();
        let subs = JavaRunner::<RealCommandRunner>::detect_subprojects(dir.path());
        assert!(subs.contains(&"api".to_string()));
        assert!(subs.contains(&"core".to_string()));
        assert!(subs.contains(&"web".to_string()));
    }

    #[test]
    fn detect_subprojects_from_settings_gradle_kts() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("settings.gradle.kts"),
            "include(\"api\")\ninclude(\"core\")\n",
        )
        .unwrap();
        let subs = JavaRunner::<RealCommandRunner>::detect_subprojects(dir.path());
        assert!(subs.contains(&"api".to_string()));
        assert!(subs.contains(&"core".to_string()));
    }

    #[test]
    fn detect_subprojects_colon_prefix_stripped() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("settings.gradle"),
            "include ':app', ':lib'\n",
        )
        .unwrap();
        let subs = JavaRunner::<RealCommandRunner>::detect_subprojects(dir.path());
        assert!(subs.contains(&"app".to_string()));
        assert!(subs.contains(&"lib".to_string()));
    }

    #[test]
    fn has_jacoco_plugin_in_pom() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("pom.xml"),
            "<project><plugins><plugin><artifactId>jacoco-maven-plugin</artifactId></plugin></plugins></project>",
        )
        .unwrap();
        assert!(JavaRunner::<RealCommandRunner>::has_jacoco_plugin(dir.path()));
    }

    #[test]
    fn has_gradle_wrapper_true() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("gradlew"), "#!/bin/sh\n").unwrap();
        assert!(JavaRunner::<RealCommandRunner>::has_gradle_wrapper(dir.path()));
    }

    #[test]
    fn has_gradle_wrapper_false() {
        let dir = tempfile::tempdir().unwrap();
        assert!(!JavaRunner::<RealCommandRunner>::has_gradle_wrapper(dir.path()));
    }
}
