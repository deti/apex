//! Recursive-descent parser for the CPG query DSL.
//!
//! Parses queries of the form:
//! ```text
//! from src in Sources("UserInput")
//! from sink in Sinks("SQLInjection")
//! where flows(src, sink) and matches(src, "request.*")
//! select src.location, sink.location
//! ```

use anyhow::{bail, Result};

use super::{Condition, DataSource, Field, QueryExpr};

/// Parse a multi-line query string into a list of `QueryExpr` clauses.
pub fn parse_query(input: &str) -> Result<Vec<QueryExpr>> {
    let mut exprs = Vec::new();
    for line in input.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let expr = parse_line(trimmed)?;
        exprs.push(expr);
    }
    if exprs.is_empty() {
        bail!("empty query: expected at least one clause");
    }
    Ok(exprs)
}

/// Parse a single line into a `QueryExpr`.
fn parse_line(line: &str) -> Result<QueryExpr> {
    if line.starts_with("from ") {
        parse_from(line)
    } else if line.starts_with("where ") {
        parse_where(line)
    } else if line.starts_with("select ") {
        parse_select(line)
    } else {
        bail!("invalid syntax: expected 'from', 'where', or 'select', got: {line}")
    }
}

/// Parse: `from <var> in <DataSource>` or `from <var> in <DataSource>(<arg>)`
fn parse_from(line: &str) -> Result<QueryExpr> {
    // "from src in Sources("UserInput")"
    let rest = &line[5..]; // skip "from "
    let parts: Vec<&str> = rest.splitn(3, ' ').collect();
    if parts.len() < 3 || parts[1] != "in" {
        bail!("invalid from clause: expected 'from <var> in <source>', got: {line}");
    }
    let variable = parts[0].to_string();
    let source_str = parts[2].trim();
    let source = parse_data_source(source_str)?;
    Ok(QueryExpr::From { variable, source })
}

/// Parse a data source like `Sources("UserInput")`, `Calls`, `NodeType("Call")`.
fn parse_data_source(s: &str) -> Result<DataSource> {
    if let Some(arg) = extract_func_arg(s, "Sources") {
        Ok(DataSource::Sources(arg))
    } else if let Some(arg) = extract_func_arg(s, "Sinks") {
        Ok(DataSource::Sinks(arg))
    } else if s == "Calls" {
        Ok(DataSource::Calls)
    } else if s == "Assignments" {
        Ok(DataSource::Assignments)
    } else if s == "Functions" {
        Ok(DataSource::Functions)
    } else if let Some(arg) = extract_func_arg(s, "NodeType") {
        Ok(DataSource::NodeType(arg))
    } else {
        bail!("unknown data source: {s}")
    }
}

/// Extract the string argument from a function-call-like syntax: `Name("arg")` -> Some("arg").
fn extract_func_arg(s: &str, func_name: &str) -> Option<String> {
    let prefix = format!("{func_name}(");
    if s.starts_with(&prefix) && s.ends_with(')') {
        let inner = &s[prefix.len()..s.len() - 1];
        // Strip quotes if present
        let inner = inner.trim_matches('"');
        Some(inner.to_string())
    } else {
        None
    }
}

/// Parse: `where <condition> [and|or <condition>]*`
fn parse_where(line: &str) -> Result<QueryExpr> {
    let rest = &line[6..]; // skip "where "
    let condition = parse_condition(rest.trim())?;
    Ok(QueryExpr::Where { condition })
}

/// Parse a condition expression with `and`/`or` support.
fn parse_condition(s: &str) -> Result<Condition> {
    // Split on " and " and " or " respecting precedence (and binds tighter).
    // Simple approach: split on " or " first, then " and " within each part.
    let or_parts = split_respecting_parens(s, " or ");
    if or_parts.len() > 1 {
        let mut cond = parse_and_conditions(&or_parts[0])?;
        for part in &or_parts[1..] {
            let right = parse_and_conditions(part)?;
            cond = Condition::Or(Box::new(cond), Box::new(right));
        }
        return Ok(cond);
    }
    parse_and_conditions(s)
}

/// Parse conditions joined by " and ".
fn parse_and_conditions(s: &str) -> Result<Condition> {
    let parts = split_respecting_parens(s, " and ");
    if parts.len() > 1 {
        let mut cond = parse_atomic_condition(parts[0].trim())?;
        for part in &parts[1..] {
            let right = parse_atomic_condition(part.trim())?;
            cond = Condition::And(Box::new(cond), Box::new(right));
        }
        return Ok(cond);
    }
    parse_atomic_condition(s.trim())
}

/// Parse a single atomic condition: `flows(a, b)`, `matches(a, "pat")`, `not(...)`,
/// `sanitized(a, b, kind)`.
fn parse_atomic_condition(s: &str) -> Result<Condition> {
    if s.starts_with("not(") && s.ends_with(')') {
        let inner = &s[4..s.len() - 1];
        let cond = parse_atomic_condition(inner.trim())?;
        return Ok(Condition::Not(Box::new(cond)));
    }

    if s.starts_with("flows(") && s.ends_with(')') {
        let inner = &s[6..s.len() - 1];
        let args = parse_args(inner, 2)?;
        return Ok(Condition::Flows(args[0].clone(), args[1].clone()));
    }

    if s.starts_with("sanitized(") && s.ends_with(')') {
        let inner = &s[10..s.len() - 1];
        let args = parse_args(inner, 3)?;
        return Ok(Condition::Sanitized(
            args[0].clone(),
            args[1].clone(),
            args[2].clone(),
        ));
    }

    if s.starts_with("matches(") && s.ends_with(')') {
        let inner = &s[8..s.len() - 1];
        let args = parse_args(inner, 2)?;
        return Ok(Condition::Matches(args[0].clone(), args[1].clone()));
    }

    bail!("unknown condition: {s}")
}

/// Parse comma-separated arguments, stripping quotes.
fn parse_args(s: &str, expected: usize) -> Result<Vec<String>> {
    let parts: Vec<String> = s
        .split(',')
        .map(|p| p.trim().trim_matches('"').to_string())
        .collect();
    if parts.len() != expected {
        bail!("expected {expected} arguments, got {}: {s}", parts.len());
    }
    Ok(parts)
}

/// Parse: `select <field>, <field>, ...`
fn parse_select(line: &str) -> Result<QueryExpr> {
    let rest = &line[7..]; // skip "select "
    let fields: Result<Vec<Field>> = rest
        .split(',')
        .map(|f| {
            let f = f.trim();
            let parts: Vec<&str> = f.splitn(2, '.').collect();
            if parts.len() != 2 {
                bail!("invalid field: expected 'variable.attribute', got: {f}");
            }
            Ok(Field {
                variable: parts[0].to_string(),
                attribute: parts[1].to_string(),
            })
        })
        .collect();
    Ok(QueryExpr::Select { fields: fields? })
}

/// Split a string on a delimiter, but only at the top level (not inside parentheses).
fn split_respecting_parens(s: &str, delim: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut depth = 0u32;
    let mut current_start = 0;
    let bytes = s.as_bytes();
    let delim_bytes = delim.as_bytes();

    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'(' {
            depth += 1;
        } else if bytes[i] == b')' {
            depth = depth.saturating_sub(1);
        }
        if depth == 0
            && i + delim_bytes.len() <= bytes.len()
            && &bytes[i..i + delim_bytes.len()] == delim_bytes
        {
            parts.push(s[current_start..i].to_string());
            i += delim_bytes.len();
            current_start = i;
            continue;
        }
        i += 1;
    }
    parts.push(s[current_start..].to_string());
    parts
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_from_clause() {
        let exprs = parse_query("from src in Sources(\"UserInput\")").unwrap();
        assert_eq!(exprs.len(), 1);
        match &exprs[0] {
            QueryExpr::From { variable, source } => {
                assert_eq!(variable, "src");
                assert_eq!(*source, DataSource::Sources("UserInput".into()));
            }
            _ => panic!("expected From"),
        }
    }

    #[test]
    fn parse_from_calls() {
        let exprs = parse_query("from c in Calls").unwrap();
        match &exprs[0] {
            QueryExpr::From { variable, source } => {
                assert_eq!(variable, "c");
                assert_eq!(*source, DataSource::Calls);
            }
            _ => panic!("expected From"),
        }
    }

    #[test]
    fn parse_from_functions() {
        let exprs = parse_query("from f in Functions").unwrap();
        match &exprs[0] {
            QueryExpr::From { variable, source } => {
                assert_eq!(variable, "f");
                assert_eq!(*source, DataSource::Functions);
            }
            _ => panic!("expected From"),
        }
    }

    #[test]
    fn parse_from_assignments() {
        let exprs = parse_query("from a in Assignments").unwrap();
        match &exprs[0] {
            QueryExpr::From { variable, source } => {
                assert_eq!(variable, "a");
                assert_eq!(*source, DataSource::Assignments);
            }
            _ => panic!("expected From"),
        }
    }

    #[test]
    fn parse_from_node_type() {
        let exprs = parse_query("from n in NodeType(\"Literal\")").unwrap();
        match &exprs[0] {
            QueryExpr::From { variable, source } => {
                assert_eq!(variable, "n");
                assert_eq!(*source, DataSource::NodeType("Literal".into()));
            }
            _ => panic!("expected From"),
        }
    }

    #[test]
    fn parse_where_flows() {
        let exprs = parse_query("where flows(src, sink)").unwrap();
        match &exprs[0] {
            QueryExpr::Where { condition } => {
                assert_eq!(*condition, Condition::Flows("src".into(), "sink".into()));
            }
            _ => panic!("expected Where"),
        }
    }

    #[test]
    fn parse_where_matches() {
        let exprs = parse_query("where matches(src, \"request.*\")").unwrap();
        match &exprs[0] {
            QueryExpr::Where { condition } => {
                assert_eq!(
                    *condition,
                    Condition::Matches("src".into(), "request.*".into())
                );
            }
            _ => panic!("expected Where"),
        }
    }

    #[test]
    fn parse_where_compound_and() {
        let exprs = parse_query("where flows(src, sink) and matches(src, \"eval\")").unwrap();
        match &exprs[0] {
            QueryExpr::Where { condition } => {
                assert!(matches!(condition, Condition::And(..)));
                if let Condition::And(left, right) = condition {
                    assert_eq!(**left, Condition::Flows("src".into(), "sink".into()));
                    assert_eq!(**right, Condition::Matches("src".into(), "eval".into()));
                }
            }
            _ => panic!("expected Where"),
        }
    }

    #[test]
    fn parse_where_compound_or() {
        let exprs = parse_query("where flows(a, b) or flows(c, d)").unwrap();
        match &exprs[0] {
            QueryExpr::Where { condition } => {
                assert!(matches!(condition, Condition::Or(..)));
            }
            _ => panic!("expected Where"),
        }
    }

    #[test]
    fn parse_where_not() {
        let exprs = parse_query("where not(sanitized(a, b, xss))").unwrap();
        match &exprs[0] {
            QueryExpr::Where { condition } => {
                assert!(matches!(condition, Condition::Not(..)));
            }
            _ => panic!("expected Where"),
        }
    }

    #[test]
    fn parse_select_fields() {
        let exprs = parse_query("select src.location, sink.name").unwrap();
        match &exprs[0] {
            QueryExpr::Select { fields } => {
                assert_eq!(fields.len(), 2);
                assert_eq!(fields[0].variable, "src");
                assert_eq!(fields[0].attribute, "location");
                assert_eq!(fields[1].variable, "sink");
                assert_eq!(fields[1].attribute, "name");
            }
            _ => panic!("expected Select"),
        }
    }

    #[test]
    fn parse_full_query() {
        let q = r#"from source in Sources("UserInput")
from sink in Sinks("SQLInjection")
where flows(source, sink)
select source.location, sink.location"#;
        let exprs = parse_query(q).unwrap();
        assert_eq!(exprs.len(), 4);
        assert!(matches!(exprs[0], QueryExpr::From { .. }));
        assert!(matches!(exprs[1], QueryExpr::From { .. }));
        assert!(matches!(exprs[2], QueryExpr::Where { .. }));
        assert!(matches!(exprs[3], QueryExpr::Select { .. }));
    }

    #[test]
    fn parse_error_invalid_syntax() {
        let result = parse_query("invalid line here");
        assert!(result.is_err());
    }

    #[test]
    fn parse_error_empty_query() {
        let result = parse_query("");
        assert!(result.is_err());
    }

    #[test]
    fn parse_error_bad_from() {
        let result = parse_query("from x");
        assert!(result.is_err());
    }

    #[test]
    fn parse_error_bad_select_field() {
        let result = parse_query("select noattribute");
        assert!(result.is_err());
    }
}
