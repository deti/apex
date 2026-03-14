//! Gradient descent constraint solver (from Angora).
//!
//! Solves simple numeric comparison constraints by treating them as
//! distance functions and descending toward zero distance.

use apex_core::error::Result;
use apex_core::types::{InputSeed, SeedOrigin};

use crate::traits::{Solver, SolverLogic};

/// Comparison operation for constraints.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CmpOp {
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
}

/// Compute distance to flipping a comparison.
/// Returns 0.0 when the branch flips (constraint satisfied).
pub fn comparison_distance(op: CmpOp, a: i64, b: i64) -> f64 {
    match op {
        CmpOp::Eq => (a as f64 - b as f64).abs(),
        CmpOp::Ne => {
            if a != b {
                0.0
            } else {
                1.0
            }
        }
        CmpOp::Lt => {
            if a < b {
                0.0
            } else {
                (a as f64 - b as f64) + 1.0
            }
        }
        CmpOp::Le => {
            if a <= b {
                0.0
            } else {
                a as f64 - b as f64
            }
        }
        CmpOp::Gt => {
            if a > b {
                0.0
            } else {
                (b as f64 - a as f64) + 1.0
            }
        }
        CmpOp::Ge => {
            if a >= b {
                0.0
            } else {
                b as f64 - a as f64
            }
        }
    }
}

/// Gradient descent solver for simple numeric constraints.
pub struct GradientSolver {
    max_iterations: usize,
}

impl GradientSolver {
    pub fn new(max_iterations: usize) -> Self {
        GradientSolver { max_iterations }
    }

    /// Attempt to solve a single comparison constraint by gradient descent.
    ///
    /// Given `op(variable, target)`, find a value for `variable` that satisfies
    /// the constraint, starting from `current_value`.
    ///
    /// Returns `Some(solution)` if found, `None` if descent stalls or max iterations reached.
    pub fn solve_comparison(&self, op: CmpOp, current_value: i64, target: i64) -> Option<i64> {
        let mut value = current_value;
        let mut best_distance = comparison_distance(op, value, target);

        if best_distance == 0.0 {
            return Some(value);
        }

        for _ in 0..self.max_iterations {
            // Compute gradient via finite differences
            let d_plus = comparison_distance(op, value.saturating_add(1), target);
            let d_minus = comparison_distance(op, value.saturating_sub(1), target);

            // Choose direction that reduces distance
            let (next_value, next_distance) = if d_plus < d_minus {
                // Positive direction is better
                let step = self.find_step_size(op, value, target, 1);
                let v = value.saturating_add(step);
                (v, comparison_distance(op, v, target))
            } else if d_minus < d_plus {
                // Negative direction is better
                let step = self.find_step_size(op, value, target, -1);
                let v = value.saturating_sub(step);
                (v, comparison_distance(op, v, target))
            } else {
                // Equal gradients — try both with step 1
                let v_plus = value.saturating_add(1);
                let v_minus = value.saturating_sub(1);
                let d_p = comparison_distance(op, v_plus, target);
                let d_m = comparison_distance(op, v_minus, target);
                if d_p <= d_m {
                    (v_plus, d_p)
                } else {
                    (v_minus, d_m)
                }
            };

            if next_distance == 0.0 {
                return Some(next_value);
            }

            if next_distance >= best_distance {
                // Stalled — no progress
                break;
            }

            value = next_value;
            best_distance = next_distance;
        }

        // Check if we ended up at a solution
        if comparison_distance(op, value, target) == 0.0 {
            Some(value)
        } else {
            None
        }
    }

    /// Find an appropriate step size using exponential search.
    /// Doubles step until distance starts increasing, then returns the last good step.
    fn find_step_size(&self, op: CmpOp, value: i64, target: i64, direction: i64) -> i64 {
        let mut step: i64 = 1;
        let mut best_step: i64 = 1;
        let mut best_distance = comparison_distance(
            op,
            value.saturating_add(direction.saturating_mul(step)),
            target,
        );

        for _ in 0..20 {
            // max 2^20 step size
            let next_step = step.saturating_mul(2);
            if next_step == step {
                break; // overflow
            }
            let v = value.saturating_add(direction.saturating_mul(next_step));
            let d = comparison_distance(op, v, target);

            if d < best_distance {
                best_step = next_step;
                best_distance = d;
                step = next_step;
            } else {
                break;
            }
        }

        best_step
    }
}

impl Solver for GradientSolver {
    fn solve(&self, constraints: &[String], negate_last: bool) -> Result<Option<InputSeed>> {
        // Note: only the last constraint is considered; earlier path constraints are ignored.
        // The portfolio solver falls back to Z3 for path-feasible solutions.
        if constraints.is_empty() {
            return Ok(None);
        }

        // Parse the last constraint (the one we're trying to solve)
        let constraint = constraints.last().unwrap();

        // Try to parse simple comparison: "(> x 5)" or "(= x 42)"
        let parsed = parse_simple_comparison(constraint);
        let (op, var_name, target) = match parsed {
            Some(p) => p,
            None => return Ok(None), // Can't handle complex constraints
        };

        // If negate_last, flip the comparison
        let op = if negate_last { negate_op(op) } else { op };

        // Start from 0 as default initial value
        let result = self.solve_comparison(op, 0, target);

        match result {
            Some(value) => {
                // Encode as JSON: {"var_name": value}
                let json = format!("{{\"{var_name}\": {value}}}");
                Ok(Some(InputSeed::new(
                    json.into_bytes(),
                    SeedOrigin::Symbolic,
                )))
            }
            None => Ok(None),
        }
    }

    fn set_logic(&mut self, _logic: SolverLogic) {
        // Gradient descent is logic-agnostic for numeric constraints
    }

    fn name(&self) -> &str {
        "gradient"
    }
}

/// Negate a comparison operator.
fn negate_op(op: CmpOp) -> CmpOp {
    match op {
        CmpOp::Eq => CmpOp::Ne,
        CmpOp::Ne => CmpOp::Eq,
        CmpOp::Lt => CmpOp::Ge,
        CmpOp::Le => CmpOp::Gt,
        CmpOp::Gt => CmpOp::Le,
        CmpOp::Ge => CmpOp::Lt,
    }
}

/// Parse a simple SMTLIB2 comparison constraint.
/// Handles: "(> x 5)", "(= x 42)", "(< x -3)", "(>= x 0)", "(<= x 100)", "(!= x 7)"
/// Returns (op, variable_name, target_value) or None for complex constraints.
///
/// Note: assumes variable is always on the left (e.g. `(> x 5)`, not `(> 5 x)`).
fn parse_simple_comparison(s: &str) -> Option<(CmpOp, String, i64)> {
    let s = s.trim();
    let s = s.strip_prefix('(')?.strip_suffix(')')?;
    let parts: Vec<&str> = s.split_whitespace().collect();
    if parts.len() != 3 {
        return None;
    }

    let op = match parts[0] {
        "=" => CmpOp::Eq,
        ">" => CmpOp::Gt,
        ">=" => CmpOp::Ge,
        "<" => CmpOp::Lt,
        "<=" => CmpOp::Le,
        "!=" | "distinct" => CmpOp::Ne,
        _ => return None,
    };

    let var_name = parts[1].to_string();
    let target: i64 = parts[2].parse().ok()?;

    Some((op, var_name, target))
}

#[cfg(test)]
mod tests {
    use super::*;

    // Distance function tests
    #[test]
    fn distance_zero_means_solved() {
        assert_eq!(comparison_distance(CmpOp::Eq, 5, 5), 0.0);
        assert_eq!(comparison_distance(CmpOp::Lt, 3, 5), 0.0);
        assert_eq!(comparison_distance(CmpOp::Le, 5, 5), 0.0);
        assert_eq!(comparison_distance(CmpOp::Gt, 5, 3), 0.0);
        assert_eq!(comparison_distance(CmpOp::Ge, 5, 5), 0.0);
        assert_eq!(comparison_distance(CmpOp::Ne, 5, 6), 0.0);
    }

    #[test]
    fn distance_positive_means_unsatisfied() {
        assert!(comparison_distance(CmpOp::Eq, 5, 10) > 0.0);
        assert!(comparison_distance(CmpOp::Lt, 10, 5) > 0.0);
        assert!(comparison_distance(CmpOp::Gt, 5, 10) > 0.0);
        assert!(comparison_distance(CmpOp::Ne, 5, 5) > 0.0);
    }

    #[test]
    fn distance_eq_proportional() {
        let d1 = comparison_distance(CmpOp::Eq, 40, 42);
        let d2 = comparison_distance(CmpOp::Eq, 0, 1000);
        assert!(d2 > d1); // farther apart = larger distance
    }

    // Gradient solver tests
    #[test]
    fn gradient_solves_simple_equality() {
        let solver = GradientSolver::new(100);
        let result = solver.solve_comparison(CmpOp::Eq, 40, 42);
        assert_eq!(result, Some(42));
    }

    #[test]
    fn gradient_solves_from_zero() {
        let solver = GradientSolver::new(200);
        let result = solver.solve_comparison(CmpOp::Eq, 0, 42);
        assert_eq!(result, Some(42));
    }

    #[test]
    fn gradient_solves_less_than() {
        let solver = GradientSolver::new(100);
        let result = solver.solve_comparison(CmpOp::Lt, 15, 10);
        assert!(result.is_some());
        assert!(result.unwrap() < 10);
    }

    #[test]
    fn gradient_solves_greater_than() {
        let solver = GradientSolver::new(100);
        let result = solver.solve_comparison(CmpOp::Gt, 5, 10);
        assert!(result.is_some());
        assert!(result.unwrap() > 10);
    }

    #[test]
    fn gradient_already_satisfied() {
        let solver = GradientSolver::new(100);
        assert_eq!(solver.solve_comparison(CmpOp::Eq, 42, 42), Some(42));
        assert_eq!(solver.solve_comparison(CmpOp::Lt, 3, 10), Some(3));
    }

    #[test]
    fn gradient_ne_unsolvable_from_equal() {
        // Ne from equal values: step either direction works
        let solver = GradientSolver::new(100);
        let result = solver.solve_comparison(CmpOp::Ne, 5, 5);
        assert!(result.is_some());
        assert_ne!(result.unwrap(), 5);
    }

    // Parser tests
    #[test]
    fn parse_simple_eq() {
        let (op, var, val) = parse_simple_comparison("(= x 42)").unwrap();
        assert_eq!(op, CmpOp::Eq);
        assert_eq!(var, "x");
        assert_eq!(val, 42);
    }

    #[test]
    fn parse_simple_gt() {
        let (op, var, val) = parse_simple_comparison("(> y 5)").unwrap();
        assert_eq!(op, CmpOp::Gt);
        assert_eq!(var, "y");
        assert_eq!(val, 5);
    }

    #[test]
    fn parse_negative_value() {
        let (op, _, val) = parse_simple_comparison("(< x -3)").unwrap();
        assert_eq!(op, CmpOp::Lt);
        assert_eq!(val, -3);
    }

    #[test]
    fn parse_invalid_returns_none() {
        assert!(parse_simple_comparison("not a constraint").is_none());
        assert!(parse_simple_comparison("(and (> x 5) (< x 10))").is_none());
        assert!(parse_simple_comparison("").is_none());
    }

    // Solver trait tests
    #[test]
    fn solver_trait_simple_constraint() {
        let solver = GradientSolver::new(100);
        let result = solver.solve(&["(= x 42)".to_string()], false).unwrap();
        assert!(result.is_some());
        let seed = result.unwrap();
        let json: String = String::from_utf8(seed.data.to_vec()).unwrap();
        assert!(json.contains("42"));
    }

    #[test]
    fn solver_trait_negate_last() {
        let solver = GradientSolver::new(100);
        // Constraint says x > 5, but negate_last = true -> solve x <= 5
        let result = solver.solve(&["(> x 5)".to_string()], true).unwrap();
        assert!(result.is_some());
        let json: String = String::from_utf8(result.unwrap().data.to_vec()).unwrap();
        assert!(json.contains("\"x\""));
        // Parse value and verify it satisfies x <= 5
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        let val = parsed["x"].as_i64().unwrap();
        assert!(val <= 5, "negate_last should produce x <= 5, got x = {val}");
    }

    #[test]
    fn solver_trait_empty_constraints() {
        let solver = GradientSolver::new(100);
        let result = solver.solve(&[], false).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn solver_trait_complex_constraint_returns_none() {
        let solver = GradientSolver::new(100);
        let result = solver
            .solve(&["(and (> x 5) (< x 10))".to_string()], false)
            .unwrap();
        assert!(result.is_none()); // can't parse complex constraints
    }

    #[test]
    fn solver_name() {
        let solver = GradientSolver::new(100);
        assert_eq!(solver.name(), "gradient");
    }

    // Negate op tests
    #[test]
    fn negate_op_roundtrip() {
        for op in [
            CmpOp::Eq,
            CmpOp::Ne,
            CmpOp::Lt,
            CmpOp::Le,
            CmpOp::Gt,
            CmpOp::Ge,
        ] {
            assert_eq!(negate_op(negate_op(op)), op);
        }
    }
}
