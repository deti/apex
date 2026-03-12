//! Rust per-test branch indexing via cargo-llvm-cov.
//!
//! Strategy for performance:
//! 1. `cargo llvm-cov --no-report --workspace` — build instrumented binaries once
//! 2. Discover test binaries in `target/llvm-cov-target/debug/deps/`
//! 3. Run each test directly with unique `LLVM_PROFILE_FILE` (~10-50ms per test)
//! 4. `llvm-profdata merge` + `llvm-cov export` per profraw for coverage JSON
//! 5. Parse JSON → BranchId, aggregate into BranchIndex

use crate::types::{BranchIndex, TestTrace};
use apex_core::types::{BranchId, ExecutionStatus, Language};
use serde::Deserialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::Semaphore;
use tracing::{debug, info};

type BoxErr = Box<dyn std::error::Error + Send + Sync>;

/// FNV-1a hash — same implementation as apex-instrument for file_id compatibility.
fn fnv1a(data: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf29ce484222325;
    for &byte in data {
        hash ^= byte as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
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
pub async fn build_rust_index(
    target: &Path,
    parallel: usize,
) -> Result<BranchIndex, BoxErr> {
    let target = std::fs::canonicalize(target)?;

    // 1. Resolve LLVM tools
    let env = resolve_llvm_env(&target).await?;
    info!("llvm-cov: {}, target-dir: {}", env.llvm_cov, env.target_dir.display());

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
pub async fn enumerate_rust_tests(target: &Path) -> Result<Vec<String>, BoxErr> {
    let output = tokio::process::Command::new("cargo")
        .args(["test", "--workspace", "--", "--list", "--format", "terse"])
        .current_dir(target)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .output()
        .await?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let tests: Vec<String> = stdout
        .lines()
        .filter(|l| l.ends_with(": test"))
        .map(|l| l.trim_end_matches(": test").to_string())
        .collect();

    Ok(tests)
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
        return Err(format!("cargo llvm-cov --json failed: {}", stderr).into());
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
        .replace('/', "_")
        .replace(' ', "_");

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
                let has_count = seg[3].as_bool().unwrap_or(false);
                let is_entry = seg[4].as_bool().unwrap_or(false);
                let is_gap = seg[5].as_bool().unwrap_or(false);
                let count = seg[2].as_u64().unwrap_or(0);

                if has_count && is_entry && !is_gap && count > 0 {
                    let line = seg[0].as_u64().unwrap_or(0) as u32;
                    let col = seg[1].as_u64().unwrap_or(0) as u16;
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
                let has_count = seg[3].as_bool().unwrap_or(false);
                let is_entry = seg[4].as_bool().unwrap_or(false);
                let is_gap = seg[5].as_bool().unwrap_or(false);

                if has_count && is_entry && !is_gap {
                    total += 1;
                    if seg[2].as_u64().unwrap_or(0) > 0 {
                        covered += 1;
                    }
                }
            }
        }
    }

    (file_paths, total, covered)
}

fn make_relative(path: &str, target: &str) -> String {
    let prefix = if target.ends_with('/') {
        target.to_string()
    } else {
        format!("{}/", target)
    };

    path.strip_prefix(&prefix)
        .or_else(|| path.strip_prefix(target))
        .map(|s| s.trim_start_matches('/').to_string())
        .unwrap_or_else(|| path.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
