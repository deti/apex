//! Rust condition parser for concolic execution.
//!
//! Extracts branch conditions from Rust source text and converts them
//! into [`ConditionTree`] IR for boundary seed generation.
//!
//! Supported constructs:
//! - `if x > 10`, `if x == 0`, etc. (numeric comparisons)
//! - `if let Some(v) = ...` / `if let Err(e) = ...`
//! - `.is_some()`, `.is_none()`, `.is_ok()`, `.is_err()`
//! - `match` guards with comparisons
//! - Logical: `&&`, `||`

use crate::condition_tree::{CompareOp, ConditionTree, Expr};

/// Parse Rust source and extract `(line_number, condition)` pairs.
pub fn parse_rust_conditions(source: &str) -> Vec<(u32, ConditionTree)> {
    let mut results = Vec::new();

    for (i, line) in source.lines().enumerate() {
        let line_num = (i + 1) as u32;
        let trimmed = line.trim();

        if let Some(tree) = try_parse_if_let(trimmed) {
            results.push((line_num, tree));
            continue;
        }

        if let Some(tree) = try_parse_option_check(trimmed) {
            results.push((line_num, tree));
            continue;
        }

        if let Some(tree) = try_parse_if_comparison(trimmed) {
            results.push((line_num, tree));
            continue;
        }

        if let Some(tree) = try_parse_match_guard(trimmed) {
            results.push((line_num, tree));
            continue;
        }
    }

    results
}

fn try_parse_if_let(line: &str) -> Option<ConditionTree> {
    if line.starts_with("if let Some(") || line.contains(" if let Some(") {
        let expr_name = extract_after_eq(line)?;
        return Some(ConditionTree::NullCheck {
            expr: Box::new(Expr::Variable(expr_name)),
            is_null: false,
        });
    }
    if line.starts_with("if let Err(") || line.contains(" if let Err(") {
        let expr_name = extract_after_eq(line)?;
        return Some(ConditionTree::TypeCheck {
            expr: Box::new(Expr::Variable(expr_name)),
            type_name: "Err".into(),
        });
    }
    if line.starts_with("if let Ok(") || line.contains(" if let Ok(") {
        let expr_name = extract_after_eq(line)?;
        return Some(ConditionTree::TypeCheck {
            expr: Box::new(Expr::Variable(expr_name)),
            type_name: "Ok".into(),
        });
    }
    if line.starts_with("if let None") || line.contains(" if let None") {
        let expr_name = extract_after_eq(line)?;
        return Some(ConditionTree::NullCheck {
            expr: Box::new(Expr::Variable(expr_name)),
            is_null: true,
        });
    }
    None
}

fn extract_after_eq(line: &str) -> Option<String> {
    let eq_pos = line.find('=')?;
    let rest = &line[eq_pos..];
    if rest.starts_with("==") {
        return None;
    }
    let after = line[eq_pos + 1..].trim();
    let after = after.trim_end_matches('{').trim();
    if after.is_empty() {
        return Some("expr".into());
    }
    Some(after.to_string())
}

fn try_parse_option_check(line: &str) -> Option<ConditionTree> {
    let trimmed = line.strip_prefix("if ")?.trim();
    let trimmed = trimmed.trim_end_matches('{').trim();

    if let Some(obj) = trimmed.strip_suffix(".is_some()") {
        return Some(ConditionTree::NullCheck {
            expr: Box::new(Expr::Variable(obj.trim().to_string())),
            is_null: false,
        });
    }
    if let Some(obj) = trimmed.strip_suffix(".is_none()") {
        return Some(ConditionTree::NullCheck {
            expr: Box::new(Expr::Variable(obj.trim().to_string())),
            is_null: true,
        });
    }
    if let Some(obj) = trimmed.strip_suffix(".is_ok()") {
        return Some(ConditionTree::TypeCheck {
            expr: Box::new(Expr::Variable(obj.trim().to_string())),
            type_name: "Ok".into(),
        });
    }
    if let Some(obj) = trimmed.strip_suffix(".is_err()") {
        return Some(ConditionTree::TypeCheck {
            expr: Box::new(Expr::Variable(obj.trim().to_string())),
            type_name: "Err".into(),
        });
    }
    None
}

fn try_parse_if_comparison(line: &str) -> Option<ConditionTree> {
    let cond_text = line.strip_prefix("if ")?.trim();
    let cond_text = cond_text.trim_end_matches('{').trim();

    if let Some(pos) = cond_text.find("&&") {
        let left = parse_simple_condition(cond_text[..pos].trim());
        let right = parse_simple_condition(cond_text[pos + 2..].trim());
        return Some(ConditionTree::And(Box::new(left), Box::new(right)));
    }
    if let Some(pos) = cond_text.find("||") {
        let left = parse_simple_condition(cond_text[..pos].trim());
        let right = parse_simple_condition(cond_text[pos + 2..].trim());
        return Some(ConditionTree::Or(Box::new(left), Box::new(right)));
    }

    let tree = parse_simple_condition(cond_text);
    if matches!(tree, ConditionTree::Unknown(_)) {
        return None;
    }
    Some(tree)
}

fn try_parse_match_guard(line: &str) -> Option<ConditionTree> {
    let if_pos = line.find(" if ")?;
    let arrow_pos = line.find("=>")?;
    if if_pos >= arrow_pos {
        return None;
    }
    let cond = &line[if_pos + 4..arrow_pos].trim();
    let tree = parse_simple_condition(cond);
    if matches!(tree, ConditionTree::Unknown(_)) {
        return None;
    }
    Some(tree)
}

fn parse_simple_condition(text: &str) -> ConditionTree {
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
            if *op_str == ">" && pos > 0 && text.as_bytes()[pos - 1] == b'=' {
                continue;
            }
            let left = text[..pos].trim();
            let right = text[pos + op_str.len()..].trim();
            if left.is_empty() || right.is_empty() {
                continue;
            }
            return ConditionTree::Compare {
                left: Box::new(parse_rust_expr(left)),
                op: *op,
                right: Box::new(parse_rust_expr(right)),
            };
        }
    }

    ConditionTree::Unknown(text.to_string())
}

fn parse_rust_expr(text: &str) -> Expr {
    let text = text.trim();

    if text == "None" || text == "null" {
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
    if let Some(dot_pos) = text.rfind('.') {
        let obj = &text[..dot_pos];
        let prop = &text[dot_pos + 1..];
        if !prop.is_empty() && !obj.is_empty() {
            return Expr::PropertyAccess {
                object: Box::new(parse_rust_expr(obj)),
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
    fn parse_rust_if_comparison() {
        let source = "fn f() {\n    if x > 10 {\n        do_thing();\n    }\n}";
        let conditions = parse_rust_conditions(source);
        assert!(!conditions.is_empty());
        assert_eq!(conditions[0].0, 2);
        assert!(matches!(conditions[0].1, ConditionTree::Compare { .. }));
    }

    #[test]
    fn parse_rust_if_let_some() {
        let source = "if let Some(v) = result {";
        let conditions = parse_rust_conditions(source);
        assert!(!conditions.is_empty());
        assert!(matches!(
            conditions[0].1,
            ConditionTree::NullCheck { is_null: false, .. }
        ));
    }

    #[test]
    fn parse_rust_is_none() {
        let source = "fn f() {\n    if val.is_none() {\n    }\n}";
        let conditions = parse_rust_conditions(source);
        assert!(!conditions.is_empty());
        assert!(matches!(
            conditions[0].1,
            ConditionTree::NullCheck { is_null: true, .. }
        ));
    }

    #[test]
    fn parse_rust_is_err() {
        let source = "if result.is_err() {";
        let conditions = parse_rust_conditions(source);
        assert!(!conditions.is_empty());
        assert!(matches!(conditions[0].1, ConditionTree::TypeCheck { .. }));
    }

    #[test]
    fn parse_rust_match_guard() {
        let source = "    Some(x) if x > 5 => do_thing(),";
        let conditions = parse_rust_conditions(source);
        assert!(!conditions.is_empty());
        assert!(matches!(conditions[0].1, ConditionTree::Compare { .. }));
    }

    #[test]
    fn parse_rust_logical_and() {
        let source = "if x > 0 && y < 100 {";
        let conditions = parse_rust_conditions(source);
        assert!(!conditions.is_empty());
        assert!(matches!(conditions[0].1, ConditionTree::And(_, _)));
    }
}
