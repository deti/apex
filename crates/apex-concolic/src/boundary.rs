//! Boundary seed generator — derives concrete test values near decision
//! boundaries from a [`ConditionTree`].
//!
//! Given a condition like `x > 10`, this module emits values at and around the
//! boundary (e.g. 9, 10, 11) so the fuzzer can quickly flip the branch.

use crate::condition_tree::{CompareOp, ConditionTree, Expr};

/// Generate concrete boundary values for the given condition tree node.
///
/// Returns string representations of values that are likely to toggle
/// the branch decision (e.g. for `x > 10` we return `["9", "10", "11"]`).
pub fn boundary_values(tree: &ConditionTree) -> Vec<String> {
    match tree {
        ConditionTree::Compare { left: _, op, right } => boundary_for_compare(*op, right),
        ConditionTree::NullCheck { .. } => {
            vec!["null".into(), "0".into(), "\"\"".into()]
        }
        ConditionTree::LengthCheck { op, value, .. } => boundary_for_compare(*op, value),
        ConditionTree::TypeCheck { type_name, .. } => type_boundary_values(type_name),
        ConditionTree::Contains { needle, .. } => match needle.as_ref() {
            Expr::StringLiteral(s) => vec![s.clone(), format!("NOT_{s}")],
            Expr::IntLiteral(n) => vec![n.to_string(), (n + 1).to_string()],
            _ => vec!["present".into(), "absent".into()],
        },
        ConditionTree::And(left, right) => {
            let mut vals = boundary_values(left);
            vals.extend(boundary_values(right));
            vals.dedup();
            vals
        }
        ConditionTree::Or(left, right) => {
            let mut vals = boundary_values(left);
            vals.extend(boundary_values(right));
            vals.dedup();
            vals
        }
        ConditionTree::Not(inner) => boundary_values(inner),
        ConditionTree::Unknown(_) => vec![],
    }
}

fn boundary_for_compare(op: CompareOp, rhs: &Expr) -> Vec<String> {
    match rhs {
        Expr::IntLiteral(n) => int_boundary(op, *n),
        Expr::FloatLiteral(f) => float_boundary(op, *f),
        Expr::StringLiteral(s) => vec![s.clone(), String::new(), format!("{s}z")],
        Expr::BoolLiteral(b) => vec![b.to_string(), (!b).to_string()],
        Expr::Null => vec!["null".into(), "0".into()],
        _ => vec![],
    }
}

fn int_boundary(op: CompareOp, n: i64) -> Vec<String> {
    match op {
        CompareOp::Gt => vec![n.to_string(), (n + 1).to_string(), (n - 1).to_string()],
        CompareOp::GtEq => vec![n.to_string(), (n - 1).to_string(), (n + 1).to_string()],
        CompareOp::Lt => vec![n.to_string(), (n - 1).to_string(), (n + 1).to_string()],
        CompareOp::LtEq => vec![n.to_string(), (n + 1).to_string(), (n - 1).to_string()],
        CompareOp::Eq => vec![n.to_string(), (n + 1).to_string(), (n - 1).to_string()],
        CompareOp::NotEq => vec![n.to_string(), (n + 1).to_string(), (n - 1).to_string()],
    }
}

fn float_boundary(op: CompareOp, f: f64) -> Vec<String> {
    let eps = 0.001;
    match op {
        CompareOp::Gt | CompareOp::GtEq => {
            vec![
                format!("{f}"),
                format!("{}", f + eps),
                format!("{}", f - eps),
            ]
        }
        CompareOp::Lt | CompareOp::LtEq => {
            vec![
                format!("{f}"),
                format!("{}", f - eps),
                format!("{}", f + eps),
            ]
        }
        CompareOp::Eq | CompareOp::NotEq => {
            vec![
                format!("{f}"),
                format!("{}", f + eps),
                format!("{}", f - eps),
            ]
        }
    }
}

fn type_boundary_values(type_name: &str) -> Vec<String> {
    match type_name {
        "string" | "String" | "str" => {
            vec!["\"test\"".into(), "0".into(), "null".into(), "true".into()]
        }
        "number" | "int" | "i32" | "i64" | "f64" | "Int" => {
            vec!["0".into(), "\"text\"".into(), "null".into(), "true".into()]
        }
        "boolean" | "bool" | "Bool" => {
            vec!["true".into(), "false".into(), "0".into(), "null".into()]
        }
        _ => vec!["null".into(), "0".into(), "\"\"".into()],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn boundary_int_gt() {
        let tree = ConditionTree::Compare {
            left: Box::new(Expr::Variable("x".into())),
            op: CompareOp::Gt,
            right: Box::new(Expr::IntLiteral(10)),
        };
        let vals = boundary_values(&tree);
        assert!(vals.contains(&"10".to_string()));
        assert!(vals.contains(&"11".to_string()));
        assert!(vals.contains(&"9".to_string()));
    }

    #[test]
    fn boundary_int_eq() {
        let tree = ConditionTree::Compare {
            left: Box::new(Expr::Variable("x".into())),
            op: CompareOp::Eq,
            right: Box::new(Expr::IntLiteral(0)),
        };
        let vals = boundary_values(&tree);
        assert!(vals.contains(&"0".to_string()));
        assert!(vals.contains(&"1".to_string()));
        assert!(vals.contains(&"-1".to_string()));
    }

    #[test]
    fn boundary_null_check() {
        let tree = ConditionTree::NullCheck {
            expr: Box::new(Expr::Variable("x".into())),
            is_null: true,
        };
        let vals = boundary_values(&tree);
        assert!(!vals.is_empty());
        assert!(vals.contains(&"null".to_string()));
    }

    #[test]
    fn boundary_type_check_string() {
        let tree = ConditionTree::TypeCheck {
            expr: Box::new(Expr::Variable("x".into())),
            type_name: "string".into(),
        };
        let vals = boundary_values(&tree);
        assert!(!vals.is_empty());
        assert!(vals.len() >= 3);
    }

    #[test]
    fn boundary_unknown_empty() {
        let tree = ConditionTree::Unknown("complex()".into());
        let vals = boundary_values(&tree);
        assert!(vals.is_empty());
    }

    #[test]
    fn boundary_and_combines() {
        let left = ConditionTree::Compare {
            left: Box::new(Expr::Variable("x".into())),
            op: CompareOp::Gt,
            right: Box::new(Expr::IntLiteral(0)),
        };
        let right = ConditionTree::Compare {
            left: Box::new(Expr::Variable("y".into())),
            op: CompareOp::Lt,
            right: Box::new(Expr::IntLiteral(100)),
        };
        let tree = ConditionTree::And(Box::new(left), Box::new(right));
        let vals = boundary_values(&tree);
        assert!(vals.len() >= 4);
    }
}
