//! Rust per-test branch indexing via cargo-llvm-cov.
//!
//! Strategy for performance:
//! 1. `cargo llvm-cov --no-report --workspace` — build instrumented binaries once
//! 2. Discover test binaries in `target/llvm-cov-target/debug/deps/`
//! 3. Run each test directly with unique `LLVM_PROFILE_FILE` (~10-50ms per test)
//! 4. `llvm-profdata merge` + `llvm-cov export` per profraw for coverage JSON
//! 5. Parse JSON → BranchId, aggregate into BranchIndex

use crate::types::{BranchIndex, TestTrace};
use apex_core::command::{CommandRunner, CommandSpec, RealCommandRunner};
use apex_core::hash::fnv1a_hash;
use apex_core::types::{BranchId, ExecutionStatus, Language};
use serde::Deserialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::Semaphore;
use tracing::{debug, info};

type BoxErr = Box<dyn std::error::Error + Send + Sync>;

// ---------------------------------------------------------------------------
// Generic builder for testability
// ---------------------------------------------------------------------------

/// Builder that delegates subprocess calls through a generic [`CommandRunner`],
/// allowing tests to inject mocks without spawning real processes.
pub struct RustIndexBuilder<R: CommandRunner> {
    runner: R,
}

impl<R: CommandRunner> RustIndexBuilder<R> {
    pub fn new(runner: R) -> Self {
        Self { runner }
    }

    /// Enumerate Rust tests by running `cargo test --list` through the runner.
    pub async fn enumerate_tests(&self, target: &Path) -> Result<Vec<String>, BoxErr> {
        let spec = CommandSpec::new("cargo", target)
            .args(["test", "--workspace", "--", "--list", "--format", "terse"])
            .timeout(120_000);
        let output = self.runner.run_command(&spec).await?;
        let stdout = String::from_utf8_lossy(&output.stdout);
        let tests: Vec<String> = stdout
            .lines()
            .filter(|l| l.ends_with(": test"))
            .map(|l| l.trim_end_matches(": test").to_string())
            .collect();
        Ok(tests)
    }
}

/// FNV-1a hash — delegates to apex_core::hash::fnv1a_hash for file_id compatibility.
fn fnv1a(data: &[u8]) -> u64 {
    // Safety: we only hash valid UTF-8 path strings, but accept &[u8] for API compat.
    let s = std::str::from_utf8(data).unwrap_or("");
    fnv1a_hash(s)
}

// ---------------------------------------------------------------------------
// LLVM coverage JSON structures
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct LlvmCovJson {
    data: Vec<LlvmCovData>,
}

#[derive(Deserialize)]
struct LlvmCovData {
    files: Vec<LlvmCovFile>,
}

#[derive(Deserialize)]
struct LlvmCovFile {
    filename: String,
    segments: Vec<Vec<serde_json::Value>>,
}

// ---------------------------------------------------------------------------
// Internal types
// ---------------------------------------------------------------------------

struct TestBinary {
    path: PathBuf,
    tests: Vec<String>,
}

struct LlvmEnv {
    llvm_cov: String,
    llvm_profdata: String,
    target_dir: PathBuf,
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Build the per-test branch index for a Rust workspace.
pub async fn build_rust_index(target: &Path, parallel: usize) -> Result<BranchIndex, BoxErr> {
    let target = std::fs::canonicalize(target)?;

    // 1. Resolve LLVM tools
    let env = resolve_llvm_env(&target).await?;
    info!(
        "llvm-cov: {}, target-dir: {}",
        env.llvm_cov,
        env.target_dir.display()
    );

    // 2. Build instrumented binaries (compile once)
    info!("building instrumented workspace...");
    let status = tokio::process::Command::new("cargo")
        .args(["llvm-cov", "--no-report", "--workspace"])
        .current_dir(&target)
        .env("LLVM_COV", &env.llvm_cov)
        .env("LLVM_PROFDATA", &env.llvm_profdata)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::inherit())
        .status()
        .await?;
    if !status.success() {
        return Err("cargo llvm-cov --no-report failed (is cargo-llvm-cov installed?)".into());
    }

    // 3. Discover test binaries
    let binaries = discover_test_binaries(&env.target_dir).await?;
    let total_tests: usize = binaries.iter().map(|b| b.tests.len()).sum();
    info!(
        "found {} test binaries with {} total tests",
        binaries.len(),
        total_tests
    );
    if total_tests == 0 {
        return Err("no tests found in workspace".into());
    }

    // 4. Full suite coverage for total branch counts
    info!("running full suite coverage...");
    let full_json = run_full_coverage(&target, &env).await?;
    let target_str = target.to_string_lossy().to_string();
    let (file_paths, total_branches, covered_branches) =
        parse_coverage_stats(&full_json, &target_str);
    info!(
        "full suite: {}/{} branches covered ({:.1}%)",
        covered_branches,
        total_branches,
        if total_branches > 0 {
            covered_branches as f64 / total_branches as f64 * 100.0
        } else {
            0.0
        }
    );

    // 5. Per-test coverage (parallel)
    info!(
        "running per-test coverage ({} tests, {} parallel)...",
        total_tests, parallel
    );
    let sem = Arc::new(Semaphore::new(parallel));
    let env_arc = Arc::new(env);
    let target_str_arc = Arc::new(target_str.clone());

    let mut handles = Vec::new();
    for binary in &binaries {
        for test_name in &binary.tests {
            let permit = sem.clone().acquire_owned().await?;
            let binary_path = binary.path.clone();
            let name = test_name.clone();
            let env = env_arc.clone();
            let tstr = target_str_arc.clone();

            let handle = tokio::spawn(async move {
                let start = Instant::now();
                let result = run_single_test(&binary_path, &name, &env, &tstr).await;
                let duration_ms = start.elapsed().as_millis() as u64;
                drop(permit);

                match result {
                    Ok(branches) => {
                        debug!("{}: {} branches in {}ms", name, branches.len(), duration_ms);
                        TestTrace {
                            test_name: name,
                            branches,
                            duration_ms,
                            status: ExecutionStatus::Pass,
                        }
                    }
                    Err(e) => {
                        debug!("{}: failed — {}", name, e);
                        TestTrace {
                            test_name: name,
                            branches: vec![],
                            duration_ms,
                            status: ExecutionStatus::Fail,
                        }
                    }
                }
            });
            handles.push(handle);
        }
    }

    let mut traces = Vec::new();
    let mut done = 0;
    for handle in handles {
        traces.push(handle.await?);
        done += 1;
        #[allow(unknown_lints, clippy::manual_is_multiple_of)]
        if done % 100 == 0 {
            info!("  {}/{} tests complete", done, total_tests);
        }
    }
    info!("  {}/{} tests complete", done, total_tests);

    // 6. Build index
    let profiles = BranchIndex::build_profiles(&traces);
    let source_hash = crate::types::hash_source_files(&target, Language::Rust);
    let created_at = format!(
        "{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
    );

    let index = BranchIndex {
        traces,
        profiles,
        file_paths,
        total_branches,
        covered_branches,
        created_at,
        language: Language::Rust,
        target_root: target.clone(),
        source_hash,
    };

    Ok(index)
}

/// Enumerate Rust tests without building index.
///
/// Convenience wrapper around [`RustIndexBuilder::enumerate_tests`] using
/// the real subprocess runner.
pub async fn enumerate_rust_tests(target: &Path) -> Result<Vec<String>, BoxErr> {
    RustIndexBuilder::new(RealCommandRunner)
        .enumerate_tests(target)
        .await
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

async fn resolve_llvm_env(target: &Path) -> Result<LlvmEnv, BoxErr> {
    // Get target dir from cargo llvm-cov show-env
    let output = tokio::process::Command::new("cargo")
        .args(["llvm-cov", "show-env"])
        .current_dir(target)
        .output()
        .await?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut target_dir = None;

    for line in stdout.lines() {
        if let Some(val) = line.strip_prefix("CARGO_TARGET_DIR=") {
            target_dir = Some(PathBuf::from(val.trim_matches('"')));
        }
    }

    // Resolve tool paths: env vars > PATH
    let llvm_cov = std::env::var("LLVM_COV").unwrap_or_else(|_| "llvm-cov".to_string());
    let llvm_profdata =
        std::env::var("LLVM_PROFDATA").unwrap_or_else(|_| "llvm-profdata".to_string());

    Ok(LlvmEnv {
        llvm_cov,
        llvm_profdata,
        target_dir: target_dir.unwrap_or_else(|| target.join("target/llvm-cov-target")),
    })
}

async fn discover_test_binaries(target_dir: &Path) -> Result<Vec<TestBinary>, BoxErr> {
    let deps_dir = target_dir.join("debug/deps");
    if !deps_dir.exists() {
        return Err(format!("deps directory not found: {}", deps_dir.display()).into());
    }

    let mut binaries = Vec::new();

    for entry in std::fs::read_dir(&deps_dir)? {
        let entry = entry?;
        let path = entry.path();

        if !path.is_file() {
            continue;
        }

        // Skip files with known non-binary extensions
        if let Some(ext) = path.extension() {
            let ext = ext.to_string_lossy();
            if ["d", "rmeta", "rlib", "dylib", "so", "o", "a"].contains(&ext.as_ref()) {
                continue;
            }
        }

        // Check executable bit (Unix)
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let meta = entry.metadata()?;
            if meta.permissions().mode() & 0o111 == 0 {
                continue;
            }
        }

        // Try listing tests from this binary
        let output = tokio::process::Command::new(&path)
            .args(["--list", "--format", "terse"])
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .output()
            .await;

        if let Ok(out) = output {
            if out.status.success() {
                let stdout = String::from_utf8_lossy(&out.stdout);
                let tests: Vec<String> = stdout
                    .lines()
                    .filter(|l| l.ends_with(": test"))
                    .map(|l| l.trim_end_matches(": test").to_string())
                    .collect();
                if !tests.is_empty() {
                    debug!(
                        "  {}: {} tests",
                        path.file_name().unwrap_or_default().to_string_lossy(),
                        tests.len()
                    );
                    binaries.push(TestBinary { path, tests });
                }
            }
        }
    }

    Ok(binaries)
}

async fn run_full_coverage(target: &Path, env: &LlvmEnv) -> Result<LlvmCovJson, BoxErr> {
    let output = tokio::process::Command::new("cargo")
        .args(["llvm-cov", "--json", "--workspace"])
        .current_dir(target)
        .env("LLVM_COV", &env.llvm_cov)
        .env("LLVM_PROFDATA", &env.llvm_profdata)
        .output()
        .await?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("cargo llvm-cov --json failed: {stderr}").into());
    }

    Ok(serde_json::from_slice(&output.stdout)?)
}

async fn run_single_test(
    binary: &Path,
    test_name: &str,
    env: &LlvmEnv,
    target_str: &str,
) -> Result<Vec<BranchId>, BoxErr> {
    let sanitized_name = test_name
        .replace("::", "__")
        .replace(['/', ' ', '<', '>', '\\'], "_");

    let tmpdir = std::env::temp_dir();
    let profraw = tmpdir.join(format!(
        "apex_rust_{}_{}_{}.profraw",
        std::process::id(),
        sanitized_name,
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
            % 1_000_000
    ));
    let profdata = profraw.with_extension("profdata");

    // Run the test with unique profile file
    let test_output = tokio::process::Command::new(binary)
        .args(["--exact", test_name, "--test-threads", "1"])
        .env("LLVM_PROFILE_FILE", profraw.to_string_lossy().as_ref())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .output()
        .await?;

    if !test_output.status.success() {
        let _ = std::fs::remove_file(&profraw);
        return Err(format!("test exited with {}", test_output.status).into());
    }

    if !profraw.exists() {
        return Ok(vec![]);
    }

    // Merge profraw → profdata
    let merge = tokio::process::Command::new(&env.llvm_profdata)
        .args(["merge", "-sparse"])
        .arg(&profraw)
        .arg("-o")
        .arg(&profdata)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .await?;

    let _ = std::fs::remove_file(&profraw);
    if !merge.success() {
        let _ = std::fs::remove_file(&profdata);
        return Err("llvm-profdata merge failed".into());
    }

    // Export coverage JSON
    let export = tokio::process::Command::new(&env.llvm_cov)
        .args(["export", "--format=text"])
        .arg(format!("--instr-profile={}", profdata.display()))
        .arg(binary)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .output()
        .await?;

    let _ = std::fs::remove_file(&profdata);

    if !export.status.success() {
        return Err("llvm-cov export failed".into());
    }

    let json: LlvmCovJson = serde_json::from_slice(&export.stdout)?;
    Ok(extract_covered_branches(&json, target_str))
}

fn extract_covered_branches(json: &LlvmCovJson, target_str: &str) -> Vec<BranchId> {
    let mut branches = Vec::new();
    for data in &json.data {
        for file in &data.files {
            let rel = make_relative(&file.filename, target_str);
            let file_id = fnv1a(rel.as_bytes());

            for seg in &file.segments {
                if seg.len() < 6 {
                    continue;
                }
                let has_count = seg.get(3)
                    .and_then(|v| v.as_bool().or_else(|| v.as_u64().map(|n| n != 0)))
                    .unwrap_or(false);
                let is_entry = seg.get(4)
                    .and_then(|v| v.as_bool().or_else(|| v.as_u64().map(|n| n != 0)))
                    .unwrap_or(false);
                let is_gap = seg.get(5)
                    .and_then(|v| v.as_bool().or_else(|| v.as_u64().map(|n| n != 0)))
                    .unwrap_or(false);
                let count = seg[2].as_f64().map(|f| f as u64).unwrap_or(0);

                if has_count && is_entry && !is_gap && count > 0 {
                    let line = seg[0].as_u64().unwrap_or(0).min(u32::MAX as u64) as u32;
                    // Skip degenerate segments with line=0 (invalid source location)
                    if line == 0 {
                        continue;
                    }
                    let col = seg[1].as_u64().unwrap_or(0).min(u16::MAX as u64) as u16;
                    branches.push(BranchId::new(file_id, line, col, 0));
                }
            }
        }
    }
    branches
}

fn parse_coverage_stats(
    json: &LlvmCovJson,
    target_str: &str,
) -> (HashMap<u64, PathBuf>, usize, usize) {
    let mut file_paths = HashMap::new();
    let mut total = 0;
    let mut covered = 0;

    for data in &json.data {
        for file in &data.files {
            let rel = make_relative(&file.filename, target_str);
            let file_id = fnv1a(rel.as_bytes());
            file_paths.insert(file_id, PathBuf::from(&rel));

            for seg in &file.segments {
                if seg.len() < 6 {
                    continue;
                }
                let has_count = seg.get(3)
                    .and_then(|v| v.as_bool().or_else(|| v.as_u64().map(|n| n != 0)))
                    .unwrap_or(false);
                let is_entry = seg.get(4)
                    .and_then(|v| v.as_bool().or_else(|| v.as_u64().map(|n| n != 0)))
                    .unwrap_or(false);
                let is_gap = seg.get(5)
                    .and_then(|v| v.as_bool().or_else(|| v.as_u64().map(|n| n != 0)))
                    .unwrap_or(false);

                if has_count && is_entry && !is_gap {
                    total += 1;
                    if seg[2].as_f64().map(|f| f as u64).unwrap_or(0) > 0 {
                        covered += 1;
                    }
                }
            }
        }
    }

    (file_paths, total, covered)
}

fn make_relative(path: &str, target: &str) -> String {
    if target.is_empty() {
        return path.to_string();
    }

    let prefix = if target.ends_with('/') {
        target.to_string()
    } else {
        format!("{target}/")
    };

    let result = path
        .strip_prefix(&prefix)
        .map(|s| s.trim_start_matches('/').to_string())
        .unwrap_or_else(|| {
            if path == target {
                ".".to_string()
            } else {
                path.to_string()
            }
        });

    if result.is_empty() {
        ".".to_string()
    } else {
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use apex_core::command::{CommandOutput, CommandRunner, CommandSpec};
    use apex_core::error::Result as CoreResult;

    /// Simple mock runner that returns a fixed output.
    struct FakeRunner {
        stdout: Vec<u8>,
    }

    #[async_trait::async_trait]
    impl CommandRunner for FakeRunner {
        async fn run_command(&self, _spec: &CommandSpec) -> CoreResult<CommandOutput> {
            Ok(CommandOutput::success(self.stdout.clone()))
        }
    }

    #[tokio::test]
    async fn builder_enumerate_tests_with_mock() {
        let runner = FakeRunner {
            stdout: b"my_crate::tests::test_one: test\nmy_crate::tests::test_two: test\n".to_vec(),
        };

        let builder = RustIndexBuilder::new(runner);
        let tests = builder.enumerate_tests(Path::new("/fake")).await.unwrap();
        assert_eq!(
            tests,
            vec!["my_crate::tests::test_one", "my_crate::tests::test_two"]
        );
    }

    #[tokio::test]
    async fn builder_enumerate_tests_empty_output() {
        let runner = FakeRunner {
            stdout: b"".to_vec(),
        };

        let builder = RustIndexBuilder::new(runner);
        let tests = builder.enumerate_tests(Path::new("/fake")).await.unwrap();
        assert!(tests.is_empty());
    }

    #[test]
    fn fnv1a_matches_instrument_crate() {
        let hash = fnv1a(b"src/main.rs");
        assert_ne!(hash, 0);
        // Same input always produces same output
        assert_eq!(hash, fnv1a(b"src/main.rs"));
        // Different input produces different output
        assert_ne!(fnv1a(b"src/main.rs"), fnv1a(b"src/lib.rs"));
    }

    #[test]
    fn make_relative_strips_prefix() {
        assert_eq!(
            make_relative("/home/user/project/src/main.rs", "/home/user/project"),
            "src/main.rs"
        );
    }

    #[test]
    fn make_relative_trailing_slash() {
        assert_eq!(
            make_relative("/home/user/project/src/lib.rs", "/home/user/project/"),
            "src/lib.rs"
        );
    }

    #[test]
    fn make_relative_no_match() {
        assert_eq!(
            make_relative("/other/path/file.rs", "/home/user/project"),
            "/other/path/file.rs"
        );
    }

    #[test]
    fn extract_branches_empty_json() {
        let json = LlvmCovJson {
            data: vec![LlvmCovData { files: vec![] }],
        };
        assert!(extract_covered_branches(&json, "/target").is_empty());
    }

    // -----------------------------------------------------------------------
    // Bug-exposing tests
    // -----------------------------------------------------------------------

    /// BUG: make_relative strips a prefix that's a substring of a different
    /// directory name. E.g., target="/home/user/project" incorrectly strips
    /// from path="/home/user/projectfoo/src/main.rs" → "foo/src/main.rs".
    /// The second strip_prefix (without trailing slash) does a pure string
    /// prefix match, not a path-component-boundary match.
    #[test]
    fn bug_make_relative_prefix_substring_of_different_dir() {
        // path is in "projectfoo", NOT in "project" — should be returned as-is
        let result = make_relative(
            "/home/user/projectfoo/src/main.rs",
            "/home/user/project",
        );
        assert_eq!(
            result, "/home/user/projectfoo/src/main.rs",
            "make_relative should not strip prefix that is a substring of a different directory"
        );
    }

    /// BUG: make_relative returns empty string when path equals target exactly.
    /// This produces an empty relative path which is invalid for file lookups.
    #[test]
    fn bug_make_relative_path_equals_target() {
        let result = make_relative("/home/user/project", "/home/user/project");
        // The path IS the target itself — stripping the prefix gives ""
        // which is not a usable path. Should return "." or the original.
        assert!(
            !result.is_empty(),
            "make_relative should not return empty string when path == target"
        );
    }

    /// Edge case: make_relative with empty target should return the path unchanged.
    #[test]
    fn bug_make_relative_empty_target() {
        let result = make_relative("/some/file.rs", "");
        // With target = "", prefix becomes "/", stripping it from "/some/file.rs"
        // gives "some/file.rs" — but "" is not a valid target directory.
        // This is a latent bug: empty target silently corrupts paths.
        assert_eq!(
            result, "/some/file.rs",
            "make_relative with empty target should return path unchanged"
        );
    }

    /// BUG: fnv1a on empty input — should match the FNV-1a spec (offset basis).
    /// Verify it's consistent with apex-instrument. Not a crash bug but
    /// important for correctness.
    #[test]
    fn fnv1a_empty_input() {
        let hash = fnv1a(b"");
        assert_eq!(hash, 0xcbf29ce484222325, "empty input should return FNV offset basis");
    }

    /// Edge case: segments with fewer than 6 elements should be skipped
    /// without panicking.
    #[test]
    fn extract_branches_short_segments_no_panic() {
        let json = LlvmCovJson {
            data: vec![LlvmCovData {
                files: vec![LlvmCovFile {
                    filename: "/project/src/lib.rs".to_string(),
                    segments: vec![
                        vec![],                                    // 0 elements
                        vec![serde_json::json!(1)],                // 1 element
                        vec![serde_json::json!(1), serde_json::json!(2),
                             serde_json::json!(3), serde_json::json!(4),
                             serde_json::json!(5)],                // 5 elements (< 6)
                    ],
                }],
            }],
        };
        let branches = extract_covered_branches(&json, "/project");
        assert!(branches.is_empty(), "short segments should be skipped");
    }

    /// Edge case: segments with null/unexpected types in fields should not panic.
    #[test]
    fn extract_branches_null_fields_no_panic() {
        let json = LlvmCovJson {
            data: vec![LlvmCovData {
                files: vec![LlvmCovFile {
                    filename: "/project/src/lib.rs".to_string(),
                    segments: vec![
                        vec![
                            serde_json::json!(null),
                            serde_json::json!(null),
                            serde_json::json!(null),
                            serde_json::json!(null),
                            serde_json::json!(null),
                            serde_json::json!(null),
                        ],
                    ],
                }],
            }],
        };
        // Should not panic — null fields default to false/0 via unwrap_or
        let branches = extract_covered_branches(&json, "/project");
        assert!(branches.is_empty(), "null segments should produce no branches");
    }

    /// parse_coverage_stats should agree with extract_covered_branches
    /// on what counts as a covered branch.
    #[test]
    fn parse_coverage_stats_agrees_with_extract() {
        let json = LlvmCovJson {
            data: vec![LlvmCovData {
                files: vec![LlvmCovFile {
                    filename: "/project/src/lib.rs".to_string(),
                    segments: vec![
                        // Covered entry
                        vec![
                            serde_json::json!(10), serde_json::json!(5),
                            serde_json::json!(3),  serde_json::json!(true),
                            serde_json::json!(true), serde_json::json!(false),
                        ],
                        // Uncovered entry
                        vec![
                            serde_json::json!(20), serde_json::json!(1),
                            serde_json::json!(0),  serde_json::json!(true),
                            serde_json::json!(true), serde_json::json!(false),
                        ],
                        // Gap
                        vec![
                            serde_json::json!(30), serde_json::json!(1),
                            serde_json::json!(5),  serde_json::json!(true),
                            serde_json::json!(true), serde_json::json!(true),
                        ],
                    ],
                }],
            }],
        };

        let branches = extract_covered_branches(&json, "/project");
        let (_, total, covered) = parse_coverage_stats(&json, "/project");

        assert_eq!(total, 2, "total should count non-gap entries");
        assert_eq!(covered, 1, "covered should count entries with count > 0");
        assert_eq!(branches.len(), covered, "extract and parse should agree on covered count");
    }

    /// parse_coverage_stats with completely empty JSON
    #[test]
    fn parse_coverage_stats_empty_data() {
        let json = LlvmCovJson { data: vec![] };
        let (paths, total, covered) = parse_coverage_stats(&json, "/project");
        assert!(paths.is_empty());
        assert_eq!(total, 0);
        assert_eq!(covered, 0);
    }

    /// Multiple files in same data entry should all be processed
    #[test]
    fn parse_coverage_stats_multiple_files() {
        let json = LlvmCovJson {
            data: vec![LlvmCovData {
                files: vec![
                    LlvmCovFile {
                        filename: "/project/src/a.rs".to_string(),
                        segments: vec![vec![
                            serde_json::json!(1), serde_json::json!(1),
                            serde_json::json!(1), serde_json::json!(true),
                            serde_json::json!(true), serde_json::json!(false),
                        ]],
                    },
                    LlvmCovFile {
                        filename: "/project/src/b.rs".to_string(),
                        segments: vec![vec![
                            serde_json::json!(1), serde_json::json!(1),
                            serde_json::json!(0), serde_json::json!(true),
                            serde_json::json!(true), serde_json::json!(false),
                        ]],
                    },
                ],
            }],
        };

        let (paths, total, covered) = parse_coverage_stats(&json, "/project");
        assert_eq!(paths.len(), 2, "both files should be in file_paths");
        assert_eq!(total, 2);
        assert_eq!(covered, 1);
    }

    /// extract_covered_branches correctly computes file_id from relative path
    #[test]
    fn extract_branches_file_id_uses_relative_path() {
        let json = LlvmCovJson {
            data: vec![LlvmCovData {
                files: vec![LlvmCovFile {
                    filename: "/project/src/lib.rs".to_string(),
                    segments: vec![vec![
                        serde_json::json!(10), serde_json::json!(5),
                        serde_json::json!(1), serde_json::json!(true),
                        serde_json::json!(true), serde_json::json!(false),
                    ]],
                }],
            }],
        };

        let branches = extract_covered_branches(&json, "/project");
        assert_eq!(branches.len(), 1);
        // file_id should be fnv1a of "src/lib.rs", NOT of the full path
        let expected_id = fnv1a(b"src/lib.rs");
        assert_eq!(branches[0].file_id, expected_id);
    }

    /// Verify line/col are correctly extracted from segments
    #[test]
    fn extract_branches_correct_line_col() {
        let json = LlvmCovJson {
            data: vec![LlvmCovData {
                files: vec![LlvmCovFile {
                    filename: "/project/src/lib.rs".to_string(),
                    segments: vec![vec![
                        serde_json::json!(42), serde_json::json!(17),
                        serde_json::json!(5),  serde_json::json!(true),
                        serde_json::json!(true), serde_json::json!(false),
                    ]],
                }],
            }],
        };

        let branches = extract_covered_branches(&json, "/project");
        assert_eq!(branches.len(), 1);
        assert_eq!(branches[0].line, 42);
        assert_eq!(branches[0].col, 17);
        assert_eq!(branches[0].direction, 0, "direction should always be 0 from LLVM segments");
    }

    /// make_relative with double slashes in path
    #[test]
    fn make_relative_double_slash() {
        let result = make_relative("/home/user//project/src/main.rs", "/home/user//project");
        assert_eq!(result, "src/main.rs");
    }

    /// FNV-1a hash should differ for paths that differ only by case
    #[test]
    fn fnv1a_case_sensitive() {
        assert_ne!(fnv1a(b"src/Main.rs"), fnv1a(b"src/main.rs"));
    }

    /// Large line/col values should not overflow
    #[test]
    fn extract_branches_large_line_col() {
        let json = LlvmCovJson {
            data: vec![LlvmCovData {
                files: vec![LlvmCovFile {
                    filename: "/project/src/lib.rs".to_string(),
                    segments: vec![vec![
                        serde_json::json!(u32::MAX as u64), serde_json::json!(u16::MAX as u64),
                        serde_json::json!(1), serde_json::json!(true),
                        serde_json::json!(true), serde_json::json!(false),
                    ]],
                }],
            }],
        };

        let branches = extract_covered_branches(&json, "/project");
        assert_eq!(branches.len(), 1);
        assert_eq!(branches[0].line, u32::MAX);
        assert_eq!(branches[0].col, u16::MAX);
    }

    /// Line value larger than u32::MAX should truncate (potential data loss bug)
    #[test]
    fn bug_extract_branches_line_overflow_truncates() {
        let big_line = u32::MAX as u64 + 1; // 4294967296
        let json = LlvmCovJson {
            data: vec![LlvmCovData {
                files: vec![LlvmCovFile {
                    filename: "/project/src/lib.rs".to_string(),
                    segments: vec![vec![
                        serde_json::json!(big_line), serde_json::json!(1),
                        serde_json::json!(1), serde_json::json!(true),
                        serde_json::json!(true), serde_json::json!(false),
                    ]],
                }],
            }],
        };

        let branches = extract_covered_branches(&json, "/project");
        assert_eq!(branches.len(), 1);
        // `as u32` truncates: u32::MAX + 1 wraps to 0
        // This is a silent data corruption bug — line number becomes 0
        assert_eq!(branches[0].line, 0, "u64 -> u32 cast silently truncates large line numbers");
    }

    /// Col value larger than u16::MAX should truncate (potential data loss bug)
    #[test]
    fn bug_extract_branches_col_overflow_truncates() {
        let big_col = u16::MAX as u64 + 1; // 65536
        let json = LlvmCovJson {
            data: vec![LlvmCovData {
                files: vec![LlvmCovFile {
                    filename: "/project/src/lib.rs".to_string(),
                    segments: vec![vec![
                        serde_json::json!(1), serde_json::json!(big_col),
                        serde_json::json!(1), serde_json::json!(true),
                        serde_json::json!(true), serde_json::json!(false),
                    ]],
                }],
            }],
        };

        let branches = extract_covered_branches(&json, "/project");
        assert_eq!(branches.len(), 1);
        // `as u16` truncates: u16::MAX + 1 wraps to 0
        assert_eq!(branches[0].col, 0, "u64 -> u16 cast silently truncates large col numbers");
    }

    #[test]
    fn extract_branches_filters_correctly() {
        let json = LlvmCovJson {
            data: vec![LlvmCovData {
                files: vec![LlvmCovFile {
                    filename: "/project/src/lib.rs".to_string(),
                    segments: vec![
                        // [line, col, count, has_count, is_entry, is_gap]
                        // Covered entry → should be included
                        vec![
                            serde_json::json!(10),
                            serde_json::json!(5),
                            serde_json::json!(3),
                            serde_json::json!(true),
                            serde_json::json!(true),
                            serde_json::json!(false),
                        ],
                        // Uncovered entry (count=0) → should NOT be included
                        vec![
                            serde_json::json!(20),
                            serde_json::json!(1),
                            serde_json::json!(0),
                            serde_json::json!(true),
                            serde_json::json!(true),
                            serde_json::json!(false),
                        ],
                        // Gap region → should NOT be included
                        vec![
                            serde_json::json!(30),
                            serde_json::json!(1),
                            serde_json::json!(5),
                            serde_json::json!(true),
                            serde_json::json!(true),
                            serde_json::json!(true),
                        ],
                        // Not region entry → should NOT be included
                        vec![
                            serde_json::json!(40),
                            serde_json::json!(1),
                            serde_json::json!(2),
                            serde_json::json!(true),
                            serde_json::json!(false),
                            serde_json::json!(false),
                        ],
                    ],
                }],
            }],
        };

        let branches = extract_covered_branches(&json, "/project");
        assert_eq!(branches.len(), 1);
        assert_eq!(branches[0].line, 10);
    }

    #[test]
    fn bug_make_relative_sibling_dir() {
        let result = make_relative("/home/user/project2/src/lib.rs", "/home/user/project");
        assert_eq!(
            result, "/home/user/project2/src/lib.rs",
            "sibling dir should not be stripped"
        );
    }

    #[test]
    fn bug_extract_branches_null_skipped() {
        let json_str = r#"{"data":[{"files":[{"filename":"src/lib.rs","segments":[[null,null,1,true,true,false]]}]}]}"#;
        let json: LlvmCovJson = serde_json::from_str(json_str).unwrap();
        let branches = extract_covered_branches(&json, "");
        assert!(branches.is_empty(), "null line/col should skip the segment");
    }

    #[test]
    fn bug_extract_branches_large_col_saturates() {
        let json_str = r#"{"data":[{"files":[{"filename":"src/lib.rs","segments":[[10,70000,1,true,true,false]]}]}]}"#;
        let json: LlvmCovJson = serde_json::from_str(json_str).unwrap();
        let branches = extract_covered_branches(&json, "");
        assert_eq!(branches[0].col, u16::MAX, "column 70000 should saturate to u16::MAX");
    }

    #[test]
    fn bug_sanitize_angle_brackets() {
        let name = "MyType<T>::test\\path";
        let sanitized = name
            .replace("::", "__")
            .replace(['/', ' ', '<', '>', '\\'], "_");
        assert!(!sanitized.contains('<'), "sanitized name should not contain <");
        assert!(!sanitized.contains('>'), "sanitized name should not contain >");
        assert!(!sanitized.contains('\\'), "sanitized name should not contain \\");
    }

    // -----------------------------------------------------------------------
    // fnv1a — determinism and collision resistance
    // -----------------------------------------------------------------------

    #[test]
    fn fnv1a_deterministic() {
        for input in &[b"hello" as &[u8], b"", b"src/main.rs", b"\xff\x00\x01"] {
            assert_eq!(fnv1a(input), fnv1a(input));
        }
    }

    #[test]
    fn fnv1a_known_value() {
        let expected = 0xcbf29ce484222325_u64 ^ 0x61;
        let expected = expected.wrapping_mul(0x100000001b3);
        assert_eq!(fnv1a(b"a"), expected);
    }

    #[test]
    fn fnv1a_no_trivial_collisions() {
        let paths = vec![
            "src/main.rs", "src/lib.rs", "src/main.r", "src/main.rss",
            "src/Main.rs", "SRC/main.rs", "src/main.rs/",
        ];
        let hashes: Vec<u64> = paths.iter().map(|p| fnv1a(p.as_bytes())).collect();
        for i in 0..hashes.len() {
            for j in (i + 1)..hashes.len() {
                assert_ne!(hashes[i], hashes[j], "collision between '{}' and '{}'", paths[i], paths[j]);
            }
        }
    }

    // -----------------------------------------------------------------------
    // make_relative — additional edge cases
    // -----------------------------------------------------------------------

    #[test]
    fn make_relative_empty_target() {
        assert_eq!(make_relative("/some/path/file.rs", ""), "/some/path/file.rs");
    }

    #[test]
    fn make_relative_unicode_path() {
        assert_eq!(
            make_relative("/home/用户/项目/src/main.rs", "/home/用户/项目"),
            "src/main.rs"
        );
    }

    #[test]
    fn bug_make_relative_partial_overlap() {
        // "/home/user/project" should NOT match "/home/user/project-extra/..."
        let result = make_relative("/home/user/project-extra/src/main.rs", "/home/user/project");
        assert_eq!(result, "/home/user/project-extra/src/main.rs");
    }

    #[test]
    fn make_relative_windows_backslashes() {
        let result = make_relative("C:\\Users\\dev\\project\\src\\main.rs", "C:\\Users\\dev\\project");
        assert_eq!(result, "C:\\Users\\dev\\project\\src\\main.rs");
    }

    #[test]
    fn bug_extract_branches_col_truncation() {
        // Columns > u16::MAX silently truncate
        let json = LlvmCovJson {
            data: vec![LlvmCovData {
                files: vec![LlvmCovFile {
                    filename: "/project/src/lib.rs".to_string(),
                    segments: vec![
                        vec![
                            serde_json::json!(10),
                            serde_json::json!(70000),  // > u16::MAX
                            serde_json::json!(1),
                            serde_json::json!(true),
                            serde_json::json!(true),
                            serde_json::json!(false),
                        ],
                    ],
                }],
            }],
        };
        let branches = extract_covered_branches(&json, "/project");
        assert_eq!(branches.len(), 1);
        // Column saturates at u16::MAX
        assert_eq!(branches[0].col, u16::MAX);
    }

    #[test]
    fn bug_null_segment_values_create_degenerate_branch() {
        // After fix: segments with null line (→ as_u64()=None → 0) are rejected by
        // the line==0 guard, so no phantom branch is created.
        let json = LlvmCovJson {
            data: vec![LlvmCovData {
                files: vec![LlvmCovFile {
                    filename: "/proj/src/lib.rs".to_string(),
                    segments: vec![vec![
                        serde_json::json!(null), // line = null → 0 → rejected
                        serde_json::json!(null), // col = null
                        serde_json::json!(5),    // count = 5
                        serde_json::json!(true), // has_count
                        serde_json::json!(true), // is_entry
                        serde_json::json!(false), // is_gap
                    ]],
                }],
            }],
        };
        let branches = extract_covered_branches(&json, "/proj");
        assert_eq!(branches.len(), 0, "null line (→0) must be rejected by line==0 guard");
    }

    #[test]
    fn bug_string_segment_values_create_degenerate_branch() {
        // After fix: string values in line/col → as_u64()=None → 0 → rejected.
        let json = LlvmCovJson {
            data: vec![LlvmCovData {
                files: vec![LlvmCovFile {
                    filename: "/proj/src/lib.rs".to_string(),
                    segments: vec![vec![
                        serde_json::json!("not_a_number"),
                        serde_json::json!("also_not"),
                        serde_json::json!(1),
                        serde_json::json!(true),
                        serde_json::json!(true),
                        serde_json::json!(false),
                    ]],
                }],
            }],
        };
        let branches = extract_covered_branches(&json, "/proj");
        assert_eq!(branches.len(), 0, "string line (→0) must be rejected by line==0 guard");
    }

    #[test]
    fn bug_float_count_treated_as_zero() {
        // After fix: as_f64().map(|f| f as u64) correctly converts 1.0 → 1,
        // so a covered region with a float count is no longer dropped.
        let json = LlvmCovJson {
            data: vec![LlvmCovData {
                files: vec![LlvmCovFile {
                    filename: "/proj/src/lib.rs".to_string(),
                    segments: vec![vec![
                        serde_json::json!(10),
                        serde_json::json!(5),
                        serde_json::json!(1.0), // float count — now correctly read as 1
                        serde_json::json!(true),
                        serde_json::json!(true),
                        serde_json::json!(false),
                    ]],
                }],
            }],
        };
        let branches = extract_covered_branches(&json, "/proj");
        assert_eq!(branches.len(), 1, "float count 1.0 must be treated as covered");
        assert_eq!(branches[0].line, 10);
    }

    #[test]
    fn bug_float_count_undercounts_coverage_stats() {
        // After fix: parse_coverage_stats also uses as_f64(), so 1.0 is counted as covered.
        let json = LlvmCovJson {
            data: vec![LlvmCovData {
                files: vec![LlvmCovFile {
                    filename: "/proj/src/lib.rs".to_string(),
                    segments: vec![vec![
                        serde_json::json!(10),
                        serde_json::json!(5),
                        serde_json::json!(1.0), // float count
                        serde_json::json!(true),
                        serde_json::json!(true),
                        serde_json::json!(false),
                    ]],
                }],
            }],
        };
        let (_, total, covered) = parse_coverage_stats(&json, "/proj");
        assert_eq!(total, 1, "segment must be counted in total");
        assert_eq!(covered, 1, "float count 1.0 must be treated as covered");
    }

    #[test]
    fn bug_negative_line_number_defaults_to_zero() {
        // After fix: negative line → as_u64()=None → 0 → rejected by line==0 guard.
        let json = LlvmCovJson {
            data: vec![LlvmCovData {
                files: vec![LlvmCovFile {
                    filename: "/proj/src/lib.rs".to_string(),
                    segments: vec![vec![
                        serde_json::json!(-1),  // negative line → as_u64()=None → 0
                        serde_json::json!(5),
                        serde_json::json!(1),
                        serde_json::json!(true),
                        serde_json::json!(true),
                        serde_json::json!(false),
                    ]],
                }],
            }],
        };
        let branches = extract_covered_branches(&json, "/proj");
        assert_eq!(branches.len(), 0, "negative line (→0) must be rejected by line==0 guard");
    }

    #[test]
    fn bug_make_relative_empty_path_produces_constant_file_id() {
        // When path == target, make_relative returns ".". Two different projects
        // whose paths equal their own targets both produce fnv1a(b"."), causing a
        // file_id collision in the index. This is a known limitation documented here.
        let id1 = fnv1a(make_relative("/proj", "/proj").as_bytes());
        let id2 = fnv1a(make_relative("/other", "/other").as_bytes());
        // Both return "." → same hash
        assert_eq!(
            id1, id2,
            "paths equal to their own targets both map to '.' → same file_id collision"
        );
    }

    #[test]
    fn bug_has_count_non_bool_defaults_false() {
        // After fix: integer 1 for has_count is accepted (as_bool() OR as_u64()!=0).
        let json = LlvmCovJson {
            data: vec![LlvmCovData {
                files: vec![LlvmCovFile {
                    filename: "/proj/src/lib.rs".to_string(),
                    segments: vec![vec![
                        serde_json::json!(10),
                        serde_json::json!(1),
                        serde_json::json!(5),
                        serde_json::json!(1),     // has_count as integer 1
                        serde_json::json!(true),
                        serde_json::json!(false),
                    ]],
                }],
            }],
        };
        let branches = extract_covered_branches(&json, "/proj");
        assert_eq!(branches.len(), 1, "integer 1 for has_count must be treated as true");
        assert_eq!(branches[0].line, 10);
    }
}
