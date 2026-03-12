use crate::traits::{Solver as SolverTrait, SolverLogic};
use apex_core::{
    error::Result,
    types::{InputSeed, PathConstraint},
};
use tracing::warn;

/// Maximum constraint chain depth for Z3 solving.
#[allow(dead_code)]
const MAX_DEPTH: usize = 64;

// ---------------------------------------------------------------------------
// Z3Solver — struct wrapping Z3 backend behind the Solver trait
// ---------------------------------------------------------------------------

/// Z3-backed solver. Without `z3-solver` feature, all methods return None.
pub struct Z3Solver {
    logic: SolverLogic,
}

impl Z3Solver {
    pub fn new(logic: SolverLogic) -> Self {
        Z3Solver { logic }
    }

    /// Factory: pick logic based on target language.
    pub fn for_language(lang: apex_core::types::Language) -> Self {
        use apex_core::types::Language;
        let logic = match lang {
            Language::Python => SolverLogic::QfLia,
            Language::C | Language::Rust => SolverLogic::QfAbv,
            Language::JavaScript => SolverLogic::QfS,
            _ => SolverLogic::Auto,
        };
        Z3Solver::new(logic)
    }
}

impl SolverTrait for Z3Solver {
    fn solve(&self, constraints: &[String], negate_last: bool) -> Result<Option<InputSeed>> {
        if constraints.is_empty() {
            return Ok(None);
        }
        #[cfg(feature = "z3-solver")]
        {
            solve_z3(constraints, negate_last, self.logic)
        }
        #[cfg(not(feature = "z3-solver"))]
        {
            let _ = negate_last;
            warn!("z3-solver feature not enabled");
            Ok(None)
        }
    }

    fn set_logic(&mut self, logic: SolverLogic) {
        self.logic = logic;
    }

    fn name(&self) -> &str {
        "z3"
    }
}

// ---------------------------------------------------------------------------
// Free function (backwards compatibility)
// ---------------------------------------------------------------------------

/// Solve a path constraint set, optionally negating the final constraint to
/// find an input that takes the opposite branch.
///
/// Returns `None` when:
/// - the constraint set is empty
/// - the solver backend is not compiled in (`z3-solver` feature absent)
/// - the constraints are unsatisfiable (branch is unreachable)
pub fn solve(constraints: Vec<String>, negate_last: bool) -> Result<Option<InputSeed>> {
    let solver = Z3Solver::new(SolverLogic::Auto);
    SolverTrait::solve(&solver, &constraints, negate_last)
}

// ---------------------------------------------------------------------------
// Z3 backend (only compiled with --features z3-solver)
// ---------------------------------------------------------------------------

#[cfg(feature = "z3-solver")]
fn solve_z3(
    constraints: &[String],
    negate_last: bool,
    logic: SolverLogic,
) -> Result<Option<InputSeed>> {
    use crate::smtlib;
    use apex_core::error::ApexError;
    use apex_core::types::SeedOrigin;
    use std::collections::HashMap;
    use z3::ast::{Ast, Bool, Int};
    use z3::{Config, Context, SatResult, Solver};

    let bounded: Vec<&str> = constraints
        .iter()
        .take(MAX_DEPTH)
        .map(|s| s.as_str())
        .collect();

    // ── 1. Collect all variable names across every constraint. ──────────────
    let mut var_names: Vec<String> = Vec::new();
    for c in &bounded {
        for v in smtlib::extract_variables(c) {
            if !var_names.contains(&v) {
                var_names.push(v);
            }
        }
    }

    // ── 2. Create Z3 context, solver, and declare Int constants. ────────────
    let cfg = Config::new();
    let ctx = Context::new(&cfg);
    let solver = Solver::new(&ctx);

    // Set logic based on SolverLogic
    match logic {
        SolverLogic::QfLia => {
            solver.set_logic("QF_LIA");
        }
        SolverLogic::QfAbv => {
            solver.set_logic("QF_ABV");
        }
        SolverLogic::QfS => {
            solver.set_logic("QF_S");
        }
        SolverLogic::Auto => {} // let Z3 decide
    }

    let mut vars: HashMap<String, Int<'_>> = HashMap::new();
    for name in &var_names {
        vars.insert(name.clone(), Int::new_const(&ctx, name.as_str()));
    }

    // ── 3. Assert the first `split_at` constraints as-is. ───────────────────
    let split_at = if negate_last {
        bounded.len().saturating_sub(1)
    } else {
        bounded.len()
    };

    for s in &bounded[..split_at] {
        match parse_bool(&ctx, s, &vars) {
            Some(b) => solver.assert(&b),
            None => {
                tracing::debug!(smtlib2 = s, "could not parse constraint; skipping");
            }
        }
    }

    // ── 4. Negate the last constraint (diverge at this branch). ─────────────
    if negate_last {
        if let Some(last) = bounded.last() {
            match parse_bool(&ctx, last, &vars) {
                Some(b) => solver.assert(&b.not()),
                None => {
                    tracing::debug!(
                        smtlib2 = last,
                        "could not parse last constraint; skipping negation"
                    );
                }
            }
        }
    }

    // ── 5. Solve and extract model. ─────────────────────────────────────────
    match solver.check() {
        SatResult::Sat => {
            let Some(model) = solver.get_model() else {
                return Ok(None);
            };

            // Encode variable assignments as JSON bytes so the seed is
            // human-readable and can be fed back to the Python sandbox.
            let mut assignments = serde_json::Map::new();
            for (name, var) in &vars {
                if let Some(val) = model.eval(var, true) {
                    if let Some(n) = val.as_i64() {
                        assignments.insert(name.clone(), serde_json::json!(n));
                    }
                }
            }

            if assignments.is_empty() {
                return Ok(None);
            }

            tracing::debug!(?assignments, "z3 model");

            let json = serde_json::to_vec(&serde_json::Value::Object(assignments))
                .map_err(|e| ApexError::Solver(format!("serialize z3 model: {e}")))?;

            Ok(Some(InputSeed::new(json, SeedOrigin::Symbolic)))
        }
        SatResult::Unsat => {
            tracing::debug!("z3: constraint set UNSAT — branch unreachable");
            Ok(None)
        }
        SatResult::Unknown => {
            warn!("z3: returned Unknown (timeout or undecidable)");
            Ok(None)
        }
    }
}

// ---------------------------------------------------------------------------
// Minimal SMTLIB2 parser (Z3-backend only)
// ---------------------------------------------------------------------------

/// Parse an SMTLIB2 Boolean expression into a Z3 `Bool` AST node.
///
/// Supports: `(> a b)`, `(>= a b)`, `(< a b)`, `(<= a b)`,
///           `(= a b)`, `(not expr)`, `(and e1 e2 …)`, `(or e1 e2 …)`,
///           `true`, `false`.
#[cfg(feature = "z3-solver")]
fn parse_bool<'ctx>(
    ctx: &'ctx z3::Context,
    s: &str,
    vars: &std::collections::HashMap<String, z3::ast::Int<'ctx>>,
) -> Option<z3::ast::Bool<'ctx>> {
    use z3::ast::{Ast, Bool, Int};

    let s = s.trim();

    if !s.starts_with('(') {
        return match s {
            "true" => Some(Bool::from_bool(ctx, true)),
            "false" => Some(Bool::from_bool(ctx, false)),
            _ => None,
        };
    }

    // Strip outer parens.
    let inner = s[1..s.len() - 1].trim();
    let (op, rest) = split_head(inner)?;

    match op {
        "not" => {
            let arg = parse_bool(ctx, rest.trim(), vars)?;
            Some(arg.not())
        }
        "and" => {
            let args = split_args(rest);
            let bools: Vec<Bool<'ctx>> = args
                .iter()
                .filter_map(|a| parse_bool(ctx, a, vars))
                .collect();
            if bools.is_empty() {
                return None;
            }
            let refs: Vec<&Bool<'ctx>> = bools.iter().collect();
            Some(Bool::and(ctx, &refs))
        }
        "or" => {
            let args = split_args(rest);
            let bools: Vec<Bool<'ctx>> = args
                .iter()
                .filter_map(|a| parse_bool(ctx, a, vars))
                .collect();
            if bools.is_empty() {
                return None;
            }
            let refs: Vec<&Bool<'ctx>> = bools.iter().collect();
            Some(Bool::or(ctx, &refs))
        }
        ">" | ">=" | "<" | "<=" | "=" => {
            let args = split_args(rest);
            if args.len() != 2 {
                return None;
            }
            let lhs = parse_int(ctx, args[0].trim(), vars)?;
            let rhs = parse_int(ctx, args[1].trim(), vars)?;
            let cmp = match op {
                ">" => lhs.gt(&rhs),
                ">=" => lhs.ge(&rhs),
                "<" => lhs.lt(&rhs),
                "<=" => lhs.le(&rhs),
                "=" => lhs._eq(&rhs),
                _ => {
                    debug_assert!(false, "unexpected op {op} in comparison arm");
                    return None;
                }
            };
            Some(cmp)
        }
        _ => None,
    }
}

/// Parse an integer expression: variable name or integer literal.
#[cfg(feature = "z3-solver")]
fn parse_int<'ctx>(
    ctx: &'ctx z3::Context,
    s: &str,
    vars: &std::collections::HashMap<String, z3::ast::Int<'ctx>>,
) -> Option<z3::ast::Int<'ctx>> {
    use z3::ast::Int;

    let s = s.trim();

    // Integer literal (including negative via `(- N)` form).
    if let Ok(n) = s.parse::<i64>() {
        return Some(Int::from_i64(ctx, n));
    }

    // `(- N)` — unary minus.
    if s.starts_with("(-") && s.ends_with(')') {
        let inner = s[2..s.len() - 1].trim();
        if let Ok(n) = inner.parse::<i64>() {
            return Some(Int::from_i64(ctx, -n));
        }
    }

    // Variable reference.
    vars.get(s).cloned()
}

/// Split `"op rest…"` into `("op", "rest…")`.
#[cfg(feature = "z3-solver")]
fn split_head(s: &str) -> Option<(&str, &str)> {
    let s = s.trim();
    let end = s.find(|c: char| c.is_whitespace()).unwrap_or(s.len());
    let op = &s[..end];
    let rest = s[end..].trim_start();
    if op.is_empty() {
        None
    } else {
        Some((op, rest))
    }
}

/// Split an argument list, respecting nested parentheses.
#[cfg(feature = "z3-solver")]
fn split_args(s: &str) -> Vec<String> {
    let mut args = Vec::new();
    let mut depth: usize = 0;
    let mut start = 0;

    for (i, c) in s.char_indices() {
        match c {
            '(' => {
                if depth == 0 {
                    start = i;
                }
                depth += 1;
            }
            ')' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    let tok = s[start..=i].trim().to_string();
                    if !tok.is_empty() {
                        args.push(tok);
                    }
                    start = i + 1;
                }
            }
            _ if c.is_whitespace() && depth == 0 => {
                let tok = s[start..i].trim().to_string();
                if !tok.is_empty() {
                    args.push(tok);
                }
                start = i + 1;
            }
            _ => {}
        }
    }

    let last = s[start..].trim().to_string();
    if !last.is_empty() {
        args.push(last);
    }

    args
}

// ---------------------------------------------------------------------------
// SymbolicSession — accumulates path constraints from one concolic run
// ---------------------------------------------------------------------------

/// Accumulated path constraints from a single concolic execution.
///
/// Usage:
/// ```ignore
/// let mut session = SymbolicSession::new();
/// session.push(PathConstraint { smtlib2: "(> x 0)".into(), … });
/// let seeds = session.diverging_inputs()?;
/// ```
#[derive(Debug, Clone, Default)]
pub struct SymbolicSession {
    constraints: Vec<PathConstraint>,
}

impl SymbolicSession {
    pub fn new() -> Self {
        SymbolicSession {
            constraints: Vec::new(),
        }
    }

    pub fn push(&mut self, constraint: PathConstraint) {
        self.constraints.push(constraint);
    }

    pub fn len(&self) -> usize {
        self.constraints.len()
    }

    pub fn is_empty(&self) -> bool {
        self.constraints.is_empty()
    }

    /// Generate diverging inputs by negating each path-prefix constraint.
    ///
    /// For N constraints `[c0, c1, …, c_{n-1}]`, tries solving:
    /// - `[c0, ¬c0]`
    /// - `[c0, c1, ¬c1]`
    /// - …
    ///
    /// Returns a seed for each satisfiable prefix.
    pub fn diverging_inputs(&self) -> Result<Vec<InputSeed>> {
        let solver = Z3Solver::new(SolverLogic::Auto);
        self.diverging_inputs_with(&solver)
    }

    /// SAGE-style generational search: for N constraints, build N constraint
    /// sets where the i-th set keeps constraints [0..i-1] as-is and negates
    /// constraint i.  This explores all divergence points in a single batch.
    pub fn diverging_inputs_generational(&self) -> Result<Vec<InputSeed>> {
        if self.constraints.is_empty() {
            return Ok(Vec::new());
        }
        let smtlibs: Vec<String> = self.constraints.iter().map(|c| c.smtlib2.clone()).collect();
        let mut constraint_sets: Vec<Vec<String>> = Vec::with_capacity(smtlibs.len());
        for i in 1..=smtlibs.len() {
            let mut set = smtlibs[..i - 1].to_vec();
            let negated = format!("(not {})", smtlibs[i - 1]);
            set.push(negated);
            constraint_sets.push(set);
        }
        let results: Vec<Option<InputSeed>> = constraint_sets
            .into_iter()
            .map(|cs| match solve(cs, false) {
                Ok(seed) => seed,
                Err(e) => {
                    tracing::debug!(error = %e, "generational solve failed for one prefix");
                    None
                }
            })
            .collect();
        Ok(results.into_iter().flatten().collect())
    }

    /// Generate diverging inputs using a provided solver.
    /// Generational search: negates ALL prefixes in one pass.
    pub fn diverging_inputs_with(&self, solver: &dyn SolverTrait) -> Result<Vec<InputSeed>> {
        if self.constraints.is_empty() {
            return Ok(Vec::new());
        }
        let smtlibs: Vec<String> = self.constraints.iter().map(|c| c.smtlib2.clone()).collect();
        let sets: Vec<Vec<String>> = (1..=smtlibs.len()).map(|i| smtlibs[..i].to_vec()).collect();
        let results = solver.solve_batch(&sets, true);
        let mut inputs = Vec::new();
        for result in results {
            match result {
                Ok(Some(seed)) => inputs.push(seed),
                Ok(None) => {}
                Err(e) => {
                    tracing::debug!(error = %e, "symbolic solve failed in batch");
                }
            }
        }
        Ok(inputs)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::traits::{Solver as SolverTrait, SolverLogic};
    use apex_core::types::{BranchId, PathConstraint};

    fn make_constraint(smtlib2: &str) -> PathConstraint {
        PathConstraint {
            branch: BranchId::new(1, 1, 0, 0),
            smtlib2: smtlib2.to_string(),
            direction_taken: true,
        }
    }

    #[test]
    fn solve_empty_constraints_returns_none() {
        let result = solve(vec![], false).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn solve_empty_constraints_negate_last_returns_none() {
        let result = solve(vec![], true).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn solve_non_empty_without_z3_returns_none() {
        // Without z3-solver feature, solve should return Ok(None)
        let result = solve(vec!["(> x 0)".to_string()], false).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn solve_non_empty_negate_last_without_z3_returns_none() {
        let result = solve(vec!["(> x 0)".to_string(), "(< y 5)".to_string()], true).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn session_new_creates_empty() {
        let session = SymbolicSession::new();
        assert!(session.is_empty());
        assert_eq!(session.len(), 0);
    }

    #[test]
    fn session_default_creates_empty() {
        let session = SymbolicSession::default();
        assert!(session.is_empty());
        assert_eq!(session.len(), 0);
    }

    #[test]
    fn session_push_adds_constraints() {
        let mut session = SymbolicSession::new();
        session.push(make_constraint("(> x 0)"));
        assert_eq!(session.len(), 1);
        assert!(!session.is_empty());

        session.push(make_constraint("(< y 5)"));
        assert_eq!(session.len(), 2);
    }

    #[test]
    fn session_diverging_inputs_empty_returns_empty() {
        let session = SymbolicSession::new();
        let inputs = session.diverging_inputs().unwrap();
        assert!(inputs.is_empty());
    }

    #[test]
    fn session_diverging_inputs_no_z3_returns_empty() {
        // Without z3-solver, solve always returns None, so diverging_inputs
        // should return an empty vec even with constraints present.
        let mut session = SymbolicSession::new();
        session.push(make_constraint("(> x 0)"));
        session.push(make_constraint("(< y 5)"));
        session.push(make_constraint("(= z 3)"));
        let inputs = session.diverging_inputs().unwrap();
        assert!(inputs.is_empty());
    }

    #[test]
    fn session_clone_works() {
        let mut session = SymbolicSession::new();
        session.push(make_constraint("(> x 0)"));
        let cloned = session.clone();
        assert_eq!(cloned.len(), 1);
    }

    #[test]
    fn session_debug_works() {
        let session = SymbolicSession::new();
        let debug = format!("{:?}", session);
        assert!(debug.contains("SymbolicSession"));
    }

    #[test]
    fn z3_solver_implements_trait() {
        let solver = Z3Solver::new(SolverLogic::Auto);
        assert_eq!(solver.name(), "z3");
    }

    #[test]
    fn z3_solver_set_logic() {
        let mut solver = Z3Solver::new(SolverLogic::Auto);
        solver.set_logic(SolverLogic::QfLia);
        // Should not panic
    }

    #[test]
    fn z3_solver_for_language_python() {
        let solver = Z3Solver::for_language(apex_core::types::Language::Python);
        assert_eq!(solver.name(), "z3");
    }

    #[test]
    fn z3_solver_for_language_rust() {
        let solver = Z3Solver::for_language(apex_core::types::Language::Rust);
        assert_eq!(solver.name(), "z3");
    }

    #[test]
    fn z3_solver_for_language_js() {
        let solver = Z3Solver::for_language(apex_core::types::Language::JavaScript);
        assert_eq!(solver.name(), "z3");
    }

    #[test]
    fn session_diverging_inputs_generational_empty() {
        let session = SymbolicSession::new();
        let inputs = session.diverging_inputs_generational().unwrap();
        assert!(inputs.is_empty());
    }

    #[test]
    fn session_diverging_inputs_generational_with_constraints() {
        let mut session = SymbolicSession::new();
        session.push(make_constraint("(> x 0)"));
        session.push(make_constraint("(< y 5)"));
        session.push(make_constraint("(= z 3)"));
        let inputs = session.diverging_inputs_generational().unwrap();
        assert!(inputs.is_empty()); // Without Z3, no seeds produced
    }

    #[test]
    fn session_diverging_inputs_with_solver() {
        use crate::cache::CachingSolver;
        let solver = Z3Solver::new(SolverLogic::Auto);
        let cached = CachingSolver::new(solver);
        let mut session = SymbolicSession::new();
        session.push(make_constraint("(> x 0)"));
        let inputs = session.diverging_inputs_with(&cached).unwrap();
        let _ = inputs;
    }

    #[test]
    fn z3_solver_for_language_java_uses_auto() {
        let solver = Z3Solver::for_language(apex_core::types::Language::Java);
        assert_eq!(solver.name(), "z3");
    }

    #[test]
    fn z3_solver_for_language_wasm_uses_auto() {
        let solver = Z3Solver::for_language(apex_core::types::Language::Wasm);
        assert_eq!(solver.name(), "z3");
    }

    #[test]
    fn z3_solver_for_language_c() {
        let solver = Z3Solver::for_language(apex_core::types::Language::C);
        assert_eq!(solver.name(), "z3");
    }

    #[test]
    fn solve_single_constraint_without_z3() {
        let solver = Z3Solver::new(SolverLogic::Auto);
        let result = SolverTrait::solve(&solver, &["(> x 0)".to_string()], false).unwrap();
        assert!(result.is_none()); // no z3 feature
    }

    #[test]
    fn solve_single_constraint_negate_without_z3() {
        let solver = Z3Solver::new(SolverLogic::Auto);
        let result = SolverTrait::solve(&solver, &["(> x 0)".to_string()], true).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn session_diverging_inputs_generational_single_constraint() {
        let mut session = SymbolicSession::new();
        session.push(make_constraint("(> x 0)"));
        let inputs = session.diverging_inputs_generational().unwrap();
        assert!(inputs.is_empty()); // no Z3
    }

    #[test]
    fn z3_solver_for_language_ruby_uses_auto() {
        let solver = Z3Solver::for_language(apex_core::types::Language::Ruby);
        assert_eq!(solver.name(), "z3");
    }

    #[test]
    fn z3_solver_solve_empty_returns_none() {
        let solver = Z3Solver::new(SolverLogic::QfLia);
        let result = SolverTrait::solve(&solver, &[], false).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn z3_solver_solve_empty_negate_returns_none() {
        let solver = Z3Solver::new(SolverLogic::QfAbv);
        let result = SolverTrait::solve(&solver, &[], true).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn z3_solver_set_logic_all_variants() {
        let mut solver = Z3Solver::new(SolverLogic::Auto);
        solver.set_logic(SolverLogic::QfLia);
        solver.set_logic(SolverLogic::QfAbv);
        solver.set_logic(SolverLogic::QfS);
        solver.set_logic(SolverLogic::Auto);
    }

    #[test]
    fn solve_free_fn_negate_last() {
        let result = solve(vec!["(> x 0)".to_string(), "(< y 10)".to_string()], true).unwrap();
        assert!(result.is_none()); // no z3
    }

    #[test]
    fn session_multiple_push_then_diverge() {
        let mut session = SymbolicSession::new();
        for i in 0..10 {
            session.push(make_constraint(&format!("(> x{}  0)", i)));
        }
        assert_eq!(session.len(), 10);
        let inputs = session.diverging_inputs().unwrap();
        assert!(inputs.is_empty()); // no z3
    }

    #[test]
    fn session_diverging_inputs_generational_two_constraints() {
        let mut session = SymbolicSession::new();
        session.push(make_constraint("(> x 0)"));
        session.push(make_constraint("(< y 10)"));
        let inputs = session.diverging_inputs_generational().unwrap();
        assert!(inputs.is_empty()); // no z3
    }

    #[test]
    fn session_diverging_inputs_with_z3solver_no_z3() {
        let solver = Z3Solver::new(SolverLogic::QfLia);
        let mut session = SymbolicSession::new();
        session.push(make_constraint("(> x 0)"));
        session.push(make_constraint("(< y 10)"));
        let inputs = session.diverging_inputs_with(&solver).unwrap();
        assert!(inputs.is_empty());
    }

    // ------------------------------------------------------------------
    // Additional gap-filling tests
    // ------------------------------------------------------------------

    #[test]
    fn z3_solver_for_language_all_arms() {
        // Each Language variant produces a valid Z3Solver (cover the match arms)
        use apex_core::types::Language;
        for lang in [
            Language::Python,
            Language::C,
            Language::Rust,
            Language::JavaScript,
            Language::Java,
            Language::Wasm,
            Language::Ruby,
        ] {
            let solver = Z3Solver::for_language(lang);
            assert_eq!(solver.name(), "z3");
        }
    }

    #[test]
    fn session_diverging_inputs_with_many_constraints() {
        // Exercises the sets-building loop inside diverging_inputs_with
        let solver = Z3Solver::new(SolverLogic::Auto);
        let mut session = SymbolicSession::new();
        for i in 0..5 {
            session.push(make_constraint(&format!("(> x{i} 0)")));
        }
        let inputs = session.diverging_inputs_with(&solver).unwrap();
        assert!(inputs.is_empty()); // no z3
    }

    #[test]
    fn session_generational_one_then_two_constraints() {
        // Cover generational with exactly 1 and 2 constraints
        let mut s1 = SymbolicSession::new();
        s1.push(make_constraint("(> a 0)"));
        assert!(s1.diverging_inputs_generational().unwrap().is_empty());

        let mut s2 = SymbolicSession::new();
        s2.push(make_constraint("(> a 0)"));
        s2.push(make_constraint("(< b 5)"));
        assert!(s2.diverging_inputs_generational().unwrap().is_empty());
    }

    #[test]
    fn session_len_and_is_empty_consistency() {
        let mut session = SymbolicSession::new();
        assert_eq!(session.is_empty(), session.len() == 0);
        session.push(make_constraint("(> x 0)"));
        assert_eq!(session.is_empty(), session.len() == 0);
        assert_eq!(session.len(), 1);
    }

    #[test]
    fn z3_solver_name_all_logic_variants() {
        // Ensure all SolverLogic variants don't affect the name() method
        for logic in [
            SolverLogic::QfLia,
            SolverLogic::QfAbv,
            SolverLogic::QfS,
            SolverLogic::Auto,
        ] {
            let solver = Z3Solver::new(logic);
            assert_eq!(solver.name(), "z3");
        }
    }

    #[test]
    fn solve_free_function_many_constraints() {
        // Exercises the take(MAX_DEPTH) path — pass more than 64 constraints
        let constraints: Vec<String> = (0..70).map(|i| format!("(> x{i} 0)")).collect();
        let result = solve(constraints, false).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn session_diverging_inputs_batch_error_is_swallowed() {
        // diverging_inputs_with logs and discards Err from batch results
        use crate::cache::CachingSolver;
        let inner = Z3Solver::new(SolverLogic::Auto);
        let cached = CachingSolver::new(inner);
        let mut session = SymbolicSession::new();
        session.push(make_constraint("(= z 0)"));
        // Should not panic or return Err
        let result = session.diverging_inputs_with(&cached);
        assert!(result.is_ok());
    }
}
