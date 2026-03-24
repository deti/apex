//! Tree-sitter-based Go CPG builder.
//!
//! Replaces the line-based regex approach with a proper AST walk, correctly
//! handling method declarations, short variable declarations (`:=`),
//! switch statements, and multi-return functions.

use apex_core::types::Language;
use tree_sitter::{Node, Parser};

use crate::builder::CpgBuilder;
use crate::{Cpg, CtrlKind, EdgeKind, NodeKind};

/// A [`CpgBuilder`] for Go that uses tree-sitter for parsing.
///
/// Walks the concrete syntax tree to extract function declarations, method
/// declarations, variable assignments (`:=` and `=`), call expressions,
/// control structures, and return statements.
pub struct TsGoCpgBuilder;

impl CpgBuilder for TsGoCpgBuilder {
    fn build(&self, source: &str, filename: &str) -> Cpg {
        build_ts_go_cpg(source, filename)
    }

    fn language(&self) -> Language {
        Language::Go
    }
}

/// Build a CPG from Go source using tree-sitter.
pub fn build_ts_go_cpg(source: &str, filename: &str) -> Cpg {
    let mut cpg = Cpg::new();
    let mut parser = Parser::new();
    parser
        .set_language(&tree_sitter_go::LANGUAGE.into())
        .expect("failed to set tree-sitter Go language");

    let Some(tree) = parser.parse(source, None) else {
        return cpg;
    };

    let root = tree.root_node();
    let src = source.as_bytes();

    walk_source_file(&root, src, filename, &mut cpg);

    cpg
}

/// Walk top-level declarations in a Go source file.
fn walk_source_file(node: &Node, src: &[u8], filename: &str, cpg: &mut Cpg) {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        match child.kind() {
            "function_declaration" => {
                parse_function_decl(&child, src, filename, cpg);
            }
            "method_declaration" => {
                parse_method_decl(&child, src, filename, cpg);
            }
            _ => {}
        }
    }
}

/// Parse a `function_declaration` node into CPG Method + body statements.
fn parse_function_decl(node: &Node, src: &[u8], filename: &str, cpg: &mut Cpg) {
    let line_no = node.start_position().row as u32 + 1;

    let fn_name = node
        .child_by_field_name("name")
        .map(|n| node_text(&n, src))
        .unwrap_or_else(|| "<anonymous>".to_string());

    let method_id = cpg.add_node(NodeKind::Method {
        name: fn_name,
        file: filename.to_string(),
        line: line_no,
    });

    parse_params_and_body(node, src, method_id, cpg);
}

/// Parse a `method_declaration` node into CPG Method + body statements.
///
/// In Go, method declarations have a receiver: `func (r Receiver) Name(params) ...`
fn parse_method_decl(node: &Node, src: &[u8], filename: &str, cpg: &mut Cpg) {
    let line_no = node.start_position().row as u32 + 1;

    let fn_name = node
        .child_by_field_name("name")
        .map(|n| node_text(&n, src))
        .unwrap_or_else(|| "<anonymous>".to_string());

    let method_id = cpg.add_node(NodeKind::Method {
        name: fn_name,
        file: filename.to_string(),
        line: line_no,
    });

    // Parse receiver — treat as first parameter
    if let Some(receiver) = node.child_by_field_name("receiver") {
        // receiver is a `parameter_list` with one entry
        if let Some(param_list) = receiver.child(1) {
            // skip the outer parens — the inner list is a parameter_declaration
            let receiver_name = extract_go_param_name(&param_list, src);
            if !receiver_name.is_empty() {
                let p_id = cpg.add_node(NodeKind::Parameter {
                    name: receiver_name,
                    index: 0,
                });
                cpg.add_edge(method_id, p_id, EdgeKind::Ast);
            }
        }
    }

    parse_params_and_body(node, src, method_id, cpg);
}

/// Shared: parse parameters and body of a function/method declaration.
fn parse_params_and_body(node: &Node, src: &[u8], method_id: u32, cpg: &mut Cpg) {
    // Parse parameters
    if let Some(params_node) = node.child_by_field_name("parameters") {
        let mut param_index = 0u32;
        let mut cursor = params_node.walk();
        for param in params_node.children(&mut cursor) {
            match param.kind() {
                "parameter_declaration" | "variadic_parameter_declaration" => {
                    let name = extract_go_param_name(&param, src);
                    if !name.is_empty() {
                        let p_id = cpg.add_node(NodeKind::Parameter {
                            name,
                            index: param_index,
                        });
                        cpg.add_edge(method_id, p_id, EdgeKind::Ast);
                        param_index += 1;
                    }
                }
                _ => {}
            }
        }
    }

    // Parse body statements
    if let Some(body) = node.child_by_field_name("body") {
        let mut prev_stmt: Option<u32> = None;
        let mut cursor = body.walk();
        for stmt in body.children(&mut cursor) {
            if stmt.kind() == "{" || stmt.kind() == "}" {
                continue;
            }
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

/// Extract a parameter name from a `parameter_declaration` node.
///
/// Go parameter declarations can be:
/// - `name type`  →  name is the first child identifier
/// - `type` only  →  no explicit name
/// - `...type`    →  variadic, name is optional
fn extract_go_param_name(node: &Node, src: &[u8]) -> String {
    // In a parameter_declaration, the first child that is an identifier is the name.
    // If there is only one child (the type), there's no name.
    let mut cursor = node.walk();
    let children: Vec<_> = node.children(&mut cursor).collect();

    // Try to find identifier children — the first one is the name (if there are >=2 children)
    let identifiers: Vec<_> = children
        .iter()
        .filter(|c| c.kind() == "identifier")
        .collect();

    if identifiers.len() >= 2 {
        // name type — first identifier is name
        return node_text(identifiers[0], src);
    } else if identifiers.len() == 1 && children.len() >= 2 {
        // could be "name type" where type is not an identifier
        return node_text(identifiers[0], src);
    }

    // Only a type — no explicit param name
    String::new()
}

/// Parse a single Go statement node into CPG nodes.
fn parse_statement(node: &Node, src: &[u8], cpg: &mut Cpg) -> Option<u32> {
    let line_no = node.start_position().row as u32 + 1;

    match node.kind() {
        "return_statement" => {
            let ret_id = cpg.add_node(NodeKind::Return { line: line_no });
            // Attach return expression(s) if present
            let mut cursor = node.walk();
            let mut arg_idx = 0u32;
            for child in node.children(&mut cursor) {
                if child.kind() != "return" {
                    attach_expr(&child, src, ret_id, arg_idx, cpg);
                    arg_idx += 1;
                }
            }
            Some(ret_id)
        }

        "expression_statement" => {
            let child = node.child(0)?;
            parse_expr_as_stmt(&child, src, cpg)
        }

        "short_var_declaration" => {
            // lhs := rhs
            let lhs_node = node.child_by_field_name("left")?;
            let rhs_node = node.child_by_field_name("right")?;
            let lhs = node_text(&lhs_node, src);
            let assign_id = cpg.add_node(NodeKind::Assignment { lhs, line: line_no });
            attach_expr_list(&rhs_node, src, assign_id, cpg);
            Some(assign_id)
        }

        "assignment_statement" => {
            // lhs = rhs  (also +=, -=, etc.)
            let lhs_node = node.child_by_field_name("left")?;
            let rhs_node = node.child_by_field_name("right")?;
            let lhs = node_text(&lhs_node, src);
            let assign_id = cpg.add_node(NodeKind::Assignment { lhs, line: line_no });
            attach_expr_list(&rhs_node, src, assign_id, cpg);
            Some(assign_id)
        }

        "var_declaration" => {
            // var x = ... or var x type = ...
            let mut last_id: Option<u32> = None;
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                if child.kind() == "var_spec" {
                    if let Some(id) = parse_var_spec(&child, src, line_no, cpg) {
                        last_id = Some(id);
                    }
                }
            }
            last_id
        }

        "if_statement" => Some(cpg.add_node(NodeKind::ControlStructure {
            kind: CtrlKind::If,
            line: line_no,
        })),

        "for_statement" => Some(cpg.add_node(NodeKind::ControlStructure {
            kind: CtrlKind::For,
            line: line_no,
        })),

        "switch_statement" | "type_switch_statement" => {
            // Go has switch, not while — map to ControlStructure::If for now
            Some(cpg.add_node(NodeKind::ControlStructure {
                kind: CtrlKind::If,
                line: line_no,
            }))
        }

        "go_statement" => {
            // go someFunc() — treat the call as a regular call
            let child = node.child(1)?;
            parse_expr_as_stmt(&child, src, cpg)
        }

        "defer_statement" => {
            // defer someFunc() — treat the call as a regular call
            let child = node.child(1)?;
            parse_expr_as_stmt(&child, src, cpg)
        }

        "send_statement" => {
            // channel <- value
            let channel = node
                .child_by_field_name("channel")
                .map(|n| node_text(&n, src))
                .unwrap_or_else(|| "<-".to_string());
            let assign_id = cpg.add_node(NodeKind::Assignment {
                lhs: channel,
                line: line_no,
            });
            if let Some(value) = node.child_by_field_name("value") {
                attach_expr(&value, src, assign_id, 0, cpg);
            }
            Some(assign_id)
        }

        "inc_statement" | "dec_statement" => {
            // x++ or x--
            let child = node.child(0)?;
            let name = node_text(&child, src);
            Some(cpg.add_node(NodeKind::Assignment {
                lhs: name,
                line: line_no,
            }))
        }

        "comment" | "empty_statement" | "labeled_statement" => None,

        _ => None,
    }
}

/// Parse a `var_spec` (inside `var_declaration`).
fn parse_var_spec(node: &Node, src: &[u8], line_no: u32, cpg: &mut Cpg) -> Option<u32> {
    let name_node = node.child_by_field_name("name")?;
    let lhs = node_text(&name_node, src);
    let assign_id = cpg.add_node(NodeKind::Assignment { lhs, line: line_no });
    if let Some(value) = node.child_by_field_name("value") {
        attach_expr(&value, src, assign_id, 0, cpg);
    }
    Some(assign_id)
}

/// Parse an expression as a standalone statement.
fn parse_expr_as_stmt(node: &Node, src: &[u8], cpg: &mut Cpg) -> Option<u32> {
    let line_no = node.start_position().row as u32 + 1;

    match node.kind() {
        "call_expression" => Some(parse_call(node, src, cpg)),
        "identifier" => Some(cpg.add_node(NodeKind::Identifier {
            name: node_text(node, src),
            line: line_no,
        })),
        _ => None,
    }
}

/// Parse a `call_expression` into a CPG Call node with arguments.
fn parse_call(node: &Node, src: &[u8], cpg: &mut Cpg) -> u32 {
    let line_no = node.start_position().row as u32 + 1;

    let callee_name = node
        .child_by_field_name("function")
        .map(|n| node_text(&n, src))
        .unwrap_or_default();

    let call_id = cpg.add_node(NodeKind::Call {
        name: callee_name,
        line: line_no,
    });

    if let Some(args_node) = node.child_by_field_name("arguments") {
        let mut arg_index = 0u32;
        let mut cursor = args_node.walk();
        for arg in args_node.children(&mut cursor) {
            match arg.kind() {
                "(" | ")" | "," => continue,
                _ => {
                    attach_expr(&arg, src, call_id, arg_index, cpg);
                    arg_index += 1;
                }
            }
        }
    }

    call_id
}

/// Attach a Go expression_list's children to `parent`.
fn attach_expr_list(node: &Node, src: &[u8], parent: u32, cpg: &mut Cpg) {
    // expression_list contains comma-separated expressions
    let mut arg_index = 0u32;
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "," {
            continue;
        }
        attach_expr(&child, src, parent, arg_index, cpg);
        arg_index += 1;
    }
}

/// Attach a Go expression as a child of `parent` via an Argument edge.
fn attach_expr(node: &Node, src: &[u8], parent: u32, arg_index: u32, cpg: &mut Cpg) {
    let line_no = node.start_position().row as u32 + 1;

    match node.kind() {
        "call_expression" => {
            let call_id = parse_call(node, src, cpg);
            cpg.add_edge(parent, call_id, EdgeKind::Argument { index: arg_index });
        }

        "identifier" | "selector_expression" => {
            let name = node_text(node, src);
            let id = cpg.add_node(NodeKind::Identifier {
                name,
                line: line_no,
            });
            cpg.add_edge(parent, id, EdgeKind::Argument { index: arg_index });
        }

        "interpreted_string_literal"
        | "raw_string_literal"
        | "int_literal"
        | "float_literal"
        | "rune_literal"
        | "true"
        | "false"
        | "nil" => {
            let value = node_text(node, src);
            let lit_id = cpg.add_node(NodeKind::Literal {
                value,
                line: line_no,
            });
            cpg.add_edge(parent, lit_id, EdgeKind::Argument { index: arg_index });
        }

        "binary_expression" | "unary_expression" => {
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
            if let Some(inner) = node.child(1) {
                attach_expr(&inner, src, parent, arg_index, cpg);
            }
        }

        "index_expression" | "slice_expression" => {
            let name = node_text(node, src);
            let id = cpg.add_node(NodeKind::Identifier {
                name,
                line: line_no,
            });
            cpg.add_edge(parent, id, EdgeKind::Argument { index: arg_index });
        }

        "composite_literal" | "func_literal" => {
            let value = node_text(node, src);
            let lit_id = cpg.add_node(NodeKind::Literal {
                value,
                line: line_no,
            });
            cpg.add_edge(parent, lit_id, EdgeKind::Argument { index: arg_index });
        }

        "expression_list" => {
            attach_expr_list(node, src, parent, cpg);
        }

        _ => {
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
        "+" | "-"
            | "*"
            | "/"
            | "%"
            | "=="
            | "!="
            | "<"
            | ">"
            | "<="
            | ">="
            | "&&"
            | "||"
            | "!"
            | "&"
            | "|"
            | "^"
            | "~"
            | "<<"
            | ">>"
            | "&^"
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

    // ── 1. Simple function declaration with params ────────────────────────────

    #[test]
    fn function_declaration_with_params() {
        let source =
            "package main\n\nfunc greet(name string, count int) string {\n    return name\n}\n";
        let cpg = build_ts_go_cpg(source, "test.go");
        let methods = method_names(&cpg);
        assert!(
            methods.contains(&"greet".to_string()),
            "should find function greet"
        );
        let params = param_names(&cpg);
        assert!(
            params.contains(&"name".to_string()),
            "should find param name"
        );
        assert!(
            params.contains(&"count".to_string()),
            "should find param count"
        );
    }

    // ── 2. Method declaration with receiver ──────────────────────────────────

    #[test]
    fn method_declaration_with_receiver() {
        let source =
            "package main\n\nfunc (s *Server) Handle(req Request) Response {\n    return s.process(req)\n}\n";
        let cpg = build_ts_go_cpg(source, "test.go");
        let methods = method_names(&cpg);
        assert!(
            methods.contains(&"Handle".to_string()),
            "should find method Handle"
        );
        let params = param_names(&cpg);
        assert!(params.contains(&"req".to_string()), "should find param req");
    }

    // ── 3. Short variable declaration (:=) ───────────────────────────────────

    #[test]
    fn short_var_declaration() {
        let source = "package main\n\nfunc foo() {\n    result := bar()\n    count := 0\n}\n";
        let cpg = build_ts_go_cpg(source, "test.go");
        let assigns = assign_lhs_names(&cpg);
        assert!(
            assigns.contains(&"result".to_string()),
            "should find result := assignment"
        );
        assert!(
            assigns.contains(&"count".to_string()),
            "should find count := assignment"
        );
    }

    // ── 4. Call expression ────────────────────────────────────────────────────

    #[test]
    fn call_expression_is_captured() {
        let source =
            "package main\n\nfunc foo() {\n    fmt.Println(\"hello\")\n    doSomething(x, y)\n}\n";
        let cpg = build_ts_go_cpg(source, "test.go");
        let calls = call_names(&cpg);
        assert!(
            calls.iter().any(|n| n.contains("Println")),
            "should find fmt.Println call"
        );
        assert!(
            calls.contains(&"doSomething".to_string()),
            "should find doSomething call"
        );
    }

    // ── 5. Control structures ─────────────────────────────────────────────────

    #[test]
    fn control_structures_all_kinds() {
        let source = "package main\n\nfunc foo(x int) {\n    if x > 0 {}\n    for i := 0; i < 10; i++ {}\n    switch x {\n    case 1:\n    }\n}\n";
        let cpg = build_ts_go_cpg(source, "test.go");

        let has_if = cpg.nodes().any(|(_, k)| {
            matches!(
                k,
                NodeKind::ControlStructure {
                    kind: CtrlKind::If,
                    ..
                }
            )
        });
        let has_for = cpg.nodes().any(|(_, k)| {
            matches!(
                k,
                NodeKind::ControlStructure {
                    kind: CtrlKind::For,
                    ..
                }
            )
        });

        assert!(has_if, "should detect if");
        assert!(has_for, "should detect for");
    }

    // ── 6. Return statement ───────────────────────────────────────────────────

    #[test]
    fn return_statement_is_captured() {
        let source = "package main\n\nfunc double(x int) int {\n    return x * 2\n}\n";
        let cpg = build_ts_go_cpg(source, "test.go");
        assert!(cpg
            .nodes()
            .any(|(_, k)| matches!(k, NodeKind::Return { .. })));
    }

    // ── 7. Empty source does not panic ────────────────────────────────────────

    #[test]
    fn empty_source_no_crash() {
        let cpg = build_ts_go_cpg("", "empty.go");
        assert_eq!(cpg.node_count(), 0);
    }

    // ── 8. Malformed source does not panic ───────────────────────────────────

    #[test]
    fn malformed_source_no_crash() {
        let source = "package main\n\nfunc { broken @@@ syntax";
        let cpg = build_ts_go_cpg(source, "broken.go");
        // tree-sitter does error recovery — no panic expected
        let _ = cpg.node_count();
    }

    // ── 9. Nested call expressions ────────────────────────────────────────────

    #[test]
    fn nested_call_expressions() {
        let source = "package main\n\nfunc foo() {\n    outer(inner(x))\n}\n";
        let cpg = build_ts_go_cpg(source, "test.go");
        let calls = call_names(&cpg);
        assert!(
            calls.contains(&"outer".to_string()),
            "should find outer call"
        );
        assert!(
            calls.contains(&"inner".to_string()),
            "should find inner call"
        );
    }

    // ── 10. CFG edges between statements ─────────────────────────────────────

    #[test]
    fn cfg_edges_between_statements() {
        let source = "package main\n\nfunc foo() {\n    x := 1\n    y := 2\n    z := 3\n}\n";
        let cpg = build_ts_go_cpg(source, "test.go");
        let cfg_count = cpg
            .edges()
            .filter(|(_, _, k)| matches!(k, EdgeKind::Cfg))
            .count();
        assert!(cfg_count >= 2, "expected >= 2 CFG edges, got {cfg_count}");
    }

    // ── 11. Variadic parameter ────────────────────────────────────────────────

    #[test]
    fn variadic_parameter_extracted() {
        let source =
            "package main\n\nfunc sum(nums ...int) int {\n    total := 0\n    return total\n}\n";
        let cpg = build_ts_go_cpg(source, "test.go");
        let params = param_names(&cpg);
        assert!(
            params.contains(&"nums".to_string()),
            "variadic param 'nums' should be found"
        );
    }

    // ── 12. Goroutine and defer ───────────────────────────────────────────────

    #[test]
    fn goroutine_and_defer_captured() {
        let source = "package main\n\nfunc foo() {\n    go doAsync()\n    defer cleanup()\n}\n";
        let cpg = build_ts_go_cpg(source, "test.go");
        let calls = call_names(&cpg);
        assert!(
            calls.contains(&"doAsync".to_string()),
            "go goroutine call should be captured"
        );
        assert!(
            calls.contains(&"cleanup".to_string()),
            "defer call should be captured"
        );
    }

    // ── 13. Multiple functions in file ────────────────────────────────────────

    #[test]
    fn multiple_functions_in_file() {
        let source =
            "package main\n\nfunc foo() {}\nfunc bar(x int) {}\nfunc baz(a, b string) {}\n";
        let cpg = build_ts_go_cpg(source, "test.go");
        let methods = method_names(&cpg);
        assert!(methods.contains(&"foo".to_string()));
        assert!(methods.contains(&"bar".to_string()));
        assert!(methods.contains(&"baz".to_string()));
    }
}
