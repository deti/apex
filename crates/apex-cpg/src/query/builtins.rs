//! Built-in predicates that resolve `DataSource` variants to sets of CPG nodes.

use crate::taint_rules::TaintRuleSet;
use crate::{Cpg, NodeId, NodeKind};

/// Resolve taint sources of a given kind from the CPG.
///
/// If `kind` is non-empty, only sources whose name matches a rule-set source
/// pattern are returned. The `kind` string itself is currently used as a
/// documentation hint; the actual filtering uses the `TaintRuleSet`.
pub fn resolve_sources(cpg: &Cpg, _kind: &str, rules: &TaintRuleSet) -> Vec<NodeId> {
    cpg.nodes()
        .filter_map(|(id, node)| {
            let is_src = match node {
                NodeKind::Parameter { .. } => true,
                NodeKind::Call { name, .. } => rules.is_source(name),
                NodeKind::Identifier { name, .. } => rules.is_source(name),
                _ => false,
            };
            is_src.then_some(id)
        })
        .collect()
}

/// Resolve taint sinks of a given kind from the CPG.
pub fn resolve_sinks(cpg: &Cpg, _kind: &str, rules: &TaintRuleSet) -> Vec<NodeId> {
    cpg.nodes()
        .filter_map(|(id, node)| {
            let is_sink = match node {
                NodeKind::Call { name, .. } => rules.is_sink(name),
                _ => false,
            };
            is_sink.then_some(id)
        })
        .collect()
}

/// Resolve all Call nodes in the CPG.
pub fn resolve_calls(cpg: &Cpg) -> Vec<NodeId> {
    cpg.nodes()
        .filter_map(|(id, node)| matches!(node, NodeKind::Call { .. }).then_some(id))
        .collect()
}

/// Resolve all Assignment nodes in the CPG.
pub fn resolve_assignments(cpg: &Cpg) -> Vec<NodeId> {
    cpg.nodes()
        .filter_map(|(id, node)| matches!(node, NodeKind::Assignment { .. }).then_some(id))
        .collect()
}

/// Resolve all Method (function definition) nodes in the CPG.
pub fn resolve_functions(cpg: &Cpg) -> Vec<NodeId> {
    cpg.nodes()
        .filter_map(|(id, node)| matches!(node, NodeKind::Method { .. }).then_some(id))
        .collect()
}

/// Resolve nodes matching a given node type name (e.g., "Call", "Literal", "Parameter").
pub fn resolve_node_type(cpg: &Cpg, type_name: &str) -> Vec<NodeId> {
    cpg.nodes()
        .filter_map(|(id, node)| {
            let matches = matches!(
                (type_name, node),
                ("Method", NodeKind::Method { .. })
                    | ("Parameter", NodeKind::Parameter { .. })
                    | ("Call", NodeKind::Call { .. })
                    | ("Identifier", NodeKind::Identifier { .. })
                    | ("Literal", NodeKind::Literal { .. })
                    | ("Return", NodeKind::Return { .. })
                    | ("ControlStructure", NodeKind::ControlStructure { .. })
                    | ("Assignment", NodeKind::Assignment { .. })
            );
            matches.then_some(id)
        })
        .collect()
}

/// Extract the "name" of a node (for matching, display, etc.).
pub fn node_name(node: &NodeKind) -> &str {
    match node {
        NodeKind::Method { name, .. } => name,
        NodeKind::Parameter { name, .. } => name,
        NodeKind::Call { name, .. } => name,
        NodeKind::Identifier { name, .. } => name,
        NodeKind::Literal { value, .. } => value,
        NodeKind::Return { .. } => "return",
        NodeKind::ControlStructure { .. } => "control",
        NodeKind::Assignment { lhs, .. } => lhs,
    }
}

/// Extract the line number of a node, if available.
pub fn node_line(node: &NodeKind) -> Option<u32> {
    match node {
        NodeKind::Method { line, .. }
        | NodeKind::Call { line, .. }
        | NodeKind::Identifier { line, .. }
        | NodeKind::Literal { line, .. }
        | NodeKind::Return { line }
        | NodeKind::ControlStructure { line, .. }
        | NodeKind::Assignment { line, .. } => Some(*line),
        NodeKind::Parameter { .. } => None,
    }
}

/// Extract the file of a node (only Method nodes carry file info).
pub fn node_file(node: &NodeKind) -> Option<&str> {
    match node {
        NodeKind::Method { file, .. } => Some(file),
        _ => None,
    }
}

/// Build a location string for a node: "file:line" or just "line:N" or "name".
pub fn node_location(node: &NodeKind) -> String {
    match node {
        NodeKind::Method { file, line, name } => format!("{file}:{line} ({name})"),
        NodeKind::Call { name, line } => format!("line:{line} ({name})"),
        NodeKind::Identifier { name, line } => format!("line:{line} ({name})"),
        NodeKind::Literal { value, line } => format!("line:{line} ({value})"),
        NodeKind::Return { line } => format!("line:{line} (return)"),
        NodeKind::ControlStructure { line, .. } => format!("line:{line} (control)"),
        NodeKind::Assignment { lhs, line } => format!("line:{line} ({lhs})"),
        NodeKind::Parameter { name, index } => format!("param:{index} ({name})"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_cpg() -> Cpg {
        let mut cpg = Cpg::new();
        cpg.add_node(NodeKind::Method {
            name: "handle_request".into(),
            file: "app.py".into(),
            line: 1,
        });
        cpg.add_node(NodeKind::Parameter {
            name: "user_input".into(),
            index: 0,
        });
        cpg.add_node(NodeKind::Call {
            name: "cursor.execute".into(),
            line: 3,
        });
        cpg.add_node(NodeKind::Call {
            name: "print".into(),
            line: 4,
        });
        cpg.add_node(NodeKind::Assignment {
            lhs: "query".into(),
            line: 2,
        });
        cpg.add_node(NodeKind::Literal {
            value: "hello".into(),
            line: 5,
        });
        cpg.add_node(NodeKind::Call {
            name: "request.args".into(),
            line: 6,
        });
        cpg
    }

    #[test]
    fn resolve_calls_returns_call_nodes() {
        let cpg = sample_cpg();
        let calls = resolve_calls(&cpg);
        // cursor.execute, print, request.args
        assert_eq!(calls.len(), 3);
    }

    #[test]
    fn resolve_functions_returns_method_nodes() {
        let cpg = sample_cpg();
        let funcs = resolve_functions(&cpg);
        assert_eq!(funcs.len(), 1);
    }

    #[test]
    fn resolve_assignments_returns_assignment_nodes() {
        let cpg = sample_cpg();
        let assigns = resolve_assignments(&cpg);
        assert_eq!(assigns.len(), 1);
    }

    #[test]
    fn resolve_sources_finds_taint_sources() {
        let cpg = sample_cpg();
        let rules = TaintRuleSet::python_defaults();
        let sources = resolve_sources(&cpg, "UserInput", &rules);
        // Parameter node is always a source, plus request.args Call
        assert!(sources.len() >= 2, "got {sources:?}");
    }

    #[test]
    fn resolve_sinks_finds_taint_sinks() {
        let cpg = sample_cpg();
        let rules = TaintRuleSet::python_defaults();
        let sinks = resolve_sinks(&cpg, "SQLInjection", &rules);
        // cursor.execute matches "execute" sink
        assert_eq!(sinks.len(), 1);
    }

    #[test]
    fn resolve_node_type_finds_literals() {
        let cpg = sample_cpg();
        let lits = resolve_node_type(&cpg, "Literal");
        assert_eq!(lits.len(), 1);
    }

    #[test]
    fn resolve_node_type_unknown_returns_empty() {
        let cpg = sample_cpg();
        let nodes = resolve_node_type(&cpg, "FooBar");
        assert!(nodes.is_empty());
    }

    #[test]
    fn node_name_extracts_names() {
        let n = NodeKind::Call {
            name: "foo".into(),
            line: 1,
        };
        assert_eq!(node_name(&n), "foo");
    }

    #[test]
    fn node_location_formats_method() {
        let n = NodeKind::Method {
            name: "main".into(),
            file: "app.py".into(),
            line: 10,
        };
        let loc = node_location(&n);
        assert!(loc.contains("app.py"));
        assert!(loc.contains("10"));
    }

    #[test]
    fn node_line_returns_line_for_call() {
        let n = NodeKind::Call {
            name: "f".into(),
            line: 42,
        };
        assert_eq!(node_line(&n), Some(42));
    }

    #[test]
    fn node_line_returns_none_for_param() {
        let n = NodeKind::Parameter {
            name: "x".into(),
            index: 0,
        };
        assert_eq!(node_line(&n), None);
    }
}
