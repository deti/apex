//! Java/Kotlin condition parser for concolic execution.
//!
//! Extracts branch conditions from Java/Kotlin source text and converts them
//! into [`ConditionTree`] IR for boundary seed generation.
//!
//! Supported constructs:
//! - `instanceof` type checks
//! - `.equals()` comparisons
//! - `switch` / `case` values
//! - `== null` / `!= null` checks
//! - Standard comparisons

use crate::condition_tree::{CompareOp, ConditionTree, Expr};

/// Parse Java/Kotlin source and extract `(line_number, condition)` pairs.
pub fn parse_java_conditions(source: &str) -> Vec<(u32, ConditionTree)> {
    let mut results = Vec::new();

    for (i, line) in source.lines().enumerate() {
        let line_num = (i + 1) as u32;
        let trimmed = line.trim();

        // instanceof
        if let Some(tree) = try_parse_instanceof(trimmed) {
            results.push((line_num, tree));
            continue;
        }

        // .equals()
        if let Some(tree) = try_parse_equals(trimmed) {
            results.push((line_num, tree));
            continue;
        }

        // null checks
        if let Some(tree) = try_parse_null_check(trimmed) {
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

fn try_parse_instanceof(line: &str) -> Option<ConditionTree> {
    let cond = extract_if_condition(line)?;
    let pos = cond.find(" instanceof ")?;
    let expr = cond[..pos].trim();
    let type_name = cond[pos + " instanceof ".len()..].trim();
    // Strip trailing `)` or `{` if present
    let type_name = type_name.trim_end_matches(')').trim_end_matches('{').trim();
    if expr.is_empty() || type_name.is_empty() {
        return None;
    }
    Some(ConditionTree::TypeCheck {
        expr: Box::new(Expr::Variable(expr.to_string())),
        type_name: type_name.to_string(),
    })
}

fn try_parse_equals(line: &str) -> Option<ConditionTree> {
    let cond = extract_if_condition(line)?;
    // x.equals("value") or "value".equals(x)
    let equals_pos = cond.find(".equals(")?;
    let paren_close = cond[equals_pos..].find(')')? + equals_pos;

    let obj = cond[..equals_pos].trim();
    let arg = cond[equals_pos + ".equals(".len()..paren_close].trim();

    // Check for negation: !x.equals(...)
    let (obj, negated) = if let Some(stripped) = obj.strip_prefix('!') {
        (stripped.trim(), true)
    } else {
        (obj, false)
    };

    let tree = ConditionTree::Compare {
        left: Box::new(parse_java_expr(obj)),
        op: CompareOp::Eq,
        right: Box::new(parse_java_expr(arg)),
    };

    if negated {
        Some(ConditionTree::Not(Box::new(tree)))
    } else {
        Some(tree)
    }
}

fn try_parse_null_check(line: &str) -> Option<ConditionTree> {
    let cond = extract_if_condition(line)?;

    // x == null
    if let Some(pos) = cond.find("== null") {
        let rest_after = cond[pos + 7..].trim();
        // Make sure it's exactly `null` and not `nullptr` etc.
        if rest_after.is_empty() || rest_after.starts_with(')') || rest_after.starts_with('{') {
            let var = cond[..pos].trim();
            if !var.is_empty() {
                return Some(ConditionTree::NullCheck {
                    expr: Box::new(Expr::Variable(var.to_string())),
                    is_null: true,
                });
            }
        }
    }
    // x != null
    if let Some(pos) = cond.find("!= null") {
        let rest_after = cond[pos + 7..].trim();
        if rest_after.is_empty() || rest_after.starts_with(')') || rest_after.starts_with('{') {
            let var = cond[..pos].trim();
            if !var.is_empty() {
                return Some(ConditionTree::NullCheck {
                    expr: Box::new(Expr::Variable(var.to_string())),
                    is_null: false,
                });
            }
        }
    }

    None
}

fn try_parse_if_comparison(line: &str) -> Option<ConditionTree> {
    let cond = extract_if_condition(line)?;

    // Skip null/instanceof (handled elsewhere)
    if cond.contains("null") || cond.contains(" instanceof ") || cond.contains(".equals(") {
        return None;
    }

    let tree = parse_simple_java_condition(&cond);
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
    Some(ConditionTree::Compare {
        left: Box::new(Expr::Variable("switch_expr".into())),
        op: CompareOp::Eq,
        right: Box::new(parse_java_expr(case_val)),
    })
}

fn extract_if_condition(line: &str) -> Option<String> {
    // Java: `if (condition)` or `} else if (condition)`
    let rest = if let Some(r) = line.strip_prefix("if (") {
        r
    } else if let Some(r) = line.strip_prefix("if(") {
        r
    } else if let Some(r) = line.strip_prefix("} else if (") {
        r
    } else if let Some(r) = line.strip_prefix("} else if(") {
        r
    } else if let Some(r) = line.strip_prefix("else if (") {
        r
    } else if let Some(r) = line.strip_prefix("else if(") {
        r
    } else {
        return None;
    };

    // Find matching closing paren
    let mut depth = 1i32;
    let mut end = 0;
    for (j, ch) in rest.chars().enumerate() {
        match ch {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    end = j;
                    break;
                }
            }
            _ => {}
        }
    }
    if end == 0 && depth != 0 {
        // No matching paren found — take everything up to `{`
        let trimmed = rest.trim_end_matches('{').trim_end_matches(')').trim();
        return Some(trimmed.to_string());
    }

    Some(rest[..end].trim().to_string())
}

fn parse_simple_java_condition(text: &str) -> ConditionTree {
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
                left: Box::new(parse_java_expr(left)),
                op: *op,
                right: Box::new(parse_java_expr(right)),
            };
        }
    }

    ConditionTree::Unknown(text.to_string())
}

fn parse_java_expr(text: &str) -> Expr {
    let text = text.trim();

    if text == "null" {
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
                object: Box::new(parse_java_expr(obj)),
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
    fn parse_java_instanceof() {
        let source = "if (obj instanceof String) {";
        let conditions = parse_java_conditions(source);
        assert!(!conditions.is_empty());
        assert!(matches!(conditions[0].1, ConditionTree::TypeCheck { .. }));
    }

    #[test]
    fn parse_java_equals() {
        let source = r#"if (name.equals("admin")) {"#;
        let conditions = parse_java_conditions(source);
        assert!(!conditions.is_empty());
        assert!(matches!(conditions[0].1, ConditionTree::Compare { .. }));
    }

    #[test]
    fn parse_java_null_check() {
        let source = "if (result == null) {";
        let conditions = parse_java_conditions(source);
        assert!(!conditions.is_empty());
        assert!(matches!(
            conditions[0].1,
            ConditionTree::NullCheck { is_null: true, .. }
        ));
    }

    #[test]
    fn parse_java_not_null() {
        let source = "if (result != null) {";
        let conditions = parse_java_conditions(source);
        assert!(!conditions.is_empty());
        assert!(matches!(
            conditions[0].1,
            ConditionTree::NullCheck { is_null: false, .. }
        ));
    }

    #[test]
    fn parse_java_case() {
        let source = r#"    case "admin":"#;
        let conditions = parse_java_conditions(source);
        assert!(!conditions.is_empty());
        assert!(matches!(conditions[0].1, ConditionTree::Compare { .. }));
    }

    #[test]
    fn parse_java_comparison() {
        let source = "if (x > 10) {";
        let conditions = parse_java_conditions(source);
        assert!(!conditions.is_empty());
        assert!(matches!(conditions[0].1, ConditionTree::Compare { .. }));
    }
}
