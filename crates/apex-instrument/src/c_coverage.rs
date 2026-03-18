//! Shared C/C++ coverage instrumentor using gcov or llvm-cov.
//!
//! Parses gcov text output to extract line-level coverage data and converts
//! it to `BranchId` entries using `fnv1a_hash` for file identification.
//!
//! The gcov workflow:
//! 1. Compile `.c` files with `gcc --coverage` (or `cc --coverage`)
//! 2. Run the resulting binary to generate `.gcda` profiling data
//! 3. Run `gcov` to convert `.gcda` → `.gcov` text files
//! 4. Parse `.gcov` output into `BranchId` entries

use apex_core::{
    error::{ApexError, Result},
    hash::fnv1a_hash,
    traits::Instrumentor,
    types::{BranchId, InstrumentedTarget, Target},
};
use async_trait::async_trait;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use tracing::{debug, info, warn};

/// Coverage instrumentor for C and C++ projects.
///
/// Supports two compilation paths:
/// - **gcov:** compile with `-fprofile-arcs -ftest-coverage`, parse `.gcov` files
/// - **llvm-cov:** compile with `-fprofile-instr-generate -fcoverage-mapping`,
///   use `llvm-cov export`
///
/// Currently implements gcov text output parsing.
pub struct CCoverageInstrumentor {
    branch_ids: Vec<BranchId>,
}

impl CCoverageInstrumentor {
    pub fn new() -> Self {
        CCoverageInstrumentor {
            branch_ids: Vec::new(),
        }
    }
}

impl Default for CCoverageInstrumentor {
    fn default() -> Self {
        Self::new()
    }
}

/// A parsed line from gcov output.
#[derive(Debug, Clone, PartialEq)]
pub enum GcovLine {
    /// Non-executable line (marked with `-:`).
    NonExecutable,
    /// Unexecuted line (marked with `#####:`).
    Unexecuted { line_number: u32, source: String },
    /// Executed line with a count.
    Executed {
        count: u64,
        line_number: u32,
        source: String,
    },
}

/// Parse a single gcov output line.
///
/// Format: `execution_count:line_number:source_text`
/// - `-:` prefix means non-executable
/// - `#####:` prefix means unexecuted (0 count)
/// - `N:` where N is a number means executed N times
pub fn parse_gcov_line(line: &str) -> Option<GcovLine> {
    let colon1 = line.find(':')?;
    let count_str = line[..colon1].trim();

    let rest = &line[colon1 + 1..];
    let colon2 = rest.find(':')?;
    let line_num_str = rest[..colon2].trim();
    let source = rest[colon2 + 1..].to_string();

    if count_str == "-" {
        return Some(GcovLine::NonExecutable);
    }

    let line_number: u32 = line_num_str.parse().ok()?;

    if count_str == "#####" {
        return Some(GcovLine::Unexecuted {
            line_number,
            source,
        });
    }

    let count: u64 = count_str.parse().ok()?;
    Some(GcovLine::Executed {
        count,
        line_number,
        source,
    })
}

/// Parse full gcov output for a single file.
/// Returns (all_branches, executed_branches, file_id).
pub fn parse_gcov_output(file_path: &str, gcov_text: &str) -> (Vec<BranchId>, Vec<BranchId>, u64) {
    let file_id = fnv1a_hash(file_path);
    let mut all_branches = Vec::new();
    let mut executed_branches = Vec::new();

    for line in gcov_text.lines() {
        match parse_gcov_line(line) {
            Some(GcovLine::Executed {
                count, line_number, ..
            }) => {
                let branch = BranchId::new(file_id, line_number, 0, 0);
                all_branches.push(branch.clone());
                if count > 0 {
                    executed_branches.push(branch);
                }
            }
            Some(GcovLine::Unexecuted { line_number, .. }) => {
                let branch = BranchId::new(file_id, line_number, 0, 0);
                all_branches.push(branch);
            }
            _ => {}
        }
    }

    (all_branches, executed_branches, file_id)
}

/// Scan a directory for .gcov files and parse them all.
pub fn scan_gcov_files(dir: &Path) -> (Vec<BranchId>, Vec<BranchId>, HashMap<u64, PathBuf>) {
    let mut all = Vec::new();
    let mut executed = Vec::new();
    let mut file_paths = HashMap::new();

    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().is_some_and(|e| e == "gcov") {
                if let Ok(content) = std::fs::read_to_string(&path) {
                    // The .gcov filename is typically source.c.gcov
                    let source_name = path
                        .file_stem()
                        .and_then(|s| s.to_str())
                        .unwrap_or("unknown");
                    let (a, e, fid) = parse_gcov_output(source_name, &content);
                    file_paths.insert(fid, PathBuf::from(source_name));
                    all.extend(a);
                    executed.extend(e);
                }
            }
        }
    }

    (all, executed, file_paths)
}

/// Timeout for C compilation (5 minutes — large projects like the Linux kernel
/// need significant compile time even for a subset).
const COMPILE_TIMEOUT_MS: u64 = 300_000;

/// Timeout for running the compiled test binary (2 minutes).
const TEST_RUN_TIMEOUT_MS: u64 = 120_000;

/// Timeout for gcov processing (1 minute).
const GCOV_TIMEOUT_MS: u64 = 60_000;

/// Collect `.c` files from a directory, recursing into subdirectories but
/// skipping build/VCS directories.
fn collect_c_files(root: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    if let Ok(entries) = std::fs::read_dir(root) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
                if !matches!(
                    name,
                    "build" | "build_apex" | ".git" | "target" | "node_modules"
                ) {
                    files.extend(collect_c_files(&path));
                }
            } else if path.extension().is_some_and(|e| e == "c") {
                files.push(path);
            }
        }
    }
    files
}

/// Check if a compiler/tool is available on PATH.
fn has_tool(name: &str) -> bool {
    std::process::Command::new("which")
        .arg(name)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Detect whether the system `gcc` is actually Apple clang (masquerading).
fn is_apple_clang() -> bool {
    std::process::Command::new("gcc")
        .arg("--version")
        .output()
        .map(|o| {
            let stdout = String::from_utf8_lossy(&o.stdout);
            stdout.contains("Apple clang") || stdout.contains("Apple LLVM")
        })
        .unwrap_or(false)
}

/// Compile C files with coverage instrumentation, run the binary, collect
/// coverage data, and parse it into branch data.
///
/// Two strategies:
/// 1. **clang path** (macOS default): `-fprofile-instr-generate -fcoverage-mapping`
///    → `llvm-profdata merge` → `llvm-cov export` → parse JSON
/// 2. **gcc path** (Linux default): `--coverage` → run → `gcov` → parse `.gcov`
async fn compile_and_run_gcov(
    target: &Target,
) -> Result<(Vec<BranchId>, Vec<BranchId>, HashMap<u64, PathBuf>, PathBuf)> {
    let c_files = collect_c_files(&target.root);
    if c_files.is_empty() {
        debug!("no .c files found in {}", target.root.display());
        return Ok((Vec::new(), Vec::new(), HashMap::new(), target.root.clone()));
    }

    let build_dir = target.root.join("build_apex_gcov");
    std::fs::create_dir_all(&build_dir)
        .map_err(|e| ApexError::Instrumentation(format!("create build dir: {e}")))?;

    // On macOS, gcc is Apple clang — use the clang/llvm-cov path.
    // On Linux with real gcc, use the gcov path.
    let use_clang_path = has_tool("clang") && (is_apple_clang() || !has_tool("gcov"));

    if use_clang_path {
        compile_and_run_llvm_cov(target, &c_files, &build_dir).await
    } else {
        compile_and_run_gcc_gcov(target, &c_files, &build_dir).await
    }
}

/// Clang/llvm-cov path: compile with source-based coverage, run binary,
/// merge profdata, export JSON, parse line counts.
async fn compile_and_run_llvm_cov(
    target: &Target,
    c_files: &[PathBuf],
    build_dir: &Path,
) -> Result<(Vec<BranchId>, Vec<BranchId>, HashMap<u64, PathBuf>, PathBuf)> {
    let binary_path = build_dir.join("apex_cov_test");
    let profraw_path = build_dir.join("default.profraw");
    let profdata_path = build_dir.join("default.profdata");

    let relative_c_files: Vec<String> = c_files
        .iter()
        .map(|f| {
            f.strip_prefix(&target.root)
                .unwrap_or(f)
                .to_string_lossy()
                .into_owned()
        })
        .collect();

    let mut args: Vec<String> = vec![
        "-fprofile-instr-generate".to_string(),
        "-fcoverage-mapping".to_string(),
        "-g".to_string(),
        "-O0".to_string(),
        "-o".to_string(),
        binary_path.to_string_lossy().into_owned(),
    ];
    args.extend(relative_c_files);

    info!(files = c_files.len(), "compiling C files with clang coverage");

    // Step 1: Compile
    let compile_out = tokio::time::timeout(
        std::time::Duration::from_millis(COMPILE_TIMEOUT_MS),
        tokio::process::Command::new("clang")
            .args(&args)
            .current_dir(&target.root)
            .output(),
    )
    .await
    .map_err(|_| ApexError::Instrumentation("clang compile timed out".into()))?
    .map_err(|e| ApexError::Instrumentation(format!("clang compile: {e}")))?;

    if !compile_out.status.success() {
        let stderr = String::from_utf8_lossy(&compile_out.stderr);
        warn!(%stderr, "clang compilation failed");
        return Ok((Vec::new(), Vec::new(), HashMap::new(), build_dir.to_path_buf()));
    }

    // Step 2: Run binary (produces .profraw)
    let run_result = tokio::time::timeout(
        std::time::Duration::from_millis(TEST_RUN_TIMEOUT_MS),
        tokio::process::Command::new(&binary_path)
            .current_dir(&target.root)
            .env("LLVM_PROFILE_FILE", &profraw_path)
            .output(),
    )
    .await;

    match &run_result {
        Ok(Ok(output)) if !output.status.success() => {
            debug!(exit_code = output.status.code().unwrap_or(-1), "binary exited non-zero");
        }
        Ok(Err(e)) => {
            warn!("failed to run binary: {e}");
            return Ok((Vec::new(), Vec::new(), HashMap::new(), build_dir.to_path_buf()));
        }
        Err(_) => {
            warn!("binary timed out after {TEST_RUN_TIMEOUT_MS}ms");
        }
        _ => {}
    }

    // Step 3: Merge profraw → profdata
    let profdata_tool = if has_tool("llvm-profdata") {
        "llvm-profdata"
    } else if has_tool("xcrun") {
        // Will use "xcrun llvm-profdata"
        "xcrun"
    } else {
        warn!("llvm-profdata not found; cannot process coverage");
        return Ok((Vec::new(), Vec::new(), HashMap::new(), build_dir.to_path_buf()));
    };

    let mut merge_args: Vec<String> = Vec::new();
    if profdata_tool == "xcrun" {
        merge_args.push("llvm-profdata".to_string());
    }
    merge_args.extend([
        "merge".to_string(),
        "-sparse".to_string(),
        profraw_path.to_string_lossy().into_owned(),
        "-o".to_string(),
        profdata_path.to_string_lossy().into_owned(),
    ]);

    let merge_out = tokio::time::timeout(
        std::time::Duration::from_millis(GCOV_TIMEOUT_MS),
        tokio::process::Command::new(profdata_tool)
            .args(&merge_args)
            .output(),
    )
    .await
    .map_err(|_| ApexError::Instrumentation("llvm-profdata merge timed out".into()))?
    .map_err(|e| ApexError::Instrumentation(format!("llvm-profdata: {e}")))?;

    if !merge_out.status.success() {
        let stderr = String::from_utf8_lossy(&merge_out.stderr);
        warn!(%stderr, "llvm-profdata merge failed");
        return Ok((Vec::new(), Vec::new(), HashMap::new(), build_dir.to_path_buf()));
    }

    // Step 4: Export coverage as JSON
    let cov_tool = if has_tool("llvm-cov") {
        "llvm-cov"
    } else if has_tool("xcrun") {
        "xcrun"
    } else {
        warn!("llvm-cov not found; cannot export coverage");
        return Ok((Vec::new(), Vec::new(), HashMap::new(), build_dir.to_path_buf()));
    };

    let mut export_args: Vec<String> = Vec::new();
    if cov_tool == "xcrun" {
        export_args.push("llvm-cov".to_string());
    }
    export_args.extend([
        "export".to_string(),
        binary_path.to_string_lossy().into_owned(),
        format!("-instr-profile={}", profdata_path.display()),
        "--format=text".to_string(),
    ]);

    let export_out = tokio::time::timeout(
        std::time::Duration::from_millis(GCOV_TIMEOUT_MS),
        tokio::process::Command::new(cov_tool)
            .args(&export_args)
            .current_dir(&target.root)
            .output(),
    )
    .await
    .map_err(|_| ApexError::Instrumentation("llvm-cov export timed out".into()))?
    .map_err(|e| ApexError::Instrumentation(format!("llvm-cov export: {e}")))?;

    if !export_out.status.success() {
        let stderr = String::from_utf8_lossy(&export_out.stderr);
        warn!(%stderr, "llvm-cov export failed");
        return Ok((Vec::new(), Vec::new(), HashMap::new(), build_dir.to_path_buf()));
    }

    // Step 5: Parse JSON coverage
    let json_str = String::from_utf8_lossy(&export_out.stdout);
    let (all, executed, file_paths) = parse_llvm_cov_json(&json_str, &target.root);

    info!(total = all.len(), executed = executed.len(), "clang coverage collected");
    Ok((all, executed, file_paths, build_dir.to_path_buf()))
}

/// Interpret a JSON value as a boolean: supports both `true`/`false` literals
/// and integer `0`/`1` (different LLVM versions use different encodings).
fn json_truthy(v: &serde_json::Value) -> bool {
    v.as_bool().unwrap_or_else(|| v.as_u64().unwrap_or(0) != 0)
}

/// Parse `llvm-cov export` JSON output into branch data.
///
/// The JSON structure has `data[].files[].segments[]` where each segment is
/// `[line, col, count, has_count, is_region_entry, is_gap_region]`.
pub fn parse_llvm_cov_json(
    json_str: &str,
    target_root: &Path,
) -> (Vec<BranchId>, Vec<BranchId>, HashMap<u64, PathBuf>) {
    let mut all_branches = Vec::new();
    let mut executed_branches = Vec::new();
    let mut file_paths: HashMap<u64, PathBuf> = HashMap::new();

    let Ok(json) = serde_json::from_str::<serde_json::Value>(json_str) else {
        warn!("failed to parse llvm-cov JSON");
        return (all_branches, executed_branches, file_paths);
    };

    let Some(data_arr) = json.get("data").and_then(|d| d.as_array()) else {
        return (all_branches, executed_branches, file_paths);
    };

    for data in data_arr {
        let Some(files) = data.get("files").and_then(|f| f.as_array()) else {
            continue;
        };
        for file in files {
            let Some(filename) = file.get("filename").and_then(|f| f.as_str()) else {
                continue;
            };
            let rel = Path::new(filename)
                .strip_prefix(target_root)
                .unwrap_or(Path::new(filename));
            let file_id = fnv1a_hash(&rel.to_string_lossy());
            file_paths.insert(file_id, rel.to_path_buf());

            // Parse segments: each is [line, col, count, has_count, is_region_entry, is_gap]
            // In LLVM JSON, has_count and is_region_entry can be bools or ints.
            let Some(segments) = file.get("segments").and_then(|s| s.as_array()) else {
                continue;
            };
            for seg in segments {
                let Some(seg_arr) = seg.as_array() else { continue };
                if seg_arr.len() < 5 {
                    continue;
                }
                let line = seg_arr[0].as_u64().unwrap_or(0) as u32;
                let col = seg_arr[1].as_u64().unwrap_or(0) as u16;
                let count = seg_arr[2].as_u64().unwrap_or(0);
                let has_count = json_truthy(&seg_arr[3]);
                let is_region_entry = json_truthy(&seg_arr[4]);

                if !has_count || !is_region_entry {
                    continue;
                }

                let branch = BranchId::new(file_id, line, col, 0);
                all_branches.push(branch.clone());
                if count > 0 {
                    executed_branches.push(branch);
                }
            }
        }
    }

    (all_branches, executed_branches, file_paths)
}

/// gcc/gcov path: compile with `--coverage`, run binary, run `gcov`,
/// parse `.gcov` text files.
async fn compile_and_run_gcc_gcov(
    target: &Target,
    c_files: &[PathBuf],
    build_dir: &Path,
) -> Result<(Vec<BranchId>, Vec<BranchId>, HashMap<u64, PathBuf>, PathBuf)> {
    let compiler = if has_tool("gcc") {
        "gcc"
    } else if has_tool("cc") {
        "cc"
    } else {
        return Err(ApexError::Instrumentation(
            "no C compiler found (need gcc or cc on PATH)".into(),
        ));
    };

    let binary_path = build_dir.join("apex_gcov_test");

    let relative_c_files: Vec<String> = c_files
        .iter()
        .map(|f| {
            f.strip_prefix(&target.root)
                .unwrap_or(f)
                .to_string_lossy()
                .into_owned()
        })
        .collect();

    let mut args: Vec<String> = vec![
        "--coverage".to_string(),
        "-g".to_string(),
        "-O0".to_string(),
        "-o".to_string(),
        binary_path.to_string_lossy().into_owned(),
    ];
    args.extend(relative_c_files.clone());

    info!(
        compiler = compiler,
        files = c_files.len(),
        "compiling C files with --coverage for gcov"
    );

    // Step 1: Compile from target root
    let compile_out = tokio::time::timeout(
        std::time::Duration::from_millis(COMPILE_TIMEOUT_MS),
        tokio::process::Command::new(compiler)
            .args(&args)
            .current_dir(&target.root)
            .output(),
    )
    .await
    .map_err(|_| {
        ApexError::Instrumentation(format!(
            "compilation timed out after {COMPILE_TIMEOUT_MS}ms"
        ))
    })?
    .map_err(|e| ApexError::Instrumentation(format!("compile: {e}")))?;

    if !compile_out.status.success() {
        let stderr = String::from_utf8_lossy(&compile_out.stderr);
        warn!(%stderr, "gcov compilation failed; returning empty coverage");
        return Ok((Vec::new(), Vec::new(), HashMap::new(), build_dir.to_path_buf()));
    }

    info!("gcov compilation succeeded, running binary");

    // Step 2: Run binary to produce .gcda files
    let run_result = tokio::time::timeout(
        std::time::Duration::from_millis(TEST_RUN_TIMEOUT_MS),
        tokio::process::Command::new(&binary_path)
            .current_dir(&target.root)
            .output(),
    )
    .await;

    match run_result {
        Ok(Ok(output)) => {
            if !output.status.success() {
                let code = output.status.code().unwrap_or(-1);
                debug!(exit_code = code, "test binary exited non-zero (may still have coverage)");
            }
        }
        Ok(Err(e)) => {
            warn!("failed to run test binary: {e}");
            return Ok((Vec::new(), Vec::new(), HashMap::new(), build_dir.to_path_buf()));
        }
        Err(_) => {
            warn!("test binary timed out after {TEST_RUN_TIMEOUT_MS}ms");
        }
    }

    // Step 3: Run gcov
    let mut gcov_args: Vec<String> = Vec::new();
    gcov_args.extend(relative_c_files);

    let gcov_out = tokio::time::timeout(
        std::time::Duration::from_millis(GCOV_TIMEOUT_MS),
        tokio::process::Command::new("gcov")
            .args(&gcov_args)
            .current_dir(&target.root)
            .output(),
    )
    .await
    .map_err(|_| {
        ApexError::Instrumentation(format!("gcov timed out after {GCOV_TIMEOUT_MS}ms"))
    })?
    .map_err(|e| ApexError::Instrumentation(format!("gcov: {e}")))?;

    if !gcov_out.status.success() {
        let stderr = String::from_utf8_lossy(&gcov_out.stderr);
        debug!(%stderr, "gcov returned non-zero (partial results may exist)");
    }

    // Step 4: Parse .gcov files
    let (all, executed, file_paths) = scan_gcov_files(&target.root);

    info!(total = all.len(), executed = executed.len(), "gcov coverage collected");
    Ok((all, executed, file_paths, build_dir.to_path_buf()))
}

#[async_trait]
impl Instrumentor for CCoverageInstrumentor {
    fn branch_ids(&self) -> &[BranchId] {
        &self.branch_ids
    }

    async fn instrument(&self, target: &Target) -> Result<InstrumentedTarget> {
        // First check for pre-existing .gcov files in the target directory.
        let (all_branches, executed_branches, file_paths) = scan_gcov_files(&target.root);

        if !all_branches.is_empty() {
            info!(
                total = all_branches.len(),
                executed = executed_branches.len(),
                "using pre-existing .gcov files"
            );
            return Ok(InstrumentedTarget {
                target: target.clone(),
                branch_ids: all_branches,
                executed_branch_ids: executed_branches,
                file_paths,
                work_dir: target.root.clone(),
            });
        }

        // No pre-existing .gcov files — compile with --coverage and run gcov.
        info!("no pre-existing .gcov files; compiling with --coverage");
        let (all_branches, executed_branches, file_paths, work_dir) =
            compile_and_run_gcov(target).await?;

        Ok(InstrumentedTarget {
            target: target.clone(),
            branch_ids: all_branches,
            executed_branch_ids: executed_branches,
            file_paths,
            work_dir,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_non_executable_line() {
        let result = parse_gcov_line("        -:    0:Source:hello.c");
        assert_eq!(result, Some(GcovLine::NonExecutable));
    }

    #[test]
    fn parse_unexecuted_line() {
        let result = parse_gcov_line("    #####:    5:    return -1;");
        assert!(matches!(
            result,
            Some(GcovLine::Unexecuted { line_number: 5, .. })
        ));
    }

    #[test]
    fn parse_executed_line() {
        let result = parse_gcov_line("       10:    3:    x = x + 1;");
        assert!(matches!(
            result,
            Some(GcovLine::Executed {
                count: 10,
                line_number: 3,
                ..
            })
        ));
    }

    #[test]
    fn parse_executed_high_count() {
        let result = parse_gcov_line("  1000000:   42:    loop_body();");
        assert!(matches!(
            result,
            Some(GcovLine::Executed {
                count: 1000000,
                line_number: 42,
                ..
            })
        ));
    }

    #[test]
    fn parse_gcov_output_mixed() {
        let gcov = "\
        -:    0:Source:test.c\n\
        -:    1:#include <stdio.h>\n\
       10:    2:int main() {\n\
        5:    3:    int x = 0;\n\
    #####:    4:    if (x > 0) {\n\
    #####:    5:        printf(\"positive\");\n\
        5:    6:    }\n\
        5:    7:    return 0;\n\
        -:    8:}";

        let (all, executed, _file_id) = parse_gcov_output("test.c", gcov);
        assert_eq!(all.len(), 6); // lines 2,3,4,5,6,7
        assert_eq!(executed.len(), 4); // lines 2,3,6,7
    }

    #[test]
    fn parse_gcov_output_empty() {
        let (all, executed, _) = parse_gcov_output("empty.c", "");
        assert!(all.is_empty());
        assert!(executed.is_empty());
    }

    #[test]
    fn parse_gcov_output_all_executed() {
        let gcov = "\
        1:    1:int main() {\n\
        1:    2:    return 0;\n\
        -:    3:}";
        let (all, executed, _) = parse_gcov_output("main.c", gcov);
        assert_eq!(all.len(), 2);
        assert_eq!(executed.len(), 2);
    }

    #[test]
    fn parse_gcov_output_uses_fnv1a() {
        let (all, _, file_id) = parse_gcov_output("src/lib.c", "    1:    1:code");
        assert_eq!(file_id, fnv1a_hash("src/lib.c"));
        assert_eq!(all[0].file_id, file_id);
    }

    #[test]
    fn scan_gcov_files_empty_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let (all, executed, paths) = scan_gcov_files(tmp.path());
        assert!(all.is_empty());
        assert!(executed.is_empty());
        assert!(paths.is_empty());
    }

    #[test]
    fn scan_gcov_files_with_gcov_file() {
        let tmp = tempfile::tempdir().unwrap();
        let gcov_content = "    1:    1:int main() {\n    1:    2:    return 0;\n";
        std::fs::write(tmp.path().join("main.c.gcov"), gcov_content).unwrap();

        let (all, executed, paths) = scan_gcov_files(tmp.path());
        assert_eq!(all.len(), 2);
        assert_eq!(executed.len(), 2);
        assert_eq!(paths.len(), 1);
        // The file stem of "main.c.gcov" is "main.c"
        assert!(paths.values().any(|p| p.to_str() == Some("main.c")));
    }

    #[test]
    fn instrumentor_default() {
        let instr = CCoverageInstrumentor::default();
        // Just verify it constructs without panic
        let _ = instr;
    }

    #[test]
    fn parse_gcov_line_invalid() {
        assert!(parse_gcov_line("not a valid line").is_none());
    }

    #[tokio::test]
    async fn instrumentor_empty_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let target = Target {
            root: tmp.path().to_path_buf(),
            language: apex_core::types::Language::C,
            test_command: Vec::new(),
        };
        let instr = CCoverageInstrumentor::new();
        let result = instr.instrument(&target).await.unwrap();
        assert!(result.branch_ids.is_empty());
        assert!(result.executed_branch_ids.is_empty());
    }

    #[test]
    fn collect_c_files_finds_files() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("main.c"), "int main() {}").unwrap();
        std::fs::write(tmp.path().join("util.c"), "void util() {}").unwrap();
        std::fs::write(tmp.path().join("readme.txt"), "not C").unwrap();
        let files = collect_c_files(tmp.path());
        assert_eq!(files.len(), 2);
        assert!(files.iter().all(|f| f.extension().unwrap() == "c"));
    }

    #[test]
    fn collect_c_files_skips_build_dirs() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join("build")).unwrap();
        std::fs::write(tmp.path().join("build/gen.c"), "").unwrap();
        std::fs::create_dir_all(tmp.path().join(".git")).unwrap();
        std::fs::write(tmp.path().join(".git/hook.c"), "").unwrap();
        std::fs::write(tmp.path().join("app.c"), "int main() {}").unwrap();
        let files = collect_c_files(tmp.path());
        assert_eq!(files.len(), 1);
    }

    #[test]
    fn collect_c_files_recurses_into_src() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join("src")).unwrap();
        std::fs::write(tmp.path().join("src/lib.c"), "void f() {}").unwrap();
        let files = collect_c_files(tmp.path());
        assert_eq!(files.len(), 1);
    }

    #[test]
    fn collect_c_files_empty_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let files = collect_c_files(tmp.path());
        assert!(files.is_empty());
    }

    #[test]
    fn has_tool_finds_cc() {
        // cc should be available on any Unix system
        assert!(has_tool("cc"));
    }

    #[test]
    fn has_tool_returns_false_for_missing() {
        assert!(!has_tool("nonexistent_tool_xyz_12345"));
    }

    #[test]
    fn timeout_constants_are_reasonable() {
        assert!(COMPILE_TIMEOUT_MS >= 60_000, "compile timeout should be >= 1 min");
        assert!(TEST_RUN_TIMEOUT_MS >= 30_000, "test timeout should be >= 30s");
        assert!(GCOV_TIMEOUT_MS >= 10_000, "gcov timeout should be >= 10s");
    }

    #[tokio::test]
    async fn instrumentor_prefers_existing_gcov_files() {
        let tmp = tempfile::tempdir().unwrap();
        // Write a .gcov file and also a .c file.
        // The instrumentor should use the .gcov file without recompiling.
        let gcov_content = "    1:    1:int main() {\n    1:    2:    return 0;\n";
        std::fs::write(tmp.path().join("main.c.gcov"), gcov_content).unwrap();
        std::fs::write(tmp.path().join("main.c"), "int main() { return 0; }").unwrap();

        let target = Target {
            root: tmp.path().to_path_buf(),
            language: apex_core::types::Language::C,
            test_command: Vec::new(),
        };
        let instr = CCoverageInstrumentor::new();
        let result = instr.instrument(&target).await.unwrap();
        assert_eq!(result.branch_ids.len(), 2);
        assert_eq!(result.executed_branch_ids.len(), 2);
        // work_dir should be the target root (no build_apex_gcov created)
        assert_eq!(result.work_dir, tmp.path().to_path_buf());
    }

    #[tokio::test]
    async fn instrumentor_gcov_fallback_compiles_c() {
        // Create a minimal C project with a main() so gcc --coverage works.
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(
            tmp.path().join("main.c"),
            "int main(void) { int x = 1; if (x) { return 0; } return 1; }\n",
        )
        .unwrap();

        let target = Target {
            root: tmp.path().to_path_buf(),
            language: apex_core::types::Language::C,
            test_command: Vec::new(),
        };

        let instr = CCoverageInstrumentor::new();
        let result = instr.instrument(&target).await.unwrap();

        // Should have found branches via gcov/clang coverage.
        // On CI without gcc/clang, this may return 0 branches.
        if has_tool("gcc") || has_tool("cc") || has_tool("clang") {
            assert!(
                !result.branch_ids.is_empty(),
                "gcov fallback should produce branch data for a valid C file"
            );
            assert!(
                !result.executed_branch_ids.is_empty(),
                "running the binary should produce some executed branches"
            );
            // work_dir should be the build_apex_gcov subdirectory
            assert!(result.work_dir.ends_with("build_apex_gcov"));
        }
    }

    #[test]
    fn parse_llvm_cov_json_basic() {
        let json = r#"{
            "data": [{
                "files": [{
                    "filename": "/project/src/main.c",
                    "segments": [
                        [1, 1, 5, true, true, false],
                        [2, 1, 0, true, true, false],
                        [3, 1, 5, true, false, false]
                    ]
                }]
            }],
            "type": "llvm.coverage.json.export",
            "version": "2.0.1"
        }"#;

        let (all, executed, paths) =
            parse_llvm_cov_json(json, Path::new("/project"));
        // Two region entries (has_count=true, is_region_entry=true): lines 1 and 2
        assert_eq!(all.len(), 2);
        // Line 1 has count=5 (executed), line 2 has count=0 (not executed)
        assert_eq!(executed.len(), 1);
        assert_eq!(paths.len(), 1);
        assert!(paths.values().any(|p| p.to_str() == Some("src/main.c")));
    }

    #[test]
    fn parse_llvm_cov_json_empty_data() {
        let json = r#"{"data": [], "type": "llvm.coverage.json.export"}"#;
        let (all, executed, paths) =
            parse_llvm_cov_json(json, Path::new("/tmp"));
        assert!(all.is_empty());
        assert!(executed.is_empty());
        assert!(paths.is_empty());
    }

    #[test]
    fn parse_llvm_cov_json_invalid() {
        let (all, executed, paths) =
            parse_llvm_cov_json("not json", Path::new("/tmp"));
        assert!(all.is_empty());
        assert!(executed.is_empty());
        assert!(paths.is_empty());
    }

    #[test]
    fn parse_llvm_cov_json_integer_booleans() {
        // Some LLVM versions use 0/1 instead of true/false
        let json = r#"{
            "data": [{
                "files": [{
                    "filename": "test.c",
                    "segments": [[10, 5, 3, 1, 1, 0]]
                }]
            }]
        }"#;
        let (all, executed, _) =
            parse_llvm_cov_json(json, Path::new("/"));
        assert_eq!(all.len(), 1);
        assert_eq!(executed.len(), 1);
    }

    #[test]
    fn json_truthy_handles_both_types() {
        assert!(json_truthy(&serde_json::Value::Bool(true)));
        assert!(!json_truthy(&serde_json::Value::Bool(false)));
        assert!(json_truthy(&serde_json::json!(1)));
        assert!(!json_truthy(&serde_json::json!(0)));
    }

    #[test]
    fn is_apple_clang_does_not_panic() {
        // Just verify it doesn't panic; result depends on platform
        let _ = is_apple_clang();
    }
}
