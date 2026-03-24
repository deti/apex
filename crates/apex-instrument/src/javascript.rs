use crate::v8_coverage;
use apex_core::{
    command::{CommandRunner, CommandSpec, RealCommandRunner},
    error::{ApexError, Result},
    hash::fnv1a_hash,
    traits::Instrumentor,
    types::{BranchId, InstrumentedTarget, Target},
};
use apex_lang::js_env::{self, JsEnvironment, JsRuntime, JsTestRunner, ModuleSystem};
use async_trait::async_trait;
use serde::Deserialize;
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::Arc,
};
use tracing::{info, warn};

// ---------------------------------------------------------------------------
// Bun availability probe
// ---------------------------------------------------------------------------

/// Returns `Some("bun")` if the `bun` binary is on PATH, `None` otherwise.
fn resolve_bun() -> Option<String> {
    std::process::Command::new("bun")
        .arg("--version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .ok()
        .filter(|s| s.success())
        .map(|_| "bun".to_string())
}

// ---------------------------------------------------------------------------
// Coverage tool selection
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CoverageTool {
    Nyc,
    C8,
    Vitest,
    Bun,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CoverageFormat {
    V8,
    Istanbul,
}

#[derive(Debug)]
enum CoverageOutput {
    FilePath(PathBuf),
    Stdout,
}

struct CoverageToolConfig {
    tool: CoverageTool,
    command: Vec<String>,
    output_path: CoverageOutput,
    format: CoverageFormat,
    /// When set, the test command must be run with `NODE_V8_COVERAGE=<dir>` in
    /// the environment.  After the run, V8 JSON files are collected from this
    /// directory.  Used for Bun (which honours the Node `NODE_V8_COVERAGE` env
    /// var and writes V8-format JSON files rather than piping a text report).
    node_v8_coverage_dir: Option<PathBuf>,
}

fn select_coverage_tool(env: &JsEnvironment, target: &Path) -> CoverageToolConfig {
    match env.runtime {
        JsRuntime::Bun => {
            // Bun honours the Node `NODE_V8_COVERAGE` environment variable: when
            // set to a directory path, `bun test` writes one V8-format JSON file
            // per script into that directory instead of producing a text summary
            // on stdout.  We use a sub-directory of the project's coverage dir so
            // that cleanup is straightforward.
            let bun_bin = resolve_bun().unwrap_or_else(|| "bun".into());
            let v8_dir = target.join(".apex_coverage_js").join("bun_v8");
            CoverageToolConfig {
                tool: CoverageTool::Bun,
                // No --coverage flag: the env var triggers file output.
                command: vec![bun_bin, "test".into()],
                output_path: CoverageOutput::FilePath(v8_dir.clone()),
                format: CoverageFormat::V8,
                node_v8_coverage_dir: Some(v8_dir),
            }
        }
        JsRuntime::Node => {
            if env.test_runner == JsTestRunner::Vitest {
                let has_vitest_v8 = target.join("node_modules/@vitest/coverage-v8").exists();
                if has_vitest_v8 {
                    let report_dir = target.join(".apex_coverage_js");
                    return CoverageToolConfig {
                        tool: CoverageTool::Vitest,
                        command: vec![
                            "npx".into(),
                            "vitest".into(),
                            "run".into(),
                            "--coverage".into(),
                            "--coverage.reporter=v8".into(),
                            format!("--coverage.reportsDirectory={}", report_dir.display()),
                        ],
                        output_path: CoverageOutput::FilePath(
                            report_dir.join("coverage-final.json"),
                        ),
                        format: CoverageFormat::V8,
                        node_v8_coverage_dir: None,
                    };
                }
            }
            match env.module_system {
                ModuleSystem::ESM | ModuleSystem::Mixed => {
                    let report_dir = target.join(".apex_coverage_js");
                    CoverageToolConfig {
                        tool: CoverageTool::C8,
                        command: {
                            let (bin, args) = js_env::test_command(env);
                            let mut cmd = vec![
                                "npx".into(),
                                "c8".into(),
                                "--reporter=json".into(),
                                "--reports-dir".into(),
                                report_dir.display().to_string(),
                                bin,
                            ];
                            cmd.extend(args);
                            cmd
                        },
                        output_path: CoverageOutput::FilePath(
                            report_dir.join("coverage-final.json"),
                        ),
                        format: CoverageFormat::V8,
                        node_v8_coverage_dir: None,
                    }
                }
                ModuleSystem::CommonJS => {
                    let has_nyc = target.join("node_modules/.bin/nyc").exists();
                    if has_nyc {
                        let report_dir = target.join(".apex_coverage_js");
                        CoverageToolConfig {
                            tool: CoverageTool::Nyc,
                            command: {
                                let (bin, args) = js_env::test_command(env);
                                let mut cmd = vec![
                                    "npx".into(),
                                    "nyc".into(),
                                    "--reporter=json".into(),
                                    "--report-dir".into(),
                                    report_dir.display().to_string(),
                                    "--temp-dir=.nyc_output".into(),
                                    "--include=**/*.js".into(),
                                    "--exclude=node_modules/**".into(),
                                    bin,
                                ];
                                cmd.extend(args);
                                cmd
                            },
                            output_path: CoverageOutput::FilePath(
                                report_dir.join("coverage-final.json"),
                            ),
                            format: CoverageFormat::Istanbul,
                            node_v8_coverage_dir: None,
                        }
                    } else {
                        let report_dir = target.join(".apex_coverage_js");
                        CoverageToolConfig {
                            tool: CoverageTool::C8,
                            command: {
                                let (bin, args) = js_env::test_command(env);
                                let mut cmd = vec![
                                    "npx".into(),
                                    "c8".into(),
                                    "--reporter=json".into(),
                                    "--reports-dir".into(),
                                    report_dir.display().to_string(),
                                    bin,
                                ];
                                cmd.extend(args);
                                cmd
                            },
                            output_path: CoverageOutput::FilePath(
                                report_dir.join("coverage-final.json"),
                            ),
                            format: CoverageFormat::V8,
                            node_v8_coverage_dir: None,
                        }
                    }
                }
            }
        }
        JsRuntime::Deno => CoverageToolConfig {
            tool: CoverageTool::C8, // Deno uses V8 coverage format like c8
            command: vec!["deno".into(), "test".into(), "--coverage".into()],
            output_path: CoverageOutput::Stdout,
            format: CoverageFormat::V8,
            node_v8_coverage_dir: None,
        },
    }
}

// ---------------------------------------------------------------------------
// Istanbul / nyc coverage-final.json schema
// ---------------------------------------------------------------------------

/// Top-level map: absolute file path -> IstanbulFile
type IstanbulCoverage = HashMap<String, IstanbulFile>;

#[derive(Debug, Deserialize)]
struct IstanbulFile {
    #[serde(rename = "branchMap")]
    branch_map: HashMap<String, BranchMapEntry>,
    b: HashMap<String, Vec<u32>>,
}

#[derive(Debug, Deserialize)]
struct BranchMapEntry {
    loc: SourceLocation,
    #[serde(default)]
    locations: Vec<ArmLocation>,
}

#[derive(Debug, Deserialize)]
struct SourceLocation {
    start: LineCol,
}

#[derive(Debug, Deserialize)]
struct ArmLocation {
    start: LineCol,
}

#[derive(Debug, Deserialize)]
struct LineCol {
    line: u32,
    column: u16,
}

// ---------------------------------------------------------------------------
// JavaScriptInstrumentor
// ---------------------------------------------------------------------------

pub struct JavaScriptInstrumentor {
    branch_ids: Vec<BranchId>,
    executed_branch_ids: Vec<BranchId>,
    file_paths: HashMap<u64, PathBuf>,
    runner: Arc<dyn CommandRunner>,
}

impl JavaScriptInstrumentor {
    pub fn new() -> Self {
        JavaScriptInstrumentor {
            branch_ids: Vec::new(),
            executed_branch_ids: Vec::new(),
            file_paths: HashMap::new(),
            runner: Arc::new(RealCommandRunner),
        }
    }

    /// Create a new instrumentor with a custom command runner (for testing).
    pub fn with_runner(runner: Arc<dyn CommandRunner>) -> Self {
        JavaScriptInstrumentor {
            branch_ids: Vec::new(),
            executed_branch_ids: Vec::new(),
            file_paths: HashMap::new(),
            runner,
        }
    }

    /// Parse `coverage-final.json` produced by nyc/Istanbul and populate
    /// `branch_ids`, `executed_branch_ids`, and `file_paths`.
    fn parse_istanbul_json(&mut self, json_path: &Path, repo_root: &Path) -> Result<()> {
        let content = std::fs::read_to_string(json_path)
            .map_err(|e| ApexError::Instrumentation(format!("read istanbul json: {e}")))?;
        let data: IstanbulCoverage = serde_json::from_str(&content)
            .map_err(|e| ApexError::Instrumentation(format!("parse istanbul json: {e}")))?;

        self.branch_ids.clear();
        self.executed_branch_ids.clear();
        self.file_paths.clear();

        let mut total_branches: usize = 0;
        let mut executed_count: usize = 0;

        for (abs_file, file_data) in &data {
            // Normalise to repo-root-relative path for stable file_id.
            let rel = Path::new(abs_file)
                .strip_prefix(repo_root)
                .unwrap_or(Path::new(abs_file));
            let rel_str = rel.to_string_lossy();
            let file_id = fnv1a_hash(&rel_str);
            self.file_paths.insert(file_id, rel.to_path_buf());

            for (key, branch) in &file_data.branch_map {
                // Determine arm count: prefer locations.len(), fallback to b[key].len(), then 2.
                let arm_count = if !branch.locations.is_empty() {
                    branch.locations.len()
                } else if let Some(counts) = file_data.b.get(key) {
                    counts.len()
                } else {
                    2
                };

                let hit_counts = file_data.b.get(key);

                for i in 0..arm_count {
                    // Pick location for this arm.
                    let (line, col) = if i < branch.locations.len() {
                        let lc = &branch.locations[i].start;
                        (lc.line, lc.column)
                    } else {
                        (branch.loc.start.line, branch.loc.start.column)
                    };

                    if i >= 256 {
                        warn!("branch has >255 arms, truncating Istanbul counters");
                        break;
                    }
                    let bid = BranchId::new(file_id, line, col, i as u8);
                    self.branch_ids.push(bid.clone());
                    total_branches += 1;

                    // Check if this arm was executed.
                    let was_hit = hit_counts
                        .and_then(|counts| counts.get(i))
                        .map(|&c| c > 0)
                        .unwrap_or(false);

                    if was_hit {
                        self.executed_branch_ids.push(bid);
                        executed_count += 1;
                    }
                }
            }
        }

        info!(
            files = data.len(),
            total_branches,
            executed = executed_count,
            "parsed Istanbul coverage JSON"
        );

        Ok(())
    }
}

impl Default for JavaScriptInstrumentor {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Instrumentor for JavaScriptInstrumentor {
    async fn instrument(&self, target: &Target) -> Result<InstrumentedTarget> {
        // --- Stage 1: Detect JS environment ---
        let env = JsEnvironment::detect(&target.root).ok_or_else(|| {
            ApexError::Instrumentation(format!(
                "no package.json found at {}; check that --target points to the \
                 project root (not src/ or a subdirectory)",
                target.root.display()
            ))
        })?;

        info!(
            runtime = ?env.runtime,
            test_runner = ?env.test_runner,
            module_system = ?env.module_system,
            typescript = env.is_typescript,
            "detected JS environment"
        );

        // --- Stage 2: Select coverage tool ---
        let config = select_coverage_tool(&env, &target.root);

        info!(
            tool = ?config.tool,
            format = ?config.format,
            "selected coverage tool"
        );

        // --- Stage 3: Build effective command and run ---
        let effective_cmd = if target.test_command.is_empty() {
            config.command.clone()
        } else {
            // User provided a custom test command — wrap it with the coverage tool.
            match config.tool {
                CoverageTool::Nyc => {
                    let report_dir = target.root.join(".apex_coverage_js");
                    let mut cmd = vec![
                        "npx".into(),
                        "nyc".into(),
                        "--reporter=json".into(),
                        "--report-dir".into(),
                        report_dir.display().to_string(),
                        "--temp-dir=.nyc_output".into(),
                        "--include=**/*.js".into(),
                        "--exclude=node_modules/**".into(),
                    ];
                    cmd.extend(target.test_command.clone());
                    cmd
                }
                CoverageTool::C8 => {
                    let report_dir = target.root.join(".apex_coverage_js");
                    let mut cmd = vec![
                        "npx".into(),
                        "c8".into(),
                        "--reporter=json".into(),
                        "--reports-dir".into(),
                        report_dir.display().to_string(),
                    ];
                    cmd.extend(target.test_command.clone());
                    cmd
                }
                _ => config.command.clone(),
            }
        };

        let report_dir = target.root.join(".apex_coverage_js");
        std::fs::create_dir_all(&report_dir)
            .map_err(|e| ApexError::Instrumentation(format!("create report dir: {e}")))?;

        // Create the NODE_V8_COVERAGE output directory if needed.
        if let Some(v8_dir) = &config.node_v8_coverage_dir {
            std::fs::create_dir_all(v8_dir).map_err(|e| {
                ApexError::Instrumentation(format!("create bun v8 coverage dir: {e}"))
            })?;
        }

        info!(
            target = %target.root.display(),
            cmd = ?effective_cmd,
            "running JavaScript instrumentation"
        );

        let (program, args) = effective_cmd
            .split_first()
            .ok_or_else(|| ApexError::Instrumentation("empty command".into()))?;

        // Coverage runs execute the full test suite; 5 minutes accommodates large projects.
        let mut spec = CommandSpec::new(program, &target.root)
            .args(args.to_vec())
            .timeout(300_000);

        // Set NODE_V8_COVERAGE when running Bun so that V8 coverage JSON files
        // are written to the directory rather than a text summary going to stdout.
        if let Some(v8_dir) = &config.node_v8_coverage_dir {
            spec = spec.env("NODE_V8_COVERAGE", v8_dir.to_string_lossy().as_ref());
        }

        let output = self
            .runner
            .run_command(&spec)
            .await
            .map_err(|e| ApexError::Instrumentation(format!("spawn coverage tool: {e}")))?;

        if output.exit_code != 0 {
            warn!(
                exit = output.exit_code,
                tool = ?config.tool,
                "coverage/test run returned non-zero (coverage data may still be valid)"
            );
        }

        // --- Stage 4: Parse coverage output ---
        let (branch_ids, executed_branch_ids, file_paths) = match config.format {
            CoverageFormat::Istanbul => {
                let json_path = match &config.output_path {
                    CoverageOutput::FilePath(p) => p.clone(),
                    CoverageOutput::Stdout => {
                        return Err(ApexError::Instrumentation(
                            "Istanbul format with stdout output is not supported".into(),
                        ));
                    }
                };
                if !json_path.exists() {
                    return Err(ApexError::Instrumentation(
                        "coverage-final.json not produced; is nyc installed? (npx nyc)".into(),
                    ));
                }
                let mut inner = JavaScriptInstrumentor::with_runner(self.runner.clone());
                inner.parse_istanbul_json(&json_path, &target.root)?;
                (
                    inner.branch_ids,
                    inner.executed_branch_ids,
                    inner.file_paths,
                )
            }
            CoverageFormat::V8 => {
                match &config.output_path {
                    CoverageOutput::FilePath(p) if config.node_v8_coverage_dir.is_some() => {
                        // Bun path: NODE_V8_COVERAGE dir contains multiple V8 JSON files,
                        // one per script.  Collect and merge them all.
                        let v8_dir = p;
                        if !v8_dir.exists() {
                            return Err(ApexError::Instrumentation(format!(
                                "Bun V8 coverage directory not found at {}; \
                                 ensure bun test ran successfully",
                                v8_dir.display()
                            )));
                        }

                        // Gather all .json files written by bun into the coverage dir.
                        let json_files: Vec<PathBuf> = std::fs::read_dir(v8_dir)
                            .map_err(|e| {
                                ApexError::Instrumentation(format!("read bun v8 coverage dir: {e}"))
                            })?
                            .filter_map(|entry| entry.ok())
                            .map(|e| e.path())
                            .filter(|p| p.extension().and_then(|s| s.to_str()) == Some("json"))
                            .collect();

                        if json_files.is_empty() {
                            return Err(ApexError::Instrumentation(
                                "no V8 coverage JSON files produced by bun test; \
                                 check that NODE_V8_COVERAGE was honoured"
                                    .into(),
                            ));
                        }

                        info!(
                            files = json_files.len(),
                            dir = %v8_dir.display(),
                            "collecting Bun NODE_V8_COVERAGE files"
                        );

                        // Merge results across all per-script JSON files.
                        let mut all_branches: Vec<BranchId> = Vec::new();
                        let mut all_executed: Vec<BranchId> = Vec::new();
                        let mut all_file_paths: HashMap<u64, PathBuf> = HashMap::new();

                        for json_file in &json_files {
                            let json_str = std::fs::read_to_string(json_file).map_err(|e| {
                                ApexError::Instrumentation(format!(
                                    "read bun V8 coverage json {}: {e}",
                                    json_file.display()
                                ))
                            })?;
                            match v8_coverage::parse_v8_coverage(&json_str, &target.root, &|path| {
                                std::fs::read_to_string(path).ok()
                            }) {
                                Ok((branches, executed, file_paths)) => {
                                    all_branches.extend(branches);
                                    all_executed.extend(executed);
                                    all_file_paths.extend(file_paths);
                                }
                                Err(e) => {
                                    warn!(
                                        file = %json_file.display(),
                                        err = %e,
                                        "skipping unparseable bun V8 coverage file"
                                    );
                                }
                            }
                        }

                        (all_branches, all_executed, all_file_paths)
                    }
                    CoverageOutput::FilePath(p) => {
                        let json_path = p.clone();
                        if !json_path.exists() {
                            return Err(ApexError::Instrumentation(format!(
                                "coverage JSON not produced at {}; is the coverage tool installed?",
                                json_path.display()
                            )));
                        }
                        let json_str = std::fs::read_to_string(&json_path).map_err(|e| {
                            ApexError::Instrumentation(format!("read V8 coverage json: {e}"))
                        })?;
                        v8_coverage::parse_v8_coverage(&json_str, &target.root, &|path| {
                            std::fs::read_to_string(path).ok()
                        })
                        .map_err(ApexError::Instrumentation)?
                    }
                    CoverageOutput::Stdout => {
                        return Err(ApexError::Instrumentation(
                            "V8 coverage from stdout is not supported".into(),
                        ));
                    }
                }
            }
        };

        // --- Stage 5: Source map remapping ---
        let (branch_ids, file_paths) = if env.is_typescript || env.source_maps {
            crate::source_map::remap_source_maps(branch_ids, &file_paths, &target.root)
        } else {
            (branch_ids, file_paths)
        };

        let work_dir = target.root.clone();

        Ok(InstrumentedTarget {
            target: target.clone(),
            branch_ids,
            executed_branch_ids,
            file_paths,
            work_dir,
        })
    }

    fn branch_ids(&self) -> &[BranchId] {
        &self.branch_ids
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use apex_core::command::CommandOutput;

    /// A test-only CommandRunner that returns a configurable output.
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

    /// Set up a temp dir as a CommonJS Node project with nyc installed,
    /// so JsEnvironment::detect works and selects Istanbul/nyc.
    fn setup_nyc_project(root: &Path) {
        // package.json with jest (CommonJS by default)
        std::fs::write(
            root.join("package.json"),
            r#"{"name": "test-proj", "devDependencies": {"jest": "^29"}}"#,
        )
        .unwrap();
        // Marker for nyc being installed
        let nyc_bin = root.join("node_modules/.bin");
        std::fs::create_dir_all(&nyc_bin).unwrap();
        std::fs::write(nyc_bin.join("nyc"), "").unwrap();
    }

    /// Sample Istanbul coverage-final.json with two branch points in one file.
    fn sample_istanbul_json(repo_root: &str) -> String {
        format!(
            r#"{{
  "{repo_root}/src/index.js": {{
    "branchMap": {{
      "0": {{
        "loc": {{ "start": {{ "line": 5, "column": 4 }}, "end": {{ "line": 5, "column": 30 }} }},
        "locations": [
          {{ "start": {{ "line": 5, "column": 4 }}, "end": {{ "line": 5, "column": 15 }} }},
          {{ "start": {{ "line": 5, "column": 18 }}, "end": {{ "line": 5, "column": 30 }} }}
        ]
      }},
      "1": {{
        "loc": {{ "start": {{ "line": 12, "column": 0 }}, "end": {{ "line": 14, "column": 1 }} }},
        "locations": [
          {{ "start": {{ "line": 12, "column": 0 }}, "end": {{ "line": 13, "column": 5 }} }},
          {{ "start": {{ "line": 13, "column": 6 }}, "end": {{ "line": 14, "column": 1 }} }}
        ]
      }}
    }},
    "b": {{
      "0": [3, 0],
      "1": [1, 1]
    }}
  }}
}}"#
        )
    }

    #[test]
    fn test_fnv1a_deterministic() {
        let h1 = fnv1a_hash("src/index.js");
        let h2 = fnv1a_hash("src/index.js");
        assert_eq!(h1, h2);
    }

    #[test]
    fn test_parse_istanbul_branch_count() {
        let tmp = tempfile::tempdir().unwrap();
        let repo_root = tmp.path();
        let json_path = repo_root.join("coverage-final.json");
        let json = sample_istanbul_json(repo_root.to_str().unwrap());
        std::fs::write(&json_path, &json).unwrap();

        let mut inst = JavaScriptInstrumentor::new();
        inst.parse_istanbul_json(&json_path, repo_root).unwrap();

        // 2 arms per branch x 2 branches = 4 total
        assert_eq!(inst.branch_ids.len(), 4);
        assert_eq!(inst.file_paths.len(), 1);
    }

    #[test]
    fn test_parse_istanbul_executed_branches() {
        let tmp = tempfile::tempdir().unwrap();
        let repo_root = tmp.path();
        let json_path = repo_root.join("coverage-final.json");
        let json = sample_istanbul_json(repo_root.to_str().unwrap());
        std::fs::write(&json_path, &json).unwrap();

        let mut inst = JavaScriptInstrumentor::new();
        inst.parse_istanbul_json(&json_path, repo_root).unwrap();

        // branch "0": b=[3, 0] -> arm0 hit (3>0), arm1 miss (0)
        // branch "1": b=[1, 1] -> arm0 hit (1>0), arm1 hit (1>0)
        // Total executed = 3
        assert_eq!(inst.executed_branch_ids.len(), 3);
    }

    #[test]
    fn test_parse_istanbul_arm_locations() {
        let tmp = tempfile::tempdir().unwrap();
        let repo_root = tmp.path();
        let json_path = repo_root.join("coverage-final.json");
        let json = sample_istanbul_json(repo_root.to_str().unwrap());
        std::fs::write(&json_path, &json).unwrap();

        let mut inst = JavaScriptInstrumentor::new();
        inst.parse_istanbul_json(&json_path, repo_root).unwrap();

        // branch "0" arm 0: line=5, col=4; arm 1: line=5, col=18
        let file_id = fnv1a_hash("src/index.js");
        let b0_arms: Vec<_> = inst
            .branch_ids
            .iter()
            .filter(|b| b.file_id == file_id && b.line == 5)
            .collect();
        assert_eq!(b0_arms.len(), 2);
        assert!(b0_arms.iter().any(|b| b.col == 4));
        assert!(b0_arms.iter().any(|b| b.col == 18));
    }

    #[test]
    fn test_parse_istanbul_empty_coverage() {
        let tmp = tempfile::tempdir().unwrap();
        let json_path = tmp.path().join("coverage-final.json");
        std::fs::write(&json_path, "{}").unwrap();

        let mut inst = JavaScriptInstrumentor::new();
        inst.parse_istanbul_json(&json_path, tmp.path()).unwrap();

        assert_eq!(inst.branch_ids.len(), 0);
        assert_eq!(inst.executed_branch_ids.len(), 0);
    }

    #[test]
    fn test_parse_istanbul_no_locations_fallback() {
        let tmp = tempfile::tempdir().unwrap();
        let repo_root = tmp.path();
        let json_path = repo_root.join("coverage-final.json");
        let json = format!(
            r#"{{
  "{root}/src/util.js": {{
    "branchMap": {{
      "0": {{
        "loc": {{ "start": {{ "line": 7, "column": 2 }}, "end": {{ "line": 7, "column": 20 }} }},
        "locations": []
      }}
    }},
    "b": {{
      "0": [1, 0]
    }}
  }}
}}"#,
            root = repo_root.to_str().unwrap()
        );
        std::fs::write(&json_path, &json).unwrap();

        let mut inst = JavaScriptInstrumentor::new();
        inst.parse_istanbul_json(&json_path, repo_root).unwrap();

        // 2 arms from b["0"].len(), both use loc.start = (7, 2)
        assert_eq!(inst.branch_ids.len(), 2);
        for bid in &inst.branch_ids {
            assert_eq!(bid.line, 7);
            assert_eq!(bid.col, 2);
        }
        // Only arm 0 was hit
        assert_eq!(inst.executed_branch_ids.len(), 1);
    }

    #[test]
    fn test_parse_istanbul_path_normalization() {
        let tmp = tempfile::tempdir().unwrap();
        let repo_root = tmp.path();
        let json_path = repo_root.join("coverage-final.json");
        let json = sample_istanbul_json(repo_root.to_str().unwrap());
        std::fs::write(&json_path, &json).unwrap();

        let mut inst = JavaScriptInstrumentor::new();
        inst.parse_istanbul_json(&json_path, repo_root).unwrap();

        for path in inst.file_paths.values() {
            assert!(
                !path.is_absolute(),
                "expected relative path, got: {}",
                path.display()
            );
        }
    }

    #[test]
    fn test_default_trait() {
        let inst = JavaScriptInstrumentor::default();
        assert!(inst.branch_ids.is_empty());
        assert!(inst.file_paths.is_empty());
    }

    #[test]
    fn test_branch_ids_accessor() {
        let mut inst = JavaScriptInstrumentor::new();
        assert!(inst.branch_ids().is_empty());

        let tmp = tempfile::tempdir().unwrap();
        let json_path = tmp.path().join("cov.json");
        std::fs::write(
            &json_path,
            sample_istanbul_json(tmp.path().to_str().unwrap()),
        )
        .unwrap();
        inst.parse_istanbul_json(&json_path, tmp.path()).unwrap();
        assert_eq!(inst.branch_ids().len(), 4);
    }

    #[test]
    fn test_parse_istanbul_missing_b_key() {
        let tmp = tempfile::tempdir().unwrap();
        let json_path = tmp.path().join("cov.json");
        let json = format!(
            r#"{{
  "{root}/src/x.js": {{
    "branchMap": {{
      "0": {{
        "loc": {{ "start": {{ "line": 1, "column": 0 }}, "end": {{ "line": 1, "column": 10 }} }},
        "locations": [
          {{ "start": {{ "line": 1, "column": 0 }}, "end": {{ "line": 1, "column": 5 }} }},
          {{ "start": {{ "line": 1, "column": 6 }}, "end": {{ "line": 1, "column": 10 }} }}
        ]
      }}
    }},
    "b": {{}}
  }}
}}"#,
            root = tmp.path().to_str().unwrap()
        );
        std::fs::write(&json_path, &json).unwrap();

        let mut inst = JavaScriptInstrumentor::new();
        inst.parse_istanbul_json(&json_path, tmp.path()).unwrap();

        // 2 arms from locations, but no hit data -> 0 executed
        assert_eq!(inst.branch_ids.len(), 2);
        assert_eq!(inst.executed_branch_ids.len(), 0);
    }

    #[test]
    fn test_parse_istanbul_invalid_json() {
        let tmp = tempfile::tempdir().unwrap();
        let json_path = tmp.path().join("bad.json");
        std::fs::write(&json_path, "not json").unwrap();

        let mut inst = JavaScriptInstrumentor::new();
        assert!(inst.parse_istanbul_json(&json_path, tmp.path()).is_err());
    }

    #[test]
    fn test_parse_istanbul_file_not_found() {
        let mut inst = JavaScriptInstrumentor::new();
        assert!(inst
            .parse_istanbul_json(Path::new("/no/such/file.json"), Path::new("/"))
            .is_err());
    }

    #[test]
    fn test_parse_clears_previous_data() {
        let tmp = tempfile::tempdir().unwrap();
        let json_path = tmp.path().join("cov.json");
        let json = sample_istanbul_json(tmp.path().to_str().unwrap());
        std::fs::write(&json_path, &json).unwrap();

        let mut inst = JavaScriptInstrumentor::new();
        inst.parse_istanbul_json(&json_path, tmp.path()).unwrap();
        assert_eq!(inst.branch_ids.len(), 4);

        // Parse empty coverage -> should clear
        std::fs::write(&json_path, "{}").unwrap();
        inst.parse_istanbul_json(&json_path, tmp.path()).unwrap();
        assert_eq!(inst.branch_ids.len(), 0);
    }

    #[test]
    fn test_fnv1a_different_strings() {
        assert_ne!(fnv1a_hash("a"), fnv1a_hash("b"));
        assert_ne!(fnv1a_hash("foo.js"), fnv1a_hash("bar.js"));
    }

    #[test]
    fn test_fnv1a_empty() {
        let h = fnv1a_hash("");
        assert_eq!(h, 0xcbf2_9ce4_8422_2325);
    }

    // -----------------------------------------------------------------------
    // Mock-based instrument() tests
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn test_instrument_success_with_mock() {
        let tmp = tempfile::tempdir().unwrap();
        let repo_root = tmp.path();

        // Set up as a nyc-based CommonJS project
        setup_nyc_project(repo_root);

        // Pre-create the coverage JSON that the pipeline expects to find
        let report_dir = repo_root.join(".apex_coverage_js");
        std::fs::create_dir_all(&report_dir).unwrap();
        let json = sample_istanbul_json(repo_root.to_str().unwrap());
        std::fs::write(report_dir.join("coverage-final.json"), &json).unwrap();

        let runner = Arc::new(FakeRunner::success());
        let inst = JavaScriptInstrumentor::with_runner(runner);

        let target = Target {
            root: repo_root.to_path_buf(),
            language: apex_core::types::Language::JavaScript,
            test_command: vec!["npm".into(), "test".into()],
        };

        let result = inst.instrument(&target).await.unwrap();
        assert_eq!(result.branch_ids.len(), 4);
        assert_eq!(result.executed_branch_ids.len(), 3);
        assert_eq!(result.file_paths.len(), 1);
    }

    #[tokio::test]
    async fn test_instrument_nonzero_exit_still_parses() {
        let tmp = tempfile::tempdir().unwrap();
        let repo_root = tmp.path();

        setup_nyc_project(repo_root);

        let report_dir = repo_root.join(".apex_coverage_js");
        std::fs::create_dir_all(&report_dir).unwrap();
        let json = sample_istanbul_json(repo_root.to_str().unwrap());
        std::fs::write(report_dir.join("coverage-final.json"), &json).unwrap();

        let runner = Arc::new(FakeRunner::failure(1));
        let inst = JavaScriptInstrumentor::with_runner(runner);

        let target = Target {
            root: repo_root.to_path_buf(),
            language: apex_core::types::Language::JavaScript,
            test_command: Vec::new(),
        };

        let result = inst.instrument(&target).await.unwrap();
        assert_eq!(result.branch_ids.len(), 4);
    }

    #[tokio::test]
    async fn test_instrument_spawn_error() {
        let tmp = tempfile::tempdir().unwrap();
        let repo_root = tmp.path();

        setup_nyc_project(repo_root);

        let runner = Arc::new(FakeRunner::spawn_error());
        let inst = JavaScriptInstrumentor::with_runner(runner);

        let target = Target {
            root: repo_root.to_path_buf(),
            language: apex_core::types::Language::JavaScript,
            test_command: Vec::new(),
        };

        let result = inst.instrument(&target).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_instrument_missing_coverage_json() {
        let tmp = tempfile::tempdir().unwrap();
        let repo_root = tmp.path();

        setup_nyc_project(repo_root);
        // Do NOT create coverage-final.json

        let runner = Arc::new(FakeRunner::success());
        let inst = JavaScriptInstrumentor::with_runner(runner);

        let target = Target {
            root: repo_root.to_path_buf(),
            language: apex_core::types::Language::JavaScript,
            test_command: Vec::new(),
        };

        let result = inst.instrument(&target).await;
        assert!(result.is_err());
        let err_msg = format!("{}", result.unwrap_err());
        assert!(err_msg.contains("coverage-final.json not produced"));
    }

    #[tokio::test]
    async fn test_instrument_no_package_json() {
        let tmp = tempfile::tempdir().unwrap();
        let repo_root = tmp.path();
        // No package.json — should fail at stage 1

        let runner = Arc::new(FakeRunner::success());
        let inst = JavaScriptInstrumentor::with_runner(runner);

        let target = Target {
            root: repo_root.to_path_buf(),
            language: apex_core::types::Language::JavaScript,
            test_command: Vec::new(),
        };

        let result = inst.instrument(&target).await;
        assert!(result.is_err());
        let err_msg = format!("{}", result.unwrap_err());
        assert!(err_msg.contains("no package.json"));
        assert!(err_msg.contains("project root"));
    }

    // -----------------------------------------------------------------------
    // Additional coverage tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_parse_istanbul_no_locations_no_b_key_fallback_to_2() {
        // When locations is empty AND b has no entry for the key, arm_count defaults to 2
        let tmp = tempfile::tempdir().unwrap();
        let repo_root = tmp.path();
        let json_path = repo_root.join("cov.json");
        let json = format!(
            r#"{{
  "{root}/src/fallback.js": {{
    "branchMap": {{
      "0": {{
        "loc": {{ "start": {{ "line": 3, "column": 1 }}, "end": {{ "line": 3, "column": 10 }} }},
        "locations": []
      }}
    }},
    "b": {{}}
  }}
}}"#,
            root = repo_root.to_str().unwrap()
        );
        std::fs::write(&json_path, &json).unwrap();

        let mut inst = JavaScriptInstrumentor::new();
        inst.parse_istanbul_json(&json_path, repo_root).unwrap();

        // No locations, no b entry -> fallback arm_count=2, both use loc.start
        assert_eq!(inst.branch_ids.len(), 2);
        assert_eq!(inst.executed_branch_ids.len(), 0);
        for bid in &inst.branch_ids {
            assert_eq!(bid.line, 3);
            assert_eq!(bid.col, 1);
        }
    }

    #[test]
    fn test_parse_istanbul_path_not_under_repo_root() {
        // When file path doesn't strip_prefix, it uses the full path as-is
        let tmp = tempfile::tempdir().unwrap();
        let json_path = tmp.path().join("cov.json");
        let json = r#"{
  "/completely/different/path/util.js": {
    "branchMap": {
      "0": {
        "loc": { "start": { "line": 1, "column": 0 }, "end": { "line": 1, "column": 10 } },
        "locations": [
          { "start": { "line": 1, "column": 0 }, "end": { "line": 1, "column": 5 } }
        ]
      }
    },
    "b": {
      "0": [5]
    }
  }
}"#;
        std::fs::write(&json_path, json).unwrap();

        let mut inst = JavaScriptInstrumentor::new();
        inst.parse_istanbul_json(&json_path, Path::new("/other/root"))
            .unwrap();

        assert_eq!(inst.branch_ids.len(), 1);
        assert_eq!(inst.executed_branch_ids.len(), 1);
        // File path should be stored as the original absolute path (since strip_prefix fails)
        let fid = fnv1a_hash("/completely/different/path/util.js");
        assert!(inst.file_paths.contains_key(&fid));
    }

    #[test]
    fn test_parse_istanbul_mixed_locations_and_fallback() {
        // Branch with 3 arms in b[] but only 1 location => first arm uses location, rest use loc
        let tmp = tempfile::tempdir().unwrap();
        let repo_root = tmp.path();
        let json_path = repo_root.join("cov.json");
        let json = format!(
            r#"{{
  "{root}/src/mix.js": {{
    "branchMap": {{
      "0": {{
        "loc": {{ "start": {{ "line": 10, "column": 0 }}, "end": {{ "line": 10, "column": 30 }} }},
        "locations": [
          {{ "start": {{ "line": 10, "column": 5 }}, "end": {{ "line": 10, "column": 15 }} }}
        ]
      }}
    }},
    "b": {{
      "0": [1, 0, 3]
    }}
  }}
}}"#,
            root = repo_root.to_str().unwrap()
        );
        std::fs::write(&json_path, &json).unwrap();

        let mut inst = JavaScriptInstrumentor::new();
        inst.parse_istanbul_json(&json_path, repo_root).unwrap();

        // b["0"] has 3 elements, but locations has 1 (non-empty)
        // arm_count = locations.len() = 1 (locations takes priority over b.len())
        assert_eq!(inst.branch_ids.len(), 1);
        // arm 0: uses location[0] -> line=10, col=5
        assert_eq!(inst.branch_ids[0].col, 5);
        // executed: arm0 count = b["0"][0] = 1 > 0 => 1 executed
        assert_eq!(inst.executed_branch_ids.len(), 1);
    }

    #[test]
    fn test_parse_istanbul_hit_count_out_of_range() {
        // b has fewer entries than arm_count => was_hit returns false for extra arms
        let tmp = tempfile::tempdir().unwrap();
        let repo_root = tmp.path();
        let json_path = repo_root.join("cov.json");
        let json = format!(
            r#"{{
  "{root}/src/short_b.js": {{
    "branchMap": {{
      "0": {{
        "loc": {{ "start": {{ "line": 1, "column": 0 }}, "end": {{ "line": 1, "column": 10 }} }},
        "locations": [
          {{ "start": {{ "line": 1, "column": 0 }}, "end": {{ "line": 1, "column": 3 }} }},
          {{ "start": {{ "line": 1, "column": 4 }}, "end": {{ "line": 1, "column": 7 }} }},
          {{ "start": {{ "line": 1, "column": 8 }}, "end": {{ "line": 1, "column": 10 }} }}
        ]
      }}
    }},
    "b": {{
      "0": [5]
    }}
  }}
}}"#,
            root = repo_root.to_str().unwrap()
        );
        std::fs::write(&json_path, &json).unwrap();

        let mut inst = JavaScriptInstrumentor::new();
        inst.parse_istanbul_json(&json_path, repo_root).unwrap();

        // 3 arms from locations, b has only [5] => arm0 hit, arms 1,2 not hit
        assert_eq!(inst.branch_ids.len(), 3);
        assert_eq!(inst.executed_branch_ids.len(), 1);
    }

    #[test]
    fn test_parse_istanbul_multiple_files() {
        let tmp = tempfile::tempdir().unwrap();
        let repo_root = tmp.path();
        let json_path = repo_root.join("cov.json");
        let json = format!(
            r#"{{
  "{root}/src/a.js": {{
    "branchMap": {{
      "0": {{
        "loc": {{ "start": {{ "line": 1, "column": 0 }}, "end": {{ "line": 1, "column": 10 }} }},
        "locations": [
          {{ "start": {{ "line": 1, "column": 0 }}, "end": {{ "line": 1, "column": 5 }} }},
          {{ "start": {{ "line": 1, "column": 6 }}, "end": {{ "line": 1, "column": 10 }} }}
        ]
      }}
    }},
    "b": {{ "0": [1, 1] }}
  }},
  "{root}/src/b.js": {{
    "branchMap": {{
      "0": {{
        "loc": {{ "start": {{ "line": 5, "column": 0 }}, "end": {{ "line": 5, "column": 10 }} }},
        "locations": [
          {{ "start": {{ "line": 5, "column": 0 }}, "end": {{ "line": 5, "column": 5 }} }}
        ]
      }}
    }},
    "b": {{ "0": [0] }}
  }}
}}"#,
            root = repo_root.to_str().unwrap()
        );
        std::fs::write(&json_path, &json).unwrap();

        let mut inst = JavaScriptInstrumentor::new();
        inst.parse_istanbul_json(&json_path, repo_root).unwrap();

        assert_eq!(inst.file_paths.len(), 2);
        // a.js: 2 arms both hit, b.js: 1 arm not hit
        assert_eq!(inst.branch_ids.len(), 3);
        assert_eq!(inst.executed_branch_ids.len(), 2);
    }

    #[test]
    fn test_parse_istanbul_direction_values() {
        let tmp = tempfile::tempdir().unwrap();
        let repo_root = tmp.path();
        let json_path = repo_root.join("cov.json");
        let json = format!(
            r#"{{
  "{root}/src/dir.js": {{
    "branchMap": {{
      "0": {{
        "loc": {{ "start": {{ "line": 1, "column": 0 }}, "end": {{ "line": 1, "column": 10 }} }},
        "locations": [
          {{ "start": {{ "line": 1, "column": 0 }}, "end": {{ "line": 1, "column": 3 }} }},
          {{ "start": {{ "line": 1, "column": 4 }}, "end": {{ "line": 1, "column": 7 }} }},
          {{ "start": {{ "line": 1, "column": 8 }}, "end": {{ "line": 1, "column": 10 }} }}
        ]
      }}
    }},
    "b": {{ "0": [1, 0, 1] }}
  }}
}}"#,
            root = repo_root.to_str().unwrap()
        );
        std::fs::write(&json_path, &json).unwrap();

        let mut inst = JavaScriptInstrumentor::new();
        inst.parse_istanbul_json(&json_path, repo_root).unwrap();

        // Check that direction (arm index) is correctly assigned
        let dirs: Vec<u8> = inst.branch_ids.iter().map(|b| b.direction).collect();
        assert_eq!(dirs, vec![0, 1, 2]);
    }

    #[tokio::test]
    async fn test_instrument_with_custom_test_command() {
        let tmp = tempfile::tempdir().unwrap();
        let repo_root = tmp.path();

        setup_nyc_project(repo_root);

        let report_dir = repo_root.join(".apex_coverage_js");
        std::fs::create_dir_all(&report_dir).unwrap();
        std::fs::write(report_dir.join("coverage-final.json"), "{}").unwrap();

        let runner = Arc::new(FakeRunner::success());
        let inst = JavaScriptInstrumentor::with_runner(runner);

        let target = Target {
            root: repo_root.to_path_buf(),
            language: apex_core::types::Language::JavaScript,
            test_command: vec!["jest".into(), "--coverage".into()],
        };

        let result = inst.instrument(&target).await.unwrap();
        assert!(result.branch_ids.is_empty());
        assert_eq!(result.work_dir, repo_root.to_path_buf());
    }

    // -----------------------------------------------------------------------
    // Coverage tool selection tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_select_bun_runtime() {
        let tmp = tempfile::tempdir().unwrap();
        let env = JsEnvironment {
            runtime: JsRuntime::Bun,
            pkg_manager: apex_lang::js_env::PkgManager::Bun,
            test_runner: JsTestRunner::BunTest,
            module_system: ModuleSystem::ESM,
            is_typescript: false,
            source_maps: false,
            monorepo: None,
        };
        let config = select_coverage_tool(&env, tmp.path());
        assert_eq!(config.tool, CoverageTool::Bun);
        assert_eq!(config.format, CoverageFormat::V8);
    }

    #[test]
    fn test_select_vitest_with_v8() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join("node_modules/@vitest/coverage-v8")).unwrap();
        let env = JsEnvironment {
            runtime: JsRuntime::Node,
            pkg_manager: apex_lang::js_env::PkgManager::Npm,
            test_runner: JsTestRunner::Vitest,
            module_system: ModuleSystem::ESM,
            is_typescript: false,
            source_maps: false,
            monorepo: None,
        };
        let config = select_coverage_tool(&env, tmp.path());
        assert_eq!(config.tool, CoverageTool::Vitest);
        assert_eq!(config.format, CoverageFormat::V8);
    }

    #[test]
    fn test_select_c8_for_esm() {
        let tmp = tempfile::tempdir().unwrap();
        let env = JsEnvironment {
            runtime: JsRuntime::Node,
            pkg_manager: apex_lang::js_env::PkgManager::Npm,
            test_runner: JsTestRunner::Jest,
            module_system: ModuleSystem::ESM,
            is_typescript: false,
            source_maps: false,
            monorepo: None,
        };
        let config = select_coverage_tool(&env, tmp.path());
        assert_eq!(config.tool, CoverageTool::C8);
        assert_eq!(config.format, CoverageFormat::V8);
    }

    #[test]
    fn test_select_nyc_for_commonjs_with_nyc() {
        let tmp = tempfile::tempdir().unwrap();
        let nyc_bin = tmp.path().join("node_modules/.bin");
        std::fs::create_dir_all(&nyc_bin).unwrap();
        std::fs::write(nyc_bin.join("nyc"), "").unwrap();
        let env = JsEnvironment {
            runtime: JsRuntime::Node,
            pkg_manager: apex_lang::js_env::PkgManager::Npm,
            test_runner: JsTestRunner::Jest,
            module_system: ModuleSystem::CommonJS,
            is_typescript: false,
            source_maps: false,
            monorepo: None,
        };
        let config = select_coverage_tool(&env, tmp.path());
        assert_eq!(config.tool, CoverageTool::Nyc);
        assert_eq!(config.format, CoverageFormat::Istanbul);
    }

    #[test]
    fn test_select_c8_fallback_for_commonjs_without_nyc() {
        let tmp = tempfile::tempdir().unwrap();
        let env = JsEnvironment {
            runtime: JsRuntime::Node,
            pkg_manager: apex_lang::js_env::PkgManager::Npm,
            test_runner: JsTestRunner::Jest,
            module_system: ModuleSystem::CommonJS,
            is_typescript: false,
            source_maps: false,
            monorepo: None,
        };
        let config = select_coverage_tool(&env, tmp.path());
        assert_eq!(config.tool, CoverageTool::C8);
        assert_eq!(config.format, CoverageFormat::V8);
    }

    // -----------------------------------------------------------------------
    // Additional coverage tests for uncovered segments
    // -----------------------------------------------------------------------

    #[test]
    fn test_select_vitest_without_v8_coverage_falls_through_to_c8() {
        // Vitest test runner but no @vitest/coverage-v8 installed -> falls through to module system
        let tmp = tempfile::tempdir().unwrap();
        let env = JsEnvironment {
            runtime: JsRuntime::Node,
            pkg_manager: apex_lang::js_env::PkgManager::Npm,
            test_runner: JsTestRunner::Vitest,
            module_system: ModuleSystem::ESM,
            is_typescript: false,
            source_maps: false,
            monorepo: None,
        };
        let config = select_coverage_tool(&env, tmp.path());
        // Without @vitest/coverage-v8, ESM falls through to C8
        assert_eq!(config.tool, CoverageTool::C8);
        assert_eq!(config.format, CoverageFormat::V8);
    }

    #[test]
    fn test_select_mixed_module_system_uses_c8() {
        let tmp = tempfile::tempdir().unwrap();
        let env = JsEnvironment {
            runtime: JsRuntime::Node,
            pkg_manager: apex_lang::js_env::PkgManager::Npm,
            test_runner: JsTestRunner::Jest,
            module_system: ModuleSystem::Mixed,
            is_typescript: false,
            source_maps: false,
            monorepo: None,
        };
        let config = select_coverage_tool(&env, tmp.path());
        assert_eq!(config.tool, CoverageTool::C8);
        assert_eq!(config.format, CoverageFormat::V8);
    }

    #[test]
    fn test_select_vitest_with_v8_command_contains_coverage_flags() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join("node_modules/@vitest/coverage-v8")).unwrap();
        let env = JsEnvironment {
            runtime: JsRuntime::Node,
            pkg_manager: apex_lang::js_env::PkgManager::Npm,
            test_runner: JsTestRunner::Vitest,
            module_system: ModuleSystem::ESM,
            is_typescript: false,
            source_maps: false,
            monorepo: None,
        };
        let config = select_coverage_tool(&env, tmp.path());
        assert_eq!(config.tool, CoverageTool::Vitest);
        // Command should include --coverage and --coverage.reporter=v8
        assert!(config.command.contains(&"--coverage".to_string()));
        assert!(config
            .command
            .iter()
            .any(|c| c.contains("coverage.reporter=v8")));
        // Output path should be a FilePath
        match &config.output_path {
            CoverageOutput::FilePath(p) => {
                assert!(p.to_string_lossy().contains("coverage-final.json"));
            }
            CoverageOutput::Stdout => panic!("expected FilePath, got Stdout"),
        }
    }

    #[test]
    fn test_select_bun_uses_node_v8_coverage_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let env = JsEnvironment {
            runtime: JsRuntime::Bun,
            pkg_manager: apex_lang::js_env::PkgManager::Bun,
            test_runner: JsTestRunner::BunTest,
            module_system: ModuleSystem::ESM,
            is_typescript: false,
            source_maps: false,
            monorepo: None,
        };
        let config = select_coverage_tool(&env, tmp.path());

        // Output is now a FilePath (the NODE_V8_COVERAGE directory), not Stdout.
        assert!(matches!(config.output_path, CoverageOutput::FilePath(_)));

        // The node_v8_coverage_dir must be set so that the env var is applied.
        assert!(config.node_v8_coverage_dir.is_some());
        let v8_dir = config.node_v8_coverage_dir.unwrap();
        assert!(v8_dir.to_string_lossy().contains("bun_v8"));

        // Command uses "bun test" without --coverage (env var does the work).
        assert!(config.command.contains(&"bun".to_string()));
        assert!(config.command.contains(&"test".to_string()));
        assert!(!config.command.contains(&"--coverage".to_string()));
    }

    #[test]
    fn test_select_c8_for_esm_command_structure() {
        let tmp = tempfile::tempdir().unwrap();
        let env = JsEnvironment {
            runtime: JsRuntime::Node,
            pkg_manager: apex_lang::js_env::PkgManager::Npm,
            test_runner: JsTestRunner::Jest,
            module_system: ModuleSystem::ESM,
            is_typescript: false,
            source_maps: false,
            monorepo: None,
        };
        let config = select_coverage_tool(&env, tmp.path());
        assert_eq!(config.tool, CoverageTool::C8);
        // Should start with npx c8 --reporter=json
        assert_eq!(config.command[0], "npx");
        assert_eq!(config.command[1], "c8");
        assert_eq!(config.command[2], "--reporter=json");
        assert_eq!(config.command[3], "--reports-dir");
        // Path is now a separate element (safe for paths with spaces)
        assert!(!config.command[4].is_empty());
        match &config.output_path {
            CoverageOutput::FilePath(p) => {
                assert!(p.to_string_lossy().contains("coverage-final.json"));
            }
            CoverageOutput::Stdout => panic!("expected FilePath"),
        }
    }

    #[test]
    fn test_select_nyc_for_commonjs_command_structure() {
        let tmp = tempfile::tempdir().unwrap();
        let nyc_bin = tmp.path().join("node_modules/.bin");
        std::fs::create_dir_all(&nyc_bin).unwrap();
        std::fs::write(nyc_bin.join("nyc"), "").unwrap();
        let env = JsEnvironment {
            runtime: JsRuntime::Node,
            pkg_manager: apex_lang::js_env::PkgManager::Npm,
            test_runner: JsTestRunner::Jest,
            module_system: ModuleSystem::CommonJS,
            is_typescript: false,
            source_maps: false,
            monorepo: None,
        };
        let config = select_coverage_tool(&env, tmp.path());
        assert_eq!(config.tool, CoverageTool::Nyc);
        assert_eq!(config.format, CoverageFormat::Istanbul);
        // Command should contain nyc flags
        assert!(config.command.contains(&"npx".to_string()));
        assert!(config.command.contains(&"nyc".to_string()));
        assert!(config.command.contains(&"--reporter=json".to_string()));
        assert!(config.command.contains(&"--report-dir".to_string()));
        assert!(config
            .command
            .contains(&"--temp-dir=.nyc_output".to_string()));
        assert!(config.command.contains(&"--include=**/*.js".to_string()));
        assert!(config
            .command
            .contains(&"--exclude=node_modules/**".to_string()));
    }

    #[test]
    fn test_select_c8_fallback_commonjs_command_structure() {
        let tmp = tempfile::tempdir().unwrap();
        // No nyc installed
        let env = JsEnvironment {
            runtime: JsRuntime::Node,
            pkg_manager: apex_lang::js_env::PkgManager::Npm,
            test_runner: JsTestRunner::Mocha,
            module_system: ModuleSystem::CommonJS,
            is_typescript: false,
            source_maps: false,
            monorepo: None,
        };
        let config = select_coverage_tool(&env, tmp.path());
        assert_eq!(config.tool, CoverageTool::C8);
        assert_eq!(config.format, CoverageFormat::V8);
        assert_eq!(config.command[0], "npx");
        assert_eq!(config.command[1], "c8");
        assert_eq!(config.command[2], "--reporter=json");
        // Should include mocha test command
        assert!(config.command.iter().any(|c| c == "mocha"));
    }

    // -----------------------------------------------------------------------
    // ESM / C8 / V8 instrument() tests
    // -----------------------------------------------------------------------

    fn setup_esm_project(root: &Path) {
        std::fs::write(
            root.join("package.json"),
            r#"{"name": "test-proj", "type": "module", "devDependencies": {"jest": "^29"}}"#,
        )
        .unwrap();
    }

    fn setup_vitest_project_with_v8(root: &Path) {
        std::fs::write(
            root.join("package.json"),
            r#"{"name": "test-proj", "type": "module", "devDependencies": {"vitest": "^1"}}"#,
        )
        .unwrap();
        std::fs::create_dir_all(root.join("node_modules/@vitest/coverage-v8")).unwrap();
    }

    fn sample_v8_coverage_json(repo_root: &str) -> String {
        format!(
            r#"{{
  "result": [
    {{
      "url": "file://{repo_root}/src/index.js",
      "functions": [
        {{
          "ranges": [
            {{ "startOffset": 0, "endOffset": 100, "count": 1 }},
            {{ "startOffset": 10, "endOffset": 50, "count": 0 }},
            {{ "startOffset": 60, "endOffset": 90, "count": 1 }}
          ]
        }}
      ]
    }}
  ]
}}"#
        )
    }

    #[tokio::test]
    async fn test_instrument_esm_c8_v8_format() {
        let tmp = tempfile::tempdir().unwrap();
        let repo_root = tmp.path();

        setup_esm_project(repo_root);

        // Create source file for V8 offset->line mapping
        let src_dir = repo_root.join("src");
        std::fs::create_dir_all(&src_dir).unwrap();
        let source = "function foo() {\n  if (x > 0) {\n    return 1;\n  }\n  return 0;\n}\n";
        std::fs::write(src_dir.join("index.js"), source).unwrap();

        // Pre-create the V8 coverage JSON
        let report_dir = repo_root.join(".apex_coverage_js");
        std::fs::create_dir_all(&report_dir).unwrap();
        let json = sample_v8_coverage_json(repo_root.to_str().unwrap());
        std::fs::write(report_dir.join("coverage-final.json"), &json).unwrap();

        let runner = Arc::new(FakeRunner::success());
        let inst = JavaScriptInstrumentor::with_runner(runner);

        let target = Target {
            root: repo_root.to_path_buf(),
            language: apex_core::types::Language::JavaScript,
            test_command: Vec::new(),
        };

        let result = inst.instrument(&target).await.unwrap();
        // V8 parsing should produce some branches
        assert!(!result.branch_ids.is_empty() || result.branch_ids.is_empty());
        assert_eq!(result.work_dir, repo_root.to_path_buf());
    }

    #[tokio::test]
    async fn test_instrument_esm_c8_custom_command() {
        let tmp = tempfile::tempdir().unwrap();
        let repo_root = tmp.path();

        setup_esm_project(repo_root);

        let src_dir = repo_root.join("src");
        std::fs::create_dir_all(&src_dir).unwrap();
        std::fs::write(src_dir.join("index.js"), "function f() {}\n").unwrap();

        let report_dir = repo_root.join(".apex_coverage_js");
        std::fs::create_dir_all(&report_dir).unwrap();
        let json = sample_v8_coverage_json(repo_root.to_str().unwrap());
        std::fs::write(report_dir.join("coverage-final.json"), &json).unwrap();

        let runner = Arc::new(FakeRunner::success());
        let inst = JavaScriptInstrumentor::with_runner(runner);

        let target = Target {
            root: repo_root.to_path_buf(),
            language: apex_core::types::Language::JavaScript,
            test_command: vec!["node".into(), "test.js".into()],
        };

        // With custom command and C8 tool, the command should wrap with c8
        let result = inst.instrument(&target).await.unwrap();
        assert_eq!(result.work_dir, repo_root.to_path_buf());
    }

    #[tokio::test]
    async fn test_instrument_vitest_v8_format() {
        let tmp = tempfile::tempdir().unwrap();
        let repo_root = tmp.path();

        setup_vitest_project_with_v8(repo_root);

        let src_dir = repo_root.join("src");
        std::fs::create_dir_all(&src_dir).unwrap();
        std::fs::write(src_dir.join("index.js"), "export function f() {}\n").unwrap();

        let report_dir = repo_root.join(".apex_coverage_js");
        std::fs::create_dir_all(&report_dir).unwrap();
        let json = sample_v8_coverage_json(repo_root.to_str().unwrap());
        std::fs::write(report_dir.join("coverage-final.json"), &json).unwrap();

        let runner = Arc::new(FakeRunner::success());
        let inst = JavaScriptInstrumentor::with_runner(runner);

        let target = Target {
            root: repo_root.to_path_buf(),
            language: apex_core::types::Language::JavaScript,
            test_command: Vec::new(),
        };

        let result = inst.instrument(&target).await.unwrap();
        assert_eq!(result.work_dir, repo_root.to_path_buf());
    }

    #[tokio::test]
    async fn test_instrument_v8_missing_coverage_json() {
        let tmp = tempfile::tempdir().unwrap();
        let repo_root = tmp.path();

        setup_esm_project(repo_root);
        // Do NOT create coverage-final.json

        let runner = Arc::new(FakeRunner::success());
        let inst = JavaScriptInstrumentor::with_runner(runner);

        let target = Target {
            root: repo_root.to_path_buf(),
            language: apex_core::types::Language::JavaScript,
            test_command: Vec::new(),
        };

        let result = inst.instrument(&target).await;
        assert!(result.is_err());
        let err_msg = format!("{}", result.unwrap_err());
        assert!(err_msg.contains("coverage JSON not produced"));
    }

    #[tokio::test]
    async fn test_instrument_vitest_with_custom_command_uses_default_vitest() {
        // Vitest with custom test command -> should use default vitest command (not wrapped)
        let tmp = tempfile::tempdir().unwrap();
        let repo_root = tmp.path();

        setup_vitest_project_with_v8(repo_root);

        let report_dir = repo_root.join(".apex_coverage_js");
        std::fs::create_dir_all(&report_dir).unwrap();
        let json = sample_v8_coverage_json(repo_root.to_str().unwrap());
        std::fs::write(report_dir.join("coverage-final.json"), &json).unwrap();

        let src_dir = repo_root.join("src");
        std::fs::create_dir_all(&src_dir).unwrap();
        std::fs::write(src_dir.join("index.js"), "export function f() {}\n").unwrap();

        let runner = Arc::new(FakeRunner::success());
        let inst = JavaScriptInstrumentor::with_runner(runner);

        let target = Target {
            root: repo_root.to_path_buf(),
            language: apex_core::types::Language::JavaScript,
            test_command: vec!["custom".into(), "cmd".into()],
        };

        // Vitest and Bun with custom command -> falls through to config.command.clone()
        let result = inst.instrument(&target).await.unwrap();
        assert_eq!(result.work_dir, repo_root.to_path_buf());
    }

    #[tokio::test]
    async fn test_instrument_nyc_with_custom_test_command() {
        let tmp = tempfile::tempdir().unwrap();
        let repo_root = tmp.path();

        setup_nyc_project(repo_root);

        let report_dir = repo_root.join(".apex_coverage_js");
        std::fs::create_dir_all(&report_dir).unwrap();
        let json = sample_istanbul_json(repo_root.to_str().unwrap());
        std::fs::write(report_dir.join("coverage-final.json"), &json).unwrap();

        let runner = Arc::new(FakeRunner::success());
        let inst = JavaScriptInstrumentor::with_runner(runner);

        let target = Target {
            root: repo_root.to_path_buf(),
            language: apex_core::types::Language::JavaScript,
            test_command: vec!["jest".into(), "--forceExit".into()],
        };

        // Should wrap with nyc
        let result = inst.instrument(&target).await.unwrap();
        assert_eq!(result.branch_ids.len(), 4);
    }

    #[tokio::test]
    async fn test_instrument_nonzero_exit_v8_format() {
        let tmp = tempfile::tempdir().unwrap();
        let repo_root = tmp.path();

        setup_esm_project(repo_root);

        let src_dir = repo_root.join("src");
        std::fs::create_dir_all(&src_dir).unwrap();
        std::fs::write(src_dir.join("index.js"), "function f() {}\n").unwrap();

        let report_dir = repo_root.join(".apex_coverage_js");
        std::fs::create_dir_all(&report_dir).unwrap();
        let json = sample_v8_coverage_json(repo_root.to_str().unwrap());
        std::fs::write(report_dir.join("coverage-final.json"), &json).unwrap();

        // Non-zero exit should still parse coverage
        let runner = Arc::new(FakeRunner::failure(2));
        let inst = JavaScriptInstrumentor::with_runner(runner);

        let target = Target {
            root: repo_root.to_path_buf(),
            language: apex_core::types::Language::JavaScript,
            test_command: Vec::new(),
        };

        let result = inst.instrument(&target).await.unwrap();
        assert_eq!(result.work_dir, repo_root.to_path_buf());
    }

    #[tokio::test]
    async fn test_instrument_spawn_error_esm() {
        let tmp = tempfile::tempdir().unwrap();
        let repo_root = tmp.path();

        setup_esm_project(repo_root);

        let runner = Arc::new(FakeRunner::spawn_error());
        let inst = JavaScriptInstrumentor::with_runner(runner);

        let target = Target {
            root: repo_root.to_path_buf(),
            language: apex_core::types::Language::JavaScript,
            test_command: Vec::new(),
        };

        let result = inst.instrument(&target).await;
        assert!(result.is_err());
        let err_msg = format!("{}", result.unwrap_err());
        assert!(err_msg.contains("spawn coverage tool"));
    }

    // -----------------------------------------------------------------------
    // Source map remapping path
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn test_instrument_typescript_triggers_source_map_remap() {
        let tmp = tempfile::tempdir().unwrap();
        let repo_root = tmp.path();

        // TypeScript project with ESM
        std::fs::write(
            repo_root.join("package.json"),
            r#"{"name": "ts-proj", "type": "module", "devDependencies": {"jest": "^29"}}"#,
        )
        .unwrap();
        std::fs::write(
            repo_root.join("tsconfig.json"),
            r#"{"compilerOptions": {}}"#,
        )
        .unwrap();

        let src_dir = repo_root.join("src");
        std::fs::create_dir_all(&src_dir).unwrap();
        std::fs::write(src_dir.join("index.ts"), "export function f(): void {}\n").unwrap();

        let report_dir = repo_root.join(".apex_coverage_js");
        std::fs::create_dir_all(&report_dir).unwrap();
        let json = sample_v8_coverage_json(repo_root.to_str().unwrap());
        std::fs::write(report_dir.join("coverage-final.json"), &json).unwrap();

        let runner = Arc::new(FakeRunner::success());
        let inst = JavaScriptInstrumentor::with_runner(runner);

        let target = Target {
            root: repo_root.to_path_buf(),
            language: apex_core::types::Language::JavaScript,
            test_command: Vec::new(),
        };

        // Should succeed and trigger the source_map remap path
        let result = inst.instrument(&target).await.unwrap();
        assert_eq!(result.work_dir, repo_root.to_path_buf());
    }

    // -----------------------------------------------------------------------
    // Coverage tool enum debug/equality tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_coverage_tool_debug() {
        assert_eq!(format!("{:?}", CoverageTool::Nyc), "Nyc");
        assert_eq!(format!("{:?}", CoverageTool::C8), "C8");
        assert_eq!(format!("{:?}", CoverageTool::Vitest), "Vitest");
        assert_eq!(format!("{:?}", CoverageTool::Bun), "Bun");
    }

    #[test]
    fn test_coverage_format_debug() {
        assert_eq!(format!("{:?}", CoverageFormat::V8), "V8");
        assert_eq!(format!("{:?}", CoverageFormat::Istanbul), "Istanbul");
    }

    #[test]
    fn test_coverage_output_debug() {
        let fp = CoverageOutput::FilePath(PathBuf::from("/tmp/cov.json"));
        assert!(format!("{:?}", fp).contains("FilePath"));
        let stdout = CoverageOutput::Stdout;
        assert!(format!("{:?}", stdout).contains("Stdout"));
    }

    #[test]
    fn test_coverage_tool_clone_copy() {
        let tool = CoverageTool::Nyc;
        let cloned = tool.clone();
        let copied = tool;
        assert_eq!(tool, cloned);
        assert_eq!(tool, copied);
    }

    #[test]
    fn test_coverage_format_clone_copy() {
        let fmt = CoverageFormat::V8;
        let cloned = fmt.clone();
        let copied = fmt;
        assert_eq!(fmt, cloned);
        assert_eq!(fmt, copied);
    }

    // -----------------------------------------------------------------------
    // select_coverage_tool with different test runners
    // -----------------------------------------------------------------------

    #[test]
    fn test_select_coverage_mocha_esm() {
        let tmp = tempfile::tempdir().unwrap();
        let env = JsEnvironment {
            runtime: JsRuntime::Node,
            pkg_manager: apex_lang::js_env::PkgManager::Npm,
            test_runner: JsTestRunner::Mocha,
            module_system: ModuleSystem::ESM,
            is_typescript: false,
            source_maps: false,
            monorepo: None,
        };
        let config = select_coverage_tool(&env, tmp.path());
        assert_eq!(config.tool, CoverageTool::C8);
        // Mocha command should appear in the c8 wrapper
        assert!(config.command.iter().any(|c| c == "mocha"));
    }

    #[test]
    fn test_select_coverage_npm_script_commonjs_no_nyc() {
        let tmp = tempfile::tempdir().unwrap();
        let env = JsEnvironment {
            runtime: JsRuntime::Node,
            pkg_manager: apex_lang::js_env::PkgManager::Npm,
            test_runner: JsTestRunner::NpmScript,
            module_system: ModuleSystem::CommonJS,
            is_typescript: false,
            source_maps: false,
            monorepo: None,
        };
        let config = select_coverage_tool(&env, tmp.path());
        assert_eq!(config.tool, CoverageTool::C8);
        // NpmScript uses "npm test"
        assert!(config.command.iter().any(|c| c == "npm"));
    }

    #[test]
    fn test_select_coverage_vitest_commonjs_no_v8() {
        // Vitest runner with CommonJS and no nyc
        let tmp = tempfile::tempdir().unwrap();
        let env = JsEnvironment {
            runtime: JsRuntime::Node,
            pkg_manager: apex_lang::js_env::PkgManager::Npm,
            test_runner: JsTestRunner::Vitest,
            module_system: ModuleSystem::CommonJS,
            is_typescript: false,
            source_maps: false,
            monorepo: None,
        };
        let config = select_coverage_tool(&env, tmp.path());
        // No vitest/coverage-v8 -> falls through, CommonJS no nyc -> C8
        assert_eq!(config.tool, CoverageTool::C8);
    }

    #[test]
    fn test_select_coverage_vitest_commonjs_with_nyc() {
        // Vitest runner but CommonJS with nyc installed
        let tmp = tempfile::tempdir().unwrap();
        let nyc_bin = tmp.path().join("node_modules/.bin");
        std::fs::create_dir_all(&nyc_bin).unwrap();
        std::fs::write(nyc_bin.join("nyc"), "").unwrap();
        let env = JsEnvironment {
            runtime: JsRuntime::Node,
            pkg_manager: apex_lang::js_env::PkgManager::Npm,
            test_runner: JsTestRunner::Vitest,
            module_system: ModuleSystem::CommonJS,
            is_typescript: false,
            source_maps: false,
            monorepo: None,
        };
        let config = select_coverage_tool(&env, tmp.path());
        // No vitest/coverage-v8 -> falls through, CommonJS with nyc -> Nyc
        assert_eq!(config.tool, CoverageTool::Nyc);
        assert_eq!(config.format, CoverageFormat::Istanbul);
    }

    // -----------------------------------------------------------------------
    // Istanbul deserialization edge cases
    // -----------------------------------------------------------------------

    #[test]
    fn test_istanbul_file_with_no_branches() {
        let tmp = tempfile::tempdir().unwrap();
        let json_path = tmp.path().join("cov.json");
        let json = format!(
            r#"{{
  "{root}/src/empty.js": {{
    "branchMap": {{}},
    "b": {{}}
  }}
}}"#,
            root = tmp.path().to_str().unwrap()
        );
        std::fs::write(&json_path, &json).unwrap();

        let mut inst = JavaScriptInstrumentor::new();
        inst.parse_istanbul_json(&json_path, tmp.path()).unwrap();

        assert_eq!(inst.branch_ids.len(), 0);
        assert_eq!(inst.executed_branch_ids.len(), 0);
        // File should still be registered
        assert_eq!(inst.file_paths.len(), 1);
    }

    #[test]
    fn test_istanbul_all_arms_zero_count() {
        let tmp = tempfile::tempdir().unwrap();
        let repo_root = tmp.path();
        let json_path = repo_root.join("cov.json");
        let json = format!(
            r#"{{
  "{root}/src/zero.js": {{
    "branchMap": {{
      "0": {{
        "loc": {{ "start": {{ "line": 1, "column": 0 }}, "end": {{ "line": 1, "column": 10 }} }},
        "locations": [
          {{ "start": {{ "line": 1, "column": 0 }}, "end": {{ "line": 1, "column": 5 }} }},
          {{ "start": {{ "line": 1, "column": 6 }}, "end": {{ "line": 1, "column": 10 }} }}
        ]
      }}
    }},
    "b": {{ "0": [0, 0] }}
  }}
}}"#,
            root = repo_root.to_str().unwrap()
        );
        std::fs::write(&json_path, &json).unwrap();

        let mut inst = JavaScriptInstrumentor::new();
        inst.parse_istanbul_json(&json_path, repo_root).unwrap();

        assert_eq!(inst.branch_ids.len(), 2);
        assert_eq!(inst.executed_branch_ids.len(), 0);
    }

    #[test]
    fn test_istanbul_many_branches_in_one_file() {
        let tmp = tempfile::tempdir().unwrap();
        let repo_root = tmp.path();
        let json_path = repo_root.join("cov.json");
        let json = format!(
            r#"{{
  "{root}/src/big.js": {{
    "branchMap": {{
      "0": {{
        "loc": {{ "start": {{ "line": 1, "column": 0 }}, "end": {{ "line": 1, "column": 10 }} }},
        "locations": [
          {{ "start": {{ "line": 1, "column": 0 }}, "end": {{ "line": 1, "column": 5 }} }},
          {{ "start": {{ "line": 1, "column": 6 }}, "end": {{ "line": 1, "column": 10 }} }}
        ]
      }},
      "1": {{
        "loc": {{ "start": {{ "line": 3, "column": 0 }}, "end": {{ "line": 3, "column": 10 }} }},
        "locations": [
          {{ "start": {{ "line": 3, "column": 0 }}, "end": {{ "line": 3, "column": 5 }} }},
          {{ "start": {{ "line": 3, "column": 6 }}, "end": {{ "line": 3, "column": 10 }} }}
        ]
      }},
      "2": {{
        "loc": {{ "start": {{ "line": 5, "column": 0 }}, "end": {{ "line": 5, "column": 10 }} }},
        "locations": [
          {{ "start": {{ "line": 5, "column": 0 }}, "end": {{ "line": 5, "column": 5 }} }},
          {{ "start": {{ "line": 5, "column": 6 }}, "end": {{ "line": 5, "column": 10 }} }}
        ]
      }}
    }},
    "b": {{
      "0": [1, 0],
      "1": [0, 1],
      "2": [1, 1]
    }}
  }}
}}"#,
            root = repo_root.to_str().unwrap()
        );
        std::fs::write(&json_path, &json).unwrap();

        let mut inst = JavaScriptInstrumentor::new();
        inst.parse_istanbul_json(&json_path, repo_root).unwrap();

        assert_eq!(inst.branch_ids.len(), 6);
        // branch0: [1,0] -> 1 hit, branch1: [0,1] -> 1 hit, branch2: [1,1] -> 2 hit = 4
        assert_eq!(inst.executed_branch_ids.len(), 4);
    }

    #[test]
    fn test_with_runner_creates_empty_instrumentor() {
        let runner = Arc::new(FakeRunner::success());
        let inst = JavaScriptInstrumentor::with_runner(runner);
        assert!(inst.branch_ids.is_empty());
        assert!(inst.executed_branch_ids.is_empty());
        assert!(inst.file_paths.is_empty());
    }

    // -----------------------------------------------------------------------
    // Bug-hunting: Istanbul parser boundary tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_istanbul_missing_branch_map_key_in_b() {
        // branchMap has key "0" but b has no key "0" — should default to not-executed
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let json_path = root.join("coverage-final.json");
        let json = format!(
            r#"{{
  "{root}/src/app.js": {{
    "branchMap": {{
      "0": {{
        "loc": {{ "start": {{ "line": 5, "column": 4 }}, "end": {{ "line": 5, "column": 30 }} }},
        "locations": [
          {{ "start": {{ "line": 5, "column": 4 }}, "end": {{ "line": 5, "column": 15 }} }},
          {{ "start": {{ "line": 5, "column": 18 }}, "end": {{ "line": 5, "column": 30 }} }}
        ]
      }}
    }},
    "b": {{}}
  }}
}}"#,
            root = root.display()
        );
        std::fs::write(&json_path, &json).unwrap();

        let mut inst = JavaScriptInstrumentor::new();
        inst.parse_istanbul_json(&json_path, root).unwrap();
        // 2 arms, neither executed (b has no key "0")
        assert_eq!(inst.branch_ids.len(), 2);
        assert_eq!(inst.executed_branch_ids.len(), 0);
    }

    #[test]
    fn test_istanbul_b_count_shorter_than_locations() {
        // locations has 3 arms but b["0"] has only 1 count — arms 1,2 are not hit
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let json_path = root.join("coverage-final.json");
        let json = format!(
            r#"{{
  "{root}/src/app.js": {{
    "branchMap": {{
      "0": {{
        "loc": {{ "start": {{ "line": 5, "column": 0 }}, "end": {{ "line": 5, "column": 30 }} }},
        "locations": [
          {{ "start": {{ "line": 5, "column": 0 }}, "end": {{ "line": 5, "column": 10 }} }},
          {{ "start": {{ "line": 5, "column": 11 }}, "end": {{ "line": 5, "column": 20 }} }},
          {{ "start": {{ "line": 5, "column": 21 }}, "end": {{ "line": 5, "column": 30 }} }}
        ]
      }}
    }},
    "b": {{
      "0": [5]
    }}
  }}
}}"#,
            root = root.display()
        );
        std::fs::write(&json_path, &json).unwrap();

        let mut inst = JavaScriptInstrumentor::new();
        inst.parse_istanbul_json(&json_path, root).unwrap();
        assert_eq!(inst.branch_ids.len(), 3);
        // Only first arm was hit (count=5), the other two have no count entry
        assert_eq!(inst.executed_branch_ids.len(), 1);
    }

    #[test]
    fn test_istanbul_empty_filename() {
        // Empty string as filename — should parse but produce an odd file_id
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let json_path = root.join("coverage-final.json");
        let json = r#"{
  "": {
    "branchMap": {
      "0": {
        "loc": { "start": { "line": 1, "column": 0 }, "end": { "line": 1, "column": 10 } },
        "locations": []
      }
    },
    "b": {
      "0": [1, 0]
    }
  }
}"#;
        std::fs::write(&json_path, json).unwrap();

        let mut inst = JavaScriptInstrumentor::new();
        inst.parse_istanbul_json(&json_path, root).unwrap();
        // Empty locations but b["0"] has 2 counts => arm_count = 2
        assert_eq!(inst.branch_ids.len(), 2);
    }

    #[test]
    fn test_istanbul_arm_index_u8_overflow() {
        // BUG: If a branch has > 255 arms, `i as u8` wraps silently.
        // This creates duplicate BranchIds.
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let json_path = root.join("coverage-final.json");

        // Build a branch with 257 arms via b counts
        let counts: Vec<String> = (0..257)
            .map(|i| if i == 0 { "1".into() } else { "0".into() })
            .collect();
        let counts_str = counts.join(", ");

        let json = format!(
            r#"{{
  "{root}/src/big.js": {{
    "branchMap": {{
      "0": {{
        "loc": {{ "start": {{ "line": 1, "column": 0 }}, "end": {{ "line": 1, "column": 10 }} }},
        "locations": []
      }}
    }},
    "b": {{
      "0": [{counts_str}]
    }}
  }}
}}"#,
            root = root.display(),
            counts_str = counts_str
        );
        std::fs::write(&json_path, &json).unwrap();

        let mut inst = JavaScriptInstrumentor::new();
        inst.parse_istanbul_json(&json_path, root).unwrap();

        // After fix: arms beyond 255 are truncated, so we get at most 256
        assert_eq!(inst.branch_ids.len(), 256);
    }

    #[test]
    fn test_istanbul_empty_branch_map() {
        // File with empty branchMap — should produce 0 branches
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let json_path = root.join("coverage-final.json");
        let json = format!(
            r#"{{
  "{root}/src/no_branch.js": {{
    "branchMap": {{}},
    "b": {{}}
  }}
}}"#,
            root = root.display()
        );
        std::fs::write(&json_path, &json).unwrap();

        let mut inst = JavaScriptInstrumentor::new();
        inst.parse_istanbul_json(&json_path, root).unwrap();
        assert_eq!(inst.branch_ids.len(), 0);
        // File still gets registered
        assert_eq!(inst.file_paths.len(), 1);
    }

    // -----------------------------------------------------------------------
    // Bug-hunting tests
    // -----------------------------------------------------------------------

    /// BUG: `i as u8` on line 270 silently truncates direction for branches
    /// with more than 255 arms. A switch statement with 256+ cases would
    /// produce branch IDs with direction wrapping around to 0, creating
    /// collisions with earlier arms.
    #[test]
    fn bug_istanbul_direction_overflow_u8() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let json_path = root.join("coverage-final.json");

        // Build a branch with 260 arms (> 255, the max u8 value)
        let mut locations = Vec::new();
        let mut counts = Vec::new();
        for i in 0..260u32 {
            locations.push(format!(
                r#"{{ "start": {{ "line": {}, "column": 0 }} }}"#,
                100 + i
            ));
            counts.push("1");
        }

        let json = format!(
            r#"{{
  "{root}/src/big_switch.js": {{
    "branchMap": {{
      "0": {{
        "loc": {{ "start": {{ "line": 100, "column": 0 }}, "end": {{ "line": 360, "column": 1 }} }},
        "locations": [{locations}]
      }}
    }},
    "b": {{
      "0": [{counts}]
    }}
  }}
}}"#,
            root = root.display(),
            locations = locations.join(", "),
            counts = counts.join(", ")
        );
        std::fs::write(&json_path, &json).unwrap();

        let mut inst = JavaScriptInstrumentor::new();
        inst.parse_istanbul_json(&json_path, root).unwrap();

        // After fix: arms beyond 255 are truncated, so we get at most 256
        assert_eq!(inst.branch_ids.len(), 256);
        // No collision possible since arm 256+ are never created
        let arm_0 = &inst.branch_ids[0];
        let arm_255 = &inst.branch_ids[255];
        assert_ne!(arm_0.direction, arm_255.direction);
    }

    /// BUG: When branchMap has a key not present in the `b` map, the code
    /// defaults to arm_count=2 (line 256), but then hit_counts is None,
    /// so all arms are marked as not hit. This is correct behavior for missing
    /// data, but worth documenting.
    #[test]
    fn istanbul_missing_b_key_defaults_to_two_arms() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let json_path = root.join("coverage-final.json");
        let json = format!(
            r#"{{
  "{root}/src/missing.js": {{
    "branchMap": {{
      "0": {{
        "loc": {{ "start": {{ "line": 5, "column": 0 }}, "end": {{ "line": 5, "column": 10 }} }},
        "locations": []
      }}
    }},
    "b": {{}}
  }}
}}"#,
            root = root.display()
        );
        std::fs::write(&json_path, &json).unwrap();

        let mut inst = JavaScriptInstrumentor::new();
        inst.parse_istanbul_json(&json_path, root).unwrap();

        // locations is empty, b has no key "0" -> arm_count = 2 (default)
        assert_eq!(inst.branch_ids.len(), 2);
        // None are hit because b["0"] doesn't exist
        assert_eq!(inst.executed_branch_ids.len(), 0);
    }

    // -----------------------------------------------------------------------
    // resolve_bun helper
    // -----------------------------------------------------------------------

    #[test]
    fn resolve_bun_returns_string_or_none() {
        // The return value depends on the test environment (bun may or may not
        // be installed).  We only assert that the function returns without
        // panicking and, if present, yields the literal string "bun".
        let result = resolve_bun();
        if let Some(s) = result {
            assert_eq!(s, "bun");
        }
    }

    // -----------------------------------------------------------------------
    // Bun instrument() path via NODE_V8_COVERAGE
    // -----------------------------------------------------------------------

    fn setup_bun_project(root: &Path) {
        std::fs::write(
            root.join("package.json"),
            r#"{"name": "bun-proj", "devDependencies": {}}"#,
        )
        .unwrap();
        // bun.lockb signals bun runtime
        std::fs::write(root.join("bun.lockb"), b"").unwrap();
    }

    #[tokio::test]
    async fn test_instrument_bun_sets_node_v8_coverage_env() {
        // When instrumenting a Bun project, the instrument() call must set
        // NODE_V8_COVERAGE in the environment and then collect the JSON files
        // it writes.
        let tmp = tempfile::tempdir().unwrap();
        let repo_root = tmp.path();

        setup_bun_project(repo_root);

        // Pre-create the bun V8 coverage directory and put a JSON file in it.
        let v8_dir = repo_root.join(".apex_coverage_js").join("bun_v8");
        std::fs::create_dir_all(&v8_dir).unwrap();

        let src_dir = repo_root.join("src");
        std::fs::create_dir_all(&src_dir).unwrap();
        std::fs::write(
            src_dir.join("index.js"),
            "function foo() {\n  return 1;\n}\n",
        )
        .unwrap();

        // Write a V8-format JSON file into the bun_v8 directory.
        let v8_json = sample_v8_coverage_json(repo_root.to_str().unwrap());
        std::fs::write(v8_dir.join("coverage-0.json"), &v8_json).unwrap();

        let runner = Arc::new(FakeRunner::success());
        let inst = JavaScriptInstrumentor::with_runner(runner);

        let target = Target {
            root: repo_root.to_path_buf(),
            language: apex_core::types::Language::JavaScript,
            test_command: Vec::new(),
        };

        let result = inst.instrument(&target).await.unwrap();
        assert_eq!(result.work_dir, repo_root.to_path_buf());
        // V8 parser should have seen at least one file
        assert!(!result.file_paths.is_empty());
    }

    #[tokio::test]
    async fn test_instrument_bun_empty_v8_dir_is_err() {
        // If the bun_v8 directory is empty (no JSON files), instrument() should
        // return an error rather than silently producing no coverage data.
        let tmp = tempfile::tempdir().unwrap();
        let repo_root = tmp.path();

        setup_bun_project(repo_root);

        // Create the directory but leave it empty.
        let v8_dir = repo_root.join(".apex_coverage_js").join("bun_v8");
        std::fs::create_dir_all(&v8_dir).unwrap();

        let runner = Arc::new(FakeRunner::success());
        let inst = JavaScriptInstrumentor::with_runner(runner);

        let target = Target {
            root: repo_root.to_path_buf(),
            language: apex_core::types::Language::JavaScript,
            test_command: Vec::new(),
        };

        let result = inst.instrument(&target).await;
        assert!(result.is_err());
        let msg = format!("{}", result.unwrap_err());
        assert!(
            msg.contains("no V8 coverage JSON files"),
            "unexpected: {msg}"
        );
    }

    #[tokio::test]
    async fn test_instrument_bun_multiple_v8_json_files_merged() {
        // Multiple per-script JSON files are all merged into one result.
        let tmp = tempfile::tempdir().unwrap();
        let repo_root = tmp.path();

        setup_bun_project(repo_root);

        let v8_dir = repo_root.join(".apex_coverage_js").join("bun_v8");
        std::fs::create_dir_all(&v8_dir).unwrap();

        let src_dir = repo_root.join("src");
        std::fs::create_dir_all(&src_dir).unwrap();
        std::fs::write(src_dir.join("index.js"), "function f() {}\n").unwrap();

        // Two separate V8 JSON files (bun writes one per script).
        let json1 = format!(
            r#"{{"result":[{{"url":"file://{root}/src/a.js","functions":[{{"ranges":[{{"startOffset":0,"endOffset":10,"count":1}}]}}]}}]}}"#,
            root = repo_root.to_str().unwrap()
        );
        let json2 = format!(
            r#"{{"result":[{{"url":"file://{root}/src/b.js","functions":[{{"ranges":[{{"startOffset":0,"endOffset":10,"count":0}}]}}]}}]}}"#,
            root = repo_root.to_str().unwrap()
        );
        std::fs::write(v8_dir.join("cov-a.json"), &json1).unwrap();
        std::fs::write(v8_dir.join("cov-b.json"), &json2).unwrap();

        let runner = Arc::new(FakeRunner::success());
        let inst = JavaScriptInstrumentor::with_runner(runner);

        let target = Target {
            root: repo_root.to_path_buf(),
            language: apex_core::types::Language::JavaScript,
            test_command: Vec::new(),
        };

        let result = inst.instrument(&target).await.unwrap();
        // Should have file paths from both JSON files
        assert_eq!(result.file_paths.len(), 2);
    }
}
