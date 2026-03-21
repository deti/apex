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
//!
//! The [`CpgBuilder`] trait allows future language builders (JS, Java, Go, …)
//! to plug into the same pipeline without changing call sites.

use apex_core::types::Language;

use crate::{Cpg, CtrlKind, EdgeKind, NodeKind};

// ─── CpgBuilder trait ─────────────────────────────────────────────────────────

/// A language-specific Code Property Graph builder.
///
/// Each language that APEX supports provides one implementation. The trait is
/// object-safe so builders can be stored as `Box<dyn CpgBuilder>`.
pub trait CpgBuilder: Send + Sync {
    /// Build a CPG from `source` code stored in `filename`.
    fn build(&self, source: &str, filename: &str) -> Cpg;

    /// The language this builder handles.
    fn language(&self) -> Language;
}

// ─── Python implementation ────────────────────────────────────────────────────

/// A [`CpgBuilder`] for Python source files.
///
/// Uses a simplified line-based parser — no tree-sitter dependency — that
/// understands `def`, assignments, calls, control structures, and `return`.
pub struct PythonCpgBuilder;

impl CpgBuilder for PythonCpgBuilder {
    fn build(&self, source: &str, filename: &str) -> Cpg {
        build_python_cpg(source, filename)
    }

    fn language(&self) -> Language {
        Language::Python
    }
}

// ─── JavaScript implementation ────────────────────────────────────────────────

/// A [`CpgBuilder`] for JavaScript source files.
///
/// Uses a simplified line-based parser that understands `function` declarations,
/// assignments, calls, and `return` statements — sufficient for taint-flow
/// analysis without a tree-sitter dependency.
pub struct JsCpgBuilder;

impl CpgBuilder for JsCpgBuilder {
    fn build(&self, source: &str, filename: &str) -> Cpg {
        build_js_cpg(source, filename)
    }

    fn language(&self) -> Language {
        Language::JavaScript
    }
}

/// Build a CPG from JavaScript source code.
pub fn build_js_cpg(source: &str, filename: &str) -> Cpg {
    let mut cpg = Cpg::new();
    let mut parser = InternalJsParser::new(filename);
    parser.parse(source, &mut cpg);
    cpg
}

// ─── Go implementation ────────────────────────────────────────────────────────

/// A [`CpgBuilder`] for Go source files.
///
/// Uses a simplified line-based parser that understands `func` declarations,
/// assignments, calls, and `return` statements.
pub struct GoCpgBuilder;

impl CpgBuilder for GoCpgBuilder {
    fn build(&self, source: &str, filename: &str) -> Cpg {
        build_go_cpg(source, filename)
    }

    fn language(&self) -> Language {
        Language::Go
    }
}

/// Build a CPG from Go source code.
pub fn build_go_cpg(source: &str, filename: &str) -> Cpg {
    let mut cpg = Cpg::new();
    let mut parser = InternalGoParser::new(filename);
    parser.parse(source, &mut cpg);
    cpg
}

// ─── Free-function convenience wrapper ────────────────────────────────────────

/// Build a CPG from Python source code.
///
/// When the `treesitter` feature is enabled, uses a tree-sitter-based parser
/// that correctly handles nested calls, decorators, comprehensions, multi-line
/// statements, and f-strings. Otherwise falls back to the simplified line-based
/// parser.
pub fn build_python_cpg(source: &str, filename: &str) -> Cpg {
    #[cfg(feature = "treesitter")]
    {
        crate::ts_python::build_ts_python_cpg(source, filename)
    }
    #[cfg(not(feature = "treesitter"))]
    {
        let mut cpg = Cpg::new();
        let mut parser = InternalPythonParser::new(filename);
        parser.parse(source, &mut cpg);
        cpg
    }
}

// ─── Internal builder state ───────────────────────────────────────────────────

#[cfg(not(feature = "treesitter"))]
struct InternalPythonParser<'a> {
    filename: &'a str,
}

#[cfg(not(feature = "treesitter"))]
impl<'a> InternalPythonParser<'a> {
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
#[cfg(not(feature = "treesitter"))]
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
#[cfg(not(feature = "treesitter"))]
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
            // Augmented assignment operators: +=, -=, *=, /=, %=, &=, |=, ^=, **=, //=
            let augmented_single = prev == b'+'
                || prev == b'-'
                || prev == b'*'
                || prev == b'/'
                || prev == b'%'
                || prev == b'&'
                || prev == b'|'
                || prev == b'^';
            let lhs = if augmented_single {
                // Check for double-char operators: **= and //=
                let prev2 = if i >= 2 { bytes[i - 2] } else { 0 };
                if (prev == b'*' && prev2 == b'*') || (prev == b'/' && prev2 == b'/') {
                    stmt[..i - 2].trim()
                } else {
                    stmt[..i - 1].trim()
                }
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

// ─── JavaScript internal parser ──────────────────────────────────────────────

/// Simplified line-based JavaScript CPG builder.
///
/// Recognises:
/// - `function name(params)` and `const/let/var name = (params) =>` declarations
/// - `name(args)` call expressions
/// - `lhs = rhs` assignments
/// - `if`/`while`/`for` control structures
/// - `return` statements
struct InternalJsParser<'a> {
    filename: &'a str,
}

impl<'a> InternalJsParser<'a> {
    fn new(filename: &'a str) -> Self {
        Self { filename }
    }

    fn parse(&mut self, source: &str, cpg: &mut Cpg) {
        let lines: Vec<&str> = source.lines().collect();
        let mut i = 0;
        while i < lines.len() {
            let trimmed = lines[i].trim();
            if trimmed.starts_with("function ") {
                i = self.parse_function(lines.as_slice(), i, cpg);
            } else if let Some(name) = Self::detect_arrow_or_func_expr(trimmed) {
                i = self.parse_named_block(lines.as_slice(), i, name, cpg);
            } else {
                i += 1;
            }
        }
    }

    /// Detect `const/let/var name = function(...) {` or `const name = (...) => {`.
    fn detect_arrow_or_func_expr(line: &str) -> Option<String> {
        for prefix in &["const ", "let ", "var "] {
            if let Some(rest) = line.strip_prefix(prefix) {
                let name_end = rest.find(|c: char| !c.is_alphanumeric() && c != '_')?;
                let name = &rest[..name_end];
                let after = rest[name_end..].trim_start();
                if let Some(rhs_raw) = after.strip_prefix('=') {
                    let rhs = rhs_raw.trim();
                    if rhs.starts_with("function") || rhs.contains("=>") {
                        return Some(name.to_string());
                    }
                }
            }
        }
        None
    }

    fn parse_function(&self, lines: &[&str], def_idx: usize, cpg: &mut Cpg) -> usize {
        let def_line = lines[def_idx].trim();
        let line_no = (def_idx + 1) as u32;
        // Extract name from `function name(`
        let after_fn = def_line.trim_start_matches("function").trim();
        let paren = after_fn.find('(').unwrap_or(after_fn.len());
        let fn_name = after_fn[..paren].trim().to_string();
        let params = if let (Some(open), Some(close)) = (after_fn.find('('), after_fn.find(')')) {
            let inner = &after_fn[open + 1..close];
            inner
                .split(',')
                .map(|p| p.split('=').next().unwrap_or(p).trim().to_string())
                .filter(|p| !p.is_empty())
                .collect::<Vec<_>>()
        } else {
            vec![]
        };
        self.emit_function(lines, def_idx, line_no, fn_name, params, cpg)
    }

    fn parse_named_block(
        &self,
        lines: &[&str],
        def_idx: usize,
        name: String,
        cpg: &mut Cpg,
    ) -> usize {
        let line_no = (def_idx + 1) as u32;
        self.emit_function(lines, def_idx, line_no, name, vec![], cpg)
    }

    fn emit_function(
        &self,
        lines: &[&str],
        def_idx: usize,
        line_no: u32,
        fn_name: String,
        params: Vec<String>,
        cpg: &mut Cpg,
    ) -> usize {
        let method_id = cpg.add_node(NodeKind::Method {
            name: fn_name,
            file: self.filename.to_string(),
            line: line_no,
        });
        for (idx, param) in params.iter().enumerate() {
            let p_id = cpg.add_node(NodeKind::Parameter {
                name: param.clone(),
                index: idx as u32,
            });
            cpg.add_edge(method_id, p_id, EdgeKind::Ast);
        }

        // Find the opening brace to determine body indentation
        let body_indent = body_indentation(lines, def_idx + 1);
        let mut prev_stmt: Option<u32> = None;
        let mut i = def_idx + 1;

        while i < lines.len() {
            let raw = lines[i];
            let trimmed = raw.trim();
            if trimmed.is_empty() || trimmed == "}" {
                let indent = leading_spaces(raw);
                if !trimmed.is_empty() && indent < body_indent {
                    break;
                }
                i += 1;
                continue;
            }
            let indent = leading_spaces(raw);
            if indent < body_indent {
                break;
            }
            if let Some(sid) = self.parse_js_stmt(trimmed, (i + 1) as u32, cpg) {
                cpg.add_edge(method_id, sid, EdgeKind::Ast);
                if let Some(prev) = prev_stmt {
                    cpg.add_edge(prev, sid, EdgeKind::Cfg);
                }
                prev_stmt = Some(sid);
            }
            i += 1;
        }
        i
    }

    fn parse_js_stmt(&self, stmt: &str, line_no: u32, cpg: &mut Cpg) -> Option<u32> {
        // Strip trailing semicolons and braces for matching
        let stmt = stmt.trim_end_matches([';', '{']).trim();
        if stmt.is_empty() || stmt.starts_with("//") {
            return None;
        }
        // return
        if stmt.starts_with("return") {
            let ret_id = cpg.add_node(NodeKind::Return { line: line_no });
            let rest = stmt.trim_start_matches("return").trim();
            if !rest.is_empty() {
                self.js_attach_expr(rest, line_no, ret_id, 0, cpg);
            }
            return Some(ret_id);
        }
        // control structures
        if let Some(ctrl) = parse_js_ctrl(stmt, line_no) {
            return Some(cpg.add_node(ctrl));
        }
        // variable declaration with assignment: const/let/var name = ...
        let decl_stmt = stmt
            .trim_start_matches("const ")
            .trim_start_matches("let ")
            .trim_start_matches("var ");
        if let Some((lhs, rhs)) = parse_assignment(decl_stmt) {
            let assign_id = cpg.add_node(NodeKind::Assignment {
                lhs: lhs.to_string(),
                line: line_no,
            });
            self.js_attach_expr(rhs.trim(), line_no, assign_id, 0, cpg);
            return Some(assign_id);
        }
        // bare assignment
        if let Some((lhs, rhs)) = parse_assignment(stmt) {
            let assign_id = cpg.add_node(NodeKind::Assignment {
                lhs: lhs.to_string(),
                line: line_no,
            });
            self.js_attach_expr(rhs.trim(), line_no, assign_id, 0, cpg);
            return Some(assign_id);
        }
        // bare call
        if let Some(call_id) = self.js_try_call(stmt, line_no, cpg) {
            return Some(call_id);
        }
        None
    }

    fn js_attach_expr(&self, expr: &str, line_no: u32, parent: u32, arg_index: u32, cpg: &mut Cpg) {
        let expr = expr.trim().trim_end_matches(';').trim();
        if expr.is_empty() {
            return;
        }
        if let Some(call_id) = self.js_try_call(expr, line_no, cpg) {
            cpg.add_edge(parent, call_id, EdgeKind::Argument { index: arg_index });
            return;
        }
        let name = expr
            .split(|c: char| !c.is_alphanumeric() && c != '_' && c != '.')
            .next()
            .unwrap_or(expr)
            .trim()
            .to_string();
        if !name.is_empty() {
            let id = cpg.add_node(NodeKind::Identifier { name, line: line_no });
            cpg.add_edge(parent, id, EdgeKind::Argument { index: arg_index });
        }
    }

    fn js_try_call(&self, expr: &str, line_no: u32, cpg: &mut Cpg) -> Option<u32> {
        let paren = expr.find('(')?;
        if paren == 0 {
            return None;
        }
        let callee = &expr[..paren];
        if !callee.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '.') {
            return None;
        }
        let close = expr.rfind(')')?;
        if close < paren {
            return None;
        }
        let call_id = cpg.add_node(NodeKind::Call {
            name: callee.to_string(),
            line: line_no,
        });
        let args_str = &expr[paren + 1..close];
        for (idx, arg) in split_args(args_str).iter().enumerate() {
            let arg = arg.trim();
            if !arg.is_empty() {
                self.js_attach_expr(arg, line_no, call_id, idx as u32, cpg);
            }
        }
        Some(call_id)
    }
}

fn parse_js_ctrl(stmt: &str, line_no: u32) -> Option<NodeKind> {
    let kind = if stmt.starts_with("if ") || stmt.starts_with("if(") {
        CtrlKind::If
    } else if stmt.starts_with("while ") || stmt.starts_with("while(") {
        CtrlKind::While
    } else if stmt.starts_with("for ") || stmt.starts_with("for(") {
        CtrlKind::For
    } else if stmt.starts_with("try ") || stmt == "try" {
        CtrlKind::Try
    } else {
        return None;
    };
    Some(NodeKind::ControlStructure { kind, line: line_no })
}

// ─── Go internal parser ───────────────────────────────────────────────────────

/// Simplified line-based Go CPG builder.
///
/// Recognises:
/// - `func name(params)` declarations
/// - `name(args)` call expressions
/// - `:=` and `=` assignments
/// - `if`/`for` control structures
/// - `return` statements
struct InternalGoParser<'a> {
    filename: &'a str,
}

impl<'a> InternalGoParser<'a> {
    fn new(filename: &'a str) -> Self {
        Self { filename }
    }

    fn parse(&mut self, source: &str, cpg: &mut Cpg) {
        let lines: Vec<&str> = source.lines().collect();
        let mut i = 0;
        while i < lines.len() {
            let trimmed = lines[i].trim();
            if trimmed.starts_with("func ") {
                i = self.parse_func(lines.as_slice(), i, cpg);
            } else {
                i += 1;
            }
        }
    }

    fn parse_func(&self, lines: &[&str], def_idx: usize, cpg: &mut Cpg) -> usize {
        let def_line = lines[def_idx].trim();
        let line_no = (def_idx + 1) as u32;

        // Strip `func ` prefix, skip optional receiver `(recv Type)`
        let after_func = def_line.trim_start_matches("func").trim();
        let after_func = if after_func.starts_with('(') {
            // receiver: skip to closing paren
            let close = after_func.find(')').map(|i| i + 1).unwrap_or(0);
            after_func[close..].trim()
        } else {
            after_func
        };

        let paren = after_func.find('(').unwrap_or(after_func.len());
        let fn_name = after_func[..paren].trim().to_string();

        let params: Vec<String> = if let (Some(open), Some(close)) =
            (after_func.find('('), after_func.find(')'))
        {
            let inner = &after_func[open + 1..close];
            inner
                .split(',')
                .flat_map(|p| {
                    // Go params: `name type` — take just the name
                    let parts: Vec<&str> = p.trim().splitn(2, ' ').collect();
                    if !parts.is_empty() && !parts[0].is_empty() {
                        vec![parts[0].to_string()]
                    } else {
                        vec![]
                    }
                })
                .filter(|p| !p.is_empty())
                .collect()
        } else {
            vec![]
        };

        let method_id = cpg.add_node(NodeKind::Method {
            name: fn_name,
            file: self.filename.to_string(),
            line: line_no,
        });
        for (idx, param) in params.iter().enumerate() {
            let p_id = cpg.add_node(NodeKind::Parameter {
                name: param.clone(),
                index: idx as u32,
            });
            cpg.add_edge(method_id, p_id, EdgeKind::Ast);
        }

        let body_indent = body_indentation(lines, def_idx + 1);
        let mut prev_stmt: Option<u32> = None;
        let mut i = def_idx + 1;

        while i < lines.len() {
            let raw = lines[i];
            let trimmed = raw.trim();
            if trimmed.is_empty() || trimmed == "}" {
                let indent = leading_spaces(raw);
                if !trimmed.is_empty() && indent < body_indent {
                    break;
                }
                i += 1;
                continue;
            }
            let indent = leading_spaces(raw);
            if indent < body_indent {
                break;
            }
            if let Some(sid) = self.parse_go_stmt(trimmed, (i + 1) as u32, cpg) {
                cpg.add_edge(method_id, sid, EdgeKind::Ast);
                if let Some(prev) = prev_stmt {
                    cpg.add_edge(prev, sid, EdgeKind::Cfg);
                }
                prev_stmt = Some(sid);
            }
            i += 1;
        }
        i
    }

    fn parse_go_stmt(&self, stmt: &str, line_no: u32, cpg: &mut Cpg) -> Option<u32> {
        if stmt.is_empty() || stmt.starts_with("//") {
            return None;
        }
        // return
        if stmt.starts_with("return") {
            let ret_id = cpg.add_node(NodeKind::Return { line: line_no });
            let rest = stmt.trim_start_matches("return").trim();
            if !rest.is_empty() {
                self.go_attach_expr(rest, line_no, ret_id, 0, cpg);
            }
            return Some(ret_id);
        }
        // control structures
        if let Some(ctrl) = parse_go_ctrl(stmt, line_no) {
            return Some(cpg.add_node(ctrl));
        }
        // short variable declaration `name := expr`
        if let Some(colon_eq) = stmt.find(":=") {
            let lhs = stmt[..colon_eq].trim();
            let rhs = stmt[colon_eq + 2..].trim();
            if !lhs.is_empty() && !rhs.is_empty() {
                let assign_id = cpg.add_node(NodeKind::Assignment {
                    lhs: lhs.to_string(),
                    line: line_no,
                });
                self.go_attach_expr(rhs, line_no, assign_id, 0, cpg);
                return Some(assign_id);
            }
        }
        // regular assignment
        if let Some((lhs, rhs)) = parse_assignment(stmt) {
            let assign_id = cpg.add_node(NodeKind::Assignment {
                lhs: lhs.to_string(),
                line: line_no,
            });
            self.go_attach_expr(rhs, line_no, assign_id, 0, cpg);
            return Some(assign_id);
        }
        // bare call
        if let Some(call_id) = self.go_try_call(stmt, line_no, cpg) {
            return Some(call_id);
        }
        None
    }

    fn go_attach_expr(&self, expr: &str, line_no: u32, parent: u32, arg_index: u32, cpg: &mut Cpg) {
        let expr = expr.trim();
        if expr.is_empty() {
            return;
        }
        if let Some(call_id) = self.go_try_call(expr, line_no, cpg) {
            cpg.add_edge(parent, call_id, EdgeKind::Argument { index: arg_index });
            return;
        }
        let name = expr
            .split(|c: char| !c.is_alphanumeric() && c != '_' && c != '.')
            .next()
            .unwrap_or(expr)
            .trim()
            .to_string();
        if !name.is_empty() {
            let id = cpg.add_node(NodeKind::Identifier { name, line: line_no });
            cpg.add_edge(parent, id, EdgeKind::Argument { index: arg_index });
        }
    }

    fn go_try_call(&self, expr: &str, line_no: u32, cpg: &mut Cpg) -> Option<u32> {
        let paren = expr.find('(')?;
        if paren == 0 {
            return None;
        }
        let callee = &expr[..paren];
        if !callee.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '.') {
            return None;
        }
        let close = expr.rfind(')')?;
        if close < paren {
            return None;
        }
        let call_id = cpg.add_node(NodeKind::Call {
            name: callee.to_string(),
            line: line_no,
        });
        let args_str = &expr[paren + 1..close];
        for (idx, arg) in split_args(args_str).iter().enumerate() {
            let arg = arg.trim();
            if !arg.is_empty() {
                self.go_attach_expr(arg, line_no, call_id, idx as u32, cpg);
            }
        }
        Some(call_id)
    }
}

fn parse_go_ctrl(stmt: &str, line_no: u32) -> Option<NodeKind> {
    let kind = if stmt.starts_with("if ") || stmt.starts_with("if(") {
        CtrlKind::If
    } else if stmt.starts_with("for ") || stmt.starts_with("for(") || stmt == "for {" {
        CtrlKind::For
    } else {
        return None;
    };
    Some(NodeKind::ControlStructure { kind, line: line_no })
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

    // ── New tests targeting previously uncovered branches ───────────────────

    /// Line 133: `attach_expr` on a bare `return` with no expression.
    /// The `if !rest.is_empty()` guard must be false → attach_expr not called.
    #[test]
    fn return_no_expr_creates_return_node_only() {
        let source = "def foo():\n    return\n";
        let cpg = build_python_cpg(source, "test.py");
        // A Return node is created …
        assert!(cpg
            .nodes()
            .any(|(_, k)| matches!(k, NodeKind::Return { .. })));
        // … but no Identifier or Call hangs off it (nothing attached by attach_expr).
        let ret_id = cpg
            .nodes()
            .find_map(|(id, k)| matches!(k, NodeKind::Return { .. }).then_some(id))
            .unwrap();
        let child_edges = cpg
            .edges_from(ret_id)
            .iter()
            .filter(|(_, _, k)| matches!(k, EdgeKind::Argument { .. }))
            .count();
        assert_eq!(
            child_edges, 0,
            "bare return should have no argument children"
        );
    }

    /// Line 166: `parse_one_statement` identifier-like fallback returns an Identifier node.
    /// A statement that is a plain word with no call syntax hits the alphanumeric guard.
    #[test]
    fn bare_identifier_statement_creates_identifier_node() {
        let source = "def foo():\n    some_var\n";
        let cpg = build_python_cpg(source, "test.py");
        let has_ident = cpg
            .nodes()
            .any(|(_, k)| matches!(k, NodeKind::Identifier { name, .. } if name == "some_var"));
        assert!(
            has_ident,
            "plain identifier statement should yield Identifier node"
        );
    }

    /// Line 168: `parse_statement` returns `None` for a statement that is neither a
    /// recognised keyword, assignment, valid call, nor plain identifier.
    /// A line like `x[0]` contains `[` so it fails the alphanumeric test and falls through.
    #[test]
    fn unrecognised_statement_produces_no_node() {
        // subscript expression — not a call, not plain identifier, not assignment
        let source = "def foo():\n    x[0]\n";
        let cpg = build_python_cpg(source, "test.py");
        // No Identifier/Call/Assignment node should exist beyond the Method itself
        let non_method = cpg
            .nodes()
            .filter(|(_, k)| !matches!(k, NodeKind::Method { .. } | NodeKind::Parameter { .. }));
        assert_eq!(
            non_method.count(),
            0,
            "unrecognised statement should produce no CPG node"
        );
    }

    /// Line 182: `attach_expr` called with an empty string returns immediately.
    /// Achieved via an assignment whose rhs trims to empty — parse_assignment won't
    /// match that, so we drive attach_expr directly via a return of a whitespace expr
    /// inside a function. The safest indirect path: `return  ` (all whitespace after
    /// "return") — rest.trim() is empty → guard at line 130 prevents attach_expr call.
    /// To hit line 182 directly, pass a string that is all spaces as the expr arg.
    /// We do this through an assignment `x =   ` which parse_assignment skips (rhs empty).
    /// Verify indirectly: the node count must not grow due to the empty attach_expr path.
    #[test]
    fn attach_expr_empty_string_is_noop() {
        // `return   ` — rest after "return" is only spaces, trim gives "", so the
        // `if !rest.is_empty()` guard at line 130 is false and attach_expr is NOT called.
        // But we can still hit line 182 via `self.attach_expr("  ", ...)` from
        // parse_statement's assignment path — however that branch won't reach there
        // because parse_assignment requires non-empty rhs.
        //
        // The simplest public-surface route: a call with an empty argument slot
        // produced by a trailing comma, e.g. `foo(x,)`.  split_args produces ["x",""]
        // so the inner loop checks `if !arg.is_empty()` and skips the empty slot —
        // that `if` guard IS the line-265 branch. attach_expr itself at line 182 is
        // reached only when the caller passes a non-empty string that trims to empty.
        // We exercise it through `foo( )` — the args_str is " ", split_args returns
        // [" "], attach_expr is called with " ", trims to "" and returns early.
        let source = "def foo():\n    bar( )\n";
        let cpg = build_python_cpg(source, "test.py");
        let call_id = cpg
            .nodes()
            .find_map(|(id, k)| {
                matches!(k, NodeKind::Call { name, .. } if name == "bar").then_some(id)
            })
            .expect("call node for bar");
        // The whitespace-only argument must produce zero argument edges
        let arg_edges = cpg
            .edges_from(call_id)
            .iter()
            .filter(|(_, _, k)| matches!(k, EdgeKind::Argument { .. }))
            .count();
        assert_eq!(
            arg_edges, 0,
            "whitespace arg should not create an Argument edge"
        );
    }

    /// Line 230: end of `attach_expr` — plain identifier/dotted name branch.
    /// Verified by ensuring a dotted identifier in an argument position creates an
    /// Identifier node connected via an Argument edge.
    #[test]
    fn attach_expr_plain_dotted_name() {
        // `obj.attr` contains only alphanumeric, `_`, and `.` chars so it is kept
        // whole by the split and stored as the full dotted name.
        let source = "def foo():\n    bar(obj.attr)\n";
        let cpg = build_python_cpg(source, "test.py");
        let has_ident = cpg
            .nodes()
            .any(|(_, k)| matches!(k, NodeKind::Identifier { name, .. } if name == "obj.attr"));
        assert!(
            has_ident,
            "dotted name argument should produce an Identifier node named 'obj.attr'"
        );
    }

    /// Line 239: `try_parse_call` returns `None` when `(` is at position 0.
    /// This is exercised by a statement starting with `(`, which falls through
    /// try_parse_call and then fails the alphanumeric test → `None` from parse_statement.
    #[test]
    fn try_parse_call_paren_at_start_returns_none() {
        let source = "def foo():\n    (x + y)\n";
        let cpg = build_python_cpg(source, "test.py");
        // No Call node should be produced
        assert!(
            !cpg.nodes().any(|(_, k)| matches!(k, NodeKind::Call { .. })),
            "expression starting with '(' should not produce a Call node"
        );
    }

    /// Line 247: `try_parse_call` returns `None` when callee contains non-identifier chars.
    /// e.g. `"str"(x)` — tree-sitter treats this as a valid call expression.
    #[test]
    #[cfg(not(feature = "treesitter"))]
    fn try_parse_call_invalid_callee_returns_none() {
        let source = "def foo():\n    \"str\"(x)\n";
        let cpg = build_python_cpg(source, "test.py");
        assert!(
            !cpg.nodes().any(|(_, k)| matches!(k, NodeKind::Call { .. })),
            "callee with non-identifier chars should not produce a Call node"
        );
    }

    /// Line 250 / 252: `rfind(')')` fails or `close < paren`.
    /// An expression like `foo(bar` has an open paren but no closing paren,
    /// so `rfind(')')` returns `None` and `try_parse_call` returns `None`.
    #[test]
    fn try_parse_call_no_closing_paren_returns_none() {
        let source = "def foo():\n    bar(x\n";
        let cpg = build_python_cpg(source, "test.py");
        assert!(
            !cpg.nodes().any(|(_, k)| matches!(k, NodeKind::Call { .. })),
            "call without closing paren should not produce a Call node"
        );
    }

    /// Line 267: end of arg-parsing loop — call with zero non-empty arguments.
    /// `foo()` has an empty args_str so the loop body never executes; we verify
    /// a Call node exists with no Argument edges.
    #[test]
    fn try_parse_call_no_args_loop_exits_cleanly() {
        let source = "def foo():\n    bar()\n";
        let cpg = build_python_cpg(source, "test.py");
        let call_id = cpg
            .nodes()
            .find_map(|(id, k)| {
                matches!(k, NodeKind::Call { name, .. } if name == "bar").then_some(id)
            })
            .expect("call node for bar");
        let arg_edges = cpg
            .edges_from(call_id)
            .iter()
            .filter(|(_, _, k)| matches!(k, EdgeKind::Argument { .. }))
            .count();
        assert_eq!(
            arg_edges, 0,
            "call with no args should have zero argument edges"
        );
    }

    /// Line 299: `parse_def_signature` returns `vec![]` when there are no parens.
    /// A `def` line without parentheses still creates a Method node with no params.
    /// Note: `def bare:` is syntactically invalid Python; tree-sitter rejects it.
    #[test]
    #[cfg(not(feature = "treesitter"))]
    fn parse_def_signature_no_parens_yields_empty_params() {
        // Without parens, `find('(').unwrap_or(line.len())` returns the full length,
        // so `name` = the whole trimmed string (e.g. "bare:").
        let source = "def bare:\n    pass\n";
        let cpg = build_python_cpg(source, "test.py");
        // At least one method node was created (name = "bare:")
        assert!(
            cpg.nodes()
                .any(|(_, k)| matches!(k, NodeKind::Method { .. })),
            "def without parens should still produce a Method node"
        );
        // The else branch at line 298-299 is taken → no params
        assert_eq!(
            param_names(&cpg).len(),
            0,
            "def without parens must have no parameters"
        );
    }

    /// Line 309: `CtrlKind::While`
    #[test]
    fn builder_detects_while_control_structure() {
        let source = "def foo():\n    while True:\n        pass\n";
        let cpg = build_python_cpg(source, "test.py");
        let has_while = cpg.nodes().any(|(_, k)| {
            matches!(
                k,
                NodeKind::ControlStructure {
                    kind: CtrlKind::While,
                    ..
                }
            )
        });
        assert!(
            has_while,
            "while statement should produce a While ControlStructure node"
        );
    }

    /// Line 311: `CtrlKind::For`
    #[test]
    fn builder_detects_for_control_structure() {
        let source = "def foo():\n    for x in items:\n        pass\n";
        let cpg = build_python_cpg(source, "test.py");
        let has_for = cpg.nodes().any(|(_, k)| {
            matches!(
                k,
                NodeKind::ControlStructure {
                    kind: CtrlKind::For,
                    ..
                }
            )
        });
        assert!(
            has_for,
            "for statement should produce a For ControlStructure node"
        );
    }

    /// Line 313: `CtrlKind::Try`
    /// Note: this test uses syntactically invalid Python (try without except).
    /// The tree-sitter parser correctly rejects it, so gate behind line-based mode.
    #[test]
    #[cfg(not(feature = "treesitter"))]
    fn builder_detects_try_control_structure() {
        let source = "def foo():\n    try:\n        pass\n";
        let cpg = build_python_cpg(source, "test.py");
        let has_try = cpg.nodes().any(|(_, k)| {
            matches!(
                k,
                NodeKind::ControlStructure {
                    kind: CtrlKind::Try,
                    ..
                }
            )
        });
        assert!(
            has_try,
            "try: statement should produce a Try ControlStructure node"
        );
    }

    /// Lines 327-341: `parse_assignment` — augmented assignment `+=`.
    #[test]
    fn parse_assignment_augmented_plus_equals() {
        let source = "def foo():\n    x += 1\n";
        let cpg = build_python_cpg(source, "test.py");
        let has_assign = cpg
            .nodes()
            .any(|(_, k)| matches!(k, NodeKind::Assignment { lhs, .. } if lhs == "x"));
        assert!(
            has_assign,
            "+= should produce an Assignment node with lhs = \"x\""
        );
    }

    /// Lines 327-341: `parse_assignment` — augmented assignment `-=`.
    #[test]
    fn parse_assignment_augmented_minus_equals() {
        let source = "def foo():\n    count -= 1\n";
        let cpg = build_python_cpg(source, "test.py");
        let has_assign = cpg
            .nodes()
            .any(|(_, k)| matches!(k, NodeKind::Assignment { lhs, .. } if lhs == "count"));
        assert!(
            has_assign,
            "-= should produce an Assignment node with lhs = \"count\""
        );
    }

    /// Lines 327-341: `parse_assignment` — `==` must be skipped (next byte is `=`).
    #[test]
    fn parse_assignment_skips_equality_comparison() {
        // `if x == y:` must not produce an Assignment node
        let source = "def foo(x, y):\n    if x == y:\n        pass\n";
        let cpg = build_python_cpg(source, "test.py");
        assert!(
            !cpg.nodes()
                .any(|(_, k)| matches!(k, NodeKind::Assignment { .. })),
            "== comparison must not be parsed as an assignment"
        );
    }

    /// Lines 327-341: `parse_assignment` — `!=` must be skipped (prev byte is `!`).
    #[test]
    fn parse_assignment_skips_not_equal() {
        let source = "def foo(x, y):\n    if x != y:\n        pass\n";
        let cpg = build_python_cpg(source, "test.py");
        assert!(
            !cpg.nodes()
                .any(|(_, k)| matches!(k, NodeKind::Assignment { .. })),
            "!= must not be parsed as an assignment"
        );
    }

    /// Line 357: `body_indentation` loop exhausts all lines without finding a
    /// non-blank line (function body is entirely blank lines).
    /// Line 359: the function falls through to the default indent of 4.
    #[test]
    fn body_indentation_all_blank_returns_default_four() {
        // Function with only blank lines in the body — body_indentation returns 4.
        // The parse loop will find no statements, but the Method node is still created.
        let source = "def foo():\n\n\n";
        let cpg = build_python_cpg(source, "test.py");
        assert!(method_names(&cpg).contains(&"foo".to_string()));
        // No statements produced — only the Method node
        let non_method_count = cpg
            .nodes()
            .filter(|(_, k)| !matches!(k, NodeKind::Method { .. }))
            .count();
        assert_eq!(
            non_method_count, 0,
            "all-blank body should produce no statement nodes"
        );
    }

    #[test]
    fn bug_parse_assignment_double_star_equals() {
        let source = "def foo():\n    x **= 2\n";
        let cpg = build_python_cpg(source, "test.py");
        let has_correct = cpg
            .nodes()
            .any(|(_, k)| matches!(k, NodeKind::Assignment { lhs, .. } if lhs == "x"));
        assert!(
            has_correct,
            "x **= 2 should produce Assignment with lhs='x'"
        );
    }

    #[test]
    fn bug_parse_assignment_double_slash_equals() {
        let source = "def foo():\n    x //= 2\n";
        let cpg = build_python_cpg(source, "test.py");
        let has_correct = cpg
            .nodes()
            .any(|(_, k)| matches!(k, NodeKind::Assignment { lhs, .. } if lhs == "x"));
        assert!(
            has_correct,
            "x //= 2 should produce Assignment with lhs='x'"
        );
    }

    #[test]
    fn bug_parse_assignment_percent_equals() {
        let source = "def foo():\n    x %= 3\n";
        let cpg = build_python_cpg(source, "test.py");
        let has_correct = cpg
            .nodes()
            .any(|(_, k)| matches!(k, NodeKind::Assignment { lhs, .. } if lhs == "x"));
        assert!(has_correct, "x %= 3 should produce Assignment with lhs='x'");
    }

    #[test]
    fn bug_parse_assignment_bitwise_or_equals() {
        let source = "def foo():\n    flags |= 0x01\n";
        let cpg = build_python_cpg(source, "test.py");
        let has_correct = cpg
            .nodes()
            .any(|(_, k)| matches!(k, NodeKind::Assignment { lhs, .. } if lhs == "flags"));
        assert!(
            has_correct,
            "flags |= 0x01 should produce Assignment with lhs='flags'"
        );
    }

    #[test]
    fn bug_parse_assignment_bitwise_and_equals() {
        let source = "def foo():\n    x &= 0xff\n";
        let cpg = build_python_cpg(source, "test.py");
        let has_correct = cpg
            .nodes()
            .any(|(_, k)| matches!(k, NodeKind::Assignment { lhs, .. } if lhs == "x"));
        assert!(
            has_correct,
            "x &= 0xff should produce Assignment with lhs='x'"
        );
    }

    #[test]
    fn bug_parse_assignment_xor_equals() {
        let source = "def foo():\n    x ^= mask\n";
        let cpg = build_python_cpg(source, "test.py");
        let has_correct = cpg
            .nodes()
            .any(|(_, k)| matches!(k, NodeKind::Assignment { lhs, .. } if lhs == "x"));
        assert!(
            has_correct,
            "x ^= mask should produce Assignment with lhs='x'"
        );
    }

    // ── CpgBuilder trait tests ────────────────────────────────────────────────

    /// PythonCpgBuilder::language() must return Language::Python.
    #[test]
    fn python_cpg_builder_language_is_python() {
        use apex_core::types::Language;
        let builder = PythonCpgBuilder;
        assert_eq!(builder.language(), Language::Python);
    }

    /// PythonCpgBuilder::build() must produce a non-empty CPG for real Python source.
    #[test]
    fn python_cpg_builder_build_produces_non_empty_cpg() {
        let builder = PythonCpgBuilder;
        let cpg = builder.build(
            "def greet(name):\n    print(name)\n",
            "greet.py",
        );
        assert!(cpg.node_count() > 0, "CpgBuilder::build should produce at least one node");
    }

    /// CpgBuilder is object-safe — can be stored as Box<dyn CpgBuilder>.
    #[test]
    fn cpg_builder_is_object_safe() {
        use apex_core::types::Language;
        let builder: Box<dyn CpgBuilder> = Box::new(PythonCpgBuilder);
        assert_eq!(builder.language(), Language::Python);
        let cpg = builder.build("def foo():\n    pass\n", "foo.py");
        // Should create at least the Method node.
        assert!(
            cpg.nodes().any(|(_, k)| matches!(k, NodeKind::Method { .. })),
            "boxed CpgBuilder should produce a Method node"
        );
    }
}
