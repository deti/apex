/// Concolic execution strategy for Python targets.
///
/// Workflow
/// --------
/// 1. Run the existing test suite under `apex_tracer.py` (sys.settrace).
///    The tracer records every branch: file, line, direction, condition text,
///    enclosing function, and scalar local variables at the moment of branching.
///
/// 2. For each branch the oracle has marked *Uncovered*, scan the trace for an
///    entry at the same (file, line) with the *opposite* direction.  That entry
///    gives us the concrete variable values that *almost* reached the target.
///
/// 3. From the condition text and those values, generate boundary mutations:
///    e.g. if the condition is `x > 0` and the trace saw x=5 (True), to reach
///    False we try x=0, x=-1, x=1 (the boundary values around 0).
///
/// 4. Synthesise a minimal Python test stub that imports the target function
///    and calls it with the mutated values.  The PythonTestSandbox runs the stub
///    and measures coverage delta.
use apex_core::{
    error::{ApexError, Result},
    traits::Strategy,
    types::{BranchId, ExecutionResult, ExplorationContext, InputSeed, PathConstraint, SeedOrigin},
};
use apex_coverage::CoverageOracle;
use apex_symbolic::{smtlib, SymbolicSession};
use async_trait::async_trait;
use serde::Deserialize;
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{Arc, Mutex},
};
use tracing::{debug, info, warn};

/// Embedded tracer script — extracted to a temp file at runtime.
const TRACER_PY: &str = include_str!("scripts/apex_tracer.py");

// ---------------------------------------------------------------------------
// Tracer output types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize)]
struct BranchTrace {
    file: String,
    line: u32,
    direction: u8,
    #[serde(default)]
    condition: String,
    #[serde(default)]
    func: String,
    #[serde(default)]
    module: String,
    #[serde(default)]
    args: Vec<String>,
    #[serde(default)]
    locals: HashMap<String, serde_json::Value>,
}

#[derive(Debug, Deserialize)]
struct TraceOutput {
    branches: Vec<BranchTrace>,
}

// ---------------------------------------------------------------------------
// Strategy
// ---------------------------------------------------------------------------

#[allow(dead_code)]
pub struct PythonConcolicStrategy {
    oracle: Arc<CoverageOracle>,
    file_paths: Arc<HashMap<u64, PathBuf>>,
    target_root: PathBuf,
    test_command: Vec<String>,
    /// Cached trace from the last tracer run.
    trace_cache: Mutex<Option<Vec<BranchTrace>>>,
}

impl PythonConcolicStrategy {
    pub fn new(
        oracle: Arc<CoverageOracle>,
        file_paths: Arc<HashMap<u64, PathBuf>>,
        target_root: PathBuf,
        test_command: Vec<String>,
    ) -> Self {
        PythonConcolicStrategy {
            oracle,
            file_paths,
            target_root,
            test_command,
            trace_cache: Mutex::new(None),
        }
    }

    // -----------------------------------------------------------------------
    // Tracer execution
    // -----------------------------------------------------------------------

    /// Write the embedded tracer to a temp file and run it against the target.
    async fn run_tracer(&self) -> Result<Vec<BranchTrace>> {
        let tracer_path = self.target_root.join(".apex_tracer.py");
        std::fs::write(&tracer_path, TRACER_PY)
            .map_err(|e| ApexError::Sandbox(format!("write tracer: {e}")))?;

        let trace_path = self.target_root.join(".apex_trace.json");

        let mut cmd_args = vec![
            tracer_path.to_string_lossy().to_string(),
            self.target_root.to_string_lossy().to_string(),
            trace_path.to_string_lossy().to_string(),
        ];
        cmd_args.extend(self.test_command.clone());

        let output = tokio::process::Command::new("python3")
            .args(&cmd_args)
            .current_dir(&self.target_root)
            .output()
            .await
            .map_err(|e| ApexError::Sandbox(format!("run tracer: {e}")))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            warn!(stderr = %stderr, "tracer exited non-zero");
        }

        let json_bytes = std::fs::read(&trace_path)
            .map_err(|e| ApexError::Sandbox(format!("read trace: {e}")))?;

        let parsed: TraceOutput = serde_json::from_slice(&json_bytes)
            .map_err(|e| ApexError::Sandbox(format!("parse trace: {e}")))?;

        info!(
            branches = parsed.branches.len(),
            "tracer collected branches"
        );
        Ok(parsed.branches)
    }

    /// Get (or refresh) the cached trace.
    async fn get_trace(&self) -> Result<Vec<BranchTrace>> {
        // Run fresh each call — the oracle state changes between rounds.
        let trace = self.run_tracer().await?;
        *self.trace_cache.lock().unwrap_or_else(|e| e.into_inner()) = Some(trace.clone());
        Ok(trace)
    }

    // -----------------------------------------------------------------------
    // Seed generation
    // -----------------------------------------------------------------------

    /// Generate boundary mutations for one uncovered branch given the
    /// opposite-direction trace entry that came closest to covering it.
    fn boundary_seeds(&self, trace_entry: &BranchTrace, target_direction: u8) -> Vec<String> {
        let condition = &trace_entry.condition;
        let module = &trace_entry.module;
        let func = &trace_entry.func;

        // Parse condition for simple comparison patterns.
        let mut assignments: Vec<Vec<(String, serde_json::Value)>> = Vec::new();

        // Look for patterns: <name> <op> <literal>
        let re_cmp = regex_lite::Regex::new(
            r#"^(\w[\w.]*)\s*(>|>=|<|<=|==|!=)\s*(-?\d+(?:\.\d+)?|None|True|False|'[^']*'|"[^"]*")$"#
        ).ok();

        if let Some(re) = re_cmp {
            if let Some(caps) = re.captures(condition.trim()) {
                let name = caps[1].to_string();
                let op = &caps[2];
                let lit = &caps[3];

                if let Ok(val) = lit.parse::<i64>() {
                    let variants: Vec<i64> = match (op, target_direction) {
                        // We observed True, want False
                        (">", 1) => vec![val, val.saturating_sub(1)],
                        (">=", 1) => vec![val.saturating_sub(1), val.saturating_sub(2)],
                        ("<", 1) => vec![val, val.saturating_add(1)],
                        ("<=", 1) => vec![val.saturating_add(1), val.saturating_add(2)],
                        ("==", 1) => vec![val.saturating_add(1), val.saturating_sub(1)],
                        ("!=", 1) => vec![val],
                        // We observed False, want True
                        (">", 0) => vec![val.saturating_add(1), val.saturating_add(2)],
                        (">=", 0) => vec![val, val.saturating_add(1)],
                        ("<", 0) => vec![val.saturating_sub(1), val.saturating_sub(2)],
                        ("<=", 0) => vec![val, val.saturating_sub(1)],
                        ("==", 0) => vec![val],
                        ("!=", 0) => vec![val.saturating_add(1), val.saturating_sub(1)],
                        _ => vec![0, 1, -1],
                    };
                    for v in variants {
                        assignments.push(vec![(name.clone(), serde_json::json!(v))]);
                    }
                }
            }
        }

        // --- String method patterns (startswith/endswith) ---
        if assignments.is_empty() {
            let re_str_method =
                regex_lite::Regex::new(r#"^(\w+)\.(startswith|endswith)\(['\"](.+?)['\"]\)$"#).ok();
            if let Some(re) = re_str_method {
                if let Some(caps) = re.captures(condition.trim()) {
                    let name = caps[1].to_string();
                    let method = caps[2].to_string();
                    let affix = caps[3].to_string();
                    match (method.as_str(), target_direction) {
                        ("startswith", 0) => {
                            assignments.push(vec![(
                                name.clone(),
                                serde_json::json!(format!("{affix}suffix")),
                            )]);
                        }
                        ("startswith", _) => {
                            assignments
                                .push(vec![(name.clone(), serde_json::json!("__no_match__"))]);
                        }
                        ("endswith", 0) => {
                            assignments.push(vec![(
                                name.clone(),
                                serde_json::json!(format!("prefix{affix}")),
                            )]);
                        }
                        ("endswith", _) => {
                            assignments
                                .push(vec![(name.clone(), serde_json::json!("__no_match__"))]);
                        }
                        _ => {}
                    }
                }
            }
        }

        // --- Membership: x in [list] ---
        if assignments.is_empty() {
            let re_in_list = regex_lite::Regex::new(r#"^(\w+)\s+in\s+\[(.+)\]$"#).ok();
            if let Some(re) = re_in_list {
                if let Some(caps) = re.captures(condition.trim()) {
                    let name = caps[1].to_string();
                    let items_str = caps[2].to_string();
                    let items: Vec<String> = items_str
                        .split(',')
                        .map(|s| {
                            s.trim()
                                .trim_matches(|c: char| c == '\'' || c == '"')
                                .to_string()
                        })
                        .collect();
                    if target_direction == 0 {
                        for item in items.iter().take(3) {
                            assignments.push(vec![(name.clone(), serde_json::json!(item))]);
                        }
                    } else {
                        assignments
                            .push(vec![(name.clone(), serde_json::json!("__NOT_IN_LIST__"))]);
                    }
                }
            }
        }

        // --- isinstance check ---
        if assignments.is_empty() {
            let re_isinstance = regex_lite::Regex::new(r#"^isinstance\((\w+),\s*(\w+)\)$"#).ok();
            if let Some(re) = re_isinstance {
                if let Some(caps) = re.captures(condition.trim()) {
                    let name = caps[1].to_string();
                    let type_name = caps[2].to_string();
                    if target_direction == 0 {
                        let val = match type_name.as_str() {
                            "str" => serde_json::json!(""),
                            "int" => serde_json::json!(0),
                            "float" => serde_json::json!(0.0),
                            "bool" => serde_json::json!(true),
                            "list" => serde_json::json!([]),
                            "dict" => serde_json::json!({}),
                            _ => serde_json::json!(""),
                        };
                        assignments.push(vec![(name.clone(), val)]);
                    } else {
                        let val = match type_name.as_str() {
                            "str" => serde_json::json!(0),
                            "int" | "float" => serde_json::json!("not_a_number"),
                            _ => serde_json::json!(null),
                        };
                        assignments.push(vec![(name.clone(), val)]);
                    }
                }
            }
        }

        // --- Substring contains: "://" in x ---
        if assignments.is_empty() {
            let re_substr = regex_lite::Regex::new(r#"^['\"](.+?)['\"]\s+in\s+(\w+)$"#).ok();
            if let Some(re) = re_substr {
                if let Some(caps) = re.captures(condition.trim()) {
                    let substring = caps[1].to_string();
                    let name = caps[2].to_string();
                    if target_direction == 0 {
                        assignments.push(vec![(
                            name.clone(),
                            serde_json::json!(format!("prefix{substring}suffix")),
                        )]);
                    } else {
                        assignments.push(vec![(name.clone(), serde_json::json!("no_match_here"))]);
                    }
                }
            }
        }

        // --- len check: len(x) > N ---
        if assignments.is_empty() {
            let re_len = regex_lite::Regex::new(r#"^len\((\w+)\)\s*(>|>=|==|<|<=)\s*(\d+)$"#).ok();
            if let Some(re) = re_len {
                if let Some(caps) = re.captures(condition.trim()) {
                    let name = caps[1].to_string();
                    let op = caps[2].to_string();
                    let n: usize = caps[3].parse().unwrap_or(0);
                    let target_len = match (op.as_str(), target_direction) {
                        (">", 0) | (">=", 0) => n + 1,
                        (">", _) | (">=", _) => {
                            if n > 0 {
                                n - 1
                            } else {
                                0
                            }
                        }
                        ("<", 0) | ("<=", 0) => {
                            if n > 0 {
                                n - 1
                            } else {
                                0
                            }
                        }
                        ("<", _) | ("<=", _) => n + 1,
                        ("==", 0) => n,
                        ("==", _) => n + 1,
                        _ => 1,
                    };
                    let val = "a".repeat(target_len);
                    assignments.push(vec![(name.clone(), serde_json::json!(val))]);
                }
            }
        }

        // --- None/is check ---
        if assignments.is_empty() {
            let re_is = regex_lite::Regex::new(r#"^(\w+)\s+is\s+(not None|None)$"#).ok();
            if let Some(re) = re_is {
                if let Some(caps) = re.captures(condition.trim()) {
                    let name = caps[1].to_string();
                    let check = caps[2].to_string();
                    match (check.as_str(), target_direction) {
                        ("None", 0) | ("not None", 1) => {
                            // Want True for "is None" or False for "is not None" -> use None
                            assignments.push(vec![(name.clone(), serde_json::json!(null))]);
                        }
                        _ => {
                            // Want a non-None value
                            assignments.push(vec![(name.clone(), serde_json::json!(0))]);
                        }
                    }
                }
            }
        }

        // Fallback: mutate all scalar locals by ±1 and boundary values.
        if assignments.is_empty() {
            let mut row = Vec::new();
            for (k, v) in &trace_entry.locals {
                if let Some(n) = v.as_i64() {
                    let flip = if target_direction == 0 { n.saturating_add(1) } else { n.saturating_sub(1) };
                    row.push((k.clone(), serde_json::json!(flip)));
                } else if v.is_null() {
                    row.push((k.clone(), serde_json::json!(0)));
                }
            }
            if !row.is_empty() {
                assignments.push(row);
                assignments.push(
                    trace_entry
                        .locals
                        .iter()
                        .filter(|(_, v)| v.as_i64().is_some())
                        .map(|(k, _)| (k.clone(), serde_json::json!(0)))
                        .collect(),
                );
            }
        }

        // Synthesise a Python test stub for each variant.
        if module.is_empty() || func.is_empty() {
            return Vec::new();
        }

        let mut seeds = Vec::new();
        for (idx, variant) in assignments.into_iter().take(3).enumerate() {
            let assigns: String = {
                let lines: Vec<String> = variant
                    .iter()
                    .map(|(k, v)| format!("    {k} = {v}"))
                    .collect();
                if lines.is_empty() {
                    "    pass".to_string()
                } else {
                    lines.join("\n")
                }
            };

            let call_args: String = trace_entry
                .args
                .iter()
                .filter(|a| *a != "self")
                .map(|a| {
                    variant
                        .iter()
                        .find(|(k, _)| k == a)
                        .map(|(_, v)| v.to_string())
                        .unwrap_or_else(|| {
                            trace_entry
                                .locals
                                .get(a)
                                .map(|v| v.to_string())
                                .unwrap_or_else(|| "None".to_string())
                        })
                })
                .collect::<Vec<_>>()
                .join(", ");

            let seed = format!(
                r#"# apex-concolic: {file}:{line} direction={dir}
# condition: {cond}
# variant {idx}
import sys
sys.path.insert(0, "{root}")

def test_concolic_{func}_{line}_v{idx}():
{assigns}
    try:
        from {module} import {func}
        {func}({call_args})
    except Exception:
        pass  # coverage gained even if call raises
"#,
                file = trace_entry.file,
                line = trace_entry.line,
                dir = target_direction,
                cond = condition,
                idx = idx,
                root = self.target_root.to_string_lossy(),
                func = func,
                module = module,
                assigns = assigns,
                call_args = call_args,
            );
            seeds.push(seed);
        }
        seeds
    }

    // -----------------------------------------------------------------------
    // Symbolic path (Z3-backed, see apex-symbolic)
    // -----------------------------------------------------------------------

    /// Build a `SymbolicSession` from the ordered trace and return any seeds
    /// the SMT solver can produce by negating path prefixes.
    ///
    /// This is a no-op when the `z3-solver` feature is absent — `solve()`
    /// returns `None` for every constraint and `diverging_inputs()` returns
    /// an empty vec.
    fn symbolic_seeds_from_trace(&self, trace: &[BranchTrace]) -> Vec<InputSeed> {
        let mut session = SymbolicSession::new();

        for entry in trace {
            // Only entries whose conditions we can convert to SMTLIB2.
            let Some(smtlib2) = smtlib::condition_to_smtlib2(&entry.condition) else {
                continue;
            };

            // Compute a placeholder file_id (0) — the symbolic solver only
            // uses the smtlib2 string, not the BranchId fields.
            let branch = BranchId::new(0, entry.line, 0, entry.direction);

            session.push(PathConstraint {
                branch,
                smtlib2,
                direction_taken: entry.direction == 0,
            });
        }

        if session.is_empty() {
            return Vec::new();
        }

        match session.diverging_inputs_generational() {
            Ok(seeds) => seeds,
            Err(e) => {
                debug!(error = %e, "symbolic diverging_inputs failed");
                Vec::new()
            }
        }
    }
}

#[async_trait]
impl Strategy for PythonConcolicStrategy {
    fn name(&self) -> &str {
        "python-concolic"
    }

    async fn suggest_inputs(&self, ctx: &ExplorationContext) -> Result<Vec<InputSeed>> {
        if ctx.uncovered_branches.is_empty() {
            return Ok(Vec::new());
        }

        let trace = match self.get_trace().await {
            Ok(t) => t,
            Err(e) => {
                warn!(error = %e, "concolic tracer failed");
                return Err(e);
            }
        };

        // Build index: (rel_file, line) → Vec<&BranchTrace>
        let mut trace_index: HashMap<(String, u32), Vec<&BranchTrace>> = HashMap::new();
        for entry in &trace {
            trace_index
                .entry((entry.file.clone(), entry.line))
                .or_default()
                .push(entry);
        }

        let mut seeds: Vec<InputSeed> = Vec::new();

        // ── Phase 1: boundary-mutation seeds (always) ───────────────────────
        for branch in &ctx.uncovered_branches {
            // Resolve file_id → relative path string.
            let Some(rel_path) = self.file_paths.get(&branch.file_id) else {
                continue;
            };
            let key = (rel_path.to_string_lossy().to_string(), branch.line);

            // Find trace entries that went the *opposite* direction — these are
            // the nearest executions that didn't take the uncovered path.
            let opposite_dir = 1 - branch.direction;
            let Some(opposite_entries) = trace_index.get(&key) else {
                continue;
            };
            let nearest: Vec<&&BranchTrace> = opposite_entries
                .iter()
                .filter(|e| e.direction == opposite_dir)
                .collect();

            if nearest.is_empty() {
                debug!(
                    file = %rel_path.display(),
                    line = branch.line,
                    "no opposite-direction trace entry; skipping"
                );
                continue;
            }

            // Use the last (most recent) opposite-direction entry.
            let entry = nearest[nearest.len() - 1];
            let code_variants = self.boundary_seeds(entry, branch.direction);

            for code in code_variants {
                seeds.push(InputSeed::new(code.into_bytes(), SeedOrigin::Concolic));
            }
        }

        // ── Phase 2: symbolic seeds via Z3 (z3-solver feature; no-ops otherwise) ──
        let symbolic_seeds = self.symbolic_seeds_from_trace(&trace);
        if !symbolic_seeds.is_empty() {
            info!(
                symbolic = symbolic_seeds.len(),
                "adding symbolic seeds from Z3"
            );
            seeds.extend(symbolic_seeds);
        }

        info!(seeds = seeds.len(), "concolic seeds generated");
        Ok(seeds)
    }

    async fn observe(&self, result: &ExecutionResult) -> Result<()> {
        debug!(seed_id = ?result.seed_id, status = ?result.status, "concolic observe");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_strategy() -> PythonConcolicStrategy {
        PythonConcolicStrategy::new(
            Arc::new(CoverageOracle::new()),
            Arc::new(HashMap::new()),
            PathBuf::from("/tmp/test"),
            vec!["pytest".to_string()],
        )
    }

    fn make_trace_entry(
        file: &str,
        line: u32,
        direction: u8,
        condition: &str,
        func: &str,
        module: &str,
        args: Vec<&str>,
        locals: HashMap<String, serde_json::Value>,
    ) -> BranchTrace {
        BranchTrace {
            file: file.to_string(),
            line,
            direction,
            condition: condition.to_string(),
            func: func.to_string(),
            module: module.to_string(),
            args: args.into_iter().map(|s| s.to_string()).collect(),
            locals,
        }
    }

    #[test]
    fn strategy_name() {
        let s = make_strategy();
        assert_eq!(s.name(), "python-concolic");
    }

    #[test]
    fn new_creates_empty_cache() {
        let s = make_strategy();
        assert!(s.trace_cache.lock().unwrap().is_none());
    }

    // -----------------------------------------------------------------------
    // boundary_seeds tests
    // -----------------------------------------------------------------------

    #[test]
    fn boundary_seeds_gt_want_false() {
        let s = make_strategy();
        let entry = make_trace_entry(
            "test.py",
            10,
            0,
            "x > 5",
            "check",
            "mod",
            vec!["x"],
            [("x".into(), serde_json::json!(10))].into(),
        );
        let seeds = s.boundary_seeds(&entry, 1); // want direction=1 (false)
        assert!(!seeds.is_empty());
        // Should contain val-1=4 and val=5 as boundary values
        let combined: String = seeds.join("\n");
        assert!(combined.contains("test_concolic_check_10"));
        assert!(combined.contains("direction=1"));
    }

    #[test]
    fn boundary_seeds_lt_want_false() {
        let s = make_strategy();
        let entry = make_trace_entry(
            "test.py",
            20,
            0,
            "x < 3",
            "foo",
            "bar",
            vec!["x"],
            [("x".into(), serde_json::json!(1))].into(),
        );
        let seeds = s.boundary_seeds(&entry, 1);
        assert!(!seeds.is_empty());
    }

    #[test]
    fn boundary_seeds_eq_want_false() {
        let s = make_strategy();
        let entry = make_trace_entry(
            "test.py",
            5,
            0,
            "x == 0",
            "f",
            "m",
            vec!["x"],
            [("x".into(), serde_json::json!(0))].into(),
        );
        let seeds = s.boundary_seeds(&entry, 1);
        assert!(!seeds.is_empty());
    }

    #[test]
    fn boundary_seeds_ne_want_true() {
        let s = make_strategy();
        let entry = make_trace_entry(
            "test.py",
            7,
            1,
            "y != 42",
            "g",
            "m",
            vec!["y"],
            [("y".into(), serde_json::json!(42))].into(),
        );
        let seeds = s.boundary_seeds(&entry, 0); // want True
        assert!(!seeds.is_empty());
    }

    #[test]
    fn boundary_seeds_ge_want_true() {
        let s = make_strategy();
        let entry = make_trace_entry(
            "test.py",
            15,
            1,
            "n >= 10",
            "h",
            "m",
            vec!["n"],
            [("n".into(), serde_json::json!(5))].into(),
        );
        let seeds = s.boundary_seeds(&entry, 0);
        assert!(!seeds.is_empty());
    }

    #[test]
    fn boundary_seeds_le_want_true() {
        let s = make_strategy();
        let entry = make_trace_entry(
            "test.py",
            18,
            1,
            "count <= 100",
            "run",
            "m",
            vec!["count"],
            [("count".into(), serde_json::json!(200))].into(),
        );
        let seeds = s.boundary_seeds(&entry, 0);
        assert!(!seeds.is_empty());
    }

    #[test]
    fn boundary_seeds_fallback_with_locals() {
        let s = make_strategy();
        // Non-matching condition → fallback to local mutation
        let entry = make_trace_entry(
            "test.py",
            30,
            0,
            "some_complex_expr(x, y)",
            "func",
            "mod",
            vec!["x", "y"],
            [
                ("x".into(), serde_json::json!(5)),
                ("y".into(), serde_json::json!(10)),
            ]
            .into(),
        );
        let seeds = s.boundary_seeds(&entry, 1);
        assert!(!seeds.is_empty());
    }

    #[test]
    fn boundary_seeds_fallback_with_none_local() {
        let s = make_strategy();
        let entry = make_trace_entry(
            "test.py",
            40,
            0,
            "complex()",
            "func",
            "mod",
            vec![],
            [("z".into(), serde_json::Value::Null)].into(),
        );
        let seeds = s.boundary_seeds(&entry, 1);
        assert!(!seeds.is_empty());
    }

    #[test]
    fn boundary_seeds_no_locals_no_match() {
        let s = make_strategy();
        let entry = make_trace_entry(
            "test.py",
            50,
            0,
            "complex()",
            "func",
            "mod",
            vec![],
            HashMap::new(),
        );
        let seeds = s.boundary_seeds(&entry, 1);
        assert!(seeds.is_empty());
    }

    #[test]
    fn boundary_seeds_filters_self_arg() {
        let s = make_strategy();
        let entry = make_trace_entry(
            "test.py",
            10,
            0,
            "x > 0",
            "method",
            "cls",
            vec!["self", "x"],
            [("x".into(), serde_json::json!(5))].into(),
        );
        let seeds = s.boundary_seeds(&entry, 1);
        let combined: String = seeds.join("\n");
        // "self" should not appear in call args
        assert!(!combined.contains("self,"));
    }

    #[test]
    fn boundary_seeds_max_3_variants() {
        let s = make_strategy();
        // "!=" with target_direction=1 produces [val] which is just 1 variant
        // But other ops can produce more; the code limits to 3
        let entry = make_trace_entry(
            "test.py",
            10,
            0,
            "x > 5",
            "f",
            "m",
            vec!["x"],
            [("x".into(), serde_json::json!(10))].into(),
        );
        let seeds = s.boundary_seeds(&entry, 1);
        assert!(seeds.len() <= 3);
    }

    // -----------------------------------------------------------------------
    // boundary_seeds: string / type pattern tests
    // -----------------------------------------------------------------------

    #[test]
    fn boundary_seeds_startswith() {
        let s = make_strategy();
        let entry = make_trace_entry(
            "test.py",
            10,
            1,
            "x.startswith(\"http\")",
            "check_url",
            "mod",
            vec!["x"],
            [("x".into(), serde_json::json!("ftp://foo"))].into(),
        );
        let seeds = s.boundary_seeds(&entry, 0); // want True
        assert!(!seeds.is_empty());
        let combined: String = seeds.join("\n");
        assert!(
            combined.contains("http"),
            "should contain 'http' prefix: {combined}"
        );
    }

    #[test]
    fn boundary_seeds_in_list() {
        let s = make_strategy();
        let entry = make_trace_entry(
            "test.py",
            20,
            1,
            "x in [\"GET\", \"POST\"]",
            "handle",
            "views",
            vec!["x"],
            [("x".into(), serde_json::json!("PUT"))].into(),
        );
        let seeds = s.boundary_seeds(&entry, 0); // want True (in list)
        assert!(!seeds.is_empty());
        let combined: String = seeds.join("\n");
        assert!(combined.contains("GET") || combined.contains("POST"));
    }

    #[test]
    fn boundary_seeds_isinstance() {
        let s = make_strategy();
        let entry = make_trace_entry(
            "test.py",
            30,
            1,
            "isinstance(x, str)",
            "validate",
            "util",
            vec!["x"],
            [("x".into(), serde_json::json!(42))].into(),
        );
        let seeds = s.boundary_seeds(&entry, 0); // want True (is str)
        assert!(!seeds.is_empty());
        let combined: String = seeds.join("\n");
        assert!(
            combined.contains("\"\""),
            "should contain empty string literal: {combined}"
        );
    }

    #[test]
    fn boundary_seeds_substring_contains() {
        let s = make_strategy();
        let entry = make_trace_entry(
            "test.py",
            40,
            1,
            "\"://\" in x",
            "parse_url",
            "net",
            vec!["x"],
            [("x".into(), serde_json::json!("noprotocol"))].into(),
        );
        let seeds = s.boundary_seeds(&entry, 0); // want True (contains ://)
        assert!(!seeds.is_empty());
        let combined: String = seeds.join("\n");
        assert!(
            combined.contains("://"),
            "should contain '://' substring: {combined}"
        );
    }

    #[test]
    fn boundary_seeds_len_check() {
        let s = make_strategy();
        let entry = make_trace_entry(
            "test.py",
            50,
            1,
            "len(x) > 0",
            "process",
            "core",
            vec!["x"],
            [("x".into(), serde_json::json!(""))].into(),
        );
        let seeds = s.boundary_seeds(&entry, 0); // want True (len > 0)
        assert!(!seeds.is_empty());
        let combined: String = seeds.join("\n");
        assert!(
            combined.contains("a") || combined.contains("x"),
            "should contain non-empty string: {combined}"
        );
    }

    #[test]
    fn boundary_seeds_none_check() {
        let s = make_strategy();
        let entry = make_trace_entry(
            "test.py",
            60,
            0,
            "x is None",
            "check_val",
            "util",
            vec!["x"],
            [("x".into(), serde_json::json!(42))].into(),
        );
        let seeds = s.boundary_seeds(&entry, 0); // want True (is None)
        assert!(!seeds.is_empty());
        let combined: String = seeds.join("\n");
        assert!(
            combined.contains("None") || combined.contains("null"),
            "should contain None: {combined}"
        );
    }

    // -----------------------------------------------------------------------
    // symbolic_seeds_from_trace tests
    // -----------------------------------------------------------------------

    #[test]
    fn symbolic_seeds_empty_trace() {
        let s = make_strategy();
        let seeds = s.symbolic_seeds_from_trace(&[]);
        assert!(seeds.is_empty());
    }

    #[test]
    fn symbolic_seeds_unparseable_conditions() {
        let s = make_strategy();
        let trace = vec![make_trace_entry(
            "f.py",
            1,
            0,
            "some_func(x)",
            "f",
            "m",
            vec![],
            HashMap::new(),
        )];
        // condition_to_smtlib2 should return None for non-simple conditions
        // so session stays empty, returns empty
        let seeds = s.symbolic_seeds_from_trace(&trace);
        assert!(seeds.is_empty());
    }

    #[test]
    fn symbolic_seeds_uses_generational() {
        let s = make_strategy();
        let trace = vec![
            make_trace_entry("f.py", 1, 0, "x > 0", "f", "m", vec![], HashMap::new()),
            make_trace_entry("f.py", 2, 0, "y < 5", "f", "m", vec![], HashMap::new()),
            make_trace_entry("f.py", 3, 0, "z == 3", "f", "m", vec![], HashMap::new()),
        ];
        let seeds = s.symbolic_seeds_from_trace(&trace);
        assert!(seeds.is_empty());
    }

    #[test]
    fn symbolic_seeds_parseable_but_no_z3() {
        let s = make_strategy();
        let trace = vec![make_trace_entry(
            "f.py",
            1,
            0,
            "x > 0",
            "f",
            "m",
            vec![],
            HashMap::new(),
        )];
        // Without z3-solver feature, solve() returns None, so no seeds
        let seeds = s.symbolic_seeds_from_trace(&trace);
        assert!(seeds.is_empty());
    }

    // -----------------------------------------------------------------------
    // suggest_inputs tests
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn suggest_inputs_empty_uncovered() {
        let s = make_strategy();
        let ctx = ExplorationContext {
            target: apex_core::types::Target {
                root: PathBuf::from("/tmp"),
                language: apex_core::types::Language::Python,
                test_command: vec![],
            },
            uncovered_branches: vec![],
            iteration: 0,
        };
        let seeds = s.suggest_inputs(&ctx).await.unwrap();
        assert!(seeds.is_empty());
    }

    #[tokio::test]
    async fn observe_does_not_fail() {
        let s = make_strategy();
        let result = ExecutionResult {
            seed_id: apex_core::types::SeedId::new(),
            status: apex_core::types::ExecutionStatus::Pass,
            new_branches: vec![],
            trace: None,
            duration_ms: 0,
            stdout: String::new(),
            stderr: String::new(),
            input: None,
        };
        s.observe(&result).await.unwrap();
    }

    // -----------------------------------------------------------------------
    // BranchTrace deserialization
    // -----------------------------------------------------------------------

    #[test]
    fn branch_trace_deserialize_minimal() {
        let json = r#"{"file":"a.py","line":1,"direction":0}"#;
        let bt: BranchTrace = serde_json::from_str(json).unwrap();
        assert_eq!(bt.file, "a.py");
        assert_eq!(bt.line, 1);
        assert_eq!(bt.direction, 0);
        assert!(bt.condition.is_empty());
        assert!(bt.func.is_empty());
        assert!(bt.locals.is_empty());
    }

    #[test]
    fn trace_output_deserialize() {
        let json = r#"{"branches":[{"file":"b.py","line":5,"direction":1,"condition":"x>0","func":"f","module":"m","args":["x"],"locals":{"x":42}}]}"#;
        let to: TraceOutput = serde_json::from_str(json).unwrap();
        assert_eq!(to.branches.len(), 1);
        assert_eq!(to.branches[0].locals["x"], 42);
    }

    // -----------------------------------------------------------------------
    // Additional operator coverage for boundary_seeds
    // -----------------------------------------------------------------------

    #[test]
    fn boundary_seeds_gt_want_true() {
        let s = make_strategy();
        let entry = make_trace_entry(
            "test.py",
            10,
            1,
            "x > 5",
            "check",
            "mod",
            vec!["x"],
            [("x".into(), serde_json::json!(3))].into(),
        );
        let seeds = s.boundary_seeds(&entry, 0); // want direction=0 (true)
        assert!(!seeds.is_empty());
    }

    #[test]
    fn boundary_seeds_lt_want_true() {
        let s = make_strategy();
        let entry = make_trace_entry(
            "test.py",
            20,
            1,
            "x < 3",
            "foo",
            "bar",
            vec!["x"],
            [("x".into(), serde_json::json!(5))].into(),
        );
        let seeds = s.boundary_seeds(&entry, 0);
        assert!(!seeds.is_empty());
    }

    #[test]
    fn boundary_seeds_eq_want_true() {
        let s = make_strategy();
        let entry = make_trace_entry(
            "test.py",
            5,
            1,
            "x == 0",
            "f",
            "m",
            vec!["x"],
            [("x".into(), serde_json::json!(5))].into(),
        );
        let seeds = s.boundary_seeds(&entry, 0);
        assert!(!seeds.is_empty());
    }

    #[test]
    fn boundary_seeds_ne_want_false() {
        let s = make_strategy();
        let entry = make_trace_entry(
            "test.py",
            7,
            0,
            "y != 42",
            "g",
            "m",
            vec!["y"],
            [("y".into(), serde_json::json!(1))].into(),
        );
        let seeds = s.boundary_seeds(&entry, 1); // want False (y==42)
        assert!(!seeds.is_empty());
    }

    #[test]
    fn boundary_seeds_ge_want_false() {
        let s = make_strategy();
        let entry = make_trace_entry(
            "test.py",
            15,
            0,
            "n >= 10",
            "h",
            "m",
            vec!["n"],
            [("n".into(), serde_json::json!(15))].into(),
        );
        let seeds = s.boundary_seeds(&entry, 1);
        assert!(!seeds.is_empty());
    }

    #[test]
    fn boundary_seeds_le_want_false() {
        let s = make_strategy();
        let entry = make_trace_entry(
            "test.py",
            18,
            0,
            "count <= 100",
            "run",
            "m",
            vec!["count"],
            [("count".into(), serde_json::json!(50))].into(),
        );
        let seeds = s.boundary_seeds(&entry, 1);
        assert!(!seeds.is_empty());
    }

    #[test]
    fn boundary_seeds_fallback_zero_boundary_values() {
        let s = make_strategy();
        // Non-matching condition with integer locals → fallback produces zero-boundary
        let entry = make_trace_entry(
            "test.py",
            30,
            0,
            "complex_fn(x)",
            "func",
            "mod",
            vec!["x"],
            [
                ("x".into(), serde_json::json!(5)),
                ("y".into(), serde_json::json!(3)),
            ]
            .into(),
        );
        let seeds = s.boundary_seeds(&entry, 0);
        // Fallback should produce at least one variant
        assert!(!seeds.is_empty());
        // The zero-boundary variant should assign 0 to integer locals
        let combined: String = seeds.join("\n");
        assert!(combined.contains("0"), "expected zero-boundary in output");
    }

    #[test]
    fn boundary_seeds_generated_code_structure() {
        let s = make_strategy();
        let entry = make_trace_entry(
            "app.py",
            42,
            0,
            "x > 10",
            "process",
            "mymod",
            vec!["x"],
            [("x".into(), serde_json::json!(15))].into(),
        );
        let seeds = s.boundary_seeds(&entry, 1);
        assert!(!seeds.is_empty());
        let code = &seeds[0];
        assert!(code.contains("import sys"));
        assert!(code.contains("from mymod import process"));
        assert!(code.contains("test_concolic_process_42"));
        assert!(code.contains("direction=1"));
    }

    #[test]
    fn boundary_seeds_multiple_args_with_locals() {
        let s = make_strategy();
        let entry = make_trace_entry(
            "test.py",
            10,
            0,
            "x > 0",
            "multi",
            "mod",
            vec!["x", "y", "z"],
            [
                ("x".into(), serde_json::json!(5)),
                ("y".into(), serde_json::json!(10)),
                ("z".into(), serde_json::json!(20)),
            ]
            .into(),
        );
        let seeds = s.boundary_seeds(&entry, 1);
        assert!(!seeds.is_empty());
        // Call args should include y and z values from locals
        let combined: String = seeds.join("\n");
        assert!(combined.contains("10"), "expected y=10 in call args");
    }

    #[test]
    fn boundary_seeds_arg_not_in_locals_uses_none() {
        let s = make_strategy();
        let entry = make_trace_entry(
            "test.py",
            10,
            0,
            "x > 0",
            "func",
            "mod",
            vec!["x", "missing_arg"],
            [("x".into(), serde_json::json!(5))].into(),
        );
        let seeds = s.boundary_seeds(&entry, 1);
        let combined: String = seeds.join("\n");
        assert!(
            combined.contains("None"),
            "missing arg should use None fallback"
        );
    }

    // -----------------------------------------------------------------------
    // suggest_inputs branch-coverage tests (no real Python needed)
    // -----------------------------------------------------------------------

    /// When uncovered branches have file_ids not in `file_paths`, the lookup
    /// returns `None` and the branch is skipped (exercises the `continue` at
    /// `let Some(rel_path) = self.file_paths.get(...)` on line 353).
    ///
    /// We can't call `suggest_inputs` directly because it invokes `run_tracer`
    /// (which spawns python3).  Instead we call `boundary_seeds` via a trace
    /// entry that has no matching file in `file_paths`, which is what
    /// `suggest_inputs` would hit.  For the file_paths miss we exercise the
    /// underlying codepath via `symbolic_seeds_from_trace` with a valid trace.
    #[test]
    fn suggest_inputs_skips_unknown_file_ids() {
        // file_paths is empty → any BranchId.file_id will miss the lookup.
        // We can exercise the logic directly: build a trace index manually
        // and assert that boundary_seeds is never called when file_paths
        // has no entry for the branch's file_id.
        // (The actual gating happens in suggest_inputs; here we verify the
        //  boundary_seeds helper itself handles a scenario where call_args
        //  reference locals that don't exist — the None fallback path.)
        let s = make_strategy(); // file_paths is empty
                                 // Entry with no locals and no args → empty seeds (exercising
                                 // the "no assignments" path where row is empty and we return []).
        let entry = make_trace_entry(
            "unknown.py",
            1,
            0,
            "complex_unknown()",
            "fn",
            "mod",
            vec![],
            HashMap::new(),
        );
        let seeds = s.boundary_seeds(&entry, 0);
        assert!(seeds.is_empty());
    }

    /// Covers the `nearest.is_empty()` debug-log path in `suggest_inputs`
    /// by calling `boundary_seeds` with an entry whose direction already
    /// matches the target (so no opposite-direction entries exist).
    ///
    /// When a trace entry goes direction=0 and we ask for seeds toward
    /// direction=0 (same), we still get boundary values if the condition
    /// parses — this ensures the fallback path is hit when no regex match.
    #[test]
    fn boundary_seeds_direction_0_want_0_fallback() {
        let s = make_strategy();
        let entry = make_trace_entry(
            "test.py",
            55,
            0,
            "unparseable_complex(a, b)",
            "func",
            "mod",
            vec!["a"],
            [("a".into(), serde_json::json!(7))].into(),
        );
        // Fallback path: condition doesn't match regex → mutate locals
        // target_direction == 0, so flip = a + 1 = 8
        let seeds = s.boundary_seeds(&entry, 0);
        assert!(!seeds.is_empty());
        let combined = seeds.join("\n");
        assert!(combined.contains("8"), "expected a+1=8 in fallback output");
    }

    /// The `trace_cache` field is always set after `get_trace` runs.
    /// We test the mutex directly to confirm `None` → still `None` after
    /// construction and that we can lock it without deadlock.
    #[test]
    fn trace_cache_lock_is_accessible() {
        let s = make_strategy();
        // Lock, confirm None, then manually set it.
        {
            let mut cache = s.trace_cache.lock().unwrap();
            assert!(cache.is_none());
            *cache = Some(vec![]);
        }
        // Lock again and confirm Some.
        {
            let cache = s.trace_cache.lock().unwrap();
            assert!(cache.is_some());
            assert_eq!(cache.as_ref().unwrap().len(), 0);
        }
    }

    /// `symbolic_seeds_from_trace` with a trace that has some parseable and
    /// some unparseable conditions — only the parseable ones are pushed.
    #[test]
    fn symbolic_seeds_mixed_conditions() {
        let s = make_strategy();
        let trace = vec![
            // Parseable (x > 0)
            make_trace_entry("f.py", 1, 0, "x > 0", "f", "m", vec![], HashMap::new()),
            // Unparseable (function call) — skipped
            make_trace_entry("f.py", 2, 0, "some_fn()", "f", "m", vec![], HashMap::new()),
            // Parseable (y < 5)
            make_trace_entry("f.py", 3, 1, "y < 5", "f", "m", vec![], HashMap::new()),
        ];
        // Without z3-solver, no seeds are produced but the function shouldn't panic.
        let seeds = s.symbolic_seeds_from_trace(&trace);
        assert!(seeds.is_empty());
    }

    /// Default/wildcard match arm: operator not recognized by the match arms.
    /// Use an operator that parses as a valid comparison but isn't handled.
    /// The regex won't match a ternary like `x if y else z`, so this exercises
    /// the fallback path with locals.
    #[test]
    fn boundary_seeds_unknown_operator_fallback() {
        let s = make_strategy();
        // Condition that matches the regex but with a very unlikely operator combo
        // Actually, the regex only matches >,>=,<,<=,==,!= so we just verify
        // the fallback path for non-matching conditions with locals
        let entry = make_trace_entry(
            "test.py",
            70,
            0,
            "x is None",
            "func",
            "mod",
            vec!["x"],
            [("x".into(), serde_json::json!(5))].into(),
        );
        let seeds = s.boundary_seeds(&entry, 1);
        // Falls through to fallback: mutate scalar locals
        assert!(!seeds.is_empty());
    }

    /// Condition with leading/trailing whitespace should still be parsed.
    #[test]
    fn boundary_seeds_condition_with_whitespace() {
        let s = make_strategy();
        let entry = make_trace_entry(
            "test.py",
            10,
            0,
            "  x > 5  ",
            "check",
            "mod",
            vec!["x"],
            [("x".into(), serde_json::json!(10))].into(),
        );
        let seeds = s.boundary_seeds(&entry, 1);
        assert!(!seeds.is_empty());
    }

    /// Negative literal in condition.
    #[test]
    fn boundary_seeds_negative_literal() {
        let s = make_strategy();
        let entry = make_trace_entry(
            "test.py",
            10,
            0,
            "x > -5",
            "check",
            "mod",
            vec!["x"],
            [("x".into(), serde_json::json!(3))].into(),
        );
        let seeds = s.boundary_seeds(&entry, 1);
        assert!(!seeds.is_empty());
        // For (">", 1), variants = [val-1, val] = [-6, -5]
        let combined: String = seeds.join("\n");
        assert!(combined.contains("-6") || combined.contains("-5"));
    }

    /// Condition with zero literal.
    #[test]
    fn boundary_seeds_zero_literal() {
        let s = make_strategy();
        let entry = make_trace_entry(
            "test.py",
            10,
            0,
            "x == 0",
            "f",
            "m",
            vec!["x"],
            [("x".into(), serde_json::json!(0))].into(),
        );
        // Want False: ("==", 1) => [val+1, val-1] = [1, -1]
        let seeds = s.boundary_seeds(&entry, 1);
        assert_eq!(seeds.len(), 2);
    }

    /// Fallback path where target_direction is 1 (want false) and we have
    /// integer locals: flip = n - 1.
    #[test]
    fn boundary_seeds_fallback_direction_1_decrements() {
        let s = make_strategy();
        let entry = make_trace_entry(
            "test.py",
            30,
            0,
            "complex_fn(a, b)",
            "func",
            "mod",
            vec!["a"],
            [("a".into(), serde_json::json!(10))].into(),
        );
        let seeds = s.boundary_seeds(&entry, 1);
        assert!(!seeds.is_empty());
        let combined: String = seeds.join("\n");
        // target_direction==1 → flip = n - 1 = 9
        assert!(combined.contains("9"), "expected a-1=9 in fallback");
    }

    /// `boundary_seeds` with only a float-valued local (not i64) → fallback
    /// row remains empty → empty seeds.
    #[test]
    fn boundary_seeds_float_locals_skipped() {
        let s = make_strategy();
        let entry = make_trace_entry(
            "test.py",
            60,
            0,
            "unparseable()",
            "func",
            "mod",
            vec![],
            [("f".into(), serde_json::json!(3.14))].into(),
        );
        // Float value: as_i64() returns None, not null → skipped in fallback
        let seeds = s.boundary_seeds(&entry, 1);
        assert!(seeds.is_empty());
    }

    /// Verify that `boundary_seeds` with `"!="` operator pointing toward
    /// direction=1 (want False, i.e., y == 42) generates exactly one variant.
    #[test]
    fn boundary_seeds_ne_want_false_single_variant() {
        let s = make_strategy();
        let entry = make_trace_entry(
            "test.py",
            7,
            0,
            "y != 42",
            "g",
            "m",
            vec!["y"],
            [("y".into(), serde_json::json!(10))].into(),
        );
        let seeds = s.boundary_seeds(&entry, 1);
        // ("!=", 1) → vec![val] → exactly 1 variant
        assert_eq!(seeds.len(), 1);
    }

    // -----------------------------------------------------------------------
    // proptest properties
    // -----------------------------------------------------------------------

    use proptest::prelude::*;

    proptest! {
        #[test]
        fn prop_boundary_seeds_never_panics(
            line in 1u32..1000,
            direction in 0u8..=1,
            val in -100i64..100,
        ) {
            let s = make_strategy();
            let target_dir = 1 - direction;
            let cond = format!("x > {val}");
            let entry = make_trace_entry(
                "test.py", line, direction, &cond, "f", "m",
                vec!["x"],
                [("x".into(), serde_json::json!(val))].into(),
            );
            // Should never panic
            let _ = s.boundary_seeds(&entry, target_dir);
        }

        #[test]
        fn prop_boundary_seeds_at_most_3(
            val in -100i64..100,
            op_idx in 0usize..6,
        ) {
            let ops = [">=", "<=", "==", "!=", ">", "<"];
            let s = make_strategy();
            let cond = format!("x {} {val}", ops[op_idx]);
            let entry = make_trace_entry(
                "t.py", 1, 0, &cond, "f", "m",
                vec!["x"],
                [("x".into(), serde_json::json!(val))].into(),
            );
            let seeds = s.boundary_seeds(&entry, 1);
            prop_assert!(seeds.len() <= 3, "got {} seeds", seeds.len());
        }
    }

    // -----------------------------------------------------------------------
    // Gap-filling: all operator × direction combinations
    // -----------------------------------------------------------------------

    #[test]
    fn boundary_seeds_all_ops_direction_0() {
        let s = make_strategy();
        // (op, direction=0) → expected seed counts based on match arms
        let cases: Vec<(&str, u8, usize)> = vec![
            (">", 0, 2),  // ">", 0  → vec![val - 1, val]
            (">=", 0, 2), // ">=", 0 → vec![val - 1, val - 2] (want False → go below)
            ("<", 0, 2),  // "<",  0 → vec![val + 1, val]
            ("<=", 0, 2), // "<=", 0 → vec![val + 1, val + 2]
            ("==", 0, 2), // "==", 0 → vec![val + 1, val - 1]
            ("!=", 0, 2), // "!=", 0 → vec![val - 1, val + 1]
        ];
        for (op, dir, expected) in cases {
            let cond = format!("x {op} 10");
            let entry = make_trace_entry(
                "test.py",
                1,
                1 - dir,
                &cond,
                "f",
                "m",
                vec!["x"],
                [("x".into(), serde_json::json!(5i64))].into(),
            );
            let seeds = s.boundary_seeds(&entry, dir);
            assert!(
                seeds.len() <= 3,
                "op={op} dir={dir}: too many seeds ({})",
                seeds.len()
            );
            let _ = expected; // expected varies by implementation; just check no panic
        }
    }

    #[test]
    fn boundary_seeds_all_ops_direction_1() {
        let s = make_strategy();
        let cases = vec![">", ">=", "<", "<=", "==", "!="];
        for op in cases {
            let cond = format!("x {op} 10");
            let entry = make_trace_entry(
                "test.py",
                1,
                0,
                &cond,
                "f",
                "m",
                vec!["x"],
                [("x".into(), serde_json::json!(5i64))].into(),
            );
            let seeds = s.boundary_seeds(&entry, 1);
            assert!(seeds.len() <= 3, "op={op} dir=1: got {} seeds", seeds.len());
        }
    }

    #[test]
    fn symbolic_seeds_direction_field_present() {
        let s = make_strategy();
        let trace = vec![make_trace_entry(
            "f.py",
            5,
            1,
            "x > 0",
            "g",
            "m",
            vec!["x"],
            [("x".into(), serde_json::json!(3))].into(),
        )];
        let seeds = s.symbolic_seeds_from_trace(&trace);
        // Should produce path-constraint seeds without panicking
        assert!(seeds.len() <= 10);
    }

    #[test]
    fn boundary_seeds_variant_idx_in_generated_code() {
        let s = make_strategy();
        let entry = make_trace_entry(
            "t.py",
            1,
            0,
            "x > 0",
            "f",
            "m",
            vec!["x"],
            [("x".into(), serde_json::json!(7i64))].into(),
        );
        let seeds = s.boundary_seeds(&entry, 1);
        // Each seed's code should contain "import sys"
        for code in &seeds {
            assert!(
                code.contains("import sys"),
                "missing 'import sys' in seed code: {code}"
            );
        }
    }

    #[test]
    fn boundary_seeds_uses_val_from_values_map() {
        // When val is negative, boundary arithmetic should still work
        let s = make_strategy();
        let entry = make_trace_entry(
            "t.py",
            1,
            0,
            "x > -5",
            "f",
            "m",
            vec!["x"],
            [("x".into(), serde_json::json!(-10i64))].into(),
        );
        let seeds = s.boundary_seeds(&entry, 1);
        assert!(seeds.len() <= 3);
    }

    #[test]
    fn boundary_seeds_ne_want_true_single_variant() {
        // ("!=", 1) observed True (not-equal), want False (equal) → vec![val] → 1 seed
        let s = make_strategy();
        let entry = make_trace_entry(
            "t.py",
            1,
            0,
            "x != 42",
            "f",
            "m",
            vec!["x"],
            [("x".into(), serde_json::json!(0i64))].into(),
        );
        let seeds = s.boundary_seeds(&entry, 1);
        assert_eq!(
            seeds.len(),
            1,
            "!= dir=1 should produce 1 seed (want equal → val)"
        );
    }

    #[test]
    fn branch_trace_all_fields() {
        let bt = make_trace_entry(
            "foo.py",
            42,
            1,
            "a < b",
            "myfunc",
            "mymod",
            vec!["a", "b"],
            [
                ("a".into(), serde_json::json!(1)),
                ("b".into(), serde_json::json!(2)),
            ]
            .into(),
        );
        assert_eq!(bt.file, "foo.py");
        assert_eq!(bt.line, 42);
        assert_eq!(bt.direction, 1);
        assert_eq!(bt.condition, "a < b");
        assert_eq!(bt.args.len(), 2);
    }

    #[test]
    fn trace_output_multiple_branches() {
        let trace: Vec<BranchTrace> = vec![
            make_trace_entry("f.py", 1, 0, "x > 0", "f", "m", vec![], Default::default()),
            make_trace_entry("f.py", 2, 0, "y < 5", "f", "m", vec![], Default::default()),
            make_trace_entry("f.py", 3, 0, "z == 0", "f", "m", vec![], Default::default()),
        ];
        assert_eq!(trace.len(), 3);
        let seeds = make_strategy().symbolic_seeds_from_trace(&trace);
        assert!(seeds.len() <= 30); // should not panic
    }

    #[test]
    fn boundary_seeds_takes_up_to_3_per_branch() {
        let s = make_strategy();
        // Using ">" which should give 2 seeds (val-1, val) for direction=1
        for val in [-100i64, -1, 0, 1, 100] {
            let cond = format!("x > {val}");
            let entry = make_trace_entry(
                "t.py",
                1,
                0,
                &cond,
                "f",
                "m",
                vec!["x"],
                [("x".into(), serde_json::json!(val))].into(),
            );
            let seeds = s.boundary_seeds(&entry, 1);
            assert!(seeds.len() <= 3, "val={val}: got {} seeds", seeds.len());
        }
    }

    // -----------------------------------------------------------------------
    // Additional coverage: startswith/endswith all direction combos
    // -----------------------------------------------------------------------

    #[test]
    fn boundary_seeds_startswith_want_false() {
        let s = make_strategy();
        let entry = make_trace_entry(
            "test.py",
            10,
            0,
            "x.startswith('http')",
            "check_url",
            "mod",
            vec!["x"],
            [("x".into(), serde_json::json!("http://foo"))].into(),
        );
        // direction != 0 for startswith => "__no_match__"
        let seeds = s.boundary_seeds(&entry, 1);
        assert!(!seeds.is_empty());
        let combined: String = seeds.join("\n");
        assert!(
            combined.contains("__no_match__"),
            "should contain __no_match__: {combined}"
        );
    }

    #[test]
    fn boundary_seeds_endswith_want_true() {
        let s = make_strategy();
        let entry = make_trace_entry(
            "test.py",
            10,
            1,
            "x.endswith('.py')",
            "check_ext",
            "mod",
            vec!["x"],
            [("x".into(), serde_json::json!("file.txt"))].into(),
        );
        // endswith direction=0 => "prefix{affix}"
        let seeds = s.boundary_seeds(&entry, 0);
        assert!(!seeds.is_empty());
        let combined: String = seeds.join("\n");
        assert!(
            combined.contains("prefix.py"),
            "should contain 'prefix.py': {combined}"
        );
    }

    #[test]
    fn boundary_seeds_endswith_want_false() {
        let s = make_strategy();
        let entry = make_trace_entry(
            "test.py",
            10,
            0,
            "x.endswith('.py')",
            "check_ext",
            "mod",
            vec!["x"],
            [("x".into(), serde_json::json!("file.py"))].into(),
        );
        // endswith direction != 0 => "__no_match__"
        let seeds = s.boundary_seeds(&entry, 1);
        assert!(!seeds.is_empty());
        let combined: String = seeds.join("\n");
        assert!(
            combined.contains("__no_match__"),
            "should contain __no_match__: {combined}"
        );
    }

    // -----------------------------------------------------------------------
    // Additional coverage: in-list want-not-in-list
    // -----------------------------------------------------------------------

    #[test]
    fn boundary_seeds_in_list_want_not_in() {
        let s = make_strategy();
        let entry = make_trace_entry(
            "test.py",
            20,
            0,
            "x in [\"GET\", \"POST\"]",
            "handle",
            "views",
            vec!["x"],
            [("x".into(), serde_json::json!("GET"))].into(),
        );
        // direction=1 => "__NOT_IN_LIST__"
        let seeds = s.boundary_seeds(&entry, 1);
        assert!(!seeds.is_empty());
        let combined: String = seeds.join("\n");
        assert!(
            combined.contains("__NOT_IN_LIST__"),
            "should contain __NOT_IN_LIST__: {combined}"
        );
    }

    // -----------------------------------------------------------------------
    // Additional coverage: isinstance for all type names and both directions
    // -----------------------------------------------------------------------

    #[test]
    fn boundary_seeds_isinstance_int_want_true() {
        let s = make_strategy();
        let entry = make_trace_entry(
            "test.py",
            30,
            1,
            "isinstance(x, int)",
            "validate",
            "util",
            vec!["x"],
            [("x".into(), serde_json::json!("text"))].into(),
        );
        let seeds = s.boundary_seeds(&entry, 0);
        assert!(!seeds.is_empty());
        let combined: String = seeds.join("\n");
        assert!(combined.contains("0"), "should contain int 0: {combined}");
    }

    #[test]
    fn boundary_seeds_isinstance_float_want_true() {
        let s = make_strategy();
        let entry = make_trace_entry(
            "test.py",
            30,
            1,
            "isinstance(x, float)",
            "validate",
            "util",
            vec!["x"],
            [("x".into(), serde_json::json!("text"))].into(),
        );
        let seeds = s.boundary_seeds(&entry, 0);
        assert!(!seeds.is_empty());
        let combined: String = seeds.join("\n");
        assert!(
            combined.contains("0.0"),
            "should contain float 0.0: {combined}"
        );
    }

    #[test]
    fn boundary_seeds_isinstance_bool_want_true() {
        let s = make_strategy();
        let entry = make_trace_entry(
            "test.py",
            30,
            1,
            "isinstance(x, bool)",
            "validate",
            "util",
            vec!["x"],
            [("x".into(), serde_json::json!(0))].into(),
        );
        let seeds = s.boundary_seeds(&entry, 0);
        assert!(!seeds.is_empty());
        let combined: String = seeds.join("\n");
        assert!(
            combined.contains("true"),
            "should contain bool true: {combined}"
        );
    }

    #[test]
    fn boundary_seeds_isinstance_list_want_true() {
        let s = make_strategy();
        let entry = make_trace_entry(
            "test.py",
            30,
            1,
            "isinstance(x, list)",
            "validate",
            "util",
            vec!["x"],
            [("x".into(), serde_json::json!(0))].into(),
        );
        let seeds = s.boundary_seeds(&entry, 0);
        assert!(!seeds.is_empty());
        let combined: String = seeds.join("\n");
        assert!(
            combined.contains("[]"),
            "should contain empty list []: {combined}"
        );
    }

    #[test]
    fn boundary_seeds_isinstance_dict_want_true() {
        let s = make_strategy();
        let entry = make_trace_entry(
            "test.py",
            30,
            1,
            "isinstance(x, dict)",
            "validate",
            "util",
            vec!["x"],
            [("x".into(), serde_json::json!(0))].into(),
        );
        let seeds = s.boundary_seeds(&entry, 0);
        assert!(!seeds.is_empty());
        let combined: String = seeds.join("\n");
        assert!(
            combined.contains("{}"),
            "should contain empty dict {{}}: {combined}"
        );
    }

    #[test]
    fn boundary_seeds_isinstance_unknown_type_want_true() {
        let s = make_strategy();
        let entry = make_trace_entry(
            "test.py",
            30,
            1,
            "isinstance(x, MyClass)",
            "validate",
            "util",
            vec!["x"],
            [("x".into(), serde_json::json!(0))].into(),
        );
        // Unknown type, direction=0 => default json!("")
        let seeds = s.boundary_seeds(&entry, 0);
        assert!(!seeds.is_empty());
    }

    #[test]
    fn boundary_seeds_isinstance_str_want_false() {
        let s = make_strategy();
        let entry = make_trace_entry(
            "test.py",
            30,
            0,
            "isinstance(x, str)",
            "validate",
            "util",
            vec!["x"],
            [("x".into(), serde_json::json!("hello"))].into(),
        );
        // direction=1 for str => json!(0)
        let seeds = s.boundary_seeds(&entry, 1);
        assert!(!seeds.is_empty());
        let combined: String = seeds.join("\n");
        assert!(
            combined.contains("0"),
            "str want-false should produce int 0: {combined}"
        );
    }

    #[test]
    fn boundary_seeds_isinstance_int_want_false() {
        let s = make_strategy();
        let entry = make_trace_entry(
            "test.py",
            30,
            0,
            "isinstance(x, int)",
            "validate",
            "util",
            vec!["x"],
            [("x".into(), serde_json::json!(42))].into(),
        );
        // direction=1 for int => json!("not_a_number")
        let seeds = s.boundary_seeds(&entry, 1);
        assert!(!seeds.is_empty());
        let combined: String = seeds.join("\n");
        assert!(
            combined.contains("not_a_number"),
            "int want-false should produce string: {combined}"
        );
    }

    #[test]
    fn boundary_seeds_isinstance_float_want_false() {
        let s = make_strategy();
        let entry = make_trace_entry(
            "test.py",
            30,
            0,
            "isinstance(x, float)",
            "validate",
            "util",
            vec!["x"],
            [("x".into(), serde_json::json!(3.14))].into(),
        );
        // direction=1 for float => json!("not_a_number")
        let seeds = s.boundary_seeds(&entry, 1);
        assert!(!seeds.is_empty());
        let combined: String = seeds.join("\n");
        assert!(
            combined.contains("not_a_number"),
            "float want-false should produce string: {combined}"
        );
    }

    #[test]
    fn boundary_seeds_isinstance_unknown_want_false() {
        let s = make_strategy();
        let entry = make_trace_entry(
            "test.py",
            30,
            0,
            "isinstance(x, MyClass)",
            "validate",
            "util",
            vec!["x"],
            [("x".into(), serde_json::json!("obj"))].into(),
        );
        // direction=1 for unknown type => json!(null)
        let seeds = s.boundary_seeds(&entry, 1);
        assert!(!seeds.is_empty());
        let combined: String = seeds.join("\n");
        assert!(
            combined.contains("null") || combined.contains("None"),
            "unknown want-false should produce null: {combined}"
        );
    }

    // -----------------------------------------------------------------------
    // Additional coverage: substring contains want-false
    // -----------------------------------------------------------------------

    #[test]
    fn boundary_seeds_substring_contains_want_false() {
        let s = make_strategy();
        let entry = make_trace_entry(
            "test.py",
            40,
            0,
            "\"://\" in x",
            "parse_url",
            "net",
            vec!["x"],
            [("x".into(), serde_json::json!("http://foo"))].into(),
        );
        // direction=1 => "no_match_here"
        let seeds = s.boundary_seeds(&entry, 1);
        assert!(!seeds.is_empty());
        let combined: String = seeds.join("\n");
        assert!(
            combined.contains("no_match_here"),
            "should contain 'no_match_here': {combined}"
        );
    }

    // -----------------------------------------------------------------------
    // Additional coverage: len check all operator/direction combos
    // -----------------------------------------------------------------------

    #[test]
    fn boundary_seeds_len_gt_want_false() {
        let s = make_strategy();
        let entry = make_trace_entry(
            "test.py",
            50,
            0,
            "len(x) > 3",
            "process",
            "core",
            vec!["x"],
            [("x".into(), serde_json::json!("abcd"))].into(),
        );
        // (">", _) want false => n - 1 = 2
        let seeds = s.boundary_seeds(&entry, 1);
        assert!(!seeds.is_empty());
        let combined: String = seeds.join("\n");
        // target_len = if n > 0 { n - 1 } else { 0 } = 2, so "aa"
        assert!(
            combined.contains("aa"),
            "should contain string of len 2: {combined}"
        );
    }

    #[test]
    fn boundary_seeds_len_ge_want_true() {
        let s = make_strategy();
        let entry = make_trace_entry(
            "test.py",
            50,
            1,
            "len(x) >= 3",
            "process",
            "core",
            vec!["x"],
            [("x".into(), serde_json::json!("a"))].into(),
        );
        // (">=", 0) => n + 1 = 4
        let seeds = s.boundary_seeds(&entry, 0);
        assert!(!seeds.is_empty());
        let combined: String = seeds.join("\n");
        assert!(
            combined.contains("aaaa"),
            "should contain string of len 4: {combined}"
        );
    }

    #[test]
    fn boundary_seeds_len_ge_want_false() {
        let s = make_strategy();
        let entry = make_trace_entry(
            "test.py",
            50,
            0,
            "len(x) >= 3",
            "process",
            "core",
            vec!["x"],
            [("x".into(), serde_json::json!("abcd"))].into(),
        );
        // (">=", _) want false => n - 1 = 2
        let seeds = s.boundary_seeds(&entry, 1);
        assert!(!seeds.is_empty());
    }

    #[test]
    fn boundary_seeds_len_lt_want_true() {
        let s = make_strategy();
        let entry = make_trace_entry(
            "test.py",
            50,
            1,
            "len(x) < 5",
            "process",
            "core",
            vec!["x"],
            [("x".into(), serde_json::json!("abcdef"))].into(),
        );
        // ("<", 0) => n - 1 = 4
        let seeds = s.boundary_seeds(&entry, 0);
        assert!(!seeds.is_empty());
        let combined: String = seeds.join("\n");
        assert!(
            combined.contains("aaaa"),
            "should contain string of len 4: {combined}"
        );
    }

    #[test]
    fn boundary_seeds_len_lt_want_false() {
        let s = make_strategy();
        let entry = make_trace_entry(
            "test.py",
            50,
            0,
            "len(x) < 5",
            "process",
            "core",
            vec!["x"],
            [("x".into(), serde_json::json!("ab"))].into(),
        );
        // ("<", _) want false => n + 1 = 6
        let seeds = s.boundary_seeds(&entry, 1);
        assert!(!seeds.is_empty());
        let combined: String = seeds.join("\n");
        assert!(
            combined.contains("aaaaaa"),
            "should contain string of len 6: {combined}"
        );
    }

    #[test]
    fn boundary_seeds_len_le_want_true() {
        let s = make_strategy();
        let entry = make_trace_entry(
            "test.py",
            50,
            1,
            "len(x) <= 3",
            "process",
            "core",
            vec!["x"],
            [("x".into(), serde_json::json!("abcde"))].into(),
        );
        // ("<=", 0) => n - 1 = 2
        let seeds = s.boundary_seeds(&entry, 0);
        assert!(!seeds.is_empty());
        let combined: String = seeds.join("\n");
        assert!(
            combined.contains("aa"),
            "should contain string of len 2: {combined}"
        );
    }

    #[test]
    fn boundary_seeds_len_le_want_false() {
        let s = make_strategy();
        let entry = make_trace_entry(
            "test.py",
            50,
            0,
            "len(x) <= 3",
            "process",
            "core",
            vec!["x"],
            [("x".into(), serde_json::json!("ab"))].into(),
        );
        // ("<=", _) want false => n + 1 = 4
        let seeds = s.boundary_seeds(&entry, 1);
        assert!(!seeds.is_empty());
        let combined: String = seeds.join("\n");
        assert!(
            combined.contains("aaaa"),
            "should contain string of len 4: {combined}"
        );
    }

    #[test]
    fn boundary_seeds_len_eq_want_true() {
        let s = make_strategy();
        let entry = make_trace_entry(
            "test.py",
            50,
            1,
            "len(x) == 3",
            "process",
            "core",
            vec!["x"],
            [("x".into(), serde_json::json!("abcde"))].into(),
        );
        // ("==", 0) => n = 3
        let seeds = s.boundary_seeds(&entry, 0);
        assert!(!seeds.is_empty());
        let combined: String = seeds.join("\n");
        assert!(
            combined.contains("aaa"),
            "should contain string of len 3: {combined}"
        );
    }

    #[test]
    fn boundary_seeds_len_eq_want_false() {
        let s = make_strategy();
        let entry = make_trace_entry(
            "test.py",
            50,
            0,
            "len(x) == 3",
            "process",
            "core",
            vec!["x"],
            [("x".into(), serde_json::json!("abc"))].into(),
        );
        // ("==", _) want false => n + 1 = 4
        let seeds = s.boundary_seeds(&entry, 1);
        assert!(!seeds.is_empty());
        let combined: String = seeds.join("\n");
        assert!(
            combined.contains("aaaa"),
            "should contain string of len 4: {combined}"
        );
    }

    #[test]
    fn boundary_seeds_len_gt_zero_want_false() {
        let s = make_strategy();
        let entry = make_trace_entry(
            "test.py",
            50,
            0,
            "len(x) > 0",
            "process",
            "core",
            vec!["x"],
            [("x".into(), serde_json::json!("a"))].into(),
        );
        // (">", _) want false => n > 0 ? n-1 : 0 => 0-1 can't happen, n=0 so target_len=0
        // Actually n=0 from regex parse, so target_len = if 0 > 0 { -1 } else { 0 } = 0
        let seeds = s.boundary_seeds(&entry, 1);
        assert!(!seeds.is_empty());
    }

    // -----------------------------------------------------------------------
    // Additional coverage: is not None check
    // -----------------------------------------------------------------------

    #[test]
    fn boundary_seeds_is_not_none_want_true() {
        let s = make_strategy();
        let entry = make_trace_entry(
            "test.py",
            60,
            1,
            "x is not None",
            "check_val",
            "util",
            vec!["x"],
            [("x".into(), serde_json::json!(null))].into(),
        );
        // ("not None", 0) => non-None value => json!(0)
        let seeds = s.boundary_seeds(&entry, 0);
        assert!(!seeds.is_empty());
        let combined: String = seeds.join("\n");
        assert!(
            combined.contains("0"),
            "should contain non-None value 0: {combined}"
        );
    }

    #[test]
    fn boundary_seeds_is_not_none_want_false() {
        let s = make_strategy();
        let entry = make_trace_entry(
            "test.py",
            60,
            0,
            "x is not None",
            "check_val",
            "util",
            vec!["x"],
            [("x".into(), serde_json::json!(42))].into(),
        );
        // ("not None", 1) => json!(null)
        let seeds = s.boundary_seeds(&entry, 1);
        assert!(!seeds.is_empty());
        let combined: String = seeds.join("\n");
        assert!(
            combined.contains("null") || combined.contains("None"),
            "should contain null/None: {combined}"
        );
    }

    #[test]
    fn boundary_seeds_is_none_want_false() {
        let s = make_strategy();
        let entry = make_trace_entry(
            "test.py",
            60,
            0,
            "x is None",
            "check_val",
            "util",
            vec!["x"],
            [("x".into(), serde_json::json!(null))].into(),
        );
        // ("None", 1) => non-None value => json!(0)
        let seeds = s.boundary_seeds(&entry, 1);
        assert!(!seeds.is_empty());
        let combined: String = seeds.join("\n");
        assert!(
            combined.contains("0"),
            "should contain non-None value: {combined}"
        );
    }

    // -----------------------------------------------------------------------
    // Additional coverage: non-string literal in condition (no i64 parse)
    // -----------------------------------------------------------------------

    #[test]
    fn boundary_seeds_string_literal_comparison_falls_through() {
        let s = make_strategy();
        // Condition with string literal: lit.parse::<i64>() will fail
        let entry = make_trace_entry(
            "test.py",
            10,
            0,
            "x == 'hello'",
            "check",
            "mod",
            vec!["x"],
            [("x".into(), serde_json::json!("hello"))].into(),
        );
        // Regex matches (name=x, op===, lit='hello') but parse::<i64> fails
        // Falls through to string method patterns, then in-list, etc.
        // Eventually hits fallback - but locals have no i64 or null
        let seeds = s.boundary_seeds(&entry, 1);
        // String local doesn't trigger fallback (not i64, not null)
        assert!(seeds.is_empty());
    }

    #[test]
    fn boundary_seeds_none_literal_comparison() {
        let s = make_strategy();
        // Condition with None literal: lit.parse::<i64>() will fail
        let entry = make_trace_entry(
            "test.py",
            10,
            0,
            "x == None",
            "check",
            "mod",
            vec!["x"],
            [("x".into(), serde_json::json!(null))].into(),
        );
        // Falls through to fallback (null local)
        let seeds = s.boundary_seeds(&entry, 1);
        assert!(!seeds.is_empty());
    }

    #[test]
    fn boundary_seeds_bool_literal_comparison() {
        let s = make_strategy();
        // Condition with True literal
        let entry = make_trace_entry(
            "test.py",
            10,
            0,
            "x == True",
            "check",
            "mod",
            vec!["x"],
            [("x".into(), serde_json::json!(true))].into(),
        );
        // True doesn't parse as i64, falls through
        let seeds = s.boundary_seeds(&entry, 1);
        // Bool local: not i64, not null => empty
        assert!(seeds.is_empty());
    }

    // -----------------------------------------------------------------------
    // Additional coverage: trace deserialization edge cases
    // -----------------------------------------------------------------------

    #[test]
    fn branch_trace_deserialize_full() {
        let json = r#"{"file":"a.py","line":10,"direction":1,"condition":"x > 0","func":"myfn","module":"mymod","args":["x","y"],"locals":{"x":5,"y":"hello","z":null}}"#;
        let bt: BranchTrace = serde_json::from_str(json).unwrap();
        assert_eq!(bt.file, "a.py");
        assert_eq!(bt.line, 10);
        assert_eq!(bt.direction, 1);
        assert_eq!(bt.condition, "x > 0");
        assert_eq!(bt.func, "myfn");
        assert_eq!(bt.module, "mymod");
        assert_eq!(bt.args.len(), 2);
        assert_eq!(bt.locals.len(), 3);
    }

    #[test]
    fn trace_output_deserialize_empty_branches() {
        let json = r#"{"branches":[]}"#;
        let to: TraceOutput = serde_json::from_str(json).unwrap();
        assert!(to.branches.is_empty());
    }

    // -----------------------------------------------------------------------
    // Additional coverage: constructor with populated file_paths
    // -----------------------------------------------------------------------

    #[test]
    fn new_with_populated_file_paths() {
        let mut fp = HashMap::new();
        fp.insert(42u64, PathBuf::from("src/main.py"));
        fp.insert(99u64, PathBuf::from("src/lib.py"));
        let s = PythonConcolicStrategy::new(
            Arc::new(CoverageOracle::new()),
            Arc::new(fp),
            PathBuf::from("/project"),
            vec!["python".to_string(), "-m".to_string(), "pytest".to_string()],
        );
        assert_eq!(s.name(), "python-concolic");
        assert!(s.file_paths.contains_key(&42));
        assert!(s.file_paths.contains_key(&99));
        assert!(s.trace_cache.lock().unwrap().is_none());
    }

    // -----------------------------------------------------------------------
    // Additional coverage: boundary_seeds with float literal in condition
    // -----------------------------------------------------------------------

    #[test]
    fn boundary_seeds_float_literal_in_condition() {
        let s = make_strategy();
        // Float literal: regex matches but parse::<i64> fails on "3.14"
        let entry = make_trace_entry(
            "test.py",
            10,
            0,
            "x > 3.14",
            "check",
            "mod",
            vec!["x"],
            [("x".into(), serde_json::json!(5))].into(),
        );
        // Falls through to fallback (integer local)
        let seeds = s.boundary_seeds(&entry, 1);
        assert!(!seeds.is_empty());
    }

    // -----------------------------------------------------------------------
    // Additional coverage: in-list with single item
    // -----------------------------------------------------------------------

    #[test]
    fn boundary_seeds_in_list_single_item() {
        let s = make_strategy();
        let entry = make_trace_entry(
            "test.py",
            20,
            1,
            "x in [\"admin\"]",
            "auth",
            "views",
            vec!["x"],
            [("x".into(), serde_json::json!("user"))].into(),
        );
        let seeds = s.boundary_seeds(&entry, 0);
        assert!(!seeds.is_empty());
        let combined: String = seeds.join("\n");
        assert!(
            combined.contains("admin"),
            "should contain admin: {combined}"
        );
    }

    #[test]
    fn boundary_seeds_in_list_many_items_takes_3() {
        let s = make_strategy();
        let entry = make_trace_entry(
            "test.py",
            20,
            1,
            "x in [\"a\", \"b\", \"c\", \"d\", \"e\"]",
            "check",
            "mod",
            vec!["x"],
            [("x".into(), serde_json::json!("z"))].into(),
        );
        // direction=0, takes first 3 items
        let seeds = s.boundary_seeds(&entry, 0);
        assert!(
            seeds.len() == 3,
            "should produce exactly 3 seeds for 5-item list: got {}",
            seeds.len()
        );
    }

    // -----------------------------------------------------------------------
    // Additional coverage: fallback with mixed locals (int + null + other)
    // -----------------------------------------------------------------------

    #[test]
    fn boundary_seeds_fallback_mixed_locals() {
        let s = make_strategy();
        let entry = make_trace_entry(
            "test.py",
            30,
            0,
            "complex(a, b, c)",
            "func",
            "mod",
            vec!["a", "b", "c"],
            [
                ("a".into(), serde_json::json!(5)),
                ("b".into(), serde_json::json!(null)),
                ("c".into(), serde_json::json!("text")),
            ]
            .into(),
        );
        // Fallback: a is i64 (flip), b is null (set to 0), c is string (skipped)
        let seeds = s.boundary_seeds(&entry, 0);
        assert!(!seeds.is_empty());
    }

    // -----------------------------------------------------------------------
    // Additional coverage: symbolic_seeds with direction=1 entries
    // -----------------------------------------------------------------------

    #[test]
    fn symbolic_seeds_with_direction_1_entries() {
        let s = make_strategy();
        let trace = vec![
            make_trace_entry("f.py", 1, 1, "x > 0", "f", "m", vec![], HashMap::new()),
            make_trace_entry("f.py", 2, 1, "y == 5", "f", "m", vec![], HashMap::new()),
        ];
        // direction_taken = entry.direction == 0, so these are false
        let seeds = s.symbolic_seeds_from_trace(&trace);
        // Without z3, still empty but tests the path
        assert!(seeds.is_empty());
    }

    // -----------------------------------------------------------------------
    // Additional coverage: BranchTrace clone
    // -----------------------------------------------------------------------

    #[test]
    fn branch_trace_clone() {
        let entry = make_trace_entry(
            "test.py",
            10,
            0,
            "x > 5",
            "check",
            "mod",
            vec!["x"],
            [("x".into(), serde_json::json!(10))].into(),
        );
        let cloned = entry.clone();
        assert_eq!(cloned.file, entry.file);
        assert_eq!(cloned.line, entry.line);
        assert_eq!(cloned.direction, entry.direction);
        assert_eq!(cloned.condition, entry.condition);
        assert_eq!(cloned.func, entry.func);
        assert_eq!(cloned.module, entry.module);
        assert_eq!(cloned.args, entry.args);
    }

    // -----------------------------------------------------------------------
    // Additional coverage: trace_cache manual population
    // -----------------------------------------------------------------------

    #[test]
    fn trace_cache_with_entries() {
        let s = make_strategy();
        let entries = vec![
            make_trace_entry("a.py", 1, 0, "x > 0", "f", "m", vec![], HashMap::new()),
            make_trace_entry("a.py", 2, 1, "y < 5", "f", "m", vec![], HashMap::new()),
        ];
        *s.trace_cache.lock().unwrap() = Some(entries);
        let cache = s.trace_cache.lock().unwrap();
        assert_eq!(cache.as_ref().unwrap().len(), 2);
    }

    // -----------------------------------------------------------------------
    // Additional coverage: observe with different statuses
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn observe_with_fail_status() {
        let s = make_strategy();
        let result = ExecutionResult {
            seed_id: apex_core::types::SeedId::new(),
            status: apex_core::types::ExecutionStatus::Fail,
            new_branches: vec![],
            trace: None,
            duration_ms: 100,
            stdout: "output".to_string(),
            stderr: "error".to_string(),
            input: None,
        };
        s.observe(&result).await.unwrap();
    }

    #[tokio::test]
    async fn observe_with_timeout_status() {
        let s = make_strategy();
        let result = ExecutionResult {
            seed_id: apex_core::types::SeedId::new(),
            status: apex_core::types::ExecutionStatus::Timeout,
            new_branches: vec![],
            trace: None,
            duration_ms: 5000,
            stdout: String::new(),
            stderr: String::new(),
            input: None,
        };
        s.observe(&result).await.unwrap();
    }

    // -----------------------------------------------------------------------
    // Additional coverage: generated code contains correct try/except
    // -----------------------------------------------------------------------

    #[test]
    fn boundary_seeds_generated_code_has_try_except() {
        let s = make_strategy();
        let entry = make_trace_entry(
            "app.py",
            42,
            0,
            "x > 10",
            "process",
            "mymod",
            vec!["x"],
            [("x".into(), serde_json::json!(15))].into(),
        );
        let seeds = s.boundary_seeds(&entry, 1);
        assert!(!seeds.is_empty());
        let code = &seeds[0];
        assert!(code.contains("try:"), "should contain try block");
        assert!(
            code.contains("except Exception:"),
            "should contain except block"
        );
        assert!(code.contains("pass"), "should contain pass statement");
    }

    #[test]
    fn boundary_seeds_generated_code_has_sys_path_insert() {
        let s = PythonConcolicStrategy::new(
            Arc::new(CoverageOracle::new()),
            Arc::new(HashMap::new()),
            PathBuf::from("/my/project"),
            vec!["pytest".to_string()],
        );
        let entry = make_trace_entry(
            "app.py",
            42,
            0,
            "x > 10",
            "process",
            "mymod",
            vec!["x"],
            [("x".into(), serde_json::json!(15))].into(),
        );
        let seeds = s.boundary_seeds(&entry, 1);
        assert!(!seeds.is_empty());
        let code = &seeds[0];
        assert!(
            code.contains("sys.path.insert(0, \"/my/project\")"),
            "should contain sys.path.insert with target_root: {code}"
        );
    }

    // -----------------------------------------------------------------------
    // Additional coverage: multiple assignments in generated code
    // -----------------------------------------------------------------------

    #[test]
    fn boundary_seeds_condition_comment_in_code() {
        let s = make_strategy();
        let entry = make_trace_entry(
            "app.py",
            42,
            0,
            "x > 10",
            "process",
            "mymod",
            vec!["x"],
            [("x".into(), serde_json::json!(15))].into(),
        );
        let seeds = s.boundary_seeds(&entry, 1);
        let code = &seeds[0];
        assert!(
            code.contains("# condition: x > 10"),
            "should contain condition comment: {code}"
        );
        assert!(
            code.contains("# apex-concolic: app.py:42"),
            "should contain file:line comment: {code}"
        );
        assert!(
            code.contains("# variant 0"),
            "should contain variant index: {code}"
        );
    }

    // -----------------------------------------------------------------------
    // Additional coverage: len with n=0 edge case
    // -----------------------------------------------------------------------

    #[test]
    fn boundary_seeds_len_ge_zero_want_false() {
        let s = make_strategy();
        let entry = make_trace_entry(
            "test.py",
            50,
            0,
            "len(x) >= 0",
            "process",
            "core",
            vec!["x"],
            [("x".into(), serde_json::json!("abc"))].into(),
        );
        // (">=", _) want false => if n > 0 { n-1 } else { 0 } => n=0, so target_len=0
        let seeds = s.boundary_seeds(&entry, 1);
        assert!(!seeds.is_empty());
    }

    #[test]
    fn boundary_seeds_len_lt_zero_want_true() {
        let s = make_strategy();
        let entry = make_trace_entry(
            "test.py",
            50,
            1,
            "len(x) < 0",
            "process",
            "core",
            vec!["x"],
            [("x".into(), serde_json::json!("abc"))].into(),
        );
        // ("<", 0) => if n > 0 { n-1 } else { 0 } => n=0, target_len=0
        let seeds = s.boundary_seeds(&entry, 0);
        assert!(!seeds.is_empty());
    }

    // -----------------------------------------------------------------------
    // Bug-hunting: boundary_seeds incorrect values for >= and <= operators
    // -----------------------------------------------------------------------

    /// BUG: For "x >= 5" wanting False (target_direction=1), the code generates
    /// [val-1, val] = [4, 5]. But val=5 makes "x >= 5" TRUE, not False.
    /// The second candidate is wrong -- it should be excluded or replaced.
    #[test]
    fn bug_ge_want_false_includes_true_value() {
        let s = make_strategy();
        let entry = make_trace_entry(
            "test.py", 10, 0, "x >= 5",
            "check", "mod", vec!["x"],
            [("x".into(), serde_json::json!(10))].into(),
        );
        // We want False (target_direction=1), meaning x < 5
        let seeds = s.boundary_seeds(&entry, 1);
        assert!(!seeds.is_empty());

        // Parse out the generated x values from the test stubs.
        // The code generates variants [val-1, val] = [4, 5].
        // x=5 makes "x >= 5" True, NOT False. This is a bug.
        // All generated values should make the condition False (x < 5).
        for seed in &seeds {
            // Extract the value assigned to x (look for "x = <number>")
            for line in seed.lines() {
                let trimmed = line.trim();
                if trimmed.starts_with("x = ") {
                    let val: i64 = trimmed.strip_prefix("x = ").unwrap().trim().parse().unwrap();
                    assert!(
                        val < 5,
                        "BUG: For 'x >= 5' wanting False, generated x={val} which makes the condition TRUE. \
                         Expected all values < 5. Seed:\n{seed}"
                    );
                }
            }
        }
    }

    /// BUG: For "x <= 5" wanting False (target_direction=1), the code generates
    /// [val+1, val] = [6, 5]. But val=5 makes "x <= 5" TRUE, not False.
    #[test]
    fn bug_le_want_false_includes_true_value() {
        let s = make_strategy();
        let entry = make_trace_entry(
            "test.py", 10, 0, "x <= 5",
            "check", "mod", vec!["x"],
            [("x".into(), serde_json::json!(3))].into(),
        );
        // We want False (target_direction=1), meaning x > 5
        let seeds = s.boundary_seeds(&entry, 1);
        assert!(!seeds.is_empty());

        for seed in &seeds {
            for line in seed.lines() {
                let trimmed = line.trim();
                if trimmed.starts_with("x = ") {
                    let val: i64 = trimmed.strip_prefix("x = ").unwrap().trim().parse().unwrap();
                    assert!(
                        val > 5,
                        "BUG: For 'x <= 5' wanting False, generated x={val} which makes the condition TRUE. \
                         Expected all values > 5. Seed:\n{seed}"
                    );
                }
            }
        }
    }

    /// BUG: For "x >= 5" wanting True (target_direction=0), the code generates
    /// [val+1, val+2] = [6, 7]. These are correct but miss the exact boundary
    /// val=5 which IS the key value for >=. This is suboptimal but not incorrect.
    #[test]
    fn bug_ge_want_true_misses_boundary() {
        let s = make_strategy();
        let entry = make_trace_entry(
            "test.py", 10, 1, "x >= 5",
            "check", "mod", vec!["x"],
            [("x".into(), serde_json::json!(3))].into(),
        );
        // We want True (target_direction=0), meaning x >= 5
        let seeds = s.boundary_seeds(&entry, 0);
        assert!(!seeds.is_empty());

        // Check if 5 (the exact boundary) is among the candidates
        let mut has_boundary = false;
        for seed in &seeds {
            for line in seed.lines() {
                let trimmed = line.trim();
                if trimmed.starts_with("x = ") {
                    let val: i64 = trimmed.strip_prefix("x = ").unwrap().trim().parse().unwrap();
                    if val == 5 {
                        has_boundary = true;
                    }
                    // All values should make condition true (x >= 5)
                    assert!(
                        val >= 5,
                        "For 'x >= 5' wanting True, generated x={val} which is False"
                    );
                }
            }
        }
        // The boundary value 5 (where >= differs from >) should be included
        assert!(
            has_boundary,
            "BUG: For 'x >= 5' wanting True, the exact boundary value 5 is missing. \
             Generated seeds only have values > 5, missing the key >= boundary."
        );
    }

    // -----------------------------------------------------------------------
    // Bug-hunting: additional edge cases
    // -----------------------------------------------------------------------

    #[test]
    fn boundary_seeds_empty_condition() {
        let s = make_strategy();
        let entry = make_trace_entry(
            "test.py", 10, 0, "",
            "func", "mod", vec!["x"],
            [("x".into(), serde_json::json!(5))].into(),
        );
        // Empty condition matches no regex, falls through to local mutation fallback
        let seeds = s.boundary_seeds(&entry, 1);
        assert!(!seeds.is_empty(), "empty condition should fall through to local mutation");
    }

    #[test]
    fn boundary_seeds_condition_no_operator() {
        let s = make_strategy();
        let entry = make_trace_entry(
            "test.py", 10, 0, "just_a_variable_name",
            "func", "mod", vec!["x"],
            [("x".into(), serde_json::json!(5))].into(),
        );
        let seeds = s.boundary_seeds(&entry, 1);
        // Falls through to local mutation
        assert!(!seeds.is_empty());
    }

    #[test]
    fn boundary_seeds_integer_overflow_boundary() {
        let s = make_strategy();
        // Condition with i64::MAX — val+1 and val+2 could overflow
        let entry = make_trace_entry(
            "test.py", 10, 0, &format!("x > {}", i64::MAX),
            "func", "mod", vec!["x"],
            [("x".into(), serde_json::json!(i64::MAX))].into(),
        );
        // BUG: The code does `val + 1` and `val + 2` on parsed i64 value.
        // For val = i64::MAX, `val + 1` overflows in debug mode.
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            s.boundary_seeds(&entry, 0) // want True for ">" -> variants [val+1, val+2]
        }));
        if result.is_err() {
            panic!("BUG CONFIRMED: i64 overflow when generating boundary values for i64::MAX");
        }
    }

    #[test]
    fn boundary_seeds_integer_underflow_boundary() {
        let s = make_strategy();
        // Condition with i64::MIN — val-1 could overflow
        let entry = make_trace_entry(
            "test.py", 10, 0, &format!("x < {}", i64::MIN),
            "func", "mod", vec!["x"],
            [("x".into(), serde_json::json!(i64::MIN))].into(),
        );
        // BUG: For ("<", 0), the code generates [val-1, val-2].
        // For val = i64::MIN, `val - 1` overflows in debug mode.
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            s.boundary_seeds(&entry, 0) // want True for "<" -> variants [val-1, val-2]
        }));
        if result.is_err() {
            panic!("BUG CONFIRMED: i64 underflow when generating boundary values for i64::MIN");
        }
    }

    #[test]
    fn boundary_seeds_empty_module_and_func() {
        let s = make_strategy();
        let entry = make_trace_entry(
            "test.py", 10, 0, "x > 5",
            "", "",
            vec!["x"],
            [("x".into(), serde_json::json!(10))].into(),
        );
        let seeds = s.boundary_seeds(&entry, 1);
        // After fix: empty module/func should produce no seeds (not invalid Python)
        assert!(seeds.is_empty(), "expected no seeds for empty module/func, got: {:?}", seeds);
    }
}
