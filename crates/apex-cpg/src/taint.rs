//! Backward taint reachability from security sinks to sources.
//!
//! Taint flows are found by BFS-backward from sink nodes over
//! `ReachingDef` edges, terminating at source nodes or at `max_depth`.
//! Sanitizer calls on the path break the flow.

use std::collections::{HashSet, VecDeque};

use crate::{Cpg, EdgeKind, NodeId, NodeKind};

/// A discovered taint flow from a source to a sink.
#[derive(Debug, Clone)]
pub struct TaintFlow {
    /// The source node (parameter, `input()` call, etc.).
    pub source: NodeId,
    /// The sink node (dangerous call).
    pub sink: NodeId,
    /// All nodes on the path from source to sink (inclusive).
    pub path: Vec<NodeId>,
    /// Variable names encountered along the path.
    pub variable_chain: Vec<String>,
}

/// User-controlled input sources.
pub const PYTHON_SOURCES: &[&str] = &[
    "request.args",
    "request.form",
    "request.json",
    "input",
    "sys.argv",
    "os.environ",
];

/// Dangerous sinks.
pub const PYTHON_SINKS: &[&str] = &[
    "subprocess.run",
    "subprocess.call",
    "subprocess.Popen",
    "os.system",
    "os.popen",
    "eval",
    "exec",
    "cursor.execute",
    "conn.execute",
    "open",
    "os.remove",
];

/// Sanitizers that break taint flow.
pub const PYTHON_SANITIZERS: &[&str] = &["shlex.quote", "os.path.normpath", "html.escape"];

/// Given a CPG with `ReachingDef` edges already materialized, find all taint
/// flows from sources to sinks via backward reachability.
///
/// # Prerequisites
///
/// **`add_reaching_def_edges` must be called on the CPG before this function.**
/// Without the `ReachingDef` edges, backward traversal has no data-flow links
/// to follow and will return no flows regardless of source/sink presence.
///
/// ```ignore
/// let mut cpg = build_python_cpg(source, "file.py");
/// add_reaching_def_edges(&mut cpg);  // required first
/// let flows = find_taint_flows(&cpg, 10);
/// ```
///
/// # Sanitizers
///
/// Sanitizer calls (e.g. `shlex.quote`) break taint propagation. During backward
/// BFS, if the traversal would pass through a sanitizer Call node — either as a
/// direct predecessor via `ReachingDef` or as an RHS subexpression via `Argument`
/// edges — that path is cut. This correctly handles the pattern:
/// `safe = shlex.quote(user_input); subprocess.run(safe)` where the `Assignment`
/// node for `safe` is the BFS node and `shlex.quote` is its `Argument` child.
pub fn find_taint_flows(cpg: &Cpg, max_depth: usize) -> Vec<TaintFlow> {
    // Caller must invoke add_reaching_def_edges(&mut cpg) before this function.
    // Without ReachingDef edges the backward traversal has no data-flow links.
    debug_assert!(
        cpg.edges()
            .any(|(_, _, k)| matches!(k, EdgeKind::ReachingDef { .. }))
            || cpg.node_count() == 0,
        "find_taint_flows called without any ReachingDef edges — \
         did you forget to call add_reaching_def_edges first?"
    );

    let sinks: Vec<NodeId> = cpg
        .nodes()
        .filter_map(|(id, k)| is_sink(k).then_some(id))
        .collect();

    let sources: Vec<NodeId> = cpg
        .nodes()
        .filter_map(|(id, k)| is_source_node(cpg, id, k).then_some(id))
        .collect();

    reachable_by(cpg, &sinks, &sources, max_depth)
}

/// Find taint flows from an explicit set of sink and source nodes.
pub fn reachable_by(
    cpg: &Cpg,
    sinks: &[NodeId],
    sources: &[NodeId],
    max_depth: usize,
) -> Vec<TaintFlow> {
    let source_set: HashSet<NodeId> = sources.iter().copied().collect();
    let sanitizer_set: HashSet<NodeId> = cpg
        .nodes()
        .filter_map(|(id, k)| is_sanitizer(k).then_some(id))
        .collect();

    let mut flows = Vec::new();

    for &sink in sinks {
        // BFS backward from sink over ReachingDef edges.
        // State: (current_node, path_so_far, variables_seen)
        let mut queue: VecDeque<(NodeId, Vec<NodeId>, Vec<String>)> =
            VecDeque::from([(sink, vec![sink], vec![])]);
        let mut visited: HashSet<NodeId> = HashSet::from([sink]);

        while let Some((current, path, vars)) = queue.pop_front() {
            if path.len() > max_depth + 1 {
                continue;
            }

            // Walk incoming ReachingDef edges backward
            for (from, _to, kind) in cpg.edges_to(current) {
                let var = match kind {
                    EdgeKind::ReachingDef { variable } => variable.clone(),
                    _ => continue,
                };

                let def_id = *from;

                // Sanitizer breaks taint
                if sanitizer_set.contains(&def_id) {
                    continue;
                }

                let mut new_path = path.clone();
                new_path.push(def_id);
                let mut new_vars = vars.clone();
                new_vars.push(var);

                if source_set.contains(&def_id) {
                    flows.push(TaintFlow {
                        source: def_id,
                        sink,
                        path: new_path,
                        variable_chain: new_vars,
                    });
                    continue;
                }

                if !visited.contains(&def_id) {
                    visited.insert(def_id);
                    queue.push_back((def_id, new_path, new_vars));
                }
            }

            // Follow Argument edges forward from Call nodes: Call→Argument children
            // represent taint inputs to the call. When traversing backward we need
            // to descend into the call's arguments to find where data comes from.
            for (_from, to, kind) in cpg.edges_from(current) {
                if !matches!(kind, EdgeKind::Argument { .. }) {
                    continue;
                }
                let arg_id = *to;
                if sanitizer_set.contains(&arg_id) {
                    continue;
                }
                let mut new_path = path.clone();
                new_path.push(arg_id);
                let new_vars = vars.clone();

                if source_set.contains(&arg_id) {
                    flows.push(TaintFlow {
                        source: arg_id,
                        sink,
                        path: new_path,
                        variable_chain: new_vars,
                    });
                    continue;
                }

                if !visited.contains(&arg_id) {
                    visited.insert(arg_id);
                    queue.push_back((arg_id, new_path, new_vars));
                }
            }
        }
    }

    flows
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

fn is_sink(k: &NodeKind) -> bool {
    match k {
        NodeKind::Call { name, .. } => PYTHON_SINKS.iter().any(|s| name == s),
        _ => false,
    }
}

fn is_sanitizer(k: &NodeKind) -> bool {
    match k {
        NodeKind::Call { name, .. } => PYTHON_SANITIZERS.iter().any(|s| name == s),
        _ => false,
    }
}

/// A node is a source if it is a Parameter (always user-controlled in a
/// public function), or a Call that matches a known source function.
fn is_source_node(cpg: &Cpg, id: NodeId, k: &NodeKind) -> bool {
    match k {
        NodeKind::Parameter { .. } => true,
        NodeKind::Call { name, .. } => PYTHON_SOURCES.iter().any(|s| name == s),
        // An Identifier can also be a source if it directly references a source call's rhs
        NodeKind::Identifier { name, .. } => {
            // Check if any incoming ReachingDef comes from a parameter
            cpg.edges_to(id).iter().any(|(from, _, ek)| {
                matches!(ek, EdgeKind::ReachingDef { .. })
                    && matches!(cpg.node(*from), Some(NodeKind::Parameter { .. }))
            }) || PYTHON_SOURCES.contains(&name.as_str())
        }
        _ => false,
    }
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::builder::build_python_cpg;
    use crate::reaching_def::add_reaching_def_edges;

    #[test]
    fn taint_finds_command_injection_flow() {
        let source = r#"
def run_command(user_input):
    cmd = user_input
    subprocess.run(cmd)
"#;
        let mut cpg = build_python_cpg(source, "test.py");
        add_reaching_def_edges(&mut cpg);
        let flows = find_taint_flows(&cpg, 10);
        assert!(
            !flows.is_empty(),
            "should find flow from parameter to subprocess.run"
        );
    }

    #[test]
    fn taint_no_flow_without_connection() {
        let source = r#"
def safe():
    cmd = "echo hello"
    subprocess.run(cmd)
"#;
        let mut cpg = build_python_cpg(source, "test.py");
        add_reaching_def_edges(&mut cpg);
        let flows = find_taint_flows(&cpg, 10);
        // "echo hello" is a Literal, not a Parameter/source — no taint path
        assert!(flows.is_empty(), "should find no flows; got: {flows:?}");
    }

    #[test]
    fn taint_finds_eval_flow() {
        let source = r#"
def dangerous(user_code):
    eval(user_code)
"#;
        let mut cpg = build_python_cpg(source, "test.py");
        add_reaching_def_edges(&mut cpg);
        let flows = find_taint_flows(&cpg, 10);
        assert!(!flows.is_empty(), "should find flow to eval");
    }

    #[test]
    fn taint_finds_sql_injection_flow() {
        let source = r#"
def query(user_id):
    q = user_id
    cursor.execute(q)
"#;
        let mut cpg = build_python_cpg(source, "test.py");
        add_reaching_def_edges(&mut cpg);
        let flows = find_taint_flows(&cpg, 10);
        assert!(!flows.is_empty(), "should find SQL injection flow");
    }

    #[test]
    fn taint_sanitizer_not_in_flow_direct_param_to_sink() {
        // shlex.quote is in PYTHON_SANITIZERS — here we verify that a sanitizer
        // call node is NOT treated as a source. The test uses a raw param path
        // (no sanitizer on the direct path) so we still expect a flow.
        let source = r#"
def run(cmd):
    subprocess.run(cmd)
"#;
        let mut cpg = build_python_cpg(source, "test.py");
        add_reaching_def_edges(&mut cpg);
        let flows = find_taint_flows(&cpg, 10);
        assert!(!flows.is_empty(), "direct param→sink should still be found");
    }

    #[test]
    fn taint_no_flow_on_safe_literal_function() {
        let source = r#"
def greet():
    name = "world"
    print(name)
"#;
        let mut cpg = build_python_cpg(source, "test.py");
        add_reaching_def_edges(&mut cpg);
        // print is not a sink
        let flows = find_taint_flows(&cpg, 10);
        assert!(flows.is_empty());
    }

    #[test]
    fn taint_flow_path_includes_sink_and_source() {
        let source = r#"
def run_command(user_input):
    cmd = user_input
    subprocess.run(cmd)
"#;
        let mut cpg = build_python_cpg(source, "test.py");
        add_reaching_def_edges(&mut cpg);
        let flows = find_taint_flows(&cpg, 10);
        let flow = flows.first().expect("should have a flow");
        // path must contain both source and sink
        assert!(flow.path.contains(&flow.source));
        assert!(flow.path.contains(&flow.sink));
    }

    #[test]
    fn taint_sanitizer_blocks_flow() {
        let source = r#"
def run(user_input):
    safe = shlex.quote(user_input)
    subprocess.run(safe)
"#;
        let mut cpg = build_python_cpg(source, "test.py");
        add_reaching_def_edges(&mut cpg);
        let flows = find_taint_flows(&cpg, 10);
        assert!(
            flows.is_empty(),
            "sanitizer should block taint flow from user_input to subprocess.run; got: {flows:?}"
        );
    }

    #[test]
    fn taint_unsanitized_flow_detected() {
        let source = r#"
def run(user_input):
    cmd = user_input
    subprocess.run(cmd)
"#;
        let mut cpg = build_python_cpg(source, "test.py");
        add_reaching_def_edges(&mut cpg);
        let flows = find_taint_flows(&cpg, 10);
        assert!(!flows.is_empty(), "unsanitized flow should be detected");
    }

    #[test]
    fn reachable_by_empty_sources_returns_no_flows() {
        let source = r#"
def foo(x):
    subprocess.run(x)
"#;
        let mut cpg = build_python_cpg(source, "test.py");
        add_reaching_def_edges(&mut cpg);
        let sinks: Vec<NodeId> = cpg
            .nodes()
            .filter_map(|(id, k)| is_sink(k).then_some(id))
            .collect();
        let flows = reachable_by(&cpg, &sinks, &[], 10);
        assert!(flows.is_empty());
    }

    #[test]
    fn taint_multiple_sinks_found() {
        let source = r#"
def attack(payload):
    eval(payload)
    exec(payload)
"#;
        let mut cpg = build_python_cpg(source, "test.py");
        add_reaching_def_edges(&mut cpg);
        let flows = find_taint_flows(&cpg, 10);
        // Should find flows to both eval and exec
        assert!(
            flows.len() >= 2,
            "expected flows to both eval and exec, got {}",
            flows.len()
        );
    }
}
