use apex_core::{
    command::{CommandRunner, CommandSpec, RealCommandRunner},
    error::{ApexError, Result},
    traits::{LanguageRunner, TestRunOutput},
    types::Language,
};
use async_trait::async_trait;
use std::{path::Path, time::Instant};
use tracing::info;

/// JVM build timeout: 10 minutes (Gradle/Maven may download deps on first run).
const JVM_BUILD_TIMEOUT_MS: u64 = 600_000;

/// Build a `CommandSpec` with JVM-friendly defaults (10-min timeout, JAVA_HOME).
fn jvm_command(program: &str, target: &Path) -> CommandSpec {
    let mut spec = CommandSpec::new(program, target).timeout(JVM_BUILD_TIMEOUT_MS);
    if let Ok(java_home) = std::env::var("JAVA_HOME") {
        spec = spec.env("JAVA_HOME", java_home);
    }
    spec
}

pub struct KotlinRunner<R: CommandRunner = RealCommandRunner> {
    runner: R,
}

impl KotlinRunner {
    pub fn new() -> Self {
        KotlinRunner {
            runner: RealCommandRunner,
        }
    }
}

impl<R: CommandRunner> KotlinRunner<R> {
    pub fn with_runner(runner: R) -> Self {
        KotlinRunner { runner }
    }
}

impl Default for KotlinRunner {
    fn default() -> Self {
        Self::new()
    }
}

/// Check if build.gradle.kts has the Kover plugin.
///
/// When Kover is present, the runner prefers `./gradlew koverXmlReport`
/// over the standard JaCoCo task. The Kover XML format is JaCoCo-compatible,
/// so existing parsers work without changes.
pub fn detect_kover_plugin(target: &Path) -> bool {
    let gradle_kts = target.join("build.gradle.kts");
    if let Ok(content) = std::fs::read_to_string(&gradle_kts) {
        content.contains("kotlinx.kover") || content.contains("kover")
    } else {
        false
    }
}

/// Detect whether the project uses Gradle or Maven.
fn detect_build_tool(target: &Path) -> &'static str {
    if target.join("build.gradle.kts").exists() || target.join("build.gradle").exists() {
        "gradle"
    } else {
        "maven"
    }
}

/// Detect the Gradle wrapper or fall back to system gradle.
fn gradle_command(target: &Path) -> &'static str {
    if target.join("gradlew").exists() {
        "./gradlew"
    } else {
        "gradle"
    }
}

#[async_trait]
impl<R: CommandRunner> LanguageRunner for KotlinRunner<R> {
    fn language(&self) -> Language {
        Language::Kotlin
    }

    fn detect(&self, target: &Path) -> bool {
        // build.gradle.kts is the primary Kotlin build file
        if target.join("build.gradle.kts").exists() {
            return true;
        }

        // Check for .kt files in src/
        if target.join("src").exists() {
            if let Ok(entries) = std::fs::read_dir(target.join("src")) {
                for entry in entries.flatten() {
                    if has_kt_files(&entry.path()) {
                        return true;
                    }
                }
            }
        }

        false
    }

    async fn install_deps(&self, target: &Path) -> Result<()> {
        let build_tool = detect_build_tool(target);
        info!(target = %target.display(), build_tool, "installing Kotlin dependencies");

        match build_tool {
            "gradle" => {
                let cmd = gradle_command(target);
                let spec = jvm_command(cmd, target).args(["build", "-x", "test"]);
                let output = self
                    .runner
                    .run_command(&spec)
                    .await
                    .map_err(|e| ApexError::LanguageRunner(format!("gradle build: {e}")))?;
                if output.exit_code != 0 {
                    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
                    return Err(ApexError::LanguageRunner(format!(
                        "gradle build failed: {stderr}"
                    )));
                }
            }
            _ => {
                let spec = jvm_command("mvn", target).args(["compile", "-q"]);
                let output = self
                    .runner
                    .run_command(&spec)
                    .await
                    .map_err(|e| ApexError::LanguageRunner(format!("mvn compile: {e}")))?;
                if output.exit_code != 0 {
                    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
                    return Err(ApexError::LanguageRunner(format!(
                        "mvn compile failed: {stderr}"
                    )));
                }
            }
        }

        Ok(())
    }

    async fn run_tests(&self, target: &Path, extra_args: &[String]) -> Result<TestRunOutput> {
        let build_tool = detect_build_tool(target);
        info!(
            target = %target.display(),
            build_tool,
            "running Kotlin tests"
        );

        let start = Instant::now();

        let output = match build_tool {
            "gradle" => {
                let cmd = gradle_command(target);
                let mut args: Vec<String> = vec!["test".into()];
                args.extend(extra_args.iter().cloned());
                let spec = jvm_command(cmd, target).args(args);
                self.runner
                    .run_command(&spec)
                    .await
                    .map_err(|e| ApexError::LanguageRunner(format!("gradle test: {e}")))?
            }
            _ => {
                let mut args: Vec<String> = vec!["test".into(), "-q".into()];
                args.extend(extra_args.iter().cloned());
                let spec = jvm_command("mvn", target).args(args);
                self.runner
                    .run_command(&spec)
                    .await
                    .map_err(|e| ApexError::LanguageRunner(format!("mvn test: {e}")))?
            }
        };

        let duration_ms = start.elapsed().as_millis() as u64;

        Ok(TestRunOutput {
            exit_code: output.exit_code,
            stdout: String::from_utf8_lossy(&output.stdout).to_string(),
            stderr: String::from_utf8_lossy(&output.stderr).to_string(),
            duration_ms,
        })
    }
}

impl<R: CommandRunner> KotlinRunner<R> {
    /// Collect coverage for a Kotlin project.
    ///
    /// When the Kover plugin is detected in `build.gradle.kts`, this runs
    /// `./gradlew koverXmlReport` (preferred). Otherwise it falls back to the
    /// standard JaCoCo task `./gradlew jacocoTestReport`. Both produce
    /// JaCoCo-compatible XML that existing parsers can read.
    pub async fn collect_coverage(&self, target: &Path) -> Result<TestRunOutput> {
        let build_tool = detect_build_tool(target);
        info!(target = %target.display(), build_tool, "collecting Kotlin coverage");

        let start = Instant::now();

        let output = match build_tool {
            "gradle" => {
                let cmd = gradle_command(target);
                let task = if detect_kover_plugin(target) {
                    "koverXmlReport"
                } else {
                    "jacocoTestReport"
                };
                let spec = jvm_command(cmd, target).args([task]);
                self.runner
                    .run_command(&spec)
                    .await
                    .map_err(|e| ApexError::LanguageRunner(format!("gradle {task}: {e}")))?
            }
            _ => {
                // Maven: JaCoCo via surefire
                let spec = jvm_command("mvn", target)
                    .args(["jacoco:report", "-q"]);
                self.runner
                    .run_command(&spec)
                    .await
                    .map_err(|e| ApexError::LanguageRunner(format!("mvn jacoco:report: {e}")))?
            }
        };

        let duration_ms = start.elapsed().as_millis() as u64;

        Ok(TestRunOutput {
            exit_code: output.exit_code,
            stdout: String::from_utf8_lossy(&output.stdout).to_string(),
            stderr: String::from_utf8_lossy(&output.stderr).to_string(),
            duration_ms,
        })
    }
}

/// Recursively check if a directory contains .kt files.
fn has_kt_files(path: &Path) -> bool {
    if path.is_file() {
        return path.extension().and_then(|e| e.to_str()) == Some("kt");
    }
    if path.is_dir() {
        if let Ok(entries) = std::fs::read_dir(path) {
            for entry in entries.flatten() {
                if has_kt_files(&entry.path()) {
                    return true;
                }
            }
        }
    }
    false
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
    fn detect_build_gradle_kts() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("build.gradle.kts"), "").unwrap();
        assert!(KotlinRunner::new().detect(dir.path()));
    }

    #[test]
    fn detect_kt_files_in_src() {
        let dir = tempfile::tempdir().unwrap();
        let src_dir = dir.path().join("src").join("main").join("kotlin");
        std::fs::create_dir_all(&src_dir).unwrap();
        std::fs::write(src_dir.join("Main.kt"), "fun main() {}").unwrap();
        assert!(KotlinRunner::new().detect(dir.path()));
    }

    #[test]
    fn detect_empty_dir_returns_false() {
        let dir = tempfile::tempdir().unwrap();
        assert!(!KotlinRunner::new().detect(dir.path()));
    }

    #[test]
    fn language_is_kotlin() {
        assert_eq!(KotlinRunner::new().language(), Language::Kotlin);
    }

    #[test]
    fn default_creates_runner() {
        let runner = KotlinRunner::default();
        assert_eq!(runner.language(), Language::Kotlin);
    }

    #[tokio::test]
    async fn install_deps_gradle_success() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("build.gradle.kts"), "").unwrap();

        let mut mock = MockCmd::new();
        mock.expect_run_command()
            .withf(|spec| spec.args.contains(&"build".to_string()))
            .times(1)
            .returning(|_| Ok(CommandOutput::success(b"BUILD SUCCESSFUL".to_vec())));

        let runner = KotlinRunner::with_runner(mock);
        let result = runner.install_deps(dir.path()).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn install_deps_gradle_failure() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("build.gradle.kts"), "").unwrap();

        let mut mock = MockCmd::new();
        mock.expect_run_command()
            .times(1)
            .returning(|_| Ok(CommandOutput::failure(1, b"BUILD FAILED".to_vec())));

        let runner = KotlinRunner::with_runner(mock);
        let result = runner.install_deps(dir.path()).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn run_tests_gradle_success() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("build.gradle.kts"), "").unwrap();

        let mut mock = MockCmd::new();
        mock.expect_run_command()
            .withf(|spec| spec.args.contains(&"test".to_string()))
            .times(1)
            .returning(|_| Ok(CommandOutput::success(b"5 tests passed".to_vec())));

        let runner = KotlinRunner::with_runner(mock);
        let result = runner.run_tests(dir.path(), &[]).await.unwrap();
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("5 tests passed"));
    }

    #[tokio::test]
    async fn run_tests_maven_success() {
        let dir = tempfile::tempdir().unwrap();

        let mut mock = MockCmd::new();
        mock.expect_run_command()
            .withf(|spec| spec.program == "mvn")
            .times(1)
            .returning(|_| Ok(CommandOutput::success(b"Tests run: 3".to_vec())));

        let runner = KotlinRunner::with_runner(mock);
        let result = runner.run_tests(dir.path(), &[]).await.unwrap();
        assert_eq!(result.exit_code, 0);
    }

    #[tokio::test]
    async fn run_tests_with_extra_args() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("build.gradle.kts"), "").unwrap();

        let mut mock = MockCmd::new();
        mock.expect_run_command()
            .withf(|spec| spec.args.iter().any(|a| a == "--info"))
            .times(1)
            .returning(|_| Ok(CommandOutput::success(b"ok".to_vec())));

        let runner = KotlinRunner::with_runner(mock);
        let result = runner
            .run_tests(dir.path(), &["--info".into()])
            .await
            .unwrap();
        assert_eq!(result.exit_code, 0);
    }

    #[test]
    fn detect_build_tool_gradle() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("build.gradle.kts"), "").unwrap();
        assert_eq!(detect_build_tool(dir.path()), "gradle");
    }

    #[test]
    fn detect_build_tool_maven_fallback() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(detect_build_tool(dir.path()), "maven");
    }

    #[test]
    fn gradle_command_with_wrapper() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("gradlew"), "#!/bin/sh\n").unwrap();
        assert_eq!(gradle_command(dir.path()), "./gradlew");
    }

    #[test]
    fn gradle_command_without_wrapper() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(gradle_command(dir.path()), "gradle");
    }

    // ------------------------------------------------------------------
    // detect_kover_plugin tests
    // ------------------------------------------------------------------

    #[test]
    fn detect_kover_plugin_present() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("build.gradle.kts"),
            r#"plugins {
    id("org.jetbrains.kotlinx.kover") version "0.7.3"
}
"#,
        )
        .unwrap();
        assert!(detect_kover_plugin(dir.path()));
    }

    #[test]
    fn detect_kover_plugin_absent() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("build.gradle.kts"),
            r#"plugins {
    id("jacoco")
}
"#,
        )
        .unwrap();
        assert!(!detect_kover_plugin(dir.path()));
    }

    #[test]
    fn detect_kover_plugin_no_file() {
        let dir = tempfile::tempdir().unwrap();
        assert!(!detect_kover_plugin(dir.path()));
    }

    // ------------------------------------------------------------------
    // collect_coverage tests
    // ------------------------------------------------------------------

    #[tokio::test]
    async fn collect_coverage_uses_kover_when_plugin_present() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("build.gradle.kts"),
            r#"plugins { id("kotlinx.kover") }"#,
        )
        .unwrap();

        let mut mock = MockCmd::new();
        mock.expect_run_command()
            .withf(|spec| spec.args.contains(&"koverXmlReport".to_string()))
            .times(1)
            .returning(|_| Ok(CommandOutput::success(b"BUILD SUCCESSFUL".to_vec())));

        let runner = KotlinRunner::with_runner(mock);
        let result = runner.collect_coverage(dir.path()).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn collect_coverage_falls_back_to_jacoco_without_kover() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("build.gradle.kts"), r#"plugins { id("jacoco") }"#)
            .unwrap();

        let mut mock = MockCmd::new();
        mock.expect_run_command()
            .withf(|spec| spec.args.contains(&"jacocoTestReport".to_string()))
            .times(1)
            .returning(|_| Ok(CommandOutput::success(b"BUILD SUCCESSFUL".to_vec())));

        let runner = KotlinRunner::with_runner(mock);
        let result = runner.collect_coverage(dir.path()).await;
        assert!(result.is_ok());
    }
}
