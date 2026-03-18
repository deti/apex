//! Ruby condition parser for concolic execution.
//!
//! Extracts branch conditions from Ruby source text and converts them
//! into [`ConditionTree`] IR for boundary seed generation.
//!
//! Supported constructs:
//! - `.nil?` checks
//! - `unless` (negated if)
//! - `case` / `when` pattern matching
//! - Standard comparisons
//! - `.empty?`, `.zero?`

use crate::condition_tree::{CompareOp, ConditionTree, Expr};

/// Parse Ruby source and extract `(line_number, condition)` pairs.
pub fn parse_ruby_conditions(source: &str) -> Vec<(u32, ConditionTree)> {
    let mut results = Vec::new();

    for (i, line) in source.lines().enumerate() {
        let line_num = (i + 1) as u32;
        let trimmed = line.trim();

        // .nil? checks
        if let Some(tree) = try_parse_nil_check(trimmed) {
            results.push((line_num, tree));
            continue;
        }

        // unless (negated if)
        if let Some(tree) = try_parse_unless(trimmed) {
            results.push((line_num, tree));
            continue;
        }

        // when (case/when)
        if let Some(tree) = try_parse_when(trimmed) {
            results.push((line_num, tree));
            continue;
        }

        // .empty? / .zero?
        if let Some(tree) = try_parse_predicate(trimmed) {
            results.push((line_num, tree));
            continue;
        }

        // Standard if comparison
        if let Some(tree) = try_parse_if_comparison(trimmed) {
            results.push((line_num, tree));
            continue;
        }
    }

    results
}

fn try_parse_nil_check(line: &str) -> Option<ConditionTree> {
    let cond = extract_ruby_condition(line, "if")?;

    // x.nil?
    if let Some(pos) = cond.find(".nil?") {
        let var = cond[..pos].trim();
        if !var.is_empty() {
            return Some(ConditionTree::NullCheck {
                expr: Box::new(Expr::Variable(var.to_string())),
                is_null: true,
            });
        }
    }
    // !x.nil? (in unless or with negation)
    if let Some(stripped) = cond.strip_prefix('!') {
        let inner = stripped.trim();
        if let Some(pos) = inner.find(".nil?") {
            let var = inner[..pos].trim();
            if !var.is_empty() {
                return Some(ConditionTree::NullCheck {
                    expr: Box::new(Expr::Variable(var.to_string())),
                    is_null: false,
                });
            }
        }
    }

    // == nil
    if let Some(pos) = cond.find("== nil") {
        let var = cond[..pos].trim();
        if !var.is_empty() {
            return Some(ConditionTree::NullCheck {
                expr: Box::new(Expr::Variable(var.to_string())),
                is_null: true,
            });
        }
    }
    // != nil
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

fn try_parse_unless(line: &str) -> Option<ConditionTree> {
    let cond = extract_ruby_condition(line, "unless")?;

    let inner = parse_simple_ruby_condition(&cond);
    Some(ConditionTree::Not(Box::new(inner)))
}

fn try_parse_when(line: &str) -> Option<ConditionTree> {
    let when_val = line.strip_prefix("when ")?.trim();
    if when_val.is_empty() {
        return None;
    }
    // Strip trailing `then` if present
    let when_val = when_val.strip_suffix("then").unwrap_or(when_val).trim();
    if when_val.is_empty() {
        return None;
    }

    Some(ConditionTree::Compare {
        left: Box::new(Expr::Variable("case_expr".into())),
        op: CompareOp::Eq,
        right: Box::new(parse_ruby_expr(when_val)),
    })
}

fn try_parse_predicate(line: &str) -> Option<ConditionTree> {
    let cond = extract_ruby_condition(line, "if")?;

    // x.empty?
    if let Some(pos) = cond.find(".empty?") {
        let var = cond[..pos].trim();
        if !var.is_empty() {
            return Some(ConditionTree::LengthCheck {
                expr: Box::new(Expr::Variable(var.to_string())),
                op: CompareOp::Eq,
                value: Box::new(Expr::IntLiteral(0)),
            });
        }
    }

    // x.zero?
    if let Some(pos) = cond.find(".zero?") {
        let var = cond[..pos].trim();
        if !var.is_empty() {
            return Some(ConditionTree::Compare {
                left: Box::new(Expr::Variable(var.to_string())),
                op: CompareOp::Eq,
                right: Box::new(Expr::IntLiteral(0)),
            });
        }
    }

    None
}

fn try_parse_if_comparison(line: &str) -> Option<ConditionTree> {
    let cond = extract_ruby_condition(line, "if")?;

    // Skip nil/predicate checks (handled above)
    if cond.contains(".nil?") || cond.contains(".empty?") || cond.contains(".zero?") {
        return None;
    }
    if cond.contains("nil") {
        return None;
    }

    // Handle && (and) and || (or)
    if let Some(pos) = cond.find("&&") {
        let left = parse_simple_ruby_condition(&cond[..pos]);
        let right = parse_simple_ruby_condition(&cond[pos + 2..]);
        return Some(ConditionTree::And(Box::new(left), Box::new(right)));
    }
    if let Some(pos) = cond.find("||") {
        let left = parse_simple_ruby_condition(&cond[..pos]);
        let right = parse_simple_ruby_condition(&cond[pos + 2..]);
        return Some(ConditionTree::Or(Box::new(left), Box::new(right)));
    }

    let tree = parse_simple_ruby_condition(&cond);
    if matches!(tree, ConditionTree::Unknown(_)) {
        return None;
    }
    Some(tree)
}

fn extract_ruby_condition(line: &str, keyword: &str) -> Option<String> {
    // Ruby: `if condition` (no parens required) or trailing `if condition`
    let trimmed = line.trim();

    // Leading keyword
    let prefix = format!("{keyword} ");
    if let Some(rest) = trimmed.strip_prefix(&prefix) {
        // Skip `if let` style (not valid Ruby but let's be safe)
        if rest.starts_with("let ") {
            return None;
        }
        let rest = rest.trim_end_matches("then").trim();
        return Some(rest.to_string());
    }

    // Trailing modifier: `expr if condition` or `expr unless condition`
    let pattern = format!(" {keyword} ");
    if let Some(pos) = trimmed.rfind(&pattern) {
        let cond = trimmed[pos + pattern.len()..].trim();
        if !cond.is_empty() {
            return Some(cond.to_string());
        }
    }

    None
}

fn parse_simple_ruby_condition(text: &str) -> ConditionTree {
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
                left: Box::new(parse_ruby_expr(left)),
                op: *op,
                right: Box::new(parse_ruby_expr(right)),
            };
        }
    }

    ConditionTree::Unknown(text.to_string())
}

fn parse_ruby_expr(text: &str) -> Expr {
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
    // Double-quoted strings
    if text.len() >= 2 && text.starts_with('"') && text.ends_with('"') {
        return Expr::StringLiteral(text[1..text.len() - 1].to_string());
    }
    // Single-quoted strings
    if text.len() >= 2 && text.starts_with('\'') && text.ends_with('\'') {
        return Expr::StringLiteral(text[1..text.len() - 1].to_string());
    }
    // Symbols: :symbol
    if let Some(sym) = text.strip_prefix(':') {
        return Expr::StringLiteral(sym.to_string());
    }
    if text.ends_with(')') {
        return Expr::Call(text.to_string());
    }
    if let Some(dot) = text.rfind('.') {
        let obj = &text[..dot];
        let prop = &text[dot + 1..];
        if !prop.is_empty() && !obj.is_empty() {
            return Expr::PropertyAccess {
                object: Box::new(parse_ruby_expr(obj)),
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
    fn parse_ruby_nil_check() {
        let source = "if value.nil?";
        let conditions = parse_ruby_conditions(source);
        assert!(!conditions.is_empty());
        assert!(matches!(
            conditions[0].1,
            ConditionTree::NullCheck { is_null: true, .. }
        ));
    }

    #[test]
    fn parse_ruby_unless() {
        let source = "unless valid";
        let conditions = parse_ruby_conditions(source);
        assert!(!conditions.is_empty());
        assert!(matches!(conditions[0].1, ConditionTree::Not(_)));
    }

    #[test]
    fn parse_ruby_when() {
        let source = r#"when "admin""#;
        let conditions = parse_ruby_conditions(source);
        assert!(!conditions.is_empty());
        assert!(matches!(conditions[0].1, ConditionTree::Compare { .. }));
    }

    #[test]
    fn parse_ruby_empty_check() {
        let source = "if list.empty?";
        let conditions = parse_ruby_conditions(source);
        assert!(!conditions.is_empty());
        assert!(matches!(conditions[0].1, ConditionTree::LengthCheck { .. }));
    }

    #[test]
    fn parse_ruby_comparison() {
        let source = "if count > 10";
        let conditions = parse_ruby_conditions(source);
        assert!(!conditions.is_empty());
        assert!(matches!(conditions[0].1, ConditionTree::Compare { .. }));
    }

    #[test]
    fn parse_ruby_when_symbol() {
        let source = "when :admin";
        let conditions = parse_ruby_conditions(source);
        assert!(!conditions.is_empty());
        assert!(matches!(conditions[0].1, ConditionTree::Compare { .. }));
    }
}
