//! C/C++ condition parser for concolic execution.
//!
//! Extracts branch conditions from C/C++ source text and converts them
//! into [`ConditionTree`] IR for boundary seed generation.
//!
//! Supported constructs:
//! - `ptr != NULL` / `ptr == NULL`
//! - `flags & MASK` (bitwise flag checks)
//! - Standard comparisons
//! - `#ifdef` / `#ifndef` are intentionally skipped (compile-time only)

use crate::condition_tree::{CompareOp, ConditionTree, Expr};

/// Parse C/C++ source and extract `(line_number, condition)` pairs.
///
/// Skips preprocessor directives (`#ifdef`, `#ifndef`, `#if defined`).
pub fn parse_c_conditions(source: &str) -> Vec<(u32, ConditionTree)> {
    let mut results = Vec::new();

    for (i, line) in source.lines().enumerate() {
        let line_num = (i + 1) as u32;
        let trimmed = line.trim();

        // Skip preprocessor directives
        if trimmed.starts_with('#') {
            continue;
        }

        // NULL checks
        if let Some(tree) = try_parse_null_check(trimmed) {
            results.push((line_num, tree));
            continue;
        }

        // Bitwise flag checks: if (flags & MASK)
        if let Some(tree) = try_parse_bitmask(trimmed) {
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

fn try_parse_null_check(line: &str) -> Option<ConditionTree> {
    let cond = extract_c_condition(line)?;

    // ptr != NULL / ptr != nullptr
    for null_tok in &["NULL", "nullptr", "0"] {
        let ne_pat = format!("!= {null_tok}");
        if let Some(pos) = cond.find(&ne_pat) {
            let var = cond[..pos].trim();
            if !var.is_empty() {
                return Some(ConditionTree::NullCheck {
                    expr: Box::new(Expr::Variable(var.to_string())),
                    is_null: false,
                });
            }
        }
        let eq_pat = format!("== {null_tok}");
        if let Some(pos) = cond.find(&eq_pat) {
            let var = cond[..pos].trim();
            if !var.is_empty() {
                return Some(ConditionTree::NullCheck {
                    expr: Box::new(Expr::Variable(var.to_string())),
                    is_null: true,
                });
            }
        }
    }

    // Simple pointer check: if (ptr) or if (!ptr)
    if !cond.contains('=')
        && !cond.contains('<')
        && !cond.contains('>')
        && !cond.contains('&')
        && !cond.contains('|')
    {
        let (var, negated) = if let Some(v) = cond.strip_prefix('!') {
            (v.trim(), true)
        } else {
            (cond.as_str(), false)
        };
        // Heuristic: single identifier likely a pointer check
        if var.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '-' || c == '>') {
            return Some(ConditionTree::NullCheck {
                expr: Box::new(Expr::Variable(var.to_string())),
                is_null: negated,
            });
        }
    }

    None
}

fn try_parse_bitmask(line: &str) -> Option<ConditionTree> {
    let cond = extract_c_condition(line)?;

    // flags & MASK  (no comparison operator — used as boolean)
    if cond.contains('&') && !cond.contains("&&") && !cond.contains('=') {
        let parts: Vec<&str> = cond.splitn(2, '&').collect();
        if parts.len() == 2 {
            let left = parts[0].trim();
            let right = parts[1].trim();
            if !left.is_empty() && !right.is_empty() {
                return Some(ConditionTree::Compare {
                    left: Box::new(Expr::Call(format!("{left} & {right}"))),
                    op: CompareOp::NotEq,
                    right: Box::new(Expr::IntLiteral(0)),
                });
            }
        }
    }

    None
}

fn try_parse_if_comparison(line: &str) -> Option<ConditionTree> {
    let cond = extract_c_condition(line)?;

    // Skip NULL checks and bitmasks (handled above)
    for tok in &["NULL", "nullptr"] {
        if cond.contains(tok) {
            return None;
        }
    }
    if cond.contains('&') && !cond.contains("&&") && !cond.contains('=') {
        return None;
    }

    // Handle && and ||
    if let Some(pos) = find_logical_op(&cond, "&&") {
        let left = parse_simple_c_condition(&cond[..pos]);
        let right = parse_simple_c_condition(&cond[pos + 2..]);
        return Some(ConditionTree::And(Box::new(left), Box::new(right)));
    }
    if let Some(pos) = find_logical_op(&cond, "||") {
        let left = parse_simple_c_condition(&cond[..pos]);
        let right = parse_simple_c_condition(&cond[pos + 2..]);
        return Some(ConditionTree::Or(Box::new(left), Box::new(right)));
    }

    let tree = parse_simple_c_condition(&cond);
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
        right: Box::new(parse_c_expr(case_val)),
    })
}

fn extract_c_condition(line: &str) -> Option<String> {
    // C: `if (condition)` or `} else if (condition)`
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

fn find_logical_op(text: &str, op: &str) -> Option<usize> {
    let mut depth = 0i32;
    let bytes = text.as_bytes();
    let op_bytes = op.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'(' => depth += 1,
            b')' => depth -= 1,
            _ => {}
        }
        if depth == 0 && i + op.len() <= bytes.len() && &bytes[i..i + op.len()] == op_bytes {
            return Some(i);
        }
        i += 1;
    }
    None
}

fn parse_simple_c_condition(text: &str) -> ConditionTree {
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
                left: Box::new(parse_c_expr(left)),
                op: *op,
                right: Box::new(parse_c_expr(right)),
            };
        }
    }

    ConditionTree::Unknown(text.to_string())
}

fn parse_c_expr(text: &str) -> Expr {
    let text = text.trim();

    if text == "NULL" || text == "nullptr" {
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
    // Single-char literals
    if text.len() >= 3 && text.starts_with('\'') && text.ends_with('\'') {
        return Expr::StringLiteral(text[1..text.len() - 1].to_string());
    }
    if text.ends_with(')') {
        return Expr::Call(text.to_string());
    }
    // Arrow access: ptr->field
    if let Some(arrow) = text.rfind("->") {
        let obj = &text[..arrow];
        let prop = &text[arrow + 2..];
        if !prop.is_empty() && !obj.is_empty() {
            return Expr::PropertyAccess {
                object: Box::new(parse_c_expr(obj)),
                property: prop.to_string(),
            };
        }
    }
    if let Some(dot) = text.rfind('.') {
        let obj = &text[..dot];
        let prop = &text[dot + 1..];
        if !prop.is_empty() && !obj.is_empty() {
            return Expr::PropertyAccess {
                object: Box::new(parse_c_expr(obj)),
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
    fn parse_c_null_check() {
        let source = "if (ptr != NULL) {";
        let conditions = parse_c_conditions(source);
        assert!(!conditions.is_empty());
        assert!(matches!(
            conditions[0].1,
            ConditionTree::NullCheck { is_null: false, .. }
        ));
    }

    #[test]
    fn parse_c_nullptr_check() {
        let source = "if (ptr == nullptr) {";
        let conditions = parse_c_conditions(source);
        assert!(!conditions.is_empty());
        assert!(matches!(
            conditions[0].1,
            ConditionTree::NullCheck { is_null: true, .. }
        ));
    }

    #[test]
    fn parse_c_bitmask() {
        let source = "if (flags & MASK) {";
        let conditions = parse_c_conditions(source);
        assert!(!conditions.is_empty());
        assert!(matches!(conditions[0].1, ConditionTree::Compare { .. }));
    }

    #[test]
    fn parse_c_skip_ifdef() {
        let source = "#ifdef FEATURE\n#ifndef DEBUG\nif (x > 0) {";
        let conditions = parse_c_conditions(source);
        assert_eq!(conditions.len(), 1);
        assert_eq!(conditions[0].0, 3); // only the runtime if
    }

    #[test]
    fn parse_c_comparison() {
        let source = "if (count >= 10) {";
        let conditions = parse_c_conditions(source);
        assert!(!conditions.is_empty());
        assert!(matches!(conditions[0].1, ConditionTree::Compare { .. }));
    }

    #[test]
    fn parse_c_case() {
        let source = "    case 42:";
        let conditions = parse_c_conditions(source);
        assert!(!conditions.is_empty());
        assert!(matches!(conditions[0].1, ConditionTree::Compare { .. }));
    }
}
