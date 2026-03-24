//! Tree-sitter-based JavaScript CPG builder.
//!
//! Replaces the line-based regex approach with a proper AST walk, correctly
//! handling arrow functions, method definitions, template literals, optional
//! chaining, and multi-line statements.

use apex_core::types::Language;
use tree_sitter::{Node, Parser};

use crate::builder::CpgBuilder;
use crate::{Cpg, CtrlKind, EdgeKind, NodeKind};

/// A [`CpgBuilder`] for JavaScript that uses tree-sitter for parsing.
///
/// Walks the concrete syntax tree to extract function declarations, arrow
/// functions, variable declarations, calls, control structures, and return
/// statements.
pub struct TsJsCpgBuilder;

impl CpgBuilder for TsJsCpgBuilder {
    fn build(&self, source: &str, filename: &str) -> Cpg {
        build_ts_js_cpg(source, filename)
    }

    fn language(&self) -> Language {
        Language::JavaScript
    }
}

/// Build a CPG from JavaScript source using tree-sitter.
pub fn build_ts_js_cpg(source: &str, filename: &str) -> Cpg {
    let mut cpg = Cpg::new();
    let mut parser = Parser::new();
    parser
        .set_language(&tree_sitter_javascript::LANGUAGE.into())
        .expect("failed to set tree-sitter JavaScript language");

    let Some(tree) = parser.parse(source, None) else {
        return cpg;
    };

    let root = tree.root_node();
    let src = source.as_bytes();

    walk_top_level(&root, src, filename, &mut cpg);

    cpg
}

/// Walk top-level statements looking for function declarations and class definitions.
fn walk_top_level(node: &Node, src: &[u8], filename: &str, cpg: &mut Cpg) {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        match child.kind() {
            "function_declaration" | "generator_function_declaration" => {
                parse_function_decl(&child, src, filename, cpg);
            }
            "class_declaration" | "class" => {
                if let Some(body) = child.child_by_field_name("body") {
                    walk_class_body(&body, src, filename, cpg);
                }
            }
            "export_statement" => {
                // export function foo() { ... }  or  export default function ...
                let mut inner_cursor = child.walk();
                for inner in child.children(&mut inner_cursor) {
                    match inner.kind() {
                        "function_declaration" | "generator_function_declaration" => {
                            parse_function_decl(&inner, src, filename, cpg);
                        }
                        "class_declaration" => {
                            if let Some(body) = inner.child_by_field_name("body") {
                                walk_class_body(&body, src, filename, cpg);
                            }
                        }
                        _ => {}
                    }
                }
            }
            "lexical_declaration" | "variable_declaration" => {
                // const foo = function() {} or const foo = () => {}
                parse_var_decl_for_functions(&child, src, filename, cpg);
            }
            _ => {}
        }
    }
}

/// Walk a class body looking for method definitions.
fn walk_class_body(node: &Node, src: &[u8], filename: &str, cpg: &mut Cpg) {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "method_definition" {
            parse_method_def(&child, src, filename, cpg);
        }
    }
}

/// Check if a variable declaration's initializer is a function, and if so, parse it.
fn parse_var_decl_for_functions(node: &Node, src: &[u8], filename: &str, cpg: &mut Cpg) {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "variable_declarator" {
            if let Some(value) = child.child_by_field_name("value") {
                match value.kind() {
                    "function" | "generator_function" | "arrow_function" => {
                        // Use the declarator name as the function name
                        let fn_name = child
                            .child_by_field_name("name")
                            .map(|n| node_text(&n, src))
                            .unwrap_or_else(|| "<anonymous>".to_string());
                        parse_function_body_with_name(&value, src, filename, &fn_name, cpg);
                    }
                    _ => {}
                }
            }
        }
    }
}

/// Parse a `function_declaration` node into CPG Method + body statements.
fn parse_function_decl(node: &Node, src: &[u8], filename: &str, cpg: &mut Cpg) {
    let fn_name = node
        .child_by_field_name("name")
        .map(|n| node_text(&n, src))
        .unwrap_or_else(|| "<anonymous>".to_string());
    parse_function_body_with_name(node, src, filename, &fn_name, cpg);
}

/// Parse a `method_definition` node into CPG Method + body statements.
fn parse_method_def(node: &Node, src: &[u8], filename: &str, cpg: &mut Cpg) {
    let fn_name = node
        .child_by_field_name("name")
        .map(|n| node_text(&n, src))
        .unwrap_or_else(|| "<anonymous>".to_string());
    parse_function_body_with_name(node, src, filename, &fn_name, cpg);
}

/// Shared logic: parse params and body from any function-like node.
fn parse_function_body_with_name(
    node: &Node,
    src: &[u8],
    filename: &str,
    fn_name: &str,
    cpg: &mut Cpg,
) {
    let line_no = node.start_position().row as u32 + 1;

    let method_id = cpg.add_node(NodeKind::Method {
        name: fn_name.to_string(),
        file: filename.to_string(),
        line: line_no,
    });

    // Parse parameters — field name is "parameters" for function_declaration,
    // "parameter" for arrow_function with single param
    let params_node = node
        .child_by_field_name("parameters")
        .or_else(|| node.child_by_field_name("parameter"));

    if let Some(params) = params_node {
        let mut param_index = 0u32;
        let mut cursor = params.walk();
        for param in params.children(&mut cursor) {
            let param_name = match param.kind() {
                "identifier" => node_text(&param, src),
                "assignment_pattern" => {
                    // param = default
                    param
                        .child_by_field_name("left")
                        .map(|n| node_text(&n, src))
                        .unwrap_or_default()
                }
                "rest_pattern" => {
                    // ...rest
                    param
                        .child(1)
                        .or_else(|| param.child(0))
                        .map(|n| node_text(&n, src))
                        .unwrap_or_default()
                }
                "object_pattern" | "array_pattern" => {
                    // Destructuring — represent as a literal pattern text
                    node_text(&param, src)
                }
                _ => continue,
            };

            if param_name.is_empty() || param_name == "," || param_name == "(" || param_name == ")"
            {
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
    let body = node.child_by_field_name("body");
    if let Some(body_node) = body {
        match body_node.kind() {
            "statement_block" => {
                let mut prev_stmt: Option<u32> = None;
                let mut cursor = body_node.walk();
                for stmt in body_node.children(&mut cursor) {
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
            // Arrow function with expression body: const f = x => x + 1
            _ => {
                if let Some(sid) = parse_expression_as_stmt(&body_node, src, cpg) {
                    cpg.add_edge(method_id, sid, EdgeKind::Ast);
                }
            }
        }
    }
}

/// Parse a single statement node into CPG nodes.
fn parse_statement(node: &Node, src: &[u8], cpg: &mut Cpg) -> Option<u32> {
    let line_no = node.start_position().row as u32 + 1;

    match node.kind() {
        "return_statement" => {
            let ret_id = cpg.add_node(NodeKind::Return { line: line_no });
            // Attach the return expression if present
            if let Some(expr) = node.child(1) {
                attach_expr(&expr, src, ret_id, 0, cpg);
            }
            Some(ret_id)
        }

        "expression_statement" => {
            let child = node.child(0)?;
            parse_expression_as_stmt(&child, src, cpg)
        }

        "lexical_declaration" | "variable_declaration" => {
            // const x = ..., let y = ..., var z = ...
            parse_var_decl_as_stmt(node, src, cpg)
        }

        "if_statement" => Some(cpg.add_node(NodeKind::ControlStructure {
            kind: CtrlKind::If,
            line: line_no,
        })),

        "while_statement" | "do_statement" => Some(cpg.add_node(NodeKind::ControlStructure {
            kind: CtrlKind::While,
            line: line_no,
        })),

        "for_statement" | "for_in_statement" | "for_of_statement" => {
            Some(cpg.add_node(NodeKind::ControlStructure {
                kind: CtrlKind::For,
                line: line_no,
            }))
        }

        "try_statement" => Some(cpg.add_node(NodeKind::ControlStructure {
            kind: CtrlKind::Try,
            line: line_no,
        })),

        "throw_statement" => {
            let call_id = cpg.add_node(NodeKind::Call {
                name: "throw".to_string(),
                line: line_no,
            });
            if let Some(expr) = node.child(1) {
                attach_expr(&expr, src, call_id, 0, cpg);
            }
            Some(call_id)
        }

        // Nested function declarations inside function bodies
        "function_declaration" | "generator_function_declaration" => {
            parse_function_decl(node, src, "<nested>", cpg);
            None
        }

        "class_declaration" => {
            if let Some(body) = node.child_by_field_name("body") {
                walk_class_body(&body, src, "<nested>", cpg);
            }
            None
        }

        "comment" | "empty_statement" => None,

        _ => None,
    }
}

/// Parse variable declarations (const/let/var) as assignment statements.
fn parse_var_decl_as_stmt(node: &Node, src: &[u8], cpg: &mut Cpg) -> Option<u32> {
    let line_no = node.start_position().row as u32 + 1;
    let mut last_id: Option<u32> = None;

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "variable_declarator" {
            let lhs = child
                .child_by_field_name("name")
                .map(|n| node_text(&n, src))
                .unwrap_or_default();

            if lhs.is_empty() {
                continue;
            }

            let assign_id = cpg.add_node(NodeKind::Assignment { lhs, line: line_no });

            if let Some(rhs) = child.child_by_field_name("value") {
                attach_expr(&rhs, src, assign_id, 0, cpg);
            }

            last_id = Some(assign_id);
        }
    }

    last_id
}

/// Parse an expression node that appears as a statement.
fn parse_expression_as_stmt(node: &Node, src: &[u8], cpg: &mut Cpg) -> Option<u32> {
    let line_no = node.start_position().row as u32 + 1;

    match node.kind() {
        "call_expression" => Some(parse_call(node, src, cpg)),
        "await_expression" => {
            // await someCall() — treat like a call
            if let Some(inner) = node.child(1) {
                parse_expression_as_stmt(&inner, src, cpg)
            } else {
                None
            }
        }
        "assignment_expression" => {
            let lhs = node
                .child_by_field_name("left")
                .map(|n| node_text(&n, src))
                .unwrap_or_default();
            let assign_id = cpg.add_node(NodeKind::Assignment { lhs, line: line_no });
            if let Some(rhs) = node.child_by_field_name("right") {
                attach_expr(&rhs, src, assign_id, 0, cpg);
            }
            Some(assign_id)
        }
        "new_expression" => {
            let callee = node
                .child_by_field_name("constructor")
                .map(|n| node_text(&n, src))
                .unwrap_or_else(|| "new".to_string());
            let call_id = cpg.add_node(NodeKind::Call {
                name: format!("new {callee}"),
                line: line_no,
            });
            if let Some(args) = node.child_by_field_name("arguments") {
                attach_arguments(&args, src, call_id, cpg);
            }
            Some(call_id)
        }
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
        attach_arguments(&args_node, src, call_id, cpg);
    }

    call_id
}

/// Attach all arguments of an `arguments` node to `parent`.
fn attach_arguments(args_node: &Node, src: &[u8], parent: u32, cpg: &mut Cpg) {
    let mut arg_index = 0u32;
    let mut cursor = args_node.walk();
    for arg in args_node.children(&mut cursor) {
        match arg.kind() {
            "(" | ")" | "," => continue,
            "spread_element" => {
                if let Some(inner) = arg.child(1) {
                    attach_expr(&inner, src, parent, arg_index, cpg);
                    arg_index += 1;
                }
            }
            _ => {
                attach_expr(&arg, src, parent, arg_index, cpg);
                arg_index += 1;
            }
        }
    }
}

/// Attach an expression as a child of `parent` via an Argument edge.
fn attach_expr(node: &Node, src: &[u8], parent: u32, arg_index: u32, cpg: &mut Cpg) {
    let line_no = node.start_position().row as u32 + 1;

    match node.kind() {
        "call_expression" => {
            let call_id = parse_call(node, src, cpg);
            cpg.add_edge(parent, call_id, EdgeKind::Argument { index: arg_index });
        }

        "identifier" | "member_expression" => {
            let name = node_text(node, src);
            let id = cpg.add_node(NodeKind::Identifier {
                name,
                line: line_no,
            });
            cpg.add_edge(parent, id, EdgeKind::Argument { index: arg_index });
        }

        "string" | "template_string" | "number" | "true" | "false" | "null" | "undefined" => {
            let value = node_text(node, src);
            let lit_id = cpg.add_node(NodeKind::Literal {
                value,
                line: line_no,
            });
            cpg.add_edge(parent, lit_id, EdgeKind::Argument { index: arg_index });
        }

        "new_expression" => {
            let callee = node
                .child_by_field_name("constructor")
                .map(|n| node_text(&n, src))
                .unwrap_or_else(|| "new".to_string());
            let call_id = cpg.add_node(NodeKind::Call {
                name: format!("new {callee}"),
                line: line_no,
            });
            if let Some(args) = node.child_by_field_name("arguments") {
                attach_arguments(&args, src, call_id, cpg);
            }
            cpg.add_edge(parent, call_id, EdgeKind::Argument { index: arg_index });
        }

        "await_expression" => {
            if let Some(inner) = node.child(1) {
                attach_expr(&inner, src, parent, arg_index, cpg);
            }
        }

        "binary_expression" | "unary_expression" | "logical_expression" => {
            // Recurse into operands
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

        "ternary_expression" => {
            if let Some(consequence) = node.child_by_field_name("consequence") {
                attach_expr(&consequence, src, parent, arg_index, cpg);
            }
        }

        "subscript_expression" => {
            let name = node_text(node, src);
            let id = cpg.add_node(NodeKind::Identifier {
                name,
                line: line_no,
            });
            cpg.add_edge(parent, id, EdgeKind::Argument { index: arg_index });
        }

        "array" | "object" => {
            let value = node_text(node, src);
            let lit_id = cpg.add_node(NodeKind::Literal {
                value,
                line: line_no,
            });
            cpg.add_edge(parent, lit_id, EdgeKind::Argument { index: arg_index });
        }

        "arrow_function" | "function" => {
            let value = format!("<function@{line_no}>");
            let lit_id = cpg.add_node(NodeKind::Literal {
                value,
                line: line_no,
            });
            cpg.add_edge(parent, lit_id, EdgeKind::Argument { index: arg_index });
        }

        "assignment_expression" => {
            let lhs = node
                .child_by_field_name("left")
                .map(|n| node_text(&n, src))
                .unwrap_or_default();
            let assign_id = cpg.add_node(NodeKind::Assignment { lhs, line: line_no });
            if let Some(rhs) = node.child_by_field_name("right") {
                attach_expr(&rhs, src, assign_id, 0, cpg);
            }
            cpg.add_edge(parent, assign_id, EdgeKind::Argument { index: arg_index });
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
            | "**"
            | "==="
            | "!=="
            | "=="
            | "!="
            | "<"
            | ">"
            | "<="
            | ">="
            | "&&"
            | "||"
            | "??"
            | "!"
            | "&"
            | "|"
            | "^"
            | "~"
            | "<<"
            | ">>"
            | ">>>"
            | "instanceof"
            | "in"
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
            "function greet(name, greeting) {\n    console.log(greeting + ' ' + name);\n}\n";
        let cpg = build_ts_js_cpg(source, "test.js");
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
            params.contains(&"greeting".to_string()),
            "should find param greeting"
        );
    }

    // ── 2. Arrow function assigned to const ──────────────────────────────────

    #[test]
    fn arrow_function_with_const() {
        let source = "const add = (a, b) => {\n    return a + b;\n};\n";
        let cpg = build_ts_js_cpg(source, "test.js");
        let methods = method_names(&cpg);
        assert!(
            methods.contains(&"add".to_string()),
            "arrow function should be found as method 'add'"
        );
        let params = param_names(&cpg);
        assert!(params.contains(&"a".to_string()));
        assert!(params.contains(&"b".to_string()));
    }

    // ── 3. Call expression ────────────────────────────────────────────────────

    #[test]
    fn call_expression_is_captured() {
        let source = "function foo() {\n    bar(1, 2);\n    baz();\n}\n";
        let cpg = build_ts_js_cpg(source, "test.js");
        let calls = call_names(&cpg);
        assert!(calls.contains(&"bar".to_string()), "should find call bar");
        assert!(calls.contains(&"baz".to_string()), "should find call baz");
    }

    // ── 4. Variable declaration assignment ───────────────────────────────────

    #[test]
    fn variable_declaration_assignment() {
        let source = "function process(input) {\n    const result = transform(input);\n    let count = 0;\n    return result;\n}\n";
        let cpg = build_ts_js_cpg(source, "test.js");
        let assigns = assign_lhs_names(&cpg);
        assert!(
            assigns.contains(&"result".to_string()),
            "should find const result assignment"
        );
        assert!(
            assigns.contains(&"count".to_string()),
            "should find let count assignment"
        );
    }

    // ── 5. Control structures ─────────────────────────────────────────────────

    #[test]
    fn control_structures_all_kinds() {
        let source = "function foo() {\n    if (x) {}\n    while (y) {}\n    for (let i = 0; i < 10; i++) {}\n    try {} catch(e) {}\n}\n";
        let cpg = build_ts_js_cpg(source, "test.js");

        let has_if = cpg.nodes().any(|(_, k)| {
            matches!(
                k,
                NodeKind::ControlStructure {
                    kind: CtrlKind::If,
                    ..
                }
            )
        });
        let has_while = cpg.nodes().any(|(_, k)| {
            matches!(
                k,
                NodeKind::ControlStructure {
                    kind: CtrlKind::While,
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
        let has_try = cpg.nodes().any(|(_, k)| {
            matches!(
                k,
                NodeKind::ControlStructure {
                    kind: CtrlKind::Try,
                    ..
                }
            )
        });

        assert!(has_if, "should detect if");
        assert!(has_while, "should detect while");
        assert!(has_for, "should detect for");
        assert!(has_try, "should detect try");
    }

    // ── 6. Method in class ────────────────────────────────────────────────────

    #[test]
    fn class_method_definition() {
        let source = "class Foo {\n    bar(x, y) {\n        return x + y;\n    }\n}\n";
        let cpg = build_ts_js_cpg(source, "test.js");
        let methods = method_names(&cpg);
        assert!(
            methods.contains(&"bar".to_string()),
            "class method should be found"
        );
        let params = param_names(&cpg);
        assert!(params.contains(&"x".to_string()));
        assert!(params.contains(&"y".to_string()));
    }

    // ── 7. Return statement ───────────────────────────────────────────────────

    #[test]
    fn return_statement_is_captured() {
        let source = "function foo(x) {\n    return x * 2;\n}\n";
        let cpg = build_ts_js_cpg(source, "test.js");
        assert!(cpg
            .nodes()
            .any(|(_, k)| matches!(k, NodeKind::Return { .. })));
    }

    // ── 8. Empty source does not panic ────────────────────────────────────────

    #[test]
    fn empty_source_no_crash() {
        let cpg = build_ts_js_cpg("", "empty.js");
        assert_eq!(cpg.node_count(), 0);
    }

    // ── 9. Malformed source does not panic ────────────────────────────────────

    #[test]
    fn malformed_source_no_crash() {
        let source = "function { broken syntax @@@ !!!";
        let cpg = build_ts_js_cpg(source, "broken.js");
        // tree-sitter does error recovery — no panic expected
        let _ = cpg.node_count();
    }

    // ── 10. Nested call expressions ───────────────────────────────────────────

    #[test]
    fn nested_call_expressions() {
        let source = "function foo() {\n    outer(inner(x));\n}\n";
        let cpg = build_ts_js_cpg(source, "test.js");
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

    // ── 11. CFG edges between statements ─────────────────────────────────────

    #[test]
    fn cfg_edges_between_statements() {
        let source = "function foo() {\n    const x = 1;\n    const y = 2;\n    const z = 3;\n}\n";
        let cpg = build_ts_js_cpg(source, "test.js");
        let cfg_count = cpg
            .edges()
            .filter(|(_, _, k)| matches!(k, EdgeKind::Cfg))
            .count();
        assert!(cfg_count >= 2, "expected >= 2 CFG edges, got {cfg_count}");
    }

    // ── 12. Default parameter ─────────────────────────────────────────────────

    #[test]
    fn default_parameter_extracted() {
        let source = "function greet(name = 'World') {\n    return 'Hello ' + name;\n}\n";
        let cpg = build_ts_js_cpg(source, "test.js");
        let params = param_names(&cpg);
        assert!(
            params.contains(&"name".to_string()),
            "default param 'name' should be found"
        );
    }

    // ── 13. For-in and for-of loops ───────────────────────────────────────────

    #[test]
    fn for_in_and_for_of() {
        let source = "function foo(items) {\n    for (const item of items) {}\n    for (const key in items) {}\n}\n";
        let cpg = build_ts_js_cpg(source, "test.js");
        let for_count = cpg
            .nodes()
            .filter(|(_, k)| {
                matches!(
                    k,
                    NodeKind::ControlStructure {
                        kind: CtrlKind::For,
                        ..
                    }
                )
            })
            .count();
        assert!(for_count >= 2, "should detect both for-of and for-in");
    }
}
