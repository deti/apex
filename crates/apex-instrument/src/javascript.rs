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
}

fn select_coverage_tool(env: &JsEnvironment, target: &Path) -> CoverageToolConfig {
    match env.runtime {
        JsRuntime::Bun => CoverageToolConfig {
            tool: CoverageTool::Bun,
            command: vec!["bun".into(), "test".into(), "--coverage".into()],
            output_path: CoverageOutput::Stdout,
            format: CoverageFormat::V8,
        },
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
                                format!("--reports-dir={}", report_dir.display()),
                                bin,
                            ];
                            cmd.extend(args);
                            cmd
                        },
                        output_path: CoverageOutput::FilePath(
                            report_dir.join("coverage-final.json"),
                        ),
                        format: CoverageFormat::V8,
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
                                    format!("--report-dir={}", report_dir.display()),
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
                                    format!("--reports-dir={}", report_dir.display()),
                                    bin,
                                ];
                                cmd.extend(args);
                                cmd
                            },
                            output_path: CoverageOutput::FilePath(
                                report_dir.join("coverage-final.json"),
                            ),
                            format: CoverageFormat::V8,
                        }
                    }
                }
            }
        }
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
    work_dir: Option<PathBuf>,
    runner: Arc<dyn CommandRunner>,
}

impl JavaScriptInstrumentor {
    pub fn new() -> Self {
        JavaScriptInstrumentor {
            branch_ids: Vec::new(),
            executed_branch_ids: Vec::new(),
            file_paths: HashMap::new(),
            work_dir: None,
            runner: Arc::new(RealCommandRunner),
        }
    }

    /// Create a new instrumentor with a custom command runner (for testing).
    pub fn with_runner(runner: Arc<dyn CommandRunner>) -> Self {
        JavaScriptInstrumentor {
            branch_ids: Vec::new(),
            executed_branch_ids: Vec::new(),
            file_paths: HashMap::new(),
            work_dir: None,
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
                "no package.json found at {}; is this a JS/TS project?",
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
                        format!("--report-dir={}", report_dir.display()),
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
                        format!("--reports-dir={}", report_dir.display()),
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

        info!(
            target = %target.root.display(),
            cmd = ?effective_cmd,
            "running JavaScript instrumentation"
        );

        let (program, args) = effective_cmd
            .split_first()
            .ok_or_else(|| ApexError::Instrumentation("empty command".into()))?;

        let spec = CommandSpec::new(program, &target.root).args(args.to_vec());
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
                (inner.branch_ids, inner.executed_branch_ids, inner.file_paths)
            }
            CoverageFormat::V8 => {
                let json_path = match &config.output_path {
                    CoverageOutput::FilePath(p) => p.clone(),
                    CoverageOutput::Stdout => {
                        // TODO: Parse V8 coverage from stdout
                        return Err(ApexError::Instrumentation(
                            "V8 coverage from stdout not yet implemented".into(),
                        ));
                    }
                };
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
                .map_err(|e| ApexError::Instrumentation(e))?
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
        assert!(inst.work_dir.is_none());
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
}
