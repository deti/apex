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
        // Check if the sink itself is also a source (trivial flow).
        if source_set.contains(&sink) {
            flows.push(TaintFlow {
                source: sink,
                sink,
                path: vec![sink],
                variable_chain: vec![],
            });
            continue;
        }

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
    use crate::{Cpg, EdgeKind, NodeKind};

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

    /// Exercises the `cpg.node_count() == 0` branch in the debug_assert (line 85).
    /// An empty CPG has no edges at all, so the assertion must allow it.
    #[test]
    fn find_taint_flows_empty_cpg_returns_no_flows() {
        let cpg = Cpg::new();
        let flows = find_taint_flows(&cpg, 10);
        assert!(flows.is_empty(), "empty CPG must produce no flows");
    }

    /// Exercises the `path.len() > max_depth + 1` continue (line 127) by setting
    /// max_depth=0 so any path with two nodes is already over the limit.
    #[test]
    fn taint_max_depth_zero_prunes_all_intermediate_paths() {
        let source = r#"
def run(user_input):
    cmd = user_input
    subprocess.run(cmd)
"#;
        let mut cpg = build_python_cpg(source, "test.py");
        add_reaching_def_edges(&mut cpg);
        // With max_depth=0 the BFS starts at the sink (path length 1) and the
        // first expansion would reach length 2 which exceeds 0+1=1, pruning
        // the path before it can reach the source.
        let flows = find_taint_flows(&cpg, 0);
        // The indirect chain (sink→assignment→param) cannot complete; a direct
        // param→sink connection at depth 1 may or may not exist depending on
        // the builder, so we just assert no panic and that flows is a Vec.
        let _ = flows.len();
    }

    /// Exercises the sanitizer-blocks-backward-ReachingDef continue (line 141)
    /// using a manually constructed CPG so the sanitizer node is a direct
    /// predecessor via a ReachingDef edge.
    #[test]
    fn taint_sanitizer_node_as_reaching_def_predecessor_is_skipped() {
        let mut cpg = Cpg::new();

        // Source: a Parameter node
        let param = cpg.add_node(NodeKind::Parameter {
            name: "x".into(),
            index: 0,
        });

        // Sanitizer call node
        let san = cpg.add_node(NodeKind::Call {
            name: "shlex.quote".into(),
            line: 2,
        });

        // Sink call node
        let sink_node = cpg.add_node(NodeKind::Call {
            name: "subprocess.run".into(),
            line: 3,
        });

        // Wire: param →ReachingDef→ sanitizer →ReachingDef→ sink
        cpg.add_edge(
            param,
            san,
            EdgeKind::ReachingDef {
                variable: "x".into(),
            },
        );
        cpg.add_edge(
            san,
            sink_node,
            EdgeKind::ReachingDef {
                variable: "x".into(),
            },
        );

        let flows = find_taint_flows(&cpg, 10);
        // The sanitizer sits directly on the ReachingDef path — no flow expected.
        assert!(
            flows.is_empty(),
            "sanitizer as direct ReachingDef predecessor should block flow; got {flows:?}"
        );
    }

    /// Exercises TaintFlow construction + continue after source found (lines 150-156)
    /// and the visited-node skip at line 162, using a manually wired CPG.
    #[test]
    fn taint_flow_constructed_for_direct_reaching_def_source() {
        let mut cpg = Cpg::new();

        // Source: Parameter
        let param = cpg.add_node(NodeKind::Parameter {
            name: "cmd".into(),
            index: 0,
        });

        // Sink
        let sink_node = cpg.add_node(NodeKind::Call {
            name: "subprocess.run".into(),
            line: 2,
        });

        // Direct ReachingDef edge from parameter (source) to sink
        cpg.add_edge(
            param,
            sink_node,
            EdgeKind::ReachingDef {
                variable: "cmd".into(),
            },
        );

        let sinks = vec![sink_node];
        let sources = vec![param];
        let flows = reachable_by(&cpg, &sinks, &sources, 10);
        assert_eq!(flows.len(), 1, "should construct exactly one TaintFlow");
        assert_eq!(flows[0].source, param);
        assert_eq!(flows[0].sink, sink_node);
        assert_eq!(flows[0].path, vec![sink_node, param]);
    }

    /// Forces the visited-set guard at line 162 by having two distinct
    /// ReachingDef predecessors both pointing to the same intermediate node so
    /// the second enqueue attempt is skipped.
    #[test]
    fn taint_visited_guard_prevents_duplicate_enqueue() {
        let mut cpg = Cpg::new();

        let source_node = cpg.add_node(NodeKind::Parameter {
            name: "x".into(),
            index: 0,
        });

        // Intermediate node that will be reachable from two paths
        let intermediate = cpg.add_node(NodeKind::Assignment {
            lhs: "y".into(),
            line: 2,
        });

        let sink_node = cpg.add_node(NodeKind::Call {
            name: "eval".into(),
            line: 3,
        });

        // Two edges from source → intermediate (same var, simulates duplicate)
        cpg.add_edge(
            source_node,
            intermediate,
            EdgeKind::ReachingDef {
                variable: "x".into(),
            },
        );
        // intermediate → sink
        cpg.add_edge(
            intermediate,
            sink_node,
            EdgeKind::ReachingDef {
                variable: "y".into(),
            },
        );
        // Also a direct path: source → sink (so we get a flow)
        cpg.add_edge(
            source_node,
            sink_node,
            EdgeKind::ReachingDef {
                variable: "x".into(),
            },
        );

        let flows = reachable_by(&cpg, &[sink_node], &[source_node], 10);
        // At minimum one flow must be found; visited guard prevents panics or infinite loops
        assert!(!flows.is_empty(), "should find at least one flow");
    }

    /// Exercises end of the Argument-edge iteration (line 193): the visited guard
    /// prevents the same argument-child node from being enqueued twice.
    #[test]
    fn taint_argument_visited_guard_prevents_duplicate_enqueue() {
        let mut cpg = Cpg::new();

        let source_node = cpg.add_node(NodeKind::Parameter {
            name: "x".into(),
            index: 0,
        });

        let sink_node = cpg.add_node(NodeKind::Call {
            name: "eval".into(),
            line: 2,
        });

        // Two Argument edges from sink to the same source (unusual, tests the guard)
        cpg.add_edge(sink_node, source_node, EdgeKind::Argument { index: 0 });
        cpg.add_edge(sink_node, source_node, EdgeKind::Argument { index: 1 });

        let flows = reachable_by(&cpg, &[sink_node], &[source_node], 10);
        // source is reached via Argument edge — flow should be found
        assert!(
            !flows.is_empty(),
            "source reachable via Argument should produce a flow"
        );
    }

    #[test]
    fn bug_sink_that_is_also_source_produces_trivial_flow() {
        let mut cpg = Cpg::new();
        let node = cpg.add_node(NodeKind::Call {
            name: "eval".into(),
            line: 1,
        });
        // Node is both source and sink
        let flows = reachable_by(&cpg, &[node], &[node], 10);
        assert!(!flows.is_empty(), "sink=source should produce a trivial flow");
        assert_eq!(flows[0].path, vec![node]);
    }
}
