//! Simplified line-based Python CPG builder.
//!
//! Parses basic Python patterns without tree-sitter:
//! - `def name(params):` → Method + Parameter nodes
//! - `name(args)` → Call + Argument nodes
//! - `lhs = rhs` → Assignment + Identifier nodes
//! - `if/while/for` → ControlStructure nodes
//! - `return expr` → Return node
//!
//! Sequential statements within a function body are connected by CFG edges.
//! All statement nodes are connected to their enclosing Method via AST edges.

use crate::{Cpg, CtrlKind, EdgeKind, NodeKind};

/// Build a CPG from Python source code.
///
/// This is a simplified builder that parses basic Python patterns without
/// requiring tree-sitter. Good enough to demonstrate taint-flow detection.
pub fn build_python_cpg(source: &str, filename: &str) -> Cpg {
    let mut cpg = Cpg::new();
    let mut parser = PythonCpgBuilder::new(filename);
    parser.parse(source, &mut cpg);
    cpg
}

// ─── Internal builder state ───────────────────────────────────────────────────

struct PythonCpgBuilder<'a> {
    filename: &'a str,
}

impl<'a> PythonCpgBuilder<'a> {
    fn new(filename: &'a str) -> Self {
        Self { filename }
    }

    fn parse(&mut self, source: &str, cpg: &mut Cpg) {
        let lines: Vec<&str> = source.lines().collect();
        let mut i = 0;
        while i < lines.len() {
            let line = lines[i];
            let trimmed = line.trim();
            if trimmed.starts_with("def ") {
                i = self.parse_function(lines.as_slice(), i, cpg);
            } else {
                i += 1;
            }
        }
    }

    /// Parse a `def name(params):` block and all indented body lines.
    /// Returns the index of the first line *after* the function body.
    fn parse_function(&self, lines: &[&str], def_idx: usize, cpg: &mut Cpg) -> usize {
        let def_line = lines[def_idx].trim();
        let line_no = (def_idx + 1) as u32;

        let (fn_name, params) = parse_def_signature(def_line);
        let method_id = cpg.add_node(NodeKind::Method {
            name: fn_name.clone(),
            file: self.filename.to_string(),
            line: line_no,
        });

        // Parameter nodes
        for (idx, param) in params.iter().enumerate() {
            let p_id = cpg.add_node(NodeKind::Parameter {
                name: param.clone(),
                index: idx as u32,
            });
            cpg.add_edge(method_id, p_id, EdgeKind::Ast);
        }

        // Determine body indentation: first non-empty line after `def`
        let body_indent = body_indentation(lines, def_idx + 1);

        // Collect body statement nodes in order for CFG chaining
        let mut prev_stmt: Option<u32> = None;
        let mut i = def_idx + 1;

        while i < lines.len() {
            let raw = lines[i];
            if raw.trim().is_empty() {
                i += 1;
                continue;
            }
            let indent = leading_spaces(raw);
            if indent < body_indent {
                // Back to outer scope — function body ends
                break;
            }

            let stmt_line = raw.trim();
            let stmt_line_no = (i + 1) as u32;

            let stmt_id = self.parse_statement(stmt_line, stmt_line_no, method_id, cpg);

            if let Some(sid) = stmt_id {
                // AST: method → statement
                cpg.add_edge(method_id, sid, EdgeKind::Ast);
                // CFG: previous statement → this statement
                if let Some(prev) = prev_stmt {
                    cpg.add_edge(prev, sid, EdgeKind::Cfg);
                }
                prev_stmt = Some(sid);
            }

            i += 1;
        }

        i
    }

    /// Parse a single statement line and return the primary node id (if any).
    fn parse_statement(
        &self,
        stmt: &str,
        line_no: u32,
        _method_id: u32,
        cpg: &mut Cpg,
    ) -> Option<u32> {
        // Skip blank/comment/pass
        if stmt.is_empty() || stmt.starts_with('#') || stmt == "pass" {
            return None;
        }

        // return <expr>
        if stmt.starts_with("return") {
            let ret_id = cpg.add_node(NodeKind::Return { line: line_no });
            let rest = stmt.trim_start_matches("return").trim();
            if !rest.is_empty() {
                // Parse the return expression as a potential call or identifier
                self.attach_expr(rest, line_no, ret_id, 0, cpg);
            }
            return Some(ret_id);
        }

        // Control structures: if / while / for / try
        if let Some(ctrl) = parse_ctrl(stmt, line_no) {
            return Some(cpg.add_node(ctrl));
        }

        // Assignment: `lhs = rhs` or augmented `lhs += rhs`
        // But skip `==` comparisons.
        if let Some((lhs, rhs)) = parse_assignment(stmt) {
            let assign_id = cpg.add_node(NodeKind::Assignment {
                lhs: lhs.to_string(),
                line: line_no,
            });
            // rhs may contain calls or identifiers
            self.attach_expr(rhs.trim(), line_no, assign_id, 0, cpg);
            return Some(assign_id);
        }

        // Bare call expression: `name(args)`
        if let Some(call_id) = self.try_parse_call(stmt, line_no, cpg) {
            return Some(call_id);
        }

        // Fallback: treat as an identifier reference
        let name = stmt.split('(').next().unwrap_or(stmt).trim().to_string();
        if !name.is_empty()
            && name
                .chars()
                .all(|c| c.is_alphanumeric() || c == '_' || c == '.')
        {
            return Some(cpg.add_node(NodeKind::Identifier {
                name,
                line: line_no,
            }));
        }

        None
    }

    /// Attach expression nodes (calls, identifiers, literals) as children of `parent`.
    fn attach_expr(&self, expr: &str, line_no: u32, parent: u32, arg_index: u32, cpg: &mut Cpg) {
        let expr = expr.trim();
        if expr.is_empty() {
            return;
        }

        // f-string or string literal
        if expr.starts_with('"')
            || expr.starts_with('\'')
            || expr.starts_with("f\"")
            || expr.starts_with("f'")
            || expr.starts_with("b\"")
            || expr.starts_with("b'")
        {
            let lit_id = cpg.add_node(NodeKind::Literal {
                value: expr.to_string(),
                line: line_no,
            });
            cpg.add_edge(parent, lit_id, EdgeKind::Argument { index: arg_index });
            return;
        }

        // Numeric literal
        if expr
            .chars()
            .next()
            .map(|c| c.is_ascii_digit())
            .unwrap_or(false)
        {
            let lit_id = cpg.add_node(NodeKind::Literal {
                value: expr.to_string(),
                line: line_no,
            });
            cpg.add_edge(parent, lit_id, EdgeKind::Argument { index: arg_index });
            return;
        }

        // Call expression — highest priority before identifier fallback
        if let Some(call_id) = self.try_parse_call(expr, line_no, cpg) {
            cpg.add_edge(parent, call_id, EdgeKind::Argument { index: arg_index });
            return;
        }

        // Plain identifier or dotted name
        let name = expr
            .split(|c: char| !c.is_alphanumeric() && c != '_' && c != '.')
            .next()
            .unwrap_or(expr)
            .trim()
            .to_string();
        if !name.is_empty() {
            let id_node = cpg.add_node(NodeKind::Identifier {
                name,
                line: line_no,
            });
            cpg.add_edge(parent, id_node, EdgeKind::Argument { index: arg_index });
        }
    }

    /// Try to parse `expr` as a call like `name(...)` or `obj.method(...)`.
    /// Returns the Call node id if successful.
    fn try_parse_call(&self, expr: &str, line_no: u32, cpg: &mut Cpg) -> Option<u32> {
        // Find first `(` that looks like a call (preceded by identifier chars)
        let paren = expr.find('(')?;
        if paren == 0 {
            return None;
        }
        let callee = &expr[..paren];
        // Callee must be a valid dotted name
        if !callee
            .chars()
            .all(|c| c.is_alphanumeric() || c == '_' || c == '.')
        {
            return None;
        }
        // Must have a closing paren somewhere
        let close = expr.rfind(')')?;
        if close < paren {
            return None;
        }

        let call_id = cpg.add_node(NodeKind::Call {
            name: callee.to_string(),
            line: line_no,
        });

        // Parse arguments (shallow — split on commas, ignore nested parens for simplicity)
        let args_str = &expr[paren + 1..close];
        let args = split_args(args_str);
        for (idx, arg) in args.iter().enumerate() {
            let arg = arg.trim();
            if !arg.is_empty() {
                self.attach_expr(arg, line_no, call_id, idx as u32, cpg);
            }
        }

        Some(call_id)
    }
}

// ─── Parsing helpers ─────────────────────────────────────────────────────────

/// Parse `def name(p1, p2, ...):` → (name, [params])
fn parse_def_signature(line: &str) -> (String, Vec<String>) {
    let line = line.trim_start_matches("def ").trim();
    let paren = line.find('(').unwrap_or(line.len());
    let name = line[..paren].trim().to_string();
    let params = if let (Some(open), Some(close)) = (line.find('('), line.find(')')) {
        let inner = &line[open + 1..close];
        inner
            .split(',')
            .map(|p| {
                // Strip type annotations and defaults
                p.split(':')
                    .next()
                    .unwrap_or(p)
                    .split('=')
                    .next()
                    .unwrap_or(p)
                    .trim()
                    .to_string()
            })
            .filter(|p| !p.is_empty() && p != "self")
            .collect()
    } else {
        vec![]
    };
    (name, params)
}

/// Parse control-structure keywords at the start of a statement.
fn parse_ctrl(stmt: &str, line_no: u32) -> Option<NodeKind> {
    let kind = if stmt.starts_with("if ") || stmt == "if:" {
        CtrlKind::If
    } else if stmt.starts_with("while ") || stmt == "while:" {
        CtrlKind::While
    } else if stmt.starts_with("for ") || stmt == "for:" {
        CtrlKind::For
    } else if stmt.starts_with("try:") || stmt == "try" {
        CtrlKind::Try
    } else {
        return None;
    };
    Some(NodeKind::ControlStructure {
        kind,
        line: line_no,
    })
}

/// Detect `lhs = rhs` (but not `==`, `!=`, `<=`, `>=`).
/// Returns `Some((lhs, rhs))` on match.
fn parse_assignment(stmt: &str) -> Option<(&str, &str)> {
    // Find `=` that is not preceded by `!`, `<`, `>`, `=` and not followed by `=`
    let bytes = stmt.as_bytes();
    for (i, &b) in bytes.iter().enumerate() {
        if b == b'=' {
            let prev = if i > 0 { bytes[i - 1] } else { 0 };
            let next = if i + 1 < bytes.len() { bytes[i + 1] } else { 0 };
            if prev == b'!' || prev == b'<' || prev == b'>' || prev == b'=' || next == b'=' {
                continue;
            }
            // augmented assignment: `+=`, `-=`, `*=`, `/=`
            let lhs = if prev == b'+' || prev == b'-' || prev == b'*' || prev == b'/' {
                stmt[..i - 1].trim()
            } else {
                stmt[..i].trim()
            };
            let rhs = stmt[i + 1..].trim();
            if !lhs.is_empty() && !rhs.is_empty() {
                return Some((lhs, rhs));
            }
        }
    }
    None
}

/// Number of leading spaces (proxy for indentation).
fn leading_spaces(line: &str) -> usize {
    line.len() - line.trim_start().len()
}

/// Find the indentation level of the function body.
fn body_indentation(lines: &[&str], start: usize) -> usize {
    for line in lines[start..].iter() {
        if !line.trim().is_empty() {
            return leading_spaces(line);
        }
    }
    4 // default Python indent
}

/// Split a comma-separated argument list, respecting nested parentheses.
fn split_args(args: &str) -> Vec<&str> {
    let mut result = Vec::new();
    let mut depth = 0usize;
    let mut start = 0;
    for (i, c) in args.char_indices() {
        match c {
            '(' | '[' | '{' => depth += 1,
            ')' | ']' | '}' => depth = depth.saturating_sub(1),
            ',' if depth == 0 => {
                result.push(&args[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    if start < args.len() {
        result.push(&args[start..]);
    }
    result
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::NodeKind;

    fn method_names(cpg: &Cpg) -> Vec<String> {
        cpg.nodes()
            .filter_map(|(_, k)| match k {
                NodeKind::Method { name, .. } => Some(name.clone()),
                _ => None,
            })
            .collect()
    }

    fn call_names(cpg: &Cpg) -> Vec<String> {
        cpg.nodes()
            .filter_map(|(_, k)| match k {
                NodeKind::Call { name, .. } => Some(name.clone()),
                _ => None,
            })
            .collect()
    }

    fn param_names(cpg: &Cpg) -> Vec<String> {
        cpg.nodes()
            .filter_map(|(_, k)| match k {
                NodeKind::Parameter { name, .. } => Some(name.clone()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn builder_creates_method_node() {
        let cpg = build_python_cpg("def foo():\n    pass\n", "test.py");
        assert!(method_names(&cpg).contains(&"foo".to_string()));
    }

    #[test]
    fn builder_parses_parameters() {
        let cpg = build_python_cpg("def greet(name, age):\n    pass\n", "test.py");
        let params = param_names(&cpg);
        assert!(params.contains(&"name".to_string()));
        assert!(params.contains(&"age".to_string()));
    }

    #[test]
    fn builder_skips_self_parameter() {
        let cpg = build_python_cpg("def method(self, x):\n    pass\n", "test.py");
        let params = param_names(&cpg);
        assert!(!params.contains(&"self".to_string()));
        assert!(params.contains(&"x".to_string()));
    }

    #[test]
    fn builder_detects_function_calls() {
        let cpg = build_python_cpg("def foo():\n    subprocess.run(cmd)\n", "test.py");
        assert!(call_names(&cpg).contains(&"subprocess.run".to_string()));
    }

    #[test]
    fn builder_detects_assignment() {
        let cpg = build_python_cpg("def foo():\n    x = 42\n", "test.py");
        let has_assign = cpg
            .nodes()
            .any(|(_, k)| matches!(k, NodeKind::Assignment { lhs, .. } if lhs == "x"));
        assert!(has_assign);
    }

    #[test]
    fn builder_creates_cfg_edges_between_statements() {
        let source = "def foo():\n    x = 1\n    y = 2\n    z = 3\n";
        let cpg = build_python_cpg(source, "test.py");
        // Three assignments → two CFG edges chaining them
        let cfg_count = cpg
            .edges()
            .filter(|(_, _, k)| matches!(k, EdgeKind::Cfg))
            .count();
        assert!(cfg_count >= 2, "expected ≥2 CFG edges, got {cfg_count}");
    }

    #[test]
    fn builder_creates_ast_edges_from_method() {
        let source = "def foo():\n    x = 1\n    bar()\n";
        let cpg = build_python_cpg(source, "test.py");
        let method_id = cpg
            .nodes()
            .find_map(|(id, k)| matches!(k, NodeKind::Method { .. }).then_some(id))
            .expect("method node");
        let ast_count = cpg
            .edges_from(method_id)
            .iter()
            .filter(|(_, _, k)| matches!(k, EdgeKind::Ast))
            .count();
        // method → parameter(s) + each body statement
        assert!(
            ast_count >= 1,
            "expected AST edges from method, got {ast_count}"
        );
    }

    #[test]
    fn builder_detects_control_structures() {
        let source = "def foo(x):\n    if x > 0:\n        pass\n";
        let cpg = build_python_cpg(source, "test.py");
        let has_if = cpg.nodes().any(|(_, k)| {
            matches!(
                k,
                NodeKind::ControlStructure {
                    kind: CtrlKind::If,
                    ..
                }
            )
        });
        assert!(has_if);
    }

    #[test]
    fn builder_detects_return() {
        let source = "def foo():\n    return 42\n";
        let cpg = build_python_cpg(source, "test.py");
        assert!(cpg
            .nodes()
            .any(|(_, k)| matches!(k, NodeKind::Return { .. })));
    }

    #[test]
    fn builder_multiple_functions() {
        let source = "def alpha():\n    pass\n\ndef beta():\n    pass\n";
        let cpg = build_python_cpg(source, "test.py");
        let names = method_names(&cpg);
        assert!(names.contains(&"alpha".to_string()));
        assert!(names.contains(&"beta".to_string()));
    }

    #[test]
    fn builder_argument_edges() {
        let source = "def foo():\n    bar(x, y)\n";
        let cpg = build_python_cpg(source, "test.py");
        let call_id = cpg
            .nodes()
            .find_map(|(id, k)| {
                matches!(k, NodeKind::Call { name, .. } if name == "bar").then_some(id)
            })
            .expect("call node");
        let arg_edges = cpg
            .edges_from(call_id)
            .iter()
            .filter(|(_, _, k)| matches!(k, EdgeKind::Argument { .. }))
            .count();
        assert_eq!(arg_edges, 2);
    }
}
