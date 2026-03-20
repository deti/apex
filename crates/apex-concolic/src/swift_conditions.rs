//! Swift condition parser for concolic execution.
//!
//! Extracts branch conditions from Swift source text and converts them
//! into [`ConditionTree`] IR for boundary seed generation.
//!
//! Supported constructs:
//! - `if let` optional binding
//! - `guard let` optional binding
//! - `case .enumCase` pattern matching
//! - Standard comparisons
//! - `== nil` / `!= nil`

use crate::condition_tree::{CompareOp, ConditionTree, Expr};

/// Parse Swift source and extract `(line_number, condition)` pairs.
pub fn parse_swift_conditions(source: &str) -> Vec<(u32, ConditionTree)> {
    let mut results = Vec::new();

    for (i, line) in source.lines().enumerate() {
        let line_num = (i + 1) as u32;
        let trimmed = line.trim();

        // if let / guard let
        if let Some(tree) = try_parse_optional_binding(trimmed) {
            results.push((line_num, tree));
            continue;
        }

        // nil checks
        if let Some(tree) = try_parse_nil_check(trimmed) {
            results.push((line_num, tree));
            continue;
        }

        // case .enumCase
        if let Some(tree) = try_parse_case_enum(trimmed) {
            results.push((line_num, tree));
            continue;
        }

        // Standard if/guard comparison
        if let Some(tree) = try_parse_if_comparison(trimmed) {
            results.push((line_num, tree));
            continue;
        }
    }

    results
}

fn try_parse_optional_binding(line: &str) -> Option<ConditionTree> {
    // if let x = expr  or  guard let x = expr else
    let rest = if let Some(r) = line.strip_prefix("if let ") {
        r
    } else if let Some(r) = line.strip_prefix("guard let ") {
        r
    } else {
        return None;
    };

    // Find `=` that's the binding (not `==`)
    let eq_pos = rest.find('=')?;
    if rest[eq_pos..].starts_with("==") {
        return None;
    }
    let expr = rest[eq_pos + 1..].trim();
    let expr = expr.trim_end_matches('{').trim_end_matches("else").trim();

    if expr.is_empty() {
        return Some(ConditionTree::NullCheck {
            expr: Box::new(Expr::Variable("binding".into())),
            is_null: false,
        });
    }

    Some(ConditionTree::NullCheck {
        expr: Box::new(Expr::Variable(expr.to_string())),
        is_null: false,
    })
}

fn try_parse_nil_check(line: &str) -> Option<ConditionTree> {
    let cond = extract_swift_condition(line)?;

    if let Some(pos) = cond.find("== nil") {
        let var = cond[..pos].trim();
        if !var.is_empty() {
            return Some(ConditionTree::NullCheck {
                expr: Box::new(Expr::Variable(var.to_string())),
                is_null: true,
            });
        }
    }
    if let Some(pos) = cond.find("!= nil") {
        let var = cond[..pos].trim();
        if !var.is_empty() {
            return Some(ConditionTree::NullCheck {
                expr: Box::new(Expr::Variable(var.to_string())),
                is_null: false,
            });
        }
    }

    None
}

fn try_parse_case_enum(line: &str) -> Option<ConditionTree> {
    // case .enumCase: or case .enumCase(let x):
    let case_val = line.strip_prefix("case ")?;
    let case_val = case_val.trim_end_matches(':').trim();

    if !case_val.starts_with('.') {
        return None;
    }

    // Extract enum case name
    let name = case_val
        .trim_start_matches('.')
        .split('(')
        .next()
        .unwrap_or(case_val.trim_start_matches('.'));

    if name.is_empty() {
        return None;
    }

    Some(ConditionTree::TypeCheck {
        expr: Box::new(Expr::Variable("match_expr".into())),
        type_name: name.to_string(),
    })
}

fn try_parse_if_comparison(line: &str) -> Option<ConditionTree> {
    let cond = extract_swift_condition(line)?;

    if cond.contains("nil") {
        return None;
    }

    // Handle , as && in Swift (multiple conditions)
    if let Some(pos) = cond.find(", ") {
        let left = parse_simple_swift_condition(&cond[..pos]);
        let right = parse_simple_swift_condition(&cond[pos + 2..]);
        if !matches!(left, ConditionTree::Unknown(_)) || !matches!(right, ConditionTree::Unknown(_))
        {
            return Some(ConditionTree::And(Box::new(left), Box::new(right)));
        }
    }

    let tree = parse_simple_swift_condition(&cond);
    if matches!(tree, ConditionTree::Unknown(_)) {
        return None;
    }
    Some(tree)
}

fn extract_swift_condition(line: &str) -> Option<String> {
    // Swift: `if condition {` or `guard condition else {`
    let rest = if let Some(r) = line.strip_prefix("if ") {
        // Skip `if let`
        if r.starts_with("let ") {
            return None;
        }
        r
    } else if let Some(r) = line.strip_prefix("guard ") {
        if r.starts_with("let ") {
            return None;
        }
        r
    } else if let Some(r) = line.strip_prefix("} else if ") {
        if r.starts_with("let ") {
            return None;
        }
        r
    } else {
        return None;
    };

    let rest = rest.trim_end_matches('{').trim_end_matches("else").trim();

    Some(rest.to_string())
}

fn parse_simple_swift_condition(text: &str) -> ConditionTree {
    let text = text.trim();

    for (op_str, op) in &[
        (">=", CompareOp::GtEq),
        ("<=", CompareOp::LtEq),
        ("!=", CompareOp::NotEq),
        ("==", CompareOp::Eq),
        (">", CompareOp::Gt),
        ("<", CompareOp::Lt),
    ] {
        if let Some(pos) = text.find(op_str) {
            let left = text[..pos].trim();
            let right = text[pos + op_str.len()..].trim();
            if left.is_empty() || right.is_empty() {
                continue;
            }
            return ConditionTree::Compare {
                left: Box::new(parse_swift_expr(left)),
                op: *op,
                right: Box::new(parse_swift_expr(right)),
            };
        }
    }

    ConditionTree::Unknown(text.to_string())
}

fn parse_swift_expr(text: &str) -> Expr {
    let text = text.trim();

    if text == "nil" {
        return Expr::Null;
    }
    if text == "true" {
        return Expr::BoolLiteral(true);
    }
    if text == "false" {
        return Expr::BoolLiteral(false);
    }
    if let Ok(n) = text.parse::<i64>() {
        return Expr::IntLiteral(n);
    }
    if let Ok(f) = text.parse::<f64>() {
        return Expr::FloatLiteral(f);
    }
    if text.len() >= 2 && text.starts_with('"') && text.ends_with('"') {
        return Expr::StringLiteral(text[1..text.len() - 1].to_string());
    }
    if text.ends_with(')') {
        return Expr::Call(text.to_string());
    }
    if let Some(dot) = text.rfind('.') {
        let obj = &text[..dot];
        let prop = &text[dot + 1..];
        if !prop.is_empty() && !obj.is_empty() {
            return Expr::PropertyAccess {
                object: Box::new(parse_swift_expr(obj)),
                property: prop.to_string(),
            };
        }
    }

    Expr::Variable(text.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_swift_if_let() {
        let source = "if let value = optional {";
        let conditions = parse_swift_conditions(source);
        assert!(!conditions.is_empty());
        assert!(matches!(
            conditions[0].1,
            ConditionTree::NullCheck { is_null: false, .. }
        ));
    }

    #[test]
    fn parse_swift_guard_let() {
        let source = "guard let value = optional else {";
        let conditions = parse_swift_conditions(source);
        assert!(!conditions.is_empty());
        assert!(matches!(
            conditions[0].1,
            ConditionTree::NullCheck { is_null: false, .. }
        ));
    }

    #[test]
    fn parse_swift_nil_check() {
        let source = "if result == nil {";
        let conditions = parse_swift_conditions(source);
        assert!(!conditions.is_empty());
        assert!(matches!(
            conditions[0].1,
            ConditionTree::NullCheck { is_null: true, .. }
        ));
    }

    #[test]
    fn parse_swift_case_enum() {
        let source = "    case .success:";
        let conditions = parse_swift_conditions(source);
        assert!(!conditions.is_empty());
        assert!(matches!(conditions[0].1, ConditionTree::TypeCheck { .. }));
    }

    #[test]
    fn parse_swift_comparison() {
        let source = "if count > 10 {";
        let conditions = parse_swift_conditions(source);
        assert!(!conditions.is_empty());
        assert!(matches!(conditions[0].1, ConditionTree::Compare { .. }));
    }

    #[test]
    fn parse_swift_case_enum_with_binding() {
        let source = "    case .failure(let error):";
        let conditions = parse_swift_conditions(source);
        assert!(!conditions.is_empty());
        assert!(matches!(conditions[0].1, ConditionTree::TypeCheck { .. }));
    }
}
