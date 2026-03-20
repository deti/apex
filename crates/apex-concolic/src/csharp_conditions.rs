//! C# condition parser for concolic execution.
//!
//! Extracts branch conditions from C# source text and converts them
//! into [`ConditionTree`] IR for boundary seed generation.
//!
//! Supported constructs:
//! - `is Type` pattern matching
//! - `?.` null-conditional operator
//! - `??` null-coalescing operator
//! - `== null` / `!= null`
//! - Standard comparisons

use crate::condition_tree::{CompareOp, ConditionTree, Expr};

/// Parse C# source and extract `(line_number, condition)` pairs.
pub fn parse_csharp_conditions(source: &str) -> Vec<(u32, ConditionTree)> {
    let mut results = Vec::new();

    for (i, line) in source.lines().enumerate() {
        let line_num = (i + 1) as u32;
        let trimmed = line.trim();

        // `is Type` pattern matching
        if let Some(tree) = try_parse_is_type(trimmed) {
            results.push((line_num, tree));
            continue;
        }

        // null-conditional: x?.Method()
        if let Some(tree) = try_parse_null_conditional(trimmed) {
            results.push((line_num, tree));
            continue;
        }

        // null-coalescing: x ?? default
        if let Some(tree) = try_parse_null_coalescing(trimmed) {
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

fn try_parse_is_type(line: &str) -> Option<ConditionTree> {
    let cond = extract_if_condition(line)?;
    // x is Type or x is not Type
    let is_pos = cond.find(" is ")?;
    let expr = cond[..is_pos].trim();
    let type_part = cond[is_pos + 4..].trim();

    if expr.is_empty() {
        return None;
    }

    // `x is not Type`
    if let Some(type_name) = type_part.strip_prefix("not ") {
        let type_name = type_name
            .split_whitespace()
            .next()
            .unwrap_or(type_name.trim());
        return Some(ConditionTree::Not(Box::new(ConditionTree::TypeCheck {
            expr: Box::new(Expr::Variable(expr.to_string())),
            type_name: type_name.to_string(),
        })));
    }

    // `x is Type` (possibly with variable binding: `x is Type name`)
    let type_name = type_part.split_whitespace().next().unwrap_or(type_part);
    if type_name == "null" {
        return Some(ConditionTree::NullCheck {
            expr: Box::new(Expr::Variable(expr.to_string())),
            is_null: true,
        });
    }
    Some(ConditionTree::TypeCheck {
        expr: Box::new(Expr::Variable(expr.to_string())),
        type_name: type_name.to_string(),
    })
}

fn try_parse_null_conditional(line: &str) -> Option<ConditionTree> {
    // Look for `?.` in an if-condition context
    let cond = extract_if_condition(line)?;
    if !cond.contains("?.") {
        return None;
    }
    // If there's a comparison operator, don't handle here
    for op in &["==", "!=", ">=", "<=", ">", "<"] {
        if cond.contains(op) {
            return None;
        }
    }
    let dot_pos = cond.find("?.")?;
    let obj = cond[..dot_pos].trim();
    if obj.is_empty() {
        return None;
    }
    Some(ConditionTree::NullCheck {
        expr: Box::new(Expr::Variable(obj.to_string())),
        is_null: false,
    })
}

fn try_parse_null_coalescing(line: &str) -> Option<ConditionTree> {
    // x ?? default — implies null check on x
    if !line.contains("??") {
        return None;
    }
    let qq_pos = line.find("??")?;
    // Don't match inside comments or strings (simple heuristic)
    if line.trim_start().starts_with("//") {
        return None;
    }
    let left = line[..qq_pos].trim();
    // Extract the variable name (last token before ??)
    let var = left
        .rsplit_once(['=', '(', ','])
        .map(|(_, v)| v.trim())
        .unwrap_or(left);
    if var.is_empty() {
        return None;
    }
    Some(ConditionTree::NullCheck {
        expr: Box::new(Expr::Variable(var.to_string())),
        is_null: false, // ?? means "use this if non-null"
    })
}

fn try_parse_null_check(line: &str) -> Option<ConditionTree> {
    let cond = extract_if_condition(line)?;

    if let Some(pos) = cond.find("== null") {
        let var = cond[..pos].trim();
        if !var.is_empty() {
            return Some(ConditionTree::NullCheck {
                expr: Box::new(Expr::Variable(var.to_string())),
                is_null: true,
            });
        }
    }
    if let Some(pos) = cond.find("!= null") {
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

fn try_parse_if_comparison(line: &str) -> Option<ConditionTree> {
    let cond = extract_if_condition(line)?;

    if cond.contains("null") || cond.contains(" is ") || cond.contains("?.") {
        return None;
    }

    let tree = parse_simple_csharp_condition(&cond);
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
        right: Box::new(parse_csharp_expr(case_val)),
    })
}

fn extract_if_condition(line: &str) -> Option<String> {
    let rest = if let Some(r) = line.strip_prefix("if (") {
        r
    } else if let Some(r) = line.strip_prefix("if(") {
        r
    } else if let Some(r) = line.strip_prefix("} else if (") {
        r
    } else if let Some(r) = line.strip_prefix("else if (") {
        r
    } else {
        return None;
    };

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
        let trimmed = rest.trim_end_matches('{').trim_end_matches(')').trim();
        return Some(trimmed.to_string());
    }

    Some(rest[..end].trim().to_string())
}

fn parse_simple_csharp_condition(text: &str) -> ConditionTree {
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
                left: Box::new(parse_csharp_expr(left)),
                op: *op,
                right: Box::new(parse_csharp_expr(right)),
            };
        }
    }

    ConditionTree::Unknown(text.to_string())
}

fn parse_csharp_expr(text: &str) -> Expr {
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
                object: Box::new(parse_csharp_expr(obj)),
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
    fn parse_csharp_is_type() {
        let source = "if (obj is string) {";
        let conditions = parse_csharp_conditions(source);
        assert!(!conditions.is_empty());
        assert!(matches!(conditions[0].1, ConditionTree::TypeCheck { .. }));
    }

    #[test]
    fn parse_csharp_null_conditional() {
        let source = "if (obj?.Property) {";
        let conditions = parse_csharp_conditions(source);
        assert!(!conditions.is_empty());
        assert!(matches!(
            conditions[0].1,
            ConditionTree::NullCheck { is_null: false, .. }
        ));
    }

    #[test]
    fn parse_csharp_null_coalescing() {
        let source = "var x = value ?? defaultVal;";
        let conditions = parse_csharp_conditions(source);
        assert!(!conditions.is_empty());
        assert!(matches!(conditions[0].1, ConditionTree::NullCheck { .. }));
    }

    #[test]
    fn parse_csharp_null_check() {
        let source = "if (result == null) {";
        let conditions = parse_csharp_conditions(source);
        assert!(!conditions.is_empty());
        assert!(matches!(
            conditions[0].1,
            ConditionTree::NullCheck { is_null: true, .. }
        ));
    }

    #[test]
    fn parse_csharp_comparison() {
        let source = "if (count > 5) {";
        let conditions = parse_csharp_conditions(source);
        assert!(!conditions.is_empty());
        assert!(matches!(conditions[0].1, ConditionTree::Compare { .. }));
    }

    #[test]
    fn parse_csharp_case() {
        let source = "    case 42:";
        let conditions = parse_csharp_conditions(source);
        assert!(!conditions.is_empty());
        assert!(matches!(conditions[0].1, ConditionTree::Compare { .. }));
    }
}
