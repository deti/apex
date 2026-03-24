use apex_core::{
    command::{
        adaptive_timeout, count_source_files, CommandRunner, CommandSpec, OpKind, RealCommandRunner,
    },
    error::{ApexError, Result},
    hash::fnv1a_hash,
    traits::Instrumentor,
    types::{BranchId, InstrumentedTarget, Language, Target},
};
use apex_lang::ruby::RubyRunner;
use async_trait::async_trait;
use serde::Deserialize;
use std::{collections::HashMap, path::Path, path::PathBuf, sync::Arc};
use tracing::{debug, info, warn};

pub struct RubyInstrumentor {
    runner: Arc<dyn CommandRunner>,
}

impl RubyInstrumentor {
    pub fn new() -> Self {
        RubyInstrumentor {
            runner: Arc::new(RealCommandRunner),
        }
    }
    pub fn with_runner(runner: Arc<dyn CommandRunner>) -> Self {
        RubyInstrumentor { runner }
    }
}

impl Default for RubyInstrumentor {
    fn default() -> Self {
        Self::new()
    }
}

// SimpleCov JSON format
#[derive(Debug, Deserialize)]
struct SimpleCovJson {
    coverage: HashMap<String, FileCoverage>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum FileCoverage {
    Lines(LineCoverage),
    Detailed(DetailedCoverage),
}

#[derive(Debug, Deserialize)]
struct LineCoverage {
    lines: Vec<Option<u64>>,
}

#[derive(Debug, Deserialize)]
struct DetailedCoverage {
    lines: Vec<Option<u64>>,
}

/// Parse SimpleCov JSON output into branch IDs.
pub fn parse_simplecov_json(json: &str) -> (Vec<BranchId>, Vec<BranchId>, HashMap<u64, PathBuf>) {
    let mut all_branches = Vec::new();
    let mut executed = Vec::new();
    let mut file_paths = HashMap::new();

    let data: SimpleCovJson = match serde_json::from_str(json) {
        Ok(d) => d,
        Err(e) => {
            warn!(error = %e, "failed to parse SimpleCov JSON");
            return (all_branches, executed, file_paths);
        }
    };

    for (file_path, coverage) in &data.coverage {
        let file_id = fnv1a_hash(file_path);
        file_paths.insert(file_id, PathBuf::from(file_path));

        let lines = match coverage {
            FileCoverage::Lines(lc) => &lc.lines,
            FileCoverage::Detailed(dc) => &dc.lines,
        };

        for (i, count) in lines.iter().enumerate() {
            if let Some(c) = count {
                let line = (i + 1) as u32;
                let branch = BranchId::new(file_id, line, 0, 0);
                all_branches.push(branch.clone());
                if *c > 0 {
                    executed.push(branch);
                }
            }
            // None = non-executable line, skip
        }
    }

    (all_branches, executed, file_paths)
}

/// Resolve the Ruby binary using the lang runner's resolution logic.
///
/// Checks: Homebrew, rbenv, asdf, then system `ruby`.
fn resolve_ruby() -> &'static str {
    RubyRunner::<RealCommandRunner>::resolve_ruby()
}

/// Check if `bundle` is available on PATH.
fn has_bundler() -> bool {
    std::process::Command::new("bundle")
        .arg("--version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .ok()
        .is_some_and(|s| s.success())
}

/// Detect the test framework and return an appropriate test command.
///
/// - If `spec/` exists -> RSpec (`bundle exec rspec`)
/// - If `test/` exists -> Minitest (`bundle exec rake test`)
/// - Fallback: `ruby -Ilib -Itest`
fn detect_test_command(target_path: &Path, use_bundler: bool) -> Vec<String> {
    let spec_dir = target_path.join("spec");
    let test_dir = target_path.join("test");

    if spec_dir.exists() {
        if use_bundler {
            vec!["bundle".into(), "exec".into(), "rspec".into()]
        } else {
            vec!["rspec".into()]
        }
    } else if test_dir.exists() {
        if use_bundler {
            vec!["bundle".into(), "exec".into(), "rake".into(), "test".into()]
        } else {
            vec!["rake".into(), "test".into()]
        }
    } else {
        let ruby = resolve_ruby().to_string();
        vec![ruby, "-Ilib".into(), "-Itest".into()]
    }
}

/// SimpleCov helper script content.
///
/// Written to `.apex_coverage_helper.rb` at the target root before test execution.
/// This is more robust than a one-liner `-e` script because it handles
/// `simplecov-json` formatter setup and output directory configuration.
const SIMPLECOV_HELPER: &str = r#"require 'simplecov'
begin
  require 'simplecov-json'
  SimpleCov.formatters = SimpleCov::Formatter::MultiFormatter.new([
    SimpleCov::Formatter::HTMLFormatter,
    SimpleCov::Formatter::JSONFormatter,
  ])
rescue LoadError
  # simplecov-json not available, fall back to default formatter
end
SimpleCov.start do
  coverage_dir 'coverage'
  add_filter '/test/'
  add_filter '/spec/'
  add_filter '/vendor/'
end
"#;

/// Ensure simplecov and simplecov-json gems are available.
///
/// If bundler is present, runs `bundle exec gem list` to check.
/// If missing, attempts `gem install`.
fn ensure_simplecov_gems(target_path: &Path, use_bundler: bool) {
    // Check if simplecov is available
    let check = if use_bundler {
        std::process::Command::new("bundle")
            .args(["exec", "ruby", "-e", "require 'simplecov'"])
            .current_dir(target_path)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
    } else {
        let ruby = resolve_ruby();
        std::process::Command::new(ruby)
            .args(["-e", "require 'simplecov'"])
            .current_dir(target_path)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
    };

    let simplecov_available = check.ok().is_some_and(|s| s.success());

    if !simplecov_available {
        info!("simplecov not found, attempting gem install");
        let install_cmd = if use_bundler { "bundle" } else { "gem" };
        let args: Vec<&str> = if use_bundler {
            vec!["exec", "gem", "install", "simplecov", "simplecov-json"]
        } else {
            vec!["install", "simplecov", "simplecov-json"]
        };
        let result = std::process::Command::new(install_cmd)
            .args(&args)
            .current_dir(target_path)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();
        match result {
            Ok(s) if s.success() => info!("installed simplecov gems"),
            Ok(s) => warn!(exit = s.code(), "gem install simplecov returned non-zero"),
            Err(e) => warn!(error = %e, "failed to install simplecov"),
        }
    }
}

#[async_trait]
impl Instrumentor for RubyInstrumentor {
    fn branch_ids(&self) -> &[BranchId] {
        &[]
    }

    async fn instrument(&self, target: &Target) -> Result<InstrumentedTarget> {
        let target_path = &target.root;
        info!(target = %target_path.display(), "instrumenting Ruby project with SimpleCov");

        // Preflight: count source files for adaptive timeout
        let file_count = count_source_files(target_path);
        let timeout = adaptive_timeout(file_count, Language::Ruby, OpKind::TestRun);
        debug!(
            file_count,
            timeout_ms = timeout,
            "adaptive timeout for Ruby test run"
        );

        // Resolve Ruby binary via lang runner
        let ruby = resolve_ruby();
        debug!(ruby_bin = ruby, "resolved Ruby binary");

        // Check bundler availability
        let use_bundler = has_bundler();
        debug!(use_bundler, "bundler availability");

        // Ensure simplecov gems are installed
        ensure_simplecov_gems(target_path, use_bundler);

        // Write the SimpleCov helper script
        let helper_path = target_path.join(".apex_coverage_helper.rb");
        std::fs::write(&helper_path, SIMPLECOV_HELPER)
            .map_err(|e| ApexError::Instrumentation(format!("write helper script: {e}")))?;

        // Detect test framework (RSpec vs Minitest vs fallback)
        let test_cmd = if target.test_command.is_empty() {
            detect_test_command(target_path, use_bundler)
        } else {
            target.test_command.clone()
        };

        info!(cmd = ?test_cmd, "detected test command");

        // Build the command: run tests with SimpleCov loaded via -r flag
        let (program, args) = if test_cmd.first().map(|s| s.as_str()) == Some("bundle") {
            // bundle exec ... with RUBYOPT to load helper
            let args: Vec<String> = test_cmd[1..].to_vec();
            ("bundle".to_string(), args)
        } else {
            // Direct ruby command or other runner
            (test_cmd[0].clone(), test_cmd[1..].to_vec())
        };

        let spec = CommandSpec::new(&program, target_path)
            .args(args)
            .env("RUBYOPT", format!("-r{}", helper_path.to_string_lossy()))
            .timeout(timeout);

        let output = self
            .runner
            .run_command(&spec)
            .await
            .map_err(|e| ApexError::Instrumentation(format!("ruby simplecov: {e}")))?;

        if output.exit_code != 0 {
            warn!(
                exit = output.exit_code,
                "ruby test run returned non-zero (coverage data may still be valid)"
            );
        }

        // Clean up helper script (best-effort)
        let _ = std::fs::remove_file(&helper_path);

        // Try to read SimpleCov JSON output
        let json_path = target_path.join("coverage").join(".resultset.json");
        let alt_path = target_path.join("coverage").join("coverage.json");

        let json_content = if json_path.exists() {
            std::fs::read_to_string(&json_path)
                .map_err(|e| ApexError::Instrumentation(e.to_string()))?
        } else if alt_path.exists() {
            std::fs::read_to_string(&alt_path)
                .map_err(|e| ApexError::Instrumentation(e.to_string()))?
        } else {
            return Err(ApexError::Instrumentation(
                "SimpleCov JSON not found at coverage/.resultset.json or coverage/coverage.json; \
                 is simplecov installed and configured?"
                    .into(),
            ));
        };

        let (branch_ids, executed_branch_ids, file_paths) = parse_simplecov_json(&json_content);

        info!(
            branches = branch_ids.len(),
            executed = executed_branch_ids.len(),
            "Ruby instrumentation complete"
        );

        Ok(InstrumentedTarget {
            target: target.clone(),
            branch_ids,
            executed_branch_ids,
            file_paths,
            work_dir: target_path.to_path_buf(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use apex_core::command::CommandOutput;

    #[test]
    fn parse_simplecov_basic() {
        let json = r#"{"coverage":{"app/models/user.rb":{"lines":[null,1,1,0,null,1]}}}"#;
        let (all, exec, files) = parse_simplecov_json(json);
        assert_eq!(all.len(), 4); // 4 executable lines (non-null)
        assert_eq!(exec.len(), 3); // 3 executed (count > 0)
        assert_eq!(files.len(), 1);
    }

    #[test]
    fn parse_simplecov_multiple_files() {
        let json = r#"{"coverage":{"a.rb":{"lines":[1,0]},"b.rb":{"lines":[1,1]}}}"#;
        let (all, exec, files) = parse_simplecov_json(json);
        assert_eq!(all.len(), 4);
        assert_eq!(exec.len(), 3);
        assert_eq!(files.len(), 2);
    }

    #[test]
    fn parse_simplecov_empty() {
        let json = r#"{"coverage":{}}"#;
        let (all, exec, files) = parse_simplecov_json(json);
        assert!(all.is_empty());
        assert!(exec.is_empty());
        assert!(files.is_empty());
    }

    #[test]
    fn parse_simplecov_invalid_json() {
        let (all, _, _) = parse_simplecov_json("not json");
        assert!(all.is_empty());
    }

    #[test]
    fn parse_simplecov_all_null_lines() {
        let json = r#"{"coverage":{"x.rb":{"lines":[null,null,null]}}}"#;
        let (all, _, _) = parse_simplecov_json(json);
        assert!(all.is_empty()); // All non-executable
    }

    #[test]
    fn parse_simplecov_file_id_deterministic() {
        let json = r#"{"coverage":{"app/user.rb":{"lines":[1]}}}"#;
        let (a1, _, _) = parse_simplecov_json(json);
        let (a2, _, _) = parse_simplecov_json(json);
        assert_eq!(a1[0].file_id, a2[0].file_id);
    }

    // --- New tests targeting uncovered regions ---

    // Target: FileCoverage::Detailed variant — serde_json untagged enum deserialization
    // The existing tests use {"lines": [...]} which matches both variants (untagged).
    // DetailedCoverage would be exercised when the JSON has additional fields.
    // Since both variants have the same fields, we verify the Detailed path
    // by checking that extra fields do not cause a parse error.
    #[test]
    fn parse_simplecov_detailed_coverage_variant() {
        // Extra "branches" key causes serde to pick the second untagged variant (Detailed)
        // if the untagged resolver tries them in order and the first one fails.
        // Even if both match, the result should be correct.
        let json = r#"{"coverage":{"lib/foo.rb":{"lines":[null,2,0,null,1],"branches":{}}}}"#;
        let (all, exec, files) = parse_simplecov_json(json);
        // 3 executable lines (non-null)
        assert_eq!(all.len(), 3);
        // 2 executed (count > 0: 2 and 1)
        assert_eq!(exec.len(), 2);
        assert_eq!(files.len(), 1);
    }

    // Target: parse_simplecov_json — line count exactly 0 produces no entry in executed
    #[test]
    fn parse_simplecov_zero_count_not_executed() {
        let json = r#"{"coverage":{"x.rb":{"lines":[0]}}}"#;
        let (all, exec, _) = parse_simplecov_json(json);
        assert_eq!(all.len(), 1);
        assert!(exec.is_empty());
    }

    // Target: parse_simplecov_json — line count of exactly 1 is executed
    #[test]
    fn parse_simplecov_count_one_is_executed() {
        let json = r#"{"coverage":{"x.rb":{"lines":[1]}}}"#;
        let (all, exec, _) = parse_simplecov_json(json);
        assert_eq!(all.len(), 1);
        assert_eq!(exec.len(), 1);
    }

    // Target: parse_simplecov_json — large u64 line count treated as executed
    #[test]
    fn parse_simplecov_large_count_executed() {
        let json = r#"{"coverage":{"x.rb":{"lines":[9999999999]}}}"#;
        let (all, exec, _) = parse_simplecov_json(json);
        assert_eq!(all.len(), 1);
        assert_eq!(exec.len(), 1);
        assert_eq!(exec[0].direction, 0);
    }

    // Target: parse_simplecov_json — empty lines array produces no branches
    #[test]
    fn parse_simplecov_empty_lines_array() {
        let json = r#"{"coverage":{"x.rb":{"lines":[]}}}"#;
        let (all, exec, files) = parse_simplecov_json(json);
        assert!(all.is_empty());
        assert!(exec.is_empty());
        assert_eq!(files.len(), 1);
    }

    // Target: parse_simplecov_json — unicode file path is stored correctly
    #[test]
    fn parse_simplecov_unicode_file_path() {
        let json =
            "{\"coverage\":{\"\u{4e2d}\u{6587}/\u{6a21}\u{578b}.rb\":{\"lines\":[1,null,2]}}}";
        let (all, exec, files) = parse_simplecov_json(json);
        assert_eq!(all.len(), 2);
        assert_eq!(exec.len(), 2);
        assert_eq!(files.len(), 1);
        let path_str = files.values().next().unwrap().to_string_lossy();
        assert!(path_str.contains('\u{4e2d}'));
    }

    // Target: parse_simplecov_json — line number is 1-indexed (first line = line 1)
    #[test]
    fn parse_simplecov_line_numbers_one_indexed() {
        let json = r#"{"coverage":{"x.rb":{"lines":[null,null,5]}}}"#;
        let (all, _, _) = parse_simplecov_json(json);
        assert_eq!(all.len(), 1);
        // Third element (index 2) -> line 3
        assert_eq!(all[0].line, 3);
    }

    // Target: parse_simplecov_json — all lines executed (all positive counts)
    #[test]
    fn parse_simplecov_all_executed() {
        let json = r#"{"coverage":{"x.rb":{"lines":[3,1,7]}}}"#;
        let (all, exec, _) = parse_simplecov_json(json);
        assert_eq!(all.len(), 3);
        assert_eq!(exec.len(), 3);
    }

    // Target: parse_simplecov_json — mixed null and zero with executed
    #[test]
    fn parse_simplecov_mixed_null_zero_executed() {
        let json = r#"{"coverage":{"x.rb":{"lines":[null,0,1,null,0,2]}}}"#;
        let (all, exec, _) = parse_simplecov_json(json);
        // 4 non-null = 4 executable
        assert_eq!(all.len(), 4);
        // 2 executed (count > 0: 1 and 2)
        assert_eq!(exec.len(), 2);
    }

    // Target: with_runner constructor
    #[test]
    fn ruby_instrumentor_with_runner_constructs() {
        use apex_core::command::RealCommandRunner;
        let runner = Arc::new(RealCommandRunner);
        let inst = RubyInstrumentor::with_runner(runner);
        // Verify branch_ids() works on constructed instance
        assert_eq!(inst.branch_ids().len(), 0);
    }

    // Target: parse_simplecov_json — multiple files have distinct file_ids
    #[test]
    fn parse_simplecov_multiple_files_distinct_ids() {
        let json = r#"{"coverage":{"a.rb":{"lines":[1]},"b.rb":{"lines":[2]}}}"#;
        let (all, _, _) = parse_simplecov_json(json);
        assert_eq!(all.len(), 2);
        assert_ne!(all[0].file_id, all[1].file_id);
    }

    // --- Test framework detection ---

    #[test]
    fn detect_test_command_rspec() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir(tmp.path().join("spec")).unwrap();
        let cmd = detect_test_command(tmp.path(), true);
        assert_eq!(cmd, vec!["bundle", "exec", "rspec"]);
    }

    #[test]
    fn detect_test_command_rspec_no_bundler() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir(tmp.path().join("spec")).unwrap();
        let cmd = detect_test_command(tmp.path(), false);
        assert_eq!(cmd, vec!["rspec"]);
    }

    #[test]
    fn detect_test_command_minitest() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir(tmp.path().join("test")).unwrap();
        let cmd = detect_test_command(tmp.path(), true);
        assert_eq!(cmd, vec!["bundle", "exec", "rake", "test"]);
    }

    #[test]
    fn detect_test_command_minitest_no_bundler() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir(tmp.path().join("test")).unwrap();
        let cmd = detect_test_command(tmp.path(), false);
        assert_eq!(cmd, vec!["rake", "test"]);
    }

    #[test]
    fn detect_test_command_rspec_preferred_over_minitest() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir(tmp.path().join("spec")).unwrap();
        std::fs::create_dir(tmp.path().join("test")).unwrap();
        let cmd = detect_test_command(tmp.path(), true);
        // spec/ takes priority
        assert_eq!(cmd, vec!["bundle", "exec", "rspec"]);
    }

    #[test]
    fn detect_test_command_fallback() {
        let tmp = tempfile::tempdir().unwrap();
        // No spec/ or test/ directory
        let cmd = detect_test_command(tmp.path(), false);
        // Should use ruby -Ilib -Itest
        assert_eq!(cmd.len(), 3);
        assert!(cmd[0].contains("ruby") || cmd[0] == "ruby");
        assert_eq!(cmd[1], "-Ilib");
        assert_eq!(cmd[2], "-Itest");
    }

    #[test]
    fn simplecov_helper_content_is_valid() {
        // Basic sanity: the helper script should contain required setup
        assert!(SIMPLECOV_HELPER.contains("require 'simplecov'"));
        assert!(SIMPLECOV_HELPER.contains("SimpleCov.start"));
        assert!(SIMPLECOV_HELPER.contains("coverage_dir"));
    }

    // --- Mock-based instrument() tests ---

    struct FakeRunner {
        exit_code: i32,
        fail: bool,
    }

    impl FakeRunner {
        fn success() -> Self {
            FakeRunner {
                exit_code: 0,
                fail: false,
            }
        }

        fn failure(exit_code: i32) -> Self {
            FakeRunner {
                exit_code,
                fail: false,
            }
        }

        fn spawn_error() -> Self {
            FakeRunner {
                exit_code: -1,
                fail: true,
            }
        }
    }

    #[async_trait]
    impl CommandRunner for FakeRunner {
        async fn run_command(
            &self,
            _spec: &CommandSpec,
        ) -> apex_core::error::Result<CommandOutput> {
            if self.fail {
                return Err(ApexError::Subprocess {
                    exit_code: -1,
                    stderr: "spawn failed".into(),
                });
            }
            Ok(CommandOutput {
                exit_code: self.exit_code,
                stdout: Vec::new(),
                stderr: Vec::new(),
            })
        }
    }

    #[tokio::test]
    async fn instrument_missing_coverage_json_returns_error() {
        let tmp = tempfile::tempdir().unwrap();
        let repo_root = tmp.path();
        // Do NOT create coverage JSON files

        let runner = Arc::new(FakeRunner::success());
        let inst = RubyInstrumentor::with_runner(runner);

        let target = Target {
            root: repo_root.to_path_buf(),
            language: Language::Ruby,
            test_command: vec!["ruby".into(), "-e".into(), "true".into()],
        };

        let result = inst.instrument(&target).await;
        assert!(result.is_err());
        let err_msg = format!("{}", result.unwrap_err());
        assert!(
            err_msg.contains("SimpleCov JSON not found"),
            "expected 'SimpleCov JSON not found' in error, got: {err_msg}"
        );
    }

    #[tokio::test]
    async fn instrument_success_with_resultset_json() {
        let tmp = tempfile::tempdir().unwrap();
        let repo_root = tmp.path();

        // Create coverage/.resultset.json
        let cov_dir = repo_root.join("coverage");
        std::fs::create_dir(&cov_dir).unwrap();
        let json = r#"{"coverage":{"app/user.rb":{"lines":[1,0,null,2]}}}"#;
        std::fs::write(cov_dir.join(".resultset.json"), json).unwrap();

        let runner = Arc::new(FakeRunner::success());
        let inst = RubyInstrumentor::with_runner(runner);

        let target = Target {
            root: repo_root.to_path_buf(),
            language: Language::Ruby,
            test_command: vec!["ruby".into(), "-e".into(), "true".into()],
        };

        let result = inst.instrument(&target).await.unwrap();
        assert_eq!(result.branch_ids.len(), 3); // 3 non-null lines
        assert_eq!(result.executed_branch_ids.len(), 2); // 2 with count > 0
        assert_eq!(result.file_paths.len(), 1);
    }

    #[tokio::test]
    async fn instrument_success_with_alt_coverage_json() {
        let tmp = tempfile::tempdir().unwrap();
        let repo_root = tmp.path();

        // Create coverage/coverage.json (alt path)
        let cov_dir = repo_root.join("coverage");
        std::fs::create_dir(&cov_dir).unwrap();
        let json = r#"{"coverage":{"lib/foo.rb":{"lines":[1]}}}"#;
        std::fs::write(cov_dir.join("coverage.json"), json).unwrap();

        let runner = Arc::new(FakeRunner::success());
        let inst = RubyInstrumentor::with_runner(runner);

        let target = Target {
            root: repo_root.to_path_buf(),
            language: Language::Ruby,
            test_command: vec!["ruby".into(), "-e".into(), "true".into()],
        };

        let result = inst.instrument(&target).await.unwrap();
        assert_eq!(result.branch_ids.len(), 1);
        assert_eq!(result.executed_branch_ids.len(), 1);
    }

    #[tokio::test]
    async fn instrument_nonzero_exit_still_parses() {
        let tmp = tempfile::tempdir().unwrap();
        let repo_root = tmp.path();

        let cov_dir = repo_root.join("coverage");
        std::fs::create_dir(&cov_dir).unwrap();
        let json = r#"{"coverage":{"app.rb":{"lines":[1,0]}}}"#;
        std::fs::write(cov_dir.join(".resultset.json"), json).unwrap();

        let runner = Arc::new(FakeRunner::failure(1));
        let inst = RubyInstrumentor::with_runner(runner);

        let target = Target {
            root: repo_root.to_path_buf(),
            language: Language::Ruby,
            test_command: vec!["ruby".into(), "-e".into(), "true".into()],
        };

        // Non-zero exit is a warning, not an error -- coverage may still exist
        let result = inst.instrument(&target).await.unwrap();
        assert_eq!(result.branch_ids.len(), 2);
    }

    #[tokio::test]
    async fn instrument_spawn_error() {
        let tmp = tempfile::tempdir().unwrap();
        let repo_root = tmp.path();

        let runner = Arc::new(FakeRunner::spawn_error());
        let inst = RubyInstrumentor::with_runner(runner);

        let target = Target {
            root: repo_root.to_path_buf(),
            language: Language::Ruby,
            test_command: vec!["ruby".into(), "-e".into(), "true".into()],
        };

        let result = inst.instrument(&target).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn instrument_writes_and_cleans_helper_script() {
        use std::sync::Mutex;

        struct TrackingRunner {
            spec_env: Mutex<Vec<(String, String)>>,
        }

        #[async_trait]
        impl CommandRunner for TrackingRunner {
            async fn run_command(
                &self,
                spec: &CommandSpec,
            ) -> apex_core::error::Result<CommandOutput> {
                let mut env = self.spec_env.lock().unwrap();
                *env = spec.env.clone();
                Ok(CommandOutput::success(Vec::new()))
            }
        }

        let tmp = tempfile::tempdir().unwrap();
        let repo_root = tmp.path();

        // Create coverage output so the instrument call succeeds
        let cov_dir = repo_root.join("coverage");
        std::fs::create_dir(&cov_dir).unwrap();
        std::fs::write(cov_dir.join(".resultset.json"), r#"{"coverage":{}}"#).unwrap();

        let runner = Arc::new(TrackingRunner {
            spec_env: Mutex::new(Vec::new()),
        });
        let runner_ref = runner.clone();
        let inst = RubyInstrumentor::with_runner(runner);

        let target = Target {
            root: repo_root.to_path_buf(),
            language: Language::Ruby,
            test_command: vec!["ruby".into(), "-e".into(), "true".into()],
        };

        inst.instrument(&target).await.unwrap();

        // Verify RUBYOPT was set to load the helper
        let env = runner_ref.spec_env.lock().unwrap();
        let rubyopt = env.iter().find(|(k, _)| k == "RUBYOPT");
        assert!(rubyopt.is_some(), "RUBYOPT should be set");
        let (_, val) = rubyopt.unwrap();
        assert!(
            val.contains(".apex_coverage_helper.rb"),
            "RUBYOPT should reference helper script, got: {val}"
        );

        // Helper should be cleaned up after instrument completes
        assert!(
            !repo_root.join(".apex_coverage_helper.rb").exists(),
            "helper script should be cleaned up"
        );
    }
}
