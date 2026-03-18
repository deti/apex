use apex_core::{
    command::{CommandRunner, CommandSpec, RealCommandRunner},
    error::{ApexError, Result},
    traits::{LanguageRunner, TestRunOutput},
    types::Language,
};
use async_trait::async_trait;
use std::{path::Path, sync::OnceLock, time::Instant};
use tracing::{debug, info, warn};

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

    /// Resolve a modern Ruby binary (>= 3.0).
    ///
    /// Search order:
    /// 1. `ruby` on PATH — if version >= 3.0, use it.
    /// 2. Homebrew Ruby at `/opt/homebrew/bin/ruby` (Apple Silicon) or
    ///    `/usr/local/bin/ruby` (Intel Mac).
    /// 3. rbenv shim at `~/.rbenv/shims/ruby`.
    /// 4. asdf shim at `~/.asdf/shims/ruby`.
    /// 5. Falls back to `ruby` (will fail later with a clear error).
    pub fn resolve_ruby() -> &'static str {
        static RUBY: OnceLock<&'static str> = OnceLock::new();
        RUBY.get_or_init(|| {
            let candidates: &[&str] = &[
                "ruby",
                "/opt/homebrew/bin/ruby",
                "/usr/local/bin/ruby",
            ];
            // Also check rbenv / asdf shims via home dir
            let home_candidates: Vec<String> = if let Some(home) = std::env::var_os("HOME") {
                let home = std::path::Path::new(&home);
                vec![
                    home.join(".rbenv/shims/ruby").to_string_lossy().to_string(),
                    home.join(".asdf/shims/ruby").to_string_lossy().to_string(),
                ]
            } else {
                Vec::new()
            };

            for candidate in candidates
                .iter()
                .copied()
                .map(String::from)
                .chain(home_candidates)
            {
                if let Some(ver) = Self::ruby_version(&candidate) {
                    if ver >= (3, 0) {
                        info!(ruby = %candidate, version = ?ver, "found modern Ruby");
                        // Leak the string so we can return &'static str
                        return Box::leak(candidate.into_boxed_str());
                    }
                    debug!(ruby = %candidate, version = ?ver, "Ruby too old (< 3.0), skipping");
                }
            }
            warn!("no Ruby >= 3.0 found; falling back to system `ruby`");
            "ruby"
        })
    }

    /// Run `ruby --version` and parse the major.minor version tuple.
    fn ruby_version(ruby_bin: &str) -> Option<(u32, u32)> {
        let output = std::process::Command::new(ruby_bin)
            .arg("--version")
            .output()
            .ok()?;
        if !output.status.success() {
            return None;
        }
        // Output looks like: "ruby 3.2.2 (2023-03-30 revision e51014f9c0) [arm64-darwin22]"
        let stdout = String::from_utf8_lossy(&output.stdout);
        let version_str = stdout.split_whitespace().nth(1)?;
        let mut parts = version_str.split('.');
        let major: u32 = parts.next()?.parse().ok()?;
        let minor: u32 = parts.next()?.parse().ok()?;
        Some((major, minor))
    }

    /// Resolve the `gem` binary corresponding to our chosen Ruby.
    fn resolve_gem() -> &'static str {
        static GEM: OnceLock<&'static str> = OnceLock::new();
        GEM.get_or_init(|| {
            let ruby = Self::resolve_ruby();
            if ruby == "ruby" {
                return "gem";
            }
            // If ruby is e.g. /opt/homebrew/bin/ruby, try /opt/homebrew/bin/gem
            let ruby_path = std::path::Path::new(ruby);
            if let Some(dir) = ruby_path.parent() {
                let gem_path = dir.join("gem");
                if gem_path.exists() {
                    return Box::leak(gem_path.to_string_lossy().into_owned().into_boxed_str());
                }
            }
            "gem"
        })
    }

    /// Resolve the `bundle` binary corresponding to our chosen Ruby.
    fn resolve_bundle() -> &'static str {
        static BUNDLE: OnceLock<&'static str> = OnceLock::new();
        BUNDLE.get_or_init(|| {
            let ruby = Self::resolve_ruby();
            if ruby == "ruby" {
                return "bundle";
            }
            let ruby_path = std::path::Path::new(ruby);
            if let Some(dir) = ruby_path.parent() {
                let bundle_path = dir.join("bundle");
                if bundle_path.exists() {
                    return Box::leak(
                        bundle_path.to_string_lossy().into_owned().into_boxed_str(),
                    );
                }
            }
            "bundle"
        })
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

    /// Read the `BUNDLED WITH` section from `Gemfile.lock` to detect the
    /// required bundler version. Returns `None` if no lock file or no
    /// `BUNDLED WITH` section exists.
    fn detect_required_bundler(target: &Path) -> Option<String> {
        let lock_path = target.join("Gemfile.lock");
        let content = std::fs::read_to_string(lock_path).ok()?;
        let mut lines = content.lines();
        while let Some(line) = lines.next() {
            if line.trim() == "BUNDLED WITH" {
                // Next line contains the version
                if let Some(version_line) = lines.next() {
                    let version = version_line.trim();
                    if !version.is_empty() {
                        return Some(version.to_string());
                    }
                }
            }
        }
        None
    }

    fn detect_test_runner(target: &Path) -> Vec<String> {
        let use_mise = Self::resolve_mise().is_some() && Self::has_mise_config(target);
        let ruby_bin = Self::resolve_ruby().to_string();
        let bundle_bin = Self::resolve_bundle().to_string();

        if target.join("spec").exists() || target.join(".rspec").exists() {
            let base = vec![bundle_bin, "exec".into(), "rspec".into()];
            if use_mise {
                let mut cmd = vec!["mise".into(), "exec".into(), "ruby".into(), "--".into()];
                cmd.extend(base);
                cmd
            } else {
                base
            }
        } else {
            let base = vec![
                ruby_bin,
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
        let ruby_bin = Self::resolve_ruby();
        let gem_bin = Self::resolve_gem();
        let bundle_bin = Self::resolve_bundle();
        info!(
            target = %target.display(),
            ruby = ruby_bin,
            "installing Ruby dependencies"
        );

        if target.join("Gemfile").exists() {
            // Ensure bundler is installed / up-to-date before running bundle install.
            // This avoids "Could not find 'bundler' (x.y.z)" when the Gemfile.lock
            // pins a newer bundler than what the system ships with.
            let bundler_spec = Self::detect_required_bundler(target);
            let gem_install_args = match &bundler_spec {
                Some(version) => {
                    info!(version = %version, "installing pinned bundler version");
                    vec!["install", "bundler", "-v", version.as_str()]
                }
                None => {
                    debug!("no specific bundler version required, installing latest");
                    vec!["install", "bundler"]
                }
            };
            let spec = CommandSpec::new(gem_bin, target)
                .args(gem_install_args)
                .timeout(120_000); // 2 min for gem install
            let output = self
                .runner
                .run_command(&spec)
                .await
                .map_err(|e| ApexError::LanguageRunner(format!("gem install bundler: {e}")))?;
            if output.exit_code != 0 {
                let stderr = String::from_utf8_lossy(&output.stderr).to_string();
                warn!(stderr = %stderr, "gem install bundler failed (non-fatal, trying bundle install anyway)");
            }

            // bundle install with 5 minute timeout (large Gemfiles can be slow)
            let spec = CommandSpec::new(bundle_bin, target)
                .args(["install"])
                .timeout(300_000);
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

        // Ensure simplecov is available for coverage instrumentation
        let check = CommandSpec::new(ruby_bin, target).args(["-e", "require 'simplecov'"]);
        let check_result = self
            .runner
            .run_command(&check)
            .await
            .map_err(|e| ApexError::LanguageRunner(e.to_string()))?;
        if check_result.exit_code != 0 {
            debug!("simplecov not found, installing simplecov + simplecov-json");
            let spec = CommandSpec::new(gem_bin, target)
                .args(["install", "simplecov", "simplecov-json"])
                .timeout(120_000);
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
        // First element should contain "bundle" (possibly full path)
        assert!(cmd[0].contains("bundle"));
    }

    #[test]
    fn detect_test_runner_minitest_fallback() {
        let dir = tempfile::tempdir().unwrap();
        let cmd = RubyRunner::<RealCommandRunner>::detect_test_runner(dir.path());
        // The ruby binary might be a full path like /opt/homebrew/bin/ruby
        assert!(cmd.iter().any(|c| c.contains("ruby")));
        assert!(cmd.contains(&"-Ilib".to_string()));
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
        // 1. gem install bundler
        mock.expect_run_command()
            .withf(|spec| {
                spec.program.contains("gem") && spec.args.contains(&"bundler".to_string())
            })
            .times(1)
            .returning(|_| Ok(CommandOutput::success(b"".to_vec())));
        // 2. bundle install
        mock.expect_run_command()
            .withf(|spec| {
                spec.program.contains("bundle") && spec.args.contains(&"install".to_string())
            })
            .times(1)
            .returning(|_| Ok(CommandOutput::success(b"".to_vec())));
        // 3. ruby -e "require 'simplecov'" check
        mock.expect_run_command()
            .withf(|spec| {
                spec.program.contains("ruby") && spec.args.contains(&"-e".to_string())
            })
            .times(1)
            .returning(|_| Ok(CommandOutput::success(b"".to_vec())));
        let runner = RubyRunner::with_runner(mock);
        assert!(runner.install_deps(dir.path()).await.is_ok());
    }

    #[tokio::test]
    async fn install_deps_installs_simplecov_when_missing() {
        let dir = tempfile::tempdir().unwrap();
        // No Gemfile — skip bundle install, but simplecov check fails
        let mut mock = MockCmd::new();
        // 1. ruby -e "require 'simplecov'" fails
        mock.expect_run_command()
            .withf(|spec| {
                spec.program.contains("ruby") && spec.args.contains(&"-e".to_string())
            })
            .times(1)
            .returning(|_| {
                Ok(CommandOutput {
                    exit_code: 1,
                    stdout: Vec::new(),
                    stderr: b"LoadError: cannot load such file -- simplecov".to_vec(),
                })
            });
        // 2. gem install simplecov simplecov-json
        mock.expect_run_command()
            .withf(|spec| {
                spec.program.contains("gem")
                    && spec.args.contains(&"simplecov".to_string())
                    && spec.args.contains(&"simplecov-json".to_string())
            })
            .times(1)
            .returning(|_| Ok(CommandOutput::success(b"".to_vec())));
        let runner = RubyRunner::with_runner(mock);
        assert!(runner.install_deps(dir.path()).await.is_ok());
    }

    #[tokio::test]
    async fn install_deps_reads_bundled_with_version() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("Gemfile"), "").unwrap();
        std::fs::write(
            dir.path().join("Gemfile.lock"),
            "GEM\n  specs:\n\nBUNDLED WITH\n   4.0.6\n",
        )
        .unwrap();
        let mut mock = MockCmd::new();
        // gem install bundler -v 4.0.6
        mock.expect_run_command()
            .withf(|spec| {
                spec.program.contains("gem")
                    && spec.args.contains(&"-v".to_string())
                    && spec.args.contains(&"4.0.6".to_string())
            })
            .times(1)
            .returning(|_| Ok(CommandOutput::success(b"".to_vec())));
        mock.expect_run_command()
            .withf(|spec| {
                spec.program.contains("bundle") && spec.args.contains(&"install".to_string())
            })
            .times(1)
            .returning(|_| Ok(CommandOutput::success(b"".to_vec())));
        mock.expect_run_command()
            .withf(|spec| {
                spec.program.contains("ruby") && spec.args.contains(&"-e".to_string())
            })
            .times(1)
            .returning(|_| Ok(CommandOutput::success(b"".to_vec())));
        let runner = RubyRunner::with_runner(mock);
        assert!(runner.install_deps(dir.path()).await.is_ok());
    }

    #[test]
    fn detect_required_bundler_parses_lockfile() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("Gemfile.lock"),
            "GEM\n  remote: https://rubygems.org/\n  specs:\n    rspec (3.12)\n\nBUNDLED WITH\n   4.0.6\n",
        )
        .unwrap();
        let version =
            RubyRunner::<RealCommandRunner>::detect_required_bundler(dir.path());
        assert_eq!(version.as_deref(), Some("4.0.6"));
    }

    #[test]
    fn detect_required_bundler_no_lockfile() {
        let dir = tempfile::tempdir().unwrap();
        let version =
            RubyRunner::<RealCommandRunner>::detect_required_bundler(dir.path());
        assert!(version.is_none());
    }

    #[test]
    fn detect_required_bundler_no_section() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("Gemfile.lock"),
            "GEM\n  specs:\n    rspec (3.12)\n",
        )
        .unwrap();
        let version =
            RubyRunner::<RealCommandRunner>::detect_required_bundler(dir.path());
        assert!(version.is_none());
    }

    #[test]
    fn resolve_ruby_returns_string() {
        // Smoke test: resolve_ruby should return something (even if just "ruby")
        let ruby = RubyRunner::<RealCommandRunner>::resolve_ruby();
        assert!(!ruby.is_empty());
    }
}
