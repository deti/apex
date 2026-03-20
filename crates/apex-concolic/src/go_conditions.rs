//! Go condition parser for concolic execution.
//!
//! Extracts branch conditions from Go source text and converts them
//! into [`ConditionTree`] IR for boundary seed generation.
//!
//! Supported constructs:
//! - `if err != nil` / `if err == nil`
//! - `if len(x) > 0` and other `len()` comparisons
//! - `switch` / `case` values
//! - Standard comparisons: `if x > 10`

use crate::condition_tree::{CompareOp, ConditionTree, Expr};

/// Parse Go source and extract `(line_number, condition)` pairs.
pub fn parse_go_conditions(source: &str) -> Vec<(u32, ConditionTree)> {
    let mut results = Vec::new();

    for (i, line) in source.lines().enumerate() {
        let line_num = (i + 1) as u32;
        let trimmed = line.trim();

        // if err != nil / if err == nil
        if let Some(tree) = try_parse_nil_check(trimmed) {
            results.push((line_num, tree));
            continue;
        }

        // if len(x) <op> n
        if let Some(tree) = try_parse_len_check(trimmed) {
            results.push((line_num, tree));
            continue;
        }

        // Standard if comparison
        if let Some(tree) = try_parse_if_comparison(trimmed) {
            results.push((line_num, tree));
            continue;
        }

        // switch/case
        if let Some(tree) = try_parse_case(trimmed) {
            results.push((line_num, tree));
            continue;
        }
    }

    results
}

fn try_parse_nil_check(line: &str) -> Option<ConditionTree> {
    let cond = extract_if_condition(line)?;

    // err != nil
    if let Some(pos) = cond.find("!= nil") {
        let var = cond[..pos].trim();
        if !var.is_empty() {
            return Some(ConditionTree::NullCheck {
                expr: Box::new(Expr::Variable(var.to_string())),
                is_null: false,
            });
        }
    }
    // err == nil
    if let Some(pos) = cond.find("== nil") {
        let var = cond[..pos].trim();
        if !var.is_empty() {
            return Some(ConditionTree::NullCheck {
                expr: Box::new(Expr::Variable(var.to_string())),
                is_null: true,
            });
        }
    }

    None
}

fn try_parse_len_check(line: &str) -> Option<ConditionTree> {
    let cond = extract_if_condition(line)?;

    // len(x) <op> n
    if !cond.starts_with("len(") {
        return None;
    }
    let paren_close = cond.find(')')?;
    let var = &cond[4..paren_close];
    let rest = cond[paren_close + 1..].trim();

    for (op_str, op) in &[
        (">=", CompareOp::GtEq),
        ("<=", CompareOp::LtEq),
        ("!=", CompareOp::NotEq),
        ("==", CompareOp::Eq),
        (">", CompareOp::Gt),
        ("<", CompareOp::Lt),
    ] {
        if let Some(pos) = rest.find(op_str) {
            let val = rest[pos + op_str.len()..]
                .trim()
                .trim_end_matches('{')
                .trim();
            return Some(ConditionTree::LengthCheck {
                expr: Box::new(Expr::Variable(var.trim().to_string())),
                op: *op,
                value: Box::new(parse_go_expr(val)),
            });
        }
    }

    None
}

fn try_parse_if_comparison(line: &str) -> Option<ConditionTree> {
    let cond = extract_if_condition(line)?;

    // Skip nil checks (handled above)
    if cond.contains("nil") {
        return None;
    }
    // Skip len checks (handled above)
    if cond.starts_with("len(") {
        return None;
    }

    // Handle && and ||
    if let Some(pos) = cond.find("&&") {
        let left = parse_simple_go_condition(&cond[..pos]);
        let right = parse_simple_go_condition(&cond[pos + 2..]);
        return Some(ConditionTree::And(Box::new(left), Box::new(right)));
    }
    if let Some(pos) = cond.find("||") {
        let left = parse_simple_go_condition(&cond[..pos]);
        let right = parse_simple_go_condition(&cond[pos + 2..]);
        return Some(ConditionTree::Or(Box::new(left), Box::new(right)));
    }

    let tree = parse_simple_go_condition(&cond);
    if matches!(tree, ConditionTree::Unknown(_)) {
        return None;
    }
    Some(tree)
}

fn try_parse_case(line: &str) -> Option<ConditionTree> {
    let case_val = line.strip_prefix("case ")?.trim_end_matches(':').trim();
    if case_val.is_empty() || case_val == "default" {
        return None;
    }
    // case "string_value":
    Some(ConditionTree::Compare {
        left: Box::new(Expr::Variable("switch_expr".into())),
        op: CompareOp::Eq,
        right: Box::new(parse_go_expr(case_val)),
    })
}

fn extract_if_condition(line: &str) -> Option<String> {
    // Go: `if cond {` or `if init; cond {` or `} else if cond {`
    let rest = if let Some(r) = line.strip_prefix("if ") {
        r
    } else if let Some(r) = line.strip_prefix("} else if ") {
        r
    } else {
        return None;
    };
    let rest = rest.trim_end_matches('{').trim();

    // Handle short-variable declaration: `if err := call(); err != nil`
    if let Some(semi_pos) = rest.find(';') {
        let cond = rest[semi_pos + 1..].trim();
        return Some(cond.to_string());
    }

    Some(rest.to_string())
}

fn parse_simple_go_condition(text: &str) -> ConditionTree {
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
            if right == "nil" {
                return ConditionTree::NullCheck {
                    expr: Box::new(Expr::Variable(left.to_string())),
                    is_null: *op == CompareOp::Eq,
                };
            }
            return ConditionTree::Compare {
                left: Box::new(parse_go_expr(left)),
                op: *op,
                right: Box::new(parse_go_expr(right)),
            };
        }
    }

    ConditionTree::Unknown(text.to_string())
}

fn parse_go_expr(text: &str) -> Expr {
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
                object: Box::new(parse_go_expr(obj)),
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
    fn parse_go_err_nil() {
        let source = "if err != nil {\n    return err\n}";
        let conditions = parse_go_conditions(source);
        assert!(!conditions.is_empty());
        assert_eq!(conditions[0].0, 1);
        assert!(matches!(
            conditions[0].1,
            ConditionTree::NullCheck { is_null: false, .. }
        ));
    }

    #[test]
    fn parse_go_err_eq_nil() {
        let source = "if err == nil {";
        let conditions = parse_go_conditions(source);
        assert!(!conditions.is_empty());
        assert!(matches!(
            conditions[0].1,
            ConditionTree::NullCheck { is_null: true, .. }
        ));
    }

    #[test]
    fn parse_go_len_check() {
        let source = "if len(items) > 0 {";
        let conditions = parse_go_conditions(source);
        assert!(!conditions.is_empty());
        assert!(matches!(conditions[0].1, ConditionTree::LengthCheck { .. }));
    }

    #[test]
    fn parse_go_comparison() {
        let source = "if count >= 10 {";
        let conditions = parse_go_conditions(source);
        assert!(!conditions.is_empty());
        assert!(matches!(conditions[0].1, ConditionTree::Compare { .. }));
    }

    #[test]
    fn parse_go_case() {
        let source = "    case \"admin\":";
        let conditions = parse_go_conditions(source);
        assert!(!conditions.is_empty());
        assert!(matches!(conditions[0].1, ConditionTree::Compare { .. }));
    }

    #[test]
    fn parse_go_short_var_decl() {
        let source = "if err := doSomething(); err != nil {";
        let conditions = parse_go_conditions(source);
        assert!(!conditions.is_empty());
        assert!(matches!(
            conditions[0].1,
            ConditionTree::NullCheck { is_null: false, .. }
        ));
    }
}
