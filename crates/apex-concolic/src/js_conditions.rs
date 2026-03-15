/// Text-based JS/TS condition parser for concolic execution.
///
/// Parses JavaScript/TypeScript boolean expressions into a [`ConditionTree`] IR
/// that the concolic engine can reason about symbolically.
///
/// Supported constructs:
/// - Logical: `&&`, `||`, `!` (with correct parenthesis depth tracking)
/// - Comparison: `===`, `!==`, `>=`, `<=`, `>`, `<`, `==`, `!=`
/// - typeof:  `typeof x === "string"`
/// - instanceof: `x instanceof Error`
/// - in operator: `"key" in obj`
/// - null/undefined checks: `x === null`, `x !== undefined`
/// - Optional chain: `x?.y` (treated as null check on x)
/// - Length check: `x.length > 0`
/// - Fallback: `ConditionTree::Unknown(text)`
use crate::condition_tree::{CompareOp, ConditionTree, Expr};

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Parse a JS/TS condition string into a [`ConditionTree`].
///
/// The parser is best-effort: anything it cannot understand is wrapped in
/// `ConditionTree::Unknown`.
pub fn parse_js_condition(input: &str) -> ConditionTree {
    let text = input.trim();
    parse_or(text)
}

// ---------------------------------------------------------------------------
// Recursive descent — precedence: || < && < unary-! < atom
// ---------------------------------------------------------------------------

fn parse_or(text: &str) -> ConditionTree {
    // Split on `||` at paren depth 0 (leftmost split — left side gets lower precedence,
    // right side recurses to capture remaining `||` chains)
    if let Some(pos) = find_operator_outside_parens(text, "||") {
        let left = parse_and(text[..pos].trim());
        let right = parse_or(text[pos + 2..].trim());
        return ConditionTree::Or(Box::new(left), Box::new(right));
    }
    parse_and(text)
}

fn parse_and(text: &str) -> ConditionTree {
    if let Some(pos) = find_operator_outside_parens(text, "&&") {
        let left = parse_not(text[..pos].trim());
        let right = parse_and(text[pos + 2..].trim());
        return ConditionTree::And(Box::new(left), Box::new(right));
    }
    parse_not(text)
}

fn parse_not(text: &str) -> ConditionTree {
    if text.starts_with('!') && !text.starts_with("!=") && !text.starts_with("!==") {
        let inner = text[1..].trim();
        return ConditionTree::Not(Box::new(parse_not(inner)));
    }
    parse_atom(text)
}

fn parse_atom(text: &str) -> ConditionTree {
    let text = text.trim();

    // Strip outer parentheses
    if let Some(inner) = strip_outer_parens(text) {
        return parse_or(inner.trim());
    }

    // typeof x === "typename"  |  typeof x !== "typename"
    if let Some(tree) = try_parse_typeof(text) {
        return tree;
    }

    // x instanceof Error
    if let Some(tree) = try_parse_instanceof(text) {
        return tree;
    }

    // "key" in obj  |  key in obj
    if let Some(tree) = try_parse_in_operator(text) {
        return tree;
    }

    // optional chain: x?.y  — treat as NullCheck on the object
    if let Some(tree) = try_parse_optional_chain(text) {
        return tree;
    }

    // x.length <op> n
    if let Some(tree) = try_parse_length_check(text) {
        return tree;
    }

    // General comparison (handles null / undefined checks too)
    if let Some(tree) = try_parse_comparison(text) {
        return tree;
    }

    ConditionTree::Unknown(text.to_string())
}

// ---------------------------------------------------------------------------
// Specific parsers
// ---------------------------------------------------------------------------

fn try_parse_typeof(text: &str) -> Option<ConditionTree> {
    // typeof <expr> ===|!==|==|!= "typename"
    if !text.starts_with("typeof ") {
        return None;
    }
    let rest = text["typeof ".len()..].trim();
    // Find comparison operator
    for op_str in &["===", "!==", "==", "!="] {
        if let Some(pos) = rest.find(op_str) {
            let expr_str = rest[..pos].trim();
            let type_name = rest[pos + op_str.len()..]
                .trim()
                .trim_matches('"')
                .trim_matches('\'')
                .to_string();
            let expr = parse_expr(expr_str);
            let op = parse_compare_op(op_str)?;
            // typeof x === "t"  ⇒ TypeCheck; typeof x !== "t" ⇒ Not(TypeCheck)
            let check = ConditionTree::TypeCheck {
                expr: Box::new(expr),
                type_name,
            };
            return Some(if op == CompareOp::NotEq || op == CompareOp::Eq {
                if op == CompareOp::NotEq {
                    ConditionTree::Not(Box::new(check))
                } else {
                    check
                }
            } else {
                check
            });
        }
    }
    None
}

fn try_parse_instanceof(text: &str) -> Option<ConditionTree> {
    let pos = find_word_outside_parens(text, " instanceof ")?;
    let expr_str = text[..pos].trim();
    let type_name = text[pos + " instanceof ".len()..].trim().to_string();
    Some(ConditionTree::TypeCheck {
        expr: Box::new(parse_expr(expr_str)),
        type_name,
    })
}

fn try_parse_in_operator(text: &str) -> Option<ConditionTree> {
    // "<key>" in obj  or  key in obj  — but NOT "foo" inside a string literal
    let pos = find_word_outside_parens(text, " in ")?;
    let needle_str = text[..pos].trim();
    let haystack_str = text[pos + " in ".len()..].trim();
    // Avoid false positives where `instanceof` got split, etc.
    if haystack_str.is_empty() || needle_str.is_empty() {
        return None;
    }
    Some(ConditionTree::Contains {
        needle: Box::new(parse_expr(needle_str)),
        haystack: Box::new(parse_expr(haystack_str)),
    })
}

fn try_parse_optional_chain(text: &str) -> Option<ConditionTree> {
    // Simplest form: x?.y  — the whole expression is just an optional access,
    // meaning we're checking that x is non-null.
    if !text.contains("?.") {
        return None;
    }
    // Only handle the case where the entire text is `x?.y` (no operator)
    if find_comparison_op_outside_parens(text).is_some() {
        return None;
    }
    // Extract the object before `?.`
    let dot_pos = text.find("?.")?;
    let obj_str = text[..dot_pos].trim();
    Some(ConditionTree::NullCheck {
        expr: Box::new(parse_expr(obj_str)),
        is_null: false, // x?.y is truthy when x is non-null
    })
}

fn try_parse_length_check(text: &str) -> Option<ConditionTree> {
    // <expr>.length <op> <value>
    let (op_str, op_pos) = find_comparison_op_outside_parens(text)?;
    let left_str = text[..op_pos].trim();
    let right_str = text[op_pos + op_str.len()..].trim();

    if !left_str.ends_with(".length") {
        return None;
    }
    let obj_str = &left_str[..left_str.len() - ".length".len()];
    let op = parse_compare_op(op_str)?;
    Some(ConditionTree::LengthCheck {
        expr: Box::new(parse_expr(obj_str)),
        op,
        value: Box::new(parse_expr(right_str)),
    })
}

fn try_parse_comparison(text: &str) -> Option<ConditionTree> {
    let (op_str, op_pos) = find_comparison_op_outside_parens(text)?;
    let left_str = text[..op_pos].trim();
    let right_str = text[op_pos + op_str.len()..].trim();
    let op = parse_compare_op(op_str)?;

    let left = parse_expr(left_str);
    let right = parse_expr(right_str);

    // Null/undefined checks — only for equality/inequality operators
    let is_null_literal = |e: &Expr| matches!(e, Expr::Null);
    if (is_null_literal(&left) || is_null_literal(&right))
        && matches!(op, CompareOp::Eq | CompareOp::NotEq)
    {
        let expr = if is_null_literal(&left) { right } else { left };
        let is_null = matches!(op, CompareOp::Eq);
        return Some(ConditionTree::NullCheck {
            expr: Box::new(expr),
            is_null,
        });
    }

    Some(ConditionTree::Compare {
        left: Box::new(left),
        op,
        right: Box::new(right),
    })
}

// ---------------------------------------------------------------------------
// Expression parser
// ---------------------------------------------------------------------------

pub fn parse_expr(text: &str) -> Expr {
    let text = text.trim();

    // null / undefined
    if text == "null" || text == "undefined" {
        return Expr::Null;
    }

    // boolean literals
    if text == "true" {
        return Expr::BoolLiteral(true);
    }
    if text == "false" {
        return Expr::BoolLiteral(false);
    }

    // string literal
    if text.len() >= 2
        && ((text.starts_with('"') && text.ends_with('"'))
            || (text.starts_with('\'') && text.ends_with('\''))
            || (text.starts_with('`') && text.ends_with('`')))
    {
        let s = text[1..text.len() - 1].to_string();
        return Expr::StringLiteral(s);
    }

    // integer / float literals
    if let Ok(i) = text.parse::<i64>() {
        return Expr::IntLiteral(i);
    }
    if let Ok(f) = text.parse::<f64>() {
        return Expr::FloatLiteral(f);
    }

    // function call: ends with `)`
    if text.ends_with(')') {
        return Expr::Call(text.to_string());
    }

    // property access: contains `.` but not `?.`
    if let Some(dot_pos) = last_property_dot(text) {
        let obj_str = &text[..dot_pos];
        let prop = text[dot_pos + 1..].to_string();
        return Expr::PropertyAccess {
            object: Box::new(parse_expr(obj_str)),
            property: prop,
        };
    }

    // variable
    Expr::Variable(text.to_string())
}

// ---------------------------------------------------------------------------
// Utility: find last plain `.` for property access (not `?.`)
// ---------------------------------------------------------------------------

fn last_property_dot(text: &str) -> Option<usize> {
    let chars: Vec<char> = text.chars().collect();
    let mut i = chars.len().checked_sub(1)?;
    loop {
        if chars[i] == '.' {
            // Make sure it's not `?.` (preceded by `?`)
            if i > 0 && chars[i - 1] == '?' {
                // optional chain — skip
            } else {
                return Some(i);
            }
        }
        if i == 0 {
            break;
        }
        i -= 1;
    }
    None
}

// ---------------------------------------------------------------------------
// Utility: operator finding
// ---------------------------------------------------------------------------

/// Find the leftmost occurrence of `needle` outside parentheses/brackets/quotes.
fn find_operator_outside_parens(text: &str, needle: &str) -> Option<usize> {
    let bytes = text.as_bytes();
    let n = needle.len();
    let mut depth = 0i32;
    let mut in_str: Option<u8> = None;
    let mut i = 0usize;

    while i < bytes.len() {
        let b = bytes[i];

        // String tracking
        if let Some(q) = in_str {
            if b == q {
                let mut backslash_count = 0;
                let mut j = i;
                while j > 0 && bytes[j - 1] == b'\\' {
                    backslash_count += 1;
                    j -= 1;
                }
                if backslash_count % 2 == 0 {
                    in_str = None;
                }
            }
            i += 1;
            continue;
        }
        if b == b'"' || b == b'\'' || b == b'`' {
            in_str = Some(b);
            i += 1;
            continue;
        }

        match b {
            b'(' | b'[' | b'{' => depth += 1,
            b')' | b']' | b'}' => depth -= 1,
            _ => {}
        }

        if depth == 0 && i + n <= bytes.len() {
            if !text.is_char_boundary(i) || !text.is_char_boundary(i + n) {
                i += 1;
                continue;
            }
            if &text[i..i + n] == needle {
                return Some(i);
            }
        }
        i += 1;
    }
    None
}

/// Find the leftmost occurrence of `needle` as a word boundary outside parens.
fn find_word_outside_parens(text: &str, needle: &str) -> Option<usize> {
    let bytes = text.as_bytes();
    let n = needle.len();
    let needle_bytes = needle.as_bytes();
    let mut i = 0;
    let mut depth = 0i32;
    let mut in_str: Option<u8> = None;
    while i < bytes.len() {
        let b = bytes[i];
        if let Some(q) = in_str {
            if b == q {
                let mut backslash_count = 0;
                let mut j = i;
                while j > 0 && bytes[j - 1] == b'\\' {
                    backslash_count += 1;
                    j -= 1;
                }
                if backslash_count % 2 == 0 {
                    in_str = None;
                }
            }
            i += 1;
            continue;
        }
        if b == b'"' || b == b'\'' || b == b'`' {
            in_str = Some(b);
            i += 1;
            continue;
        }
        match b {
            b'(' | b'[' | b'{' => depth += 1,
            b')' | b']' | b'}' => depth -= 1,
            _ => {}
        }
        if depth == 0 && i + n <= bytes.len() && &bytes[i..i + n] == needle_bytes {
            return Some(i);
        }
        i += 1;
    }
    None
}

/// Find the comparison operator in `text` at paren depth 0.
/// Returns `(op_str, byte_position)` for the first (leftmost) match.
/// Tries longer operators first to avoid `==` matching inside `===`.
fn find_comparison_op_outside_parens(text: &str) -> Option<(&'static str, usize)> {
    const OPS: &[&str] = &["===", "!==", ">=", "<=", ">", "<", "==", "!="];
    let mut best: Option<(&'static str, usize)> = None;

    for &op in OPS {
        if let Some(pos) = find_operator_outside_parens(text, op) {
            // Prefer the leftmost; on tie, the earlier-listed (longer) op wins
            if best.is_none_or(|(_, prev)| pos < prev) {
                best = Some((op, pos));
            }
        }
    }

    // Validate: make sure the chosen op is not part of an arrow `=>`.
    // e.g. if we found `>` at position 1 in `=>`, discard it.
    if let Some((op, pos)) = best {
        if (op == ">" || op == ">=") && pos > 0 {
            let bytes = text.as_bytes();
            if bytes[pos - 1] == b'=' {
                // This `>` is the second character of `=>` — skip it
                return None;
            }
        }
        best
    } else {
        None
    }
}

fn parse_compare_op(op: &str) -> Option<CompareOp> {
    match op {
        "===" | "==" => Some(CompareOp::Eq),
        "!==" | "!=" => Some(CompareOp::NotEq),
        "<" => Some(CompareOp::Lt),
        "<=" => Some(CompareOp::LtEq),
        ">" => Some(CompareOp::Gt),
        ">=" => Some(CompareOp::GtEq),
        _ => None,
    }
}

/// If `text` is wrapped in a matching pair of outer parentheses, return the interior.
fn strip_outer_parens(text: &str) -> Option<&str> {
    if !text.starts_with('(') || !text.ends_with(')') {
        return None;
    }
    let inner = &text[1..text.len() - 1];
    // Verify depth never goes negative, skipping characters inside string literals
    let mut depth = 0i32;
    let mut in_str: Option<char> = None;
    for ch in inner.chars() {
        if let Some(q) = in_str {
            if ch == q {
                in_str = None;
            }
            continue;
        }
        match ch {
            '"' | '\'' | '`' => in_str = Some(ch),
            '(' => depth += 1,
            ')' => {
                if depth == 0 {
                    return None;
                }
                depth -= 1;
            }
            _ => {}
        }
    }
    Some(inner)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::condition_tree::{CompareOp, ConditionTree, Expr};

    // --- comparison ---

    #[test]
    fn parse_strict_equality() {
        let tree = parse_js_condition("x === 42");
        assert_eq!(
            tree,
            ConditionTree::Compare {
                left: Box::new(Expr::Variable("x".into())),
                op: CompareOp::Eq,
                right: Box::new(Expr::IntLiteral(42)),
            }
        );
    }

    #[test]
    fn parse_strict_inequality() {
        let tree = parse_js_condition("x !== 0");
        assert_eq!(
            tree,
            ConditionTree::Compare {
                left: Box::new(Expr::Variable("x".into())),
                op: CompareOp::NotEq,
                right: Box::new(Expr::IntLiteral(0)),
            }
        );
    }

    #[test]
    fn parse_comparison_gt() {
        let tree = parse_js_condition("count > 10");
        assert_eq!(
            tree,
            ConditionTree::Compare {
                left: Box::new(Expr::Variable("count".into())),
                op: CompareOp::Gt,
                right: Box::new(Expr::IntLiteral(10)),
            }
        );
    }

    // --- typeof ---

    #[test]
    fn parse_typeof() {
        let tree = parse_js_condition(r#"typeof x === "string""#);
        assert_eq!(
            tree,
            ConditionTree::TypeCheck {
                expr: Box::new(Expr::Variable("x".into())),
                type_name: "string".into(),
            }
        );
    }

    // --- instanceof ---

    #[test]
    fn parse_instanceof() {
        let tree = parse_js_condition("err instanceof Error");
        assert_eq!(
            tree,
            ConditionTree::TypeCheck {
                expr: Box::new(Expr::Variable("err".into())),
                type_name: "Error".into(),
            }
        );
    }

    // --- in operator ---

    #[test]
    fn parse_in_operator() {
        let tree = parse_js_condition(r#""key" in obj"#);
        assert_eq!(
            tree,
            ConditionTree::Contains {
                needle: Box::new(Expr::StringLiteral("key".into())),
                haystack: Box::new(Expr::Variable("obj".into())),
            }
        );
    }

    // --- logical ---

    #[test]
    fn parse_and() {
        let tree = parse_js_condition("a === 1 && b === 2");
        assert!(matches!(tree, ConditionTree::And(_, _)));
    }

    #[test]
    fn parse_or() {
        let tree = parse_js_condition("a === 1 || b === 2");
        assert!(matches!(tree, ConditionTree::Or(_, _)));
    }

    #[test]
    fn parse_not() {
        let tree = parse_js_condition("!done");
        assert_eq!(
            tree,
            ConditionTree::Not(Box::new(ConditionTree::Unknown("done".into())))
        );
    }

    // --- null checks ---

    #[test]
    fn parse_null_check_eq() {
        let tree = parse_js_condition("x === null");
        assert_eq!(
            tree,
            ConditionTree::NullCheck {
                expr: Box::new(Expr::Variable("x".into())),
                is_null: true,
            }
        );
    }

    #[test]
    fn parse_null_check_ne() {
        let tree = parse_js_condition("x !== null");
        assert_eq!(
            tree,
            ConditionTree::NullCheck {
                expr: Box::new(Expr::Variable("x".into())),
                is_null: false,
            }
        );
    }

    // --- optional chain ---

    #[test]
    fn parse_optional_chain() {
        let tree = parse_js_condition("obj?.prop");
        assert_eq!(
            tree,
            ConditionTree::NullCheck {
                expr: Box::new(Expr::Variable("obj".into())),
                is_null: false,
            }
        );
    }

    // --- length check ---

    #[test]
    fn parse_length_check() {
        let tree = parse_js_condition("arr.length > 0");
        assert_eq!(
            tree,
            ConditionTree::LengthCheck {
                expr: Box::new(Expr::Variable("arr".into())),
                op: CompareOp::Gt,
                value: Box::new(Expr::IntLiteral(0)),
            }
        );
    }

    // --- fallback ---

    #[test]
    fn parse_unknown_fallback() {
        let tree = parse_js_condition("someComplexExpression()");
        assert!(matches!(tree, ConditionTree::Unknown(_)));
    }

    // --- expression atoms ---

    #[test]
    fn parse_expr_int() {
        assert_eq!(parse_expr("99"), Expr::IntLiteral(99));
    }

    #[test]
    fn parse_expr_string() {
        assert_eq!(
            parse_expr(r#""hello""#),
            Expr::StringLiteral("hello".into())
        );
    }

    #[test]
    fn parse_expr_null() {
        assert_eq!(parse_expr("null"), Expr::Null);
    }

    #[test]
    fn parse_expr_undefined() {
        assert_eq!(parse_expr("undefined"), Expr::Null);
    }

    #[test]
    fn parse_expr_property() {
        assert_eq!(
            parse_expr("err.message"),
            Expr::PropertyAccess {
                object: Box::new(Expr::Variable("err".into())),
                property: "message".into(),
            }
        );
    }

    // --- switch-case comparison ---

    #[test]
    fn parse_switch_case_comparison() {
        // Simulates a condition derived from a switch/case: value === "admin"
        let tree = parse_js_condition(r#"role === "admin""#);
        assert_eq!(
            tree,
            ConditionTree::Compare {
                left: Box::new(Expr::Variable("role".into())),
                op: CompareOp::Eq,
                right: Box::new(Expr::StringLiteral("admin".into())),
            }
        );
    }

    // --- bug regression tests ---

    #[test]
    fn bug_parse_expr_single_quote_no_panic() {
        // A single `"` used to panic on text[1..0] slice
        let expr = parse_expr("\"");
        assert!(matches!(expr, Expr::Variable(_)));
    }

    #[test]
    fn bug_parse_expr_single_backtick_no_panic() {
        // A single backtick used to panic similarly
        let expr = parse_expr("`");
        assert!(matches!(expr, Expr::Variable(_)));
    }

    #[test]
    fn bug_escaped_backslash_parsed() {
        // "test\\" has an escaped backslash — the closing quote is real.
        // The old code thought the quote was escaped and never closed the string.
        let tree = parse_js_condition(r#""test\\" === x"#);
        assert!(
            matches!(tree, ConditionTree::Compare { .. }),
            "expected Compare, got {:?}",
            tree
        );
    }

    #[test]
    fn bug_triple_and_all_parts_parsed() {
        // Triple && chains used to garble the right-hand side because
        // parse_and sent the right operand to parse_not instead of recursing.
        let tree = parse_js_condition("a === 1 && b === 2 && c === 3");
        // Should be And(Compare(a,1), And(Compare(b,2), Compare(c,3)))
        match &tree {
            ConditionTree::And(left, right) => {
                assert!(
                    matches!(left.as_ref(), ConditionTree::Compare { .. }),
                    "left should be Compare, got {:?}",
                    left
                );
                match right.as_ref() {
                    ConditionTree::And(mid, inner_right) => {
                        assert!(
                            matches!(mid.as_ref(), ConditionTree::Compare { .. }),
                            "mid should be Compare, got {:?}",
                            mid
                        );
                        assert!(
                            matches!(inner_right.as_ref(), ConditionTree::Compare { .. }),
                            "inner_right should be Compare, got {:?}",
                            inner_right
                        );
                    }
                    other => panic!("right should be And, got {:?}", other),
                }
            }
            other => panic!("expected And at top level, got {:?}", other),
        }
    }

    // --- bug regression tests ---

    #[test]
    fn bug_multibyte_utf8_no_panic() {
        let result = parse_js_condition("café === 'hello'");
        assert!(matches!(result, ConditionTree::Compare { .. }));
    }

    #[test]
    fn bug_unicode_emoji_no_panic() {
        let result = parse_js_condition("x === '🎉'");
        assert!(matches!(result, ConditionTree::Compare { .. }));
    }

    #[test]
    fn bug_null_gt_stays_as_compare() {
        let result = parse_js_condition("null > x");
        assert!(matches!(result, ConditionTree::Compare { .. }));
    }

    #[test]
    fn bug_x_lt_null_stays_as_compare() {
        let result = parse_js_condition("x < null");
        assert!(matches!(result, ConditionTree::Compare { .. }));
    }

    #[test]
    fn bug_in_operator_inside_string_not_matched() {
        let result = parse_js_condition(r#"x === "key in obj""#);
        assert!(matches!(result, ConditionTree::Compare { .. }));
    }

    #[test]
    fn bug_strip_parens_with_close_paren_in_string() {
        let result = strip_outer_parens(r#"(x === ")")"#);
        assert!(
            result.is_some(),
            "Should strip outer parens even with ) in string"
        );
    }
}
