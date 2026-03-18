use apex_core::{
    command::{CommandRunner, CommandSpec, RealCommandRunner},
    error::{ApexError, Result},
    traits::{LanguageRunner, TestRunOutput},
    types::Language,
};
use async_trait::async_trait;
use std::{path::Path, time::Instant};
use tracing::{debug, info};

pub struct RubyRunner<R: CommandRunner = RealCommandRunner> {
    runner: R,
}

impl RubyRunner {
    pub fn new() -> Self {
        RubyRunner {
            runner: RealCommandRunner,
        }
    }
}

impl<R: CommandRunner> RubyRunner<R> {
    pub fn with_runner(runner: R) -> Self {
        RubyRunner { runner }
    }

    /// Returns `Some("mise")` when mise is available on PATH, `None` otherwise.
    fn resolve_mise() -> Option<String> {
        std::process::Command::new("mise")
            .arg("--version")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .ok()
            .filter(|s| s.success())
            .map(|_| "mise".to_string())
    }

    /// Returns true when a mise config file is present in `target`.
    fn has_mise_config(target: &Path) -> bool {
        target.join(".mise.toml").exists() || target.join(".tool-versions").exists()
    }

    fn detect_test_runner(target: &Path) -> Vec<String> {
        let use_mise = Self::resolve_mise().is_some() && Self::has_mise_config(target);

        if target.join("spec").exists() || target.join(".rspec").exists() {
            let base = vec!["bundle".into(), "exec".into(), "rspec".into()];
            if use_mise {
                let mut cmd = vec!["mise".into(), "exec".into(), "ruby".into(), "--".into()];
                cmd.extend(base);
                cmd
            } else {
                base
            }
        } else {
            let base = vec![
                "ruby".into(),
                "-Ilib".into(),
                "-Itest".into(),
                "-e".into(),
                "Dir.glob('test/**/test_*.rb').each{|f| require(File.expand_path(f))}".into(),
            ];
            if use_mise {
                let mut cmd = vec!["mise".into(), "exec".into(), "ruby".into(), "--".into()];
                cmd.extend(base);
                cmd
            } else {
                base
            }
        }
    }
}

impl Default for RubyRunner {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl<R: CommandRunner> LanguageRunner for RubyRunner<R> {
    fn language(&self) -> Language {
        Language::Ruby
    }

    fn detect(&self, target: &Path) -> bool {
        target.join("Gemfile").exists()
            || target.join("Rakefile").exists()
            || std::fs::read_dir(target)
                .map(|entries| {
                    entries.flatten().any(|e| {
                        e.path()
                            .extension()
                            .map(|ext| ext == "gemspec")
                            .unwrap_or(false)
                    })
                })
                .unwrap_or(false)
    }

    async fn install_deps(&self, target: &Path) -> Result<()> {
        info!(target = %target.display(), "installing Ruby dependencies");
        if target.join("Gemfile").exists() {
            let spec = CommandSpec::new("bundle", target).args(["install"]);
            let output = self
                .runner
                .run_command(&spec)
                .await
                .map_err(|e| ApexError::LanguageRunner(format!("bundle install: {e}")))?;
            if output.exit_code != 0 {
                let stderr = String::from_utf8_lossy(&output.stderr).to_string();
                return Err(ApexError::LanguageRunner(format!(
                    "bundle install failed: {stderr}"
                )));
            }
        }
        // Ensure simplecov is available for coverage
        let check = CommandSpec::new("ruby", target).args(["-e", "require 'simplecov'"]);
        let check_result = self
            .runner
            .run_command(&check)
            .await
            .map_err(|e| ApexError::LanguageRunner(e.to_string()))?;
        if check_result.exit_code != 0 {
            debug!("simplecov not found, installing");
            let spec =
                CommandSpec::new("gem", target).args(["install", "simplecov", "simplecov-json"]);
            let output = self
                .runner
                .run_command(&spec)
                .await
                .map_err(|e| ApexError::LanguageRunner(e.to_string()))?;
            if output.exit_code != 0 {
                let stderr = String::from_utf8_lossy(&output.stderr).to_string();
                return Err(ApexError::LanguageRunner(format!(
                    "gem install simplecov failed: {stderr}"
                )));
            }
        }
        Ok(())
    }

    async fn run_tests(&self, target: &Path, extra_args: &[String]) -> Result<TestRunOutput> {
        let cmd_parts = Self::detect_test_runner(target);
        let mut args: Vec<String> = cmd_parts[1..].to_vec();
        args.extend(extra_args.iter().cloned());
        info!(target = %target.display(), cmd = ?cmd_parts, "running Ruby tests");
        let spec = CommandSpec::new(&cmd_parts[0], target).args(args);
        let start = Instant::now();
        let output = self
            .runner
            .run_command(&spec)
            .await
            .map_err(|e| ApexError::LanguageRunner(format!("run tests: {e}")))?;
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
    mockall::mock! {
        Cmd {}
        #[async_trait]
        impl CommandRunner for Cmd {
            async fn run_command(&self, spec: &CommandSpec) -> Result<CommandOutput>;
        }
    }

    #[test]
    fn detect_gemfile() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("Gemfile"),
            "source 'https://rubygems.org'\n",
        )
        .unwrap();
        assert!(RubyRunner::new().detect(dir.path()));
    }

    #[test]
    fn detect_rakefile() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("Rakefile"), "task :test\n").unwrap();
        assert!(RubyRunner::new().detect(dir.path()));
    }

    #[test]
    fn detect_empty_dir_returns_false() {
        let dir = tempfile::tempdir().unwrap();
        assert!(!RubyRunner::new().detect(dir.path()));
    }

    #[test]
    fn detect_test_runner_rspec() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("spec")).unwrap();
        let cmd = RubyRunner::<RealCommandRunner>::detect_test_runner(dir.path());
        assert!(cmd.contains(&"rspec".to_string()));
    }

    #[test]
    fn detect_test_runner_minitest_fallback() {
        let dir = tempfile::tempdir().unwrap();
        let cmd = RubyRunner::<RealCommandRunner>::detect_test_runner(dir.path());
        assert!(cmd.contains(&"ruby".to_string()));
    }

    #[test]
    fn language_is_ruby() {
        assert_eq!(RubyRunner::new().language(), Language::Ruby);
    }

    #[test]
    fn resolve_mise_returns_option() {
        // This is a probe test: mise may or may not be installed in CI.
        // We only assert that the return type is Option<String> and that
        // if Some is returned it equals "mise".
        let result = RubyRunner::<RealCommandRunner>::resolve_mise();
        if let Some(ref val) = result {
            assert_eq!(val, "mise");
        }
        // Either None (mise not on PATH) or Some("mise") — both valid.
        assert!(result.is_none() || result.as_deref() == Some("mise"));
    }

    #[test]
    fn mise_prefix_when_config_present() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("spec")).unwrap();
        std::fs::write(dir.path().join(".mise.toml"), "[tools]\nruby = '3.2'\n").unwrap();

        // When mise is NOT on PATH, no prefix is added (correct fallback).
        // When mise IS on PATH, the command starts with ["mise", "exec", "ruby", "--"].
        let cmd = RubyRunner::<RealCommandRunner>::detect_test_runner(dir.path());
        let has_mise = RubyRunner::<RealCommandRunner>::resolve_mise().is_some();
        if has_mise {
            assert_eq!(&cmd[..4], &["mise", "exec", "ruby", "--"]);
            assert!(cmd.contains(&"rspec".to_string()));
        } else {
            // Fallback path: no mise prefix
            assert!(cmd.contains(&"rspec".to_string()));
            assert!(!cmd.contains(&"mise".to_string()));
        }
    }

    #[test]
    fn no_mise_prefix_when_no_config() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("spec")).unwrap();
        // No .mise.toml or .tool-versions — mise prefix must not be applied.

        let cmd = RubyRunner::<RealCommandRunner>::detect_test_runner(dir.path());
        assert!(!cmd.contains(&"mise".to_string()));
        assert!(cmd.contains(&"rspec".to_string()));
    }

    #[tokio::test]
    async fn install_deps_bundle_success() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("Gemfile"), "").unwrap();
        let mut mock = MockCmd::new();
        mock.expect_run_command()
            .withf(|spec| spec.program == "bundle")
            .times(1)
            .returning(|_| Ok(CommandOutput::success(b"".to_vec())));
        mock.expect_run_command()
            .withf(|spec| spec.program == "ruby")
            .times(1)
            .returning(|_| Ok(CommandOutput::success(b"".to_vec())));
        let runner = RubyRunner::with_runner(mock);
        assert!(runner.install_deps(dir.path()).await.is_ok());
    }
}
