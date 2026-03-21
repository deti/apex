//! Tree-sitter-based Python CPG builder.
//!
//! Replaces the line-based regex approach with a proper AST walk, correctly
//! handling nested calls, decorators, comprehensions, multi-line statements,
//! and f-strings.

use apex_core::types::Language;
use tree_sitter::{Node, Parser};

use crate::builder::CpgBuilder;
use crate::{Cpg, CtrlKind, EdgeKind, NodeKind};

/// A [`CpgBuilder`] for Python that uses tree-sitter for parsing.
///
/// Walks the concrete syntax tree to extract function definitions, assignments,
/// calls, control structures, return statements, and import declarations.
pub struct TreeSitterPythonCpgBuilder;

impl CpgBuilder for TreeSitterPythonCpgBuilder {
    fn build(&self, source: &str, filename: &str) -> Cpg {
        build_ts_python_cpg(source, filename)
    }

    fn language(&self) -> Language {
        Language::Python
    }
}

/// Build a CPG from Python source using tree-sitter.
pub fn build_ts_python_cpg(source: &str, filename: &str) -> Cpg {
    let mut cpg = Cpg::new();
    let mut parser = Parser::new();
    parser
        .set_language(&tree_sitter_python::LANGUAGE.into())
        .expect("failed to set tree-sitter Python language");

    let Some(tree) = parser.parse(source, None) else {
        return cpg;
    };

    let root = tree.root_node();
    let src = source.as_bytes();

    // Walk top-level children looking for function definitions (and decorated ones)
    walk_top_level(&root, src, filename, &mut cpg);

    cpg
}

/// Walk top-level statements, looking for function definitions.
fn walk_top_level(node: &Node, src: &[u8], filename: &str, cpg: &mut Cpg) {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        match child.kind() {
            "function_definition" => {
                parse_function_def(&child, src, filename, cpg);
            }
            "decorated_definition" => {
                // The actual function_definition is a child of the decorated_definition
                let mut inner_cursor = child.walk();
                for inner in child.children(&mut inner_cursor) {
                    if inner.kind() == "function_definition" {
                        parse_function_def(&inner, src, filename, cpg);
                    }
                }
            }
            // Recurse into class bodies to find methods
            "class_definition" => {
                if let Some(body) = child.child_by_field_name("body") {
                    walk_top_level(&body, src, filename, cpg);
                }
            }
            _ => {}
        }
    }
}

/// Parse a function_definition node into CPG Method + body statements.
fn parse_function_def(node: &Node, src: &[u8], filename: &str, cpg: &mut Cpg) {
    let line_no = node.start_position().row as u32 + 1;

    // Extract function name
    let fn_name = node
        .child_by_field_name("name")
        .map(|n| node_text(&n, src))
        .unwrap_or_default();

    let method_id = cpg.add_node(NodeKind::Method {
        name: fn_name,
        file: filename.to_string(),
        line: line_no,
    });

    // Extract parameters
    if let Some(params_node) = node.child_by_field_name("parameters") {
        let mut param_index = 0u32;
        let mut cursor = params_node.walk();
        for param in params_node.children(&mut cursor) {
            let param_name = match param.kind() {
                "identifier" => {
                    let name = node_text(&param, src);
                    if name == "self" || name == "cls" {
                        continue;
                    }
                    name
                }
                "default_parameter" | "typed_default_parameter" => {
                    // name = default  or  name: type = default
                    param
                        .child_by_field_name("name")
                        .map(|n| node_text(&n, src))
                        .unwrap_or_default()
                }
                "typed_parameter" => {
                    // name: type
                    param
                        .child(0)
                        .map(|n| node_text(&n, src))
                        .unwrap_or_default()
                }
                "list_splat_pattern" | "dictionary_splat_pattern" => {
                    // *args or **kwargs — extract the identifier child
                    param
                        .child(1)
                        .or_else(|| param.child(0))
                        .map(|n| node_text(&n, src))
                        .unwrap_or_default()
                }
                _ => continue,
            };

            if param_name == "self" || param_name == "cls" || param_name.is_empty() {
                continue;
            }

            let p_id = cpg.add_node(NodeKind::Parameter {
                name: param_name,
                index: param_index,
            });
            cpg.add_edge(method_id, p_id, EdgeKind::Ast);
            param_index += 1;
        }
    }

    // Parse body statements
    if let Some(body) = node.child_by_field_name("body") {
        let mut prev_stmt: Option<u32> = None;
        let mut cursor = body.walk();
        for stmt in body.children(&mut cursor) {
            if let Some(sid) = parse_statement(&stmt, src, cpg) {
                cpg.add_edge(method_id, sid, EdgeKind::Ast);
                if let Some(prev) = prev_stmt {
                    cpg.add_edge(prev, sid, EdgeKind::Cfg);
                }
                prev_stmt = Some(sid);
            }
        }
    }
}

/// Parse a single statement AST node into CPG nodes.
fn parse_statement(node: &Node, src: &[u8], cpg: &mut Cpg) -> Option<u32> {
    let line_no = node.start_position().row as u32 + 1;

    match node.kind() {
        "return_statement" => {
            let ret_id = cpg.add_node(NodeKind::Return { line: line_no });
            // Attach the return expression if present
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                if child.kind() != "return" {
                    attach_expr(&child, src, ret_id, 0, cpg);
                }
            }
            Some(ret_id)
        }

        "expression_statement" => {
            // The expression is the first child
            let child = node.child(0)?;
            parse_expr_as_statement(&child, src, cpg)
        }

        "assignment" => {
            let lhs_node = node.child_by_field_name("left")?;
            let rhs_node = node.child_by_field_name("right")?;
            let lhs = node_text(&lhs_node, src);
            let assign_id = cpg.add_node(NodeKind::Assignment {
                lhs,
                line: line_no,
            });
            attach_expr(&rhs_node, src, assign_id, 0, cpg);
            Some(assign_id)
        }

        "augmented_assignment" => {
            let lhs_node = node.child_by_field_name("left")?;
            let rhs_node = node.child_by_field_name("right")?;
            let lhs = node_text(&lhs_node, src);
            let assign_id = cpg.add_node(NodeKind::Assignment {
                lhs,
                line: line_no,
            });
            attach_expr(&rhs_node, src, assign_id, 0, cpg);
            Some(assign_id)
        }

        "if_statement" => Some(cpg.add_node(NodeKind::ControlStructure {
            kind: CtrlKind::If,
            line: line_no,
        })),

        "while_statement" => Some(cpg.add_node(NodeKind::ControlStructure {
            kind: CtrlKind::While,
            line: line_no,
        })),

        "for_statement" => Some(cpg.add_node(NodeKind::ControlStructure {
            kind: CtrlKind::For,
            line: line_no,
        })),

        "try_statement" => Some(cpg.add_node(NodeKind::ControlStructure {
            kind: CtrlKind::Try,
            line: line_no,
        })),

        "import_statement" | "import_from_statement" => {
            // Represent imports as Call nodes (e.g., "import os" -> Call { name: "import" })
            let import_text = node_text(node, src);
            let call_id = cpg.add_node(NodeKind::Call {
                name: "import".to_string(),
                line: line_no,
            });
            let lit_id = cpg.add_node(NodeKind::Literal {
                value: import_text,
                line: line_no,
            });
            cpg.add_edge(call_id, lit_id, EdgeKind::Argument { index: 0 });
            Some(call_id)
        }

        // Skip comment, pass, etc.
        "comment" | "pass_statement" => None,

        // For decorated definitions inside a function body
        "decorated_definition" => {
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                if child.kind() == "function_definition" {
                    parse_function_def(&child, src, "<nested>", cpg);
                }
            }
            None
        }

        // Nested function definitions
        "function_definition" => {
            parse_function_def(node, src, "<nested>", cpg);
            None
        }

        _ => None,
    }
}

/// Parse an expression node that appears as a statement (expression_statement child).
fn parse_expr_as_statement(node: &Node, src: &[u8], cpg: &mut Cpg) -> Option<u32> {
    let line_no = node.start_position().row as u32 + 1;

    match node.kind() {
        "call" => Some(parse_call(node, src, cpg)),

        "assignment" => {
            let lhs_node = node.child_by_field_name("left")?;
            let rhs_node = node.child_by_field_name("right")?;
            let lhs = node_text(&lhs_node, src);
            let assign_id = cpg.add_node(NodeKind::Assignment {
                lhs,
                line: line_no,
            });
            attach_expr(&rhs_node, src, assign_id, 0, cpg);
            Some(assign_id)
        }

        "augmented_assignment" => {
            let lhs_node = node.child_by_field_name("left")?;
            let rhs_node = node.child_by_field_name("right")?;
            let lhs = node_text(&lhs_node, src);
            let assign_id = cpg.add_node(NodeKind::Assignment {
                lhs,
                line: line_no,
            });
            attach_expr(&rhs_node, src, assign_id, 0, cpg);
            Some(assign_id)
        }

        "identifier" => {
            let name = node_text(node, src);
            Some(cpg.add_node(NodeKind::Identifier { name, line: line_no }))
        }

        _ => None,
    }
}

/// Parse a call expression node into a CPG Call node with arguments.
fn parse_call(node: &Node, src: &[u8], cpg: &mut Cpg) -> u32 {
    let line_no = node.start_position().row as u32 + 1;

    // Extract callee name
    let callee_name = node
        .child_by_field_name("function")
        .map(|n| node_text(&n, src))
        .unwrap_or_default();

    let call_id = cpg.add_node(NodeKind::Call {
        name: callee_name,
        line: line_no,
    });

    // Parse arguments
    if let Some(args_node) = node.child_by_field_name("arguments") {
        let mut arg_index = 0u32;
        let mut cursor = args_node.walk();
        for arg in args_node.children(&mut cursor) {
            match arg.kind() {
                "(" | ")" | "," => continue,
                "keyword_argument" => {
                    // name=value — attach the value
                    if let Some(value) = arg.child_by_field_name("value") {
                        attach_expr(&value, src, call_id, arg_index, cpg);
                        arg_index += 1;
                    }
                }
                "list_splat" | "dictionary_splat" => {
                    // *args / **kwargs — attach the inner expression
                    if let Some(inner) = arg.child(1) {
                        attach_expr(&inner, src, call_id, arg_index, cpg);
                        arg_index += 1;
                    }
                }
                _ => {
                    attach_expr(&arg, src, call_id, arg_index, cpg);
                    arg_index += 1;
                }
            }
        }
    }

    call_id
}

/// Attach an expression as a child of `parent` via an Argument edge.
fn attach_expr(node: &Node, src: &[u8], parent: u32, arg_index: u32, cpg: &mut Cpg) {
    let line_no = node.start_position().row as u32 + 1;

    match node.kind() {
        "call" => {
            let call_id = parse_call(node, src, cpg);
            cpg.add_edge(parent, call_id, EdgeKind::Argument { index: arg_index });
        }

        "identifier" | "attribute" => {
            let name = node_text(node, src);
            let id = cpg.add_node(NodeKind::Identifier {
                name,
                line: line_no,
            });
            cpg.add_edge(parent, id, EdgeKind::Argument { index: arg_index });
        }

        "string" | "concatenated_string" | "integer" | "float" | "true" | "false" | "none" => {
            let value = node_text(node, src);
            let lit_id = cpg.add_node(NodeKind::Literal {
                value,
                line: line_no,
            });
            cpg.add_edge(parent, lit_id, EdgeKind::Argument { index: arg_index });
        }

        // f-strings are represented as interpolated strings
        "interpolation" => {
            // Inside an f-string: {expr} — recurse into the expression
            if let Some(expr) = node.child(1) {
                attach_expr(&expr, src, parent, arg_index, cpg);
            }
        }

        // List/set/dict comprehensions and generator expressions
        "list_comprehension" | "set_comprehension" | "dictionary_comprehension"
        | "generator_expression" => {
            let value = node_text(node, src);
            let lit_id = cpg.add_node(NodeKind::Literal {
                value,
                line: line_no,
            });
            cpg.add_edge(parent, lit_id, EdgeKind::Argument { index: arg_index });
        }

        "list" | "tuple" | "dictionary" | "set" => {
            let value = node_text(node, src);
            let lit_id = cpg.add_node(NodeKind::Literal {
                value,
                line: line_no,
            });
            cpg.add_edge(parent, lit_id, EdgeKind::Argument { index: arg_index });
        }

        "binary_operator" | "unary_operator" | "boolean_operator" | "comparison_operator"
        | "not_operator" => {
            // For operators, try to attach sub-expressions
            let mut cursor = node.walk();
            let mut sub_index = arg_index;
            for child in node.children(&mut cursor) {
                if !is_operator_token(child.kind()) {
                    attach_expr(&child, src, parent, sub_index, cpg);
                    sub_index += 1;
                }
            }
        }

        "parenthesized_expression" => {
            // Unwrap parens and recurse
            if let Some(inner) = node.child(1) {
                attach_expr(&inner, src, parent, arg_index, cpg);
            }
        }

        "conditional_expression" => {
            // ternary: body if condition else alternative
            if let Some(body) = node.child(0) {
                attach_expr(&body, src, parent, arg_index, cpg);
            }
        }

        "subscript" => {
            // x[i] — treat the whole thing as an identifier reference
            let name = node_text(node, src);
            let id = cpg.add_node(NodeKind::Identifier {
                name,
                line: line_no,
            });
            cpg.add_edge(parent, id, EdgeKind::Argument { index: arg_index });
        }

        "lambda" => {
            let value = node_text(node, src);
            let lit_id = cpg.add_node(NodeKind::Literal {
                value,
                line: line_no,
            });
            cpg.add_edge(parent, lit_id, EdgeKind::Argument { index: arg_index });
        }

        "await" => {
            // await expr — recurse into the expression
            if let Some(expr) = node.child(1) {
                attach_expr(&expr, src, parent, arg_index, cpg);
            }
        }

        _ => {
            // Fallback: emit as literal with the raw text
            let value = node_text(node, src);
            if !value.is_empty() {
                let lit_id = cpg.add_node(NodeKind::Literal {
                    value,
                    line: line_no,
                });
                cpg.add_edge(parent, lit_id, EdgeKind::Argument { index: arg_index });
            }
        }
    }
}

/// Check if a tree-sitter node kind is an operator token.
fn is_operator_token(kind: &str) -> bool {
    matches!(
        kind,
        "+"  | "-"
            | "*"
            | "/"
            | "//"
            | "%"
            | "**"
            | "=="
            | "!="
            | "<"
            | ">"
            | "<="
            | ">="
            | "and"
            | "or"
            | "not"
            | "in"
            | "is"
            | "&"
            | "|"
            | "^"
            | "~"
            | "<<"
            | ">>"
            | "not_in"
            | "is_not"
    )
}

/// Get the text content of a tree-sitter node.
fn node_text(node: &Node, src: &[u8]) -> String {
    node.utf8_text(src).unwrap_or("").to_string()
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

    fn assign_lhs_names(cpg: &Cpg) -> Vec<String> {
        cpg.nodes()
            .filter_map(|(_, k)| match k {
                NodeKind::Assignment { lhs, .. } => Some(lhs.clone()),
                _ => None,
            })
            .collect()
    }

    // ── 1. Nested calls ──────────────────────────────────────────────────────

    #[test]
    fn nested_calls_are_parsed() {
        let source = "def foo():\n    print(len(items))\n";
        let cpg = build_ts_python_cpg(source, "test.py");
        let calls = call_names(&cpg);
        assert!(calls.contains(&"print".to_string()), "should find outer call: print");
        assert!(calls.contains(&"len".to_string()), "should find inner call: len");
    }

    // ── 2. Decorated functions ───────────────────────────────────────────────

    #[test]
    fn decorated_function_is_parsed() {
        let source = "@staticmethod\ndef helper(x):\n    return x + 1\n";
        let cpg = build_ts_python_cpg(source, "test.py");
        let methods = method_names(&cpg);
        assert!(methods.contains(&"helper".to_string()), "decorated function should be found");
        let params = param_names(&cpg);
        assert!(params.contains(&"x".to_string()), "parameter x should be found");
    }

    // ── 3. Multi-line statements ─────────────────────────────────────────────

    #[test]
    fn multi_line_call_is_parsed() {
        let source = "def foo():\n    result = some_func(\n        arg1,\n        arg2,\n        arg3\n    )\n";
        let cpg = build_ts_python_cpg(source, "test.py");
        let calls = call_names(&cpg);
        assert!(
            calls.contains(&"some_func".to_string()),
            "multi-line call should be parsed correctly"
        );
        // Should find the assignment
        let assigns = assign_lhs_names(&cpg);
        assert!(assigns.contains(&"result".to_string()));
    }

    // ── 4. F-strings ─────────────────────────────────────────────────────────

    #[test]
    fn fstring_with_call_is_parsed() {
        let source = "def foo(name):\n    msg = f\"Hello {name.upper()}\"\n";
        let cpg = build_ts_python_cpg(source, "test.py");
        let assigns = assign_lhs_names(&cpg);
        assert!(assigns.contains(&"msg".to_string()), "f-string assignment should be found");
    }

    // ── 5. List comprehension ────────────────────────────────────────────────

    #[test]
    fn list_comprehension_is_handled() {
        let source = "def foo(items):\n    result = [x * 2 for x in items]\n";
        let cpg = build_ts_python_cpg(source, "test.py");
        let assigns = assign_lhs_names(&cpg);
        assert!(
            assigns.contains(&"result".to_string()),
            "comprehension assignment should be found"
        );
    }

    // ── 6. Import statements ─────────────────────────────────────────────────

    #[test]
    fn import_statements_are_captured() {
        let source = "def foo():\n    import os\n    from pathlib import Path\n";
        let cpg = build_ts_python_cpg(source, "test.py");
        let calls = call_names(&cpg);
        let import_count = calls.iter().filter(|n| *n == "import").count();
        assert!(import_count >= 2, "both import statements should be captured, got {import_count}");
    }

    // ── 7. Chained method calls ──────────────────────────────────────────────

    #[test]
    fn chained_method_calls() {
        let source = "def foo():\n    result = obj.method1().method2()\n";
        let cpg = build_ts_python_cpg(source, "test.py");
        let assigns = assign_lhs_names(&cpg);
        assert!(assigns.contains(&"result".to_string()));
        // Should have at least one call node
        assert!(!call_names(&cpg).is_empty(), "should find at least one call in chain");
    }

    // ── 8. Multiple decorators ───────────────────────────────────────────────

    #[test]
    fn multiple_decorators() {
        let source = "@app.route('/api')\n@login_required\ndef view(request):\n    return response\n";
        let cpg = build_ts_python_cpg(source, "test.py");
        let methods = method_names(&cpg);
        assert!(methods.contains(&"view".to_string()), "doubly-decorated function should be found");
        let params = param_names(&cpg);
        assert!(params.contains(&"request".to_string()));
    }

    // ── 9. Augmented assignment ──────────────────────────────────────────────

    #[test]
    fn augmented_assignments() {
        let source = "def foo():\n    x += 1\n    y -= 2\n    z **= 3\n";
        let cpg = build_ts_python_cpg(source, "test.py");
        let assigns = assign_lhs_names(&cpg);
        assert!(assigns.contains(&"x".to_string()), "+= should produce assignment for x");
        assert!(assigns.contains(&"y".to_string()), "-= should produce assignment for y");
        assert!(assigns.contains(&"z".to_string()), "**= should produce assignment for z");
    }

    // ── 10. Control structures ───────────────────────────────────────────────

    #[test]
    fn control_structures_all_kinds() {
        let source = "def foo():\n    if True:\n        pass\n    while True:\n        pass\n    for x in y:\n        pass\n    try:\n        pass\n    except:\n        pass\n";
        let cpg = build_ts_python_cpg(source, "test.py");

        let has_if = cpg.nodes().any(|(_, k)| {
            matches!(k, NodeKind::ControlStructure { kind: CtrlKind::If, .. })
        });
        let has_while = cpg.nodes().any(|(_, k)| {
            matches!(k, NodeKind::ControlStructure { kind: CtrlKind::While, .. })
        });
        let has_for = cpg.nodes().any(|(_, k)| {
            matches!(k, NodeKind::ControlStructure { kind: CtrlKind::For, .. })
        });
        let has_try = cpg.nodes().any(|(_, k)| {
            matches!(k, NodeKind::ControlStructure { kind: CtrlKind::Try, .. })
        });

        assert!(has_if, "should detect if");
        assert!(has_while, "should detect while");
        assert!(has_for, "should detect for");
        assert!(has_try, "should detect try");
    }

    // ── 11. Return with nested call ──────────────────────────────────────────

    #[test]
    fn return_nested_call() {
        let source = "def foo(x):\n    return bar(baz(x))\n";
        let cpg = build_ts_python_cpg(source, "test.py");
        assert!(cpg.nodes().any(|(_, k)| matches!(k, NodeKind::Return { .. })));
        let calls = call_names(&cpg);
        assert!(calls.contains(&"bar".to_string()));
        assert!(calls.contains(&"baz".to_string()));
    }

    // ── 12. CFG edges between statements ─────────────────────────────────────

    #[test]
    fn cfg_edges_between_statements() {
        let source = "def foo():\n    x = 1\n    y = 2\n    z = 3\n";
        let cpg = build_ts_python_cpg(source, "test.py");
        let cfg_count = cpg
            .edges()
            .filter(|(_, _, k)| matches!(k, EdgeKind::Cfg))
            .count();
        assert!(cfg_count >= 2, "expected >= 2 CFG edges, got {cfg_count}");
    }

    // ── 13. Keyword arguments ────────────────────────────────────────────────

    #[test]
    fn keyword_arguments_in_call() {
        let source = "def foo():\n    bar(x=1, y=2)\n";
        let cpg = build_ts_python_cpg(source, "test.py");
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
        assert_eq!(arg_edges, 2, "keyword args should produce argument edges");
    }

    // ── 14. Class method with self ───────────────────────────────────────────

    #[test]
    fn class_method_skips_self() {
        let source = "class Foo:\n    def method(self, x, y):\n        pass\n";
        let cpg = build_ts_python_cpg(source, "test.py");
        let params = param_names(&cpg);
        assert!(!params.contains(&"self".to_string()), "self should be skipped");
        assert!(params.contains(&"x".to_string()));
        assert!(params.contains(&"y".to_string()));
    }

    // ── 15. Dict comprehension ───────────────────────────────────────────────

    #[test]
    fn dict_comprehension_is_handled() {
        let source = "def foo(items):\n    result = {k: v for k, v in items}\n";
        let cpg = build_ts_python_cpg(source, "test.py");
        let assigns = assign_lhs_names(&cpg);
        assert!(assigns.contains(&"result".to_string()));
    }
}
