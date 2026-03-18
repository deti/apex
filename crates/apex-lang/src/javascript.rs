use apex_core::{
    command::{CommandRunner, CommandSpec, RealCommandRunner},
    error::{ApexError, Result},
    traits::{LanguageRunner, PreflightInfo, TestRunOutput},
    types::Language,
};
use async_trait::async_trait;
use std::{path::Path, time::Instant};
use tracing::info;

use crate::js_env::{self, JsEnvironment};

pub struct JavaScriptRunner<R: CommandRunner = RealCommandRunner> {
    runner: R,
}

impl JavaScriptRunner {
    pub fn new() -> Self {
        JavaScriptRunner {
            runner: RealCommandRunner,
        }
    }
}

impl<R: CommandRunner> JavaScriptRunner<R> {
    pub fn with_runner(runner: R) -> Self {
        JavaScriptRunner { runner }
    }

    /// Detect the test runner from package.json contents.
    /// Returns (binary, args).
    fn detect_test_runner(target: &Path) -> (String, Vec<String>) {
        let runner = js_env::detect_test_runner(target);
        let runtime = if target.join("bun.lockb").exists() || target.join("bunfig.toml").exists() {
            js_env::JsRuntime::Bun
        } else if target.join("deno.json").exists() || target.join("deno.jsonc").exists() {
            js_env::JsRuntime::Deno
        } else {
            js_env::JsRuntime::Node
        };
        let env = JsEnvironment {
            runtime,
            pkg_manager: js_env::PkgManager::Npm,
            test_runner: runner,
            module_system: js_env::ModuleSystem::CommonJS,
            is_typescript: false,
            source_maps: false,
            monorepo: None,
        };
        js_env::test_command(&env)
    }

    /// Detect which package manager is in use.
    fn detect_package_manager(target: &Path) -> &'static str {
        if let Some(env) = JsEnvironment::detect(target) {
            return js_env::install_command(&env);
        }
        // Fallback when there is no package.json: inspect lockfiles directly.
        if target.join("bun.lockb").exists() || target.join("bunfig.toml").exists() {
            return "bun";
        }
        if target.join("deno.json").exists() || target.join("deno.jsonc").exists() {
            return "deno";
        }
        if target.join("yarn.lock").exists() {
            return "yarn";
        }
        if target.join("pnpm-lock.yaml").exists() {
            return "pnpm";
        }
        "npm"
    }
}

impl<R: CommandRunner> JavaScriptRunner<R> {
    /// Check if a binary is on PATH and return its version string.
    fn tool_version(bin: &str, args: &[&str]) -> Option<String> {
        let output = std::process::Command::new(bin).args(args).output().ok()?;
        if !output.status.success() {
            return None;
        }
        let stdout = String::from_utf8_lossy(&output.stdout);
        Some(stdout.lines().next().unwrap_or("").trim().to_string())
    }

    /// Detect monorepo patterns.
    fn detect_monorepo(target: &Path) -> Option<&'static str> {
        if target.join("lerna.json").exists() {
            return Some("lerna");
        }
        if target.join("nx.json").exists() {
            return Some("nx");
        }
        if target.join("pnpm-workspace.yaml").exists() {
            return Some("pnpm-workspaces");
        }
        if target.join("turbo.json").exists() {
            return Some("turborepo");
        }
        // Check package.json for workspaces field
        if let Ok(content) = std::fs::read_to_string(target.join("package.json")) {
            if content.contains("\"workspaces\"") {
                return Some("npm-workspaces");
            }
        }
        None
    }
}

impl Default for JavaScriptRunner {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl<R: CommandRunner> LanguageRunner for JavaScriptRunner<R> {
    fn language(&self) -> Language {
        Language::JavaScript
    }

    fn detect(&self, target: &Path) -> bool {
        target.join("package.json").exists()
            || target.join("deno.json").exists()
            || target.join("deno.jsonc").exists()
    }

    async fn install_deps(&self, target: &Path) -> Result<()> {
        if !target.join("package.json").exists() {
            return Ok(());
        }

        let pm = Self::detect_package_manager(target);
        info!(target = %target.display(), pm, "installing JavaScript dependencies");

        // Large projects with many transitive deps can take 2+ minutes to install.
        let spec = CommandSpec::new(pm, target)
            .args(["install"])
            .timeout(180_000);
        let output = self
            .runner
            .run_command(&spec)
            .await
            .map_err(|e| ApexError::LanguageRunner(format!("{pm} install: {e}")))?;

        if output.exit_code != 0 {
            let stderr = String::from_utf8_lossy(&output.stderr).to_string();
            return Err(ApexError::LanguageRunner(format!(
                "{pm} install failed: {stderr}"
            )));
        }

        Ok(())
    }

    async fn run_tests(&self, target: &Path, extra_args: &[String]) -> Result<TestRunOutput> {
        let (binary, mut args) = Self::detect_test_runner(target);
        args.extend(extra_args.iter().cloned());

        info!(
            target = %target.display(),
            binary,
            cmd = ?args,
            "running JavaScript tests"
        );

        let spec = CommandSpec::new(&binary, target).args(args);

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

        // Detect package manager
        let pm = Self::detect_package_manager(target);
        info.package_manager = Some(pm.into());

        // Detect test runner
        let (bin, args) = Self::detect_test_runner(target);
        let test_name = if args.first().map(|s| s.as_str()) == Some("test") && bin == "npm" {
            "npm-test-script".to_string()
        } else {
            args.first().cloned().unwrap_or_else(|| bin.clone())
        };
        info.test_framework = Some(test_name);

        // Check runtime
        if let Some(ver) = Self::tool_version("node", &["--version"]) {
            info.available_tools.push(("node".into(), ver.clone()));
            // Check Node version for V8 coverage format
            let major: Option<u32> = ver
                .trim_start_matches('v')
                .split('.')
                .next()
                .and_then(|s| s.parse().ok());
            if let Some(m) = major {
                info.extra.push(("node_major_version".into(), m.to_string()));
                if m < 16 {
                    info.warnings.push(format!(
                        "Node.js v{m} detected; V8 coverage requires Node >= 16"
                    ));
                }
            }
        } else {
            // Check for Bun or Deno as alternatives
            if let Some(ver) = Self::tool_version("bun", &["--version"]) {
                info.available_tools.push(("bun".into(), ver));
            } else if let Some(ver) = Self::tool_version("deno", &["--version"]) {
                info.available_tools.push(("deno".into(), ver));
            } else {
                info.missing_tools.push("node (or bun/deno)".into());
            }
        }

        // Check if node_modules exists
        info.deps_installed = target.join("node_modules").exists();

        // Detect monorepo
        if let Some(mono_kind) = Self::detect_monorepo(target) {
            info.extra.push(("monorepo".into(), mono_kind.into()));
            info.warnings.push(format!(
                "{mono_kind} monorepo detected: test commands may need workspace-aware invocation"
            ));
        }

        // Detect TypeScript
        let is_ts = target.join("tsconfig.json").exists();
        if is_ts {
            info.extra.push(("typescript".into(), "true".into()));
        }

        // Build system
        info.build_system = Some(pm.into());

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
    fn detect_with_package_json() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("package.json"), "{}").unwrap();
        assert!(JavaScriptRunner::new().detect(dir.path()));
    }

    #[test]
    fn detect_empty_dir_returns_false() {
        let dir = tempfile::tempdir().unwrap();
        assert!(!JavaScriptRunner::new().detect(dir.path()));
    }

    #[test]
    fn detect_test_runner_jest() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("package.json"),
            r#"{"devDependencies": {"jest": "^29"}}"#,
        )
        .unwrap();
        let (bin, args) = JavaScriptRunner::<RealCommandRunner>::detect_test_runner(dir.path());
        assert_eq!(bin, "npx");
        assert_eq!(args, vec!["jest", "--passWithNoTests"]);
    }

    #[test]
    fn detect_test_runner_mocha() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("package.json"),
            r#"{"devDependencies": {"mocha": "^10"}}"#,
        )
        .unwrap();
        let (bin, args) = JavaScriptRunner::<RealCommandRunner>::detect_test_runner(dir.path());
        assert_eq!(bin, "npx");
        assert_eq!(args, vec!["mocha"]);
    }

    #[test]
    fn detect_test_runner_vitest() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("package.json"),
            r#"{"devDependencies": {"vitest": "^1"}}"#,
        )
        .unwrap();
        let (bin, args) = JavaScriptRunner::<RealCommandRunner>::detect_test_runner(dir.path());
        assert_eq!(bin, "npx");
        assert_eq!(args, vec!["vitest", "run"]);
    }

    #[test]
    fn detect_test_runner_npm_test_script() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("package.json"),
            r#"{"scripts": {"test": "node test.js"}}"#,
        )
        .unwrap();
        let (bin, args) = JavaScriptRunner::<RealCommandRunner>::detect_test_runner(dir.path());
        assert_eq!(bin, "npm");
        assert_eq!(args, vec!["test"]);
    }

    #[test]
    fn detect_test_runner_default_fallback() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("package.json"), r#"{"name": "foo"}"#).unwrap();
        let (bin, args) = JavaScriptRunner::<RealCommandRunner>::detect_test_runner(dir.path());
        assert_eq!(bin, "npx");
        assert_eq!(args, vec!["jest", "--passWithNoTests"]);
    }

    #[test]
    fn detect_package_manager_yarn() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("yarn.lock"), "").unwrap();
        assert_eq!(
            JavaScriptRunner::<RealCommandRunner>::detect_package_manager(dir.path()),
            "yarn"
        );
    }

    #[test]
    fn detect_package_manager_pnpm() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("pnpm-lock.yaml"), "").unwrap();
        assert_eq!(
            JavaScriptRunner::<RealCommandRunner>::detect_package_manager(dir.path()),
            "pnpm"
        );
    }

    #[test]
    fn detect_package_manager_npm_default() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(
            JavaScriptRunner::<RealCommandRunner>::detect_package_manager(dir.path()),
            "npm"
        );
    }

    #[test]
    fn detect_package_manager_yarn_takes_priority_over_pnpm() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("yarn.lock"), "").unwrap();
        std::fs::write(dir.path().join("pnpm-lock.yaml"), "").unwrap();
        assert_eq!(
            JavaScriptRunner::<RealCommandRunner>::detect_package_manager(dir.path()),
            "yarn"
        );
    }

    #[test]
    fn language_is_javascript() {
        assert_eq!(JavaScriptRunner::new().language(), Language::JavaScript);
    }

    // ---- Mock-based tests ----

    #[tokio::test]
    async fn install_deps_npm_success() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("package.json"), r#"{"name":"test"}"#).unwrap();

        let mut mock = MockCmd::new();
        mock.expect_run_command()
            .withf(|spec| spec.program == "npm" && spec.args.contains(&"install".to_string()))
            .times(1)
            .returning(|_| Ok(CommandOutput::success(b"added 42 packages".to_vec())));

        let runner = JavaScriptRunner::with_runner(mock);
        let result = runner.install_deps(dir.path()).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn install_deps_yarn_success() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("package.json"), r#"{"name":"test"}"#).unwrap();
        std::fs::write(dir.path().join("yarn.lock"), "").unwrap();

        let mut mock = MockCmd::new();
        mock.expect_run_command()
            .withf(|spec| spec.program == "yarn")
            .times(1)
            .returning(|_| Ok(CommandOutput::success(b"Done".to_vec())));

        let runner = JavaScriptRunner::with_runner(mock);
        let result = runner.install_deps(dir.path()).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn install_deps_failure() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("package.json"), r#"{"name":"test"}"#).unwrap();

        let mut mock = MockCmd::new();
        mock.expect_run_command()
            .times(1)
            .returning(|_| Ok(CommandOutput::failure(1, b"ERR! 404 Not Found".to_vec())));

        let runner = JavaScriptRunner::with_runner(mock);
        let result = runner.install_deps(dir.path()).await;
        assert!(result.is_err());
        let msg = format!("{}", result.unwrap_err());
        assert!(msg.contains("install failed"), "unexpected: {msg}");
    }

    #[tokio::test]
    async fn install_deps_no_package_json_noop() {
        let dir = tempfile::tempdir().unwrap();

        let mock = MockCmd::new();
        // No expectations -- run_command should NOT be called.
        let runner = JavaScriptRunner::with_runner(mock);
        let result = runner.install_deps(dir.path()).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn run_tests_success() {
        let dir = tempfile::tempdir().unwrap();

        let mut mock = MockCmd::new();
        mock.expect_run_command()
            .withf(|spec| spec.program == "npx")
            .times(1)
            .returning(|_| Ok(CommandOutput::success(b"Tests: 5 passed".to_vec())));

        let runner = JavaScriptRunner::with_runner(mock);
        let result = runner.run_tests(dir.path(), &[]).await.unwrap();
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("5 passed"));
    }

    #[tokio::test]
    async fn run_tests_failure() {
        let dir = tempfile::tempdir().unwrap();

        let mut mock = MockCmd::new();
        mock.expect_run_command().times(1).returning(|_| {
            Ok(CommandOutput {
                exit_code: 1,
                stdout: b"Tests: 2 failed".to_vec(),
                stderr: b"FAIL src/app.test.js".to_vec(),
            })
        });

        let runner = JavaScriptRunner::with_runner(mock);
        let result = runner.run_tests(dir.path(), &[]).await.unwrap();
        assert_eq!(result.exit_code, 1);
        assert!(result.stdout.contains("2 failed"));
    }

    #[tokio::test]
    async fn run_tests_command_not_found() {
        let dir = tempfile::tempdir().unwrap();

        let mut mock = MockCmd::new();
        mock.expect_run_command().times(1).returning(|_| {
            Err(ApexError::Subprocess {
                exit_code: -1,
                stderr: "spawn npx: No such file or directory".into(),
            })
        });

        let runner = JavaScriptRunner::with_runner(mock);
        let result = runner.run_tests(dir.path(), &[]).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn run_tests_with_extra_args() {
        let dir = tempfile::tempdir().unwrap();

        let mut mock = MockCmd::new();
        mock.expect_run_command()
            .withf(|spec| spec.args.iter().any(|a| a == "--coverage"))
            .times(1)
            .returning(|_| Ok(CommandOutput::success(b"coverage output".to_vec())));

        let runner = JavaScriptRunner::with_runner(mock);
        let result = runner
            .run_tests(dir.path(), &["--coverage".into()])
            .await
            .unwrap();
        assert_eq!(result.exit_code, 0);
    }

    // ------------------------------------------------------------------
    // default() constructor
    // ------------------------------------------------------------------

    #[test]
    fn default_creates_runner() {
        let runner = JavaScriptRunner::default();
        assert_eq!(runner.language(), Language::JavaScript);
    }

    // ------------------------------------------------------------------
    // detect_test_runner — no package.json at all (String::new() path)
    // ------------------------------------------------------------------

    #[test]
    fn detect_test_runner_no_package_json() {
        let dir = tempfile::tempdir().unwrap();
        // No package.json, so pkg_content will be String::new()
        let (bin, args) = JavaScriptRunner::<RealCommandRunner>::detect_test_runner(dir.path());
        // Falls through to default: npx jest --passWithNoTests
        assert_eq!(bin, "npx");
        assert_eq!(args, vec!["jest", "--passWithNoTests"]);
    }

    // ------------------------------------------------------------------
    // install_deps — pnpm variant
    // ------------------------------------------------------------------

    #[tokio::test]
    async fn install_deps_pnpm_success() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("package.json"), r#"{"name":"test"}"#).unwrap();
        std::fs::write(dir.path().join("pnpm-lock.yaml"), "").unwrap();

        let mut mock = MockCmd::new();
        mock.expect_run_command()
            .withf(|spec| spec.program == "pnpm" && spec.args.contains(&"install".to_string()))
            .times(1)
            .returning(|_| Ok(CommandOutput::success(b"done".to_vec())));

        let runner = JavaScriptRunner::with_runner(mock);
        let result = runner.install_deps(dir.path()).await;
        assert!(result.is_ok());
    }

    // ------------------------------------------------------------------
    // install_deps — command spawn error
    // ------------------------------------------------------------------

    #[tokio::test]
    async fn install_deps_command_error() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("package.json"), r#"{"name":"test"}"#).unwrap();

        let mut mock = MockCmd::new();
        mock.expect_run_command().times(1).returning(|_| {
            Err(ApexError::Subprocess {
                exit_code: -1,
                stderr: "spawn npm: not found".into(),
            })
        });

        let runner = JavaScriptRunner::with_runner(mock);
        let result = runner.install_deps(dir.path()).await;
        assert!(result.is_err());
        let msg = format!("{}", result.unwrap_err());
        assert!(msg.contains("install"), "unexpected: {msg}");
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

        let runner = JavaScriptRunner::with_runner(mock);
        let result = runner.run_tests(dir.path(), &[]).await.unwrap();
        assert!(result.duration_ms < u64::MAX);
    }

    // ------------------------------------------------------------------
    // detect — nonexistent dir
    // ------------------------------------------------------------------

    #[test]
    fn detect_nonexistent_dir_returns_false() {
        let runner = JavaScriptRunner::new();
        assert!(!runner.detect(Path::new("/nonexistent/path/that/does/not/exist")));
    }

    // ------------------------------------------------------------------
    // detect_test_runner — jest takes priority over mocha
    // ------------------------------------------------------------------

    #[test]
    fn detect_test_runner_jest_over_mocha() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("package.json"),
            r#"{"devDependencies": {"jest": "^29", "mocha": "^10"}}"#,
        )
        .unwrap();
        let (bin, args) = JavaScriptRunner::<RealCommandRunner>::detect_test_runner(dir.path());
        assert_eq!(bin, "npx");
        assert_eq!(args[0], "jest");
    }

    // ------------------------------------------------------------------
    // detect_test_runner — mocha takes priority over vitest
    // ------------------------------------------------------------------

    #[test]
    fn detect_test_runner_mocha_over_vitest() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("package.json"),
            r#"{"devDependencies": {"mocha": "^10", "vitest": "^1"}}"#,
        )
        .unwrap();
        let (bin, args) = JavaScriptRunner::<RealCommandRunner>::detect_test_runner(dir.path());
        assert_eq!(bin, "npx");
        assert_eq!(args[0], "mocha");
    }

    // ------------------------------------------------------------------
    // run_tests — picks up test runner from package.json
    // ------------------------------------------------------------------

    #[tokio::test]
    async fn run_tests_uses_mocha_when_detected() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("package.json"),
            r#"{"devDependencies": {"mocha": "^10"}}"#,
        )
        .unwrap();

        let mut mock = MockCmd::new();
        mock.expect_run_command()
            .withf(|spec| spec.program == "npx" && spec.args.contains(&"mocha".to_string()))
            .times(1)
            .returning(|_| Ok(CommandOutput::success(b"2 passing".to_vec())));

        let runner = JavaScriptRunner::with_runner(mock);
        let result = runner.run_tests(dir.path(), &[]).await.unwrap();
        assert_eq!(result.exit_code, 0);
    }

    // ------------------------------------------------------------------
    // run_tests — npm test script path
    // ------------------------------------------------------------------

    #[tokio::test]
    async fn run_tests_uses_npm_test_when_detected() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("package.json"),
            r#"{"scripts": {"test": "node test.js"}}"#,
        )
        .unwrap();

        let mut mock = MockCmd::new();
        mock.expect_run_command()
            .withf(|spec| spec.program == "npm" && spec.args.contains(&"test".to_string()))
            .times(1)
            .returning(|_| Ok(CommandOutput::success(b"pass".to_vec())));

        let runner = JavaScriptRunner::with_runner(mock);
        let result = runner.run_tests(dir.path(), &[]).await.unwrap();
        assert_eq!(result.exit_code, 0);
    }

    // ------------------------------------------------------------------
    // run_tests — vitest path
    // ------------------------------------------------------------------

    #[tokio::test]
    async fn run_tests_uses_vitest_when_detected() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("package.json"),
            r#"{"devDependencies": {"vitest": "^1"}}"#,
        )
        .unwrap();

        let mut mock = MockCmd::new();
        mock.expect_run_command()
            .withf(|spec| {
                spec.program == "npx"
                    && spec.args.contains(&"vitest".to_string())
                    && spec.args.contains(&"run".to_string())
            })
            .times(1)
            .returning(|_| Ok(CommandOutput::success(b"pass".to_vec())));

        let runner = JavaScriptRunner::with_runner(mock);
        let result = runner.run_tests(dir.path(), &[]).await.unwrap();
        assert_eq!(result.exit_code, 0);
    }

    // ------------------------------------------------------------------
    // install_deps — yarn command error (spawn fails)
    // ------------------------------------------------------------------

    #[tokio::test]
    async fn install_deps_yarn_command_error() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("package.json"), r#"{"name":"test"}"#).unwrap();
        std::fs::write(dir.path().join("yarn.lock"), "").unwrap();

        let mut mock = MockCmd::new();
        mock.expect_run_command()
            .withf(|spec| spec.program == "yarn")
            .times(1)
            .returning(|_| {
                Err(ApexError::Subprocess {
                    exit_code: -1,
                    stderr: "spawn yarn: not found".into(),
                })
            });

        let runner = JavaScriptRunner::with_runner(mock);
        let result = runner.install_deps(dir.path()).await;
        assert!(result.is_err());
        let msg = format!("{}", result.unwrap_err());
        assert!(msg.contains("install"), "unexpected: {msg}");
    }

    // ------------------------------------------------------------------
    // install_deps — pnpm command error (spawn fails)
    // ------------------------------------------------------------------

    #[tokio::test]
    async fn install_deps_pnpm_command_error() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("package.json"), r#"{"name":"test"}"#).unwrap();
        std::fs::write(dir.path().join("pnpm-lock.yaml"), "").unwrap();

        let mut mock = MockCmd::new();
        mock.expect_run_command()
            .withf(|spec| spec.program == "pnpm")
            .times(1)
            .returning(|_| {
                Err(ApexError::Subprocess {
                    exit_code: -1,
                    stderr: "spawn pnpm: not found".into(),
                })
            });

        let runner = JavaScriptRunner::with_runner(mock);
        let result = runner.install_deps(dir.path()).await;
        assert!(result.is_err());
    }

    // ------------------------------------------------------------------
    // install_deps — pnpm nonzero exit → Err
    // ------------------------------------------------------------------

    #[tokio::test]
    async fn install_deps_pnpm_nonzero_exit_is_err() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("package.json"), r#"{"name":"test"}"#).unwrap();
        std::fs::write(dir.path().join("pnpm-lock.yaml"), "").unwrap();

        let mut mock = MockCmd::new();
        mock.expect_run_command()
            .withf(|spec| spec.program == "pnpm")
            .times(1)
            .returning(|_| {
                Ok(CommandOutput::failure(
                    1,
                    b"ERR_PNPM_META_FETCH_FAIL".to_vec(),
                ))
            });

        let runner = JavaScriptRunner::with_runner(mock);
        let result = runner.install_deps(dir.path()).await;
        assert!(result.is_err());
        let msg = format!("{}", result.unwrap_err());
        assert!(msg.contains("install failed"), "unexpected: {msg}");
    }

    // ------------------------------------------------------------------
    // install_deps — yarn nonzero exit → Err
    // ------------------------------------------------------------------

    #[tokio::test]
    async fn install_deps_yarn_nonzero_exit_is_err() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("package.json"), r#"{"name":"test"}"#).unwrap();
        std::fs::write(dir.path().join("yarn.lock"), "").unwrap();

        let mut mock = MockCmd::new();
        mock.expect_run_command()
            .withf(|spec| spec.program == "yarn")
            .times(1)
            .returning(|_| {
                Ok(CommandOutput::failure(
                    1,
                    b"error Couldn't find package".to_vec(),
                ))
            });

        let runner = JavaScriptRunner::with_runner(mock);
        let result = runner.install_deps(dir.path()).await;
        assert!(result.is_err());
        let msg = format!("{}", result.unwrap_err());
        assert!(msg.contains("install failed"), "unexpected: {msg}");
    }

    // ------------------------------------------------------------------
    // detect_test_runner — package.json read error falls back to String::new()
    // ------------------------------------------------------------------

    #[test]
    fn detect_test_runner_package_json_exists_but_empty() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("package.json"), "").unwrap();
        // Empty file → no jest/mocha/vitest/scripts found → default fallback
        let (bin, args) = JavaScriptRunner::<RealCommandRunner>::detect_test_runner(dir.path());
        assert_eq!(bin, "npx");
        assert_eq!(args[0], "jest");
    }

    #[test]
    fn detect_package_manager_bun() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("bun.lockb"), "").unwrap();
        assert_eq!(
            JavaScriptRunner::<RealCommandRunner>::detect_package_manager(dir.path()),
            "bun"
        );
    }

    // ------------------------------------------------------------------
    // Task 12: Deno detection
    // ------------------------------------------------------------------

    #[test]
    fn detect_deno_project() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("deno.json"), "{}").unwrap();
        assert!(JavaScriptRunner::new().detect(dir.path()));
    }

    #[test]
    fn detect_deno_project_jsonc() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("deno.jsonc"), "{}").unwrap();
        assert!(JavaScriptRunner::new().detect(dir.path()));
    }

    #[test]
    fn detect_package_manager_deno() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("deno.json"), "{}").unwrap();
        assert_eq!(
            JavaScriptRunner::<RealCommandRunner>::detect_package_manager(dir.path()),
            "deno"
        );
    }

    #[test]
    fn detect_test_runner_deno() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("deno.json"), "{}").unwrap();
        let (bin, args) = JavaScriptRunner::<RealCommandRunner>::detect_test_runner(dir.path());
        assert_eq!(bin, "deno");
        assert_eq!(args, vec!["test"]);
    }

    // ------------------------------------------------------------------
    // preflight_check tests
    // ------------------------------------------------------------------

    #[test]
    fn preflight_check_basic_npm() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("package.json"), r#"{"name":"test"}"#).unwrap();
        let runner = JavaScriptRunner::new();
        let info = runner.preflight_check(dir.path()).unwrap();
        assert_eq!(info.package_manager.as_deref(), Some("npm"));
        assert!(info.test_framework.is_some());
    }

    #[test]
    fn preflight_check_deps_installed_with_node_modules() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("node_modules")).unwrap();
        let runner = JavaScriptRunner::new();
        let info = runner.preflight_check(dir.path()).unwrap();
        assert!(info.deps_installed);
    }

    #[test]
    fn preflight_check_deps_not_installed() {
        let dir = tempfile::tempdir().unwrap();
        let runner = JavaScriptRunner::new();
        let info = runner.preflight_check(dir.path()).unwrap();
        assert!(!info.deps_installed);
    }

    #[test]
    fn preflight_check_detects_typescript() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("tsconfig.json"), "{}").unwrap();
        let runner = JavaScriptRunner::new();
        let info = runner.preflight_check(dir.path()).unwrap();
        assert!(info.extra.iter().any(|(k, v)| k == "typescript" && v == "true"));
    }

    #[test]
    fn preflight_check_detects_lerna_monorepo() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("lerna.json"), "{}").unwrap();
        let runner = JavaScriptRunner::new();
        let info = runner.preflight_check(dir.path()).unwrap();
        assert!(info.extra.iter().any(|(k, v)| k == "monorepo" && v == "lerna"));
    }

    #[test]
    fn preflight_check_detects_nx_monorepo() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("nx.json"), "{}").unwrap();
        let runner = JavaScriptRunner::new();
        let info = runner.preflight_check(dir.path()).unwrap();
        assert!(info.extra.iter().any(|(k, v)| k == "monorepo" && v == "nx"));
    }

    #[test]
    fn preflight_check_detects_turborepo() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("turbo.json"), "{}").unwrap();
        let runner = JavaScriptRunner::new();
        let info = runner.preflight_check(dir.path()).unwrap();
        assert!(info.extra.iter().any(|(k, v)| k == "monorepo" && v == "turborepo"));
    }

    #[test]
    fn preflight_check_detects_pnpm_workspaces() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("pnpm-workspace.yaml"), "").unwrap();
        let runner = JavaScriptRunner::new();
        let info = runner.preflight_check(dir.path()).unwrap();
        assert!(
            info.extra
                .iter()
                .any(|(k, v)| k == "monorepo" && v == "pnpm-workspaces")
        );
    }

    #[test]
    fn preflight_check_detects_npm_workspaces() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("package.json"),
            r#"{"name":"root","workspaces":["packages/*"]}"#,
        )
        .unwrap();
        let runner = JavaScriptRunner::new();
        let info = runner.preflight_check(dir.path()).unwrap();
        assert!(
            info.extra
                .iter()
                .any(|(k, v)| k == "monorepo" && v == "npm-workspaces")
        );
    }

    #[test]
    fn detect_monorepo_none() {
        let dir = tempfile::tempdir().unwrap();
        assert!(JavaScriptRunner::<RealCommandRunner>::detect_monorepo(dir.path()).is_none());
    }

    #[test]
    fn preflight_check_yarn_pm() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("yarn.lock"), "").unwrap();
        let runner = JavaScriptRunner::new();
        let info = runner.preflight_check(dir.path()).unwrap();
        assert_eq!(info.package_manager.as_deref(), Some("yarn"));
    }
}
