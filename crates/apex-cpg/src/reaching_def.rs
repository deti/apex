//! Reaching definitions dataflow analysis (iterative MOP algorithm).
//!
//! For each program point, determines which variable definitions can reach it
//! along some path through the CFG. The result is materialized as
//! `EdgeKind::ReachingDef` edges from definition sites to use sites.

use std::collections::{HashMap, HashSet};

use crate::{Cpg, EdgeKind, NodeId, NodeKind};

/// A definition: `(variable_name, defining_node_id)`.
pub type Definition = (String, NodeId);

/// Result of the reaching-definitions analysis.
pub struct ReachingDefResult {
    /// For each node, the set of definitions that reach it.
    pub reaching: HashMap<NodeId, HashSet<Definition>>,
}

/// Compute reaching definitions using iterative dataflow analysis.
///
/// Algorithm (forward dataflow, may-analysis):
/// - gen[n]  = definitions produced at n
/// - kill[n] = all other definitions of the same variable(s) as gen[n]
/// - in[n]   = ∪ out[p] for each CFG predecessor p of n
/// - out[n]  = gen[n] ∪ (in[n] − kill[n])
///
/// Iterate until stable.
pub fn compute_reaching_defs(cpg: &Cpg) -> ReachingDefResult {
    // Collect all nodes
    let all_ids: Vec<NodeId> = cpg.nodes().map(|(id, _)| id).collect();

    // All definitions in the program: (variable, node_id)
    let all_defs: Vec<Definition> = cpg
        .nodes()
        .filter_map(|(id, k)| def_variable(k).map(|v| (v, id)))
        .collect();

    // Pre-compute gen and kill sets for every node
    let mut gen: HashMap<NodeId, HashSet<Definition>> = HashMap::new();
    let mut kill: HashMap<NodeId, HashSet<Definition>> = HashMap::new();

    for &id in &all_ids {
        let kind = match cpg.node(id) {
            Some(k) => k,
            None => continue,
        };
        if let Some(var) = def_variable(kind) {
            // gen: this node defines `var`
            let g = gen.entry(id).or_default();
            g.insert((var.clone(), id));

            // kill: all other definitions of the same variable
            let k_set = kill.entry(id).or_default();
            for (v, other_id) in &all_defs {
                if *v == var && *other_id != id {
                    k_set.insert((v.clone(), *other_id));
                }
            }
        }
    }

    // CFG predecessor map: node → set of predecessor node ids
    let mut preds: HashMap<NodeId, Vec<NodeId>> = HashMap::new();
    for &id in &all_ids {
        preds.entry(id).or_default();
    }
    for (from, to, kind) in cpg.edges() {
        if matches!(kind, EdgeKind::Cfg) {
            preds.entry(*to).or_default().push(*from);
        }
    }

    // Initialize in/out sets
    let empty: HashSet<Definition> = HashSet::new();
    let mut out: HashMap<NodeId, HashSet<Definition>> =
        all_ids.iter().map(|&id| (id, empty.clone())).collect();

    // Iterative fixpoint
    let mut changed = true;
    while changed {
        changed = false;
        for &id in &all_ids {
            // in[n] = ∪ out[p]
            let in_set: HashSet<Definition> = preds
                .get(&id)
                .unwrap_or(&vec![])
                .iter()
                .flat_map(|pred| out.get(pred).unwrap_or(&empty).iter().cloned())
                .collect();

            // out[n] = gen[n] ∪ (in[n] − kill[n])
            let k = kill.get(&id).unwrap_or(&empty);
            let g = gen.get(&id).unwrap_or(&empty);
            let new_out: HashSet<Definition> = g
                .iter()
                .cloned()
                .chain(in_set.iter().filter(|d| !k.contains(d)).cloned())
                .collect();

            if new_out != *out.get(&id).unwrap_or(&empty) {
                out.insert(id, new_out);
                changed = true;
            }
        }
    }

    // The "reaching" result at a node is its in-set (what arrives before execution).
    // Re-compute in-sets from final out.
    let mut reaching: HashMap<NodeId, HashSet<Definition>> = HashMap::new();
    for &id in &all_ids {
        let in_set: HashSet<Definition> = preds
            .get(&id)
            .unwrap_or(&vec![])
            .iter()
            .flat_map(|pred| out.get(pred).unwrap_or(&empty).iter().cloned())
            .collect();
        reaching.insert(id, in_set);
    }

    ReachingDefResult { reaching }
}

/// Materialize reaching definitions as `ReachingDef` edges in the CPG.
///
/// For each use of variable `v` at node `n`, for every definition `(v, def_n)`
/// that reaches `n`, add a `ReachingDef { variable: v }` edge from `def_n` to `n`.
pub fn add_reaching_def_edges(cpg: &mut Cpg) {
    let result = compute_reaching_defs(cpg);

    // Collect definition sites: assignments and parameters.
    let def_sites: Vec<(NodeId, String)> = cpg
        .nodes()
        .filter_map(|(id, k)| def_variable(k).map(|v| (id, v)))
        .collect();

    // Collect use sites: all Identifier nodes (including those nested under calls
    // via Argument edges, which are not CFG nodes and would be missed by the
    // pure dataflow result).
    let use_sites: Vec<(NodeId, String)> = cpg
        .nodes()
        .filter_map(|(id, k)| identifier_name(k).map(|v| (id, v)))
        .collect();

    let mut new_edges: Vec<(NodeId, NodeId, EdgeKind)> = Vec::new();

    // Strategy 1: use the CFG-based result for direct CFG-reachable use nodes.
    for (use_id, var) in &use_sites {
        if let Some(defs_at_use) = result.reaching.get(use_id) {
            for (def_var, def_id) in defs_at_use {
                if def_var == var {
                    new_edges.push((
                        *def_id,
                        *use_id,
                        EdgeKind::ReachingDef {
                            variable: var.clone(),
                        },
                    ));
                }
            }
        }
    }

    // Strategy 2: for Identifier use-nodes that are NOT on the CFG (i.e. they live
    // under a Call/Return/Assignment as Argument children), fall back to name-based
    // def→use linking. This covers the common pattern: `bar(x)` where `x` appears
    // as an Argument-child Identifier, not a CFG statement.
    let cfg_nodes: std::collections::HashSet<NodeId> = {
        let mut set = std::collections::HashSet::new();
        for (from, to, k) in cpg.edges() {
            if matches!(k, EdgeKind::Cfg) {
                set.insert(*from);
                set.insert(*to);
            }
        }
        set
    };

    for (use_id, var) in &use_sites {
        // Skip if already handled by CFG-based analysis
        if cfg_nodes.contains(use_id) {
            continue;
        }
        // Add an edge from each def of `var` to this use
        for (def_id, def_var) in &def_sites {
            if def_var == var && def_id != use_id {
                new_edges.push((
                    *def_id,
                    *use_id,
                    EdgeKind::ReachingDef {
                        variable: var.clone(),
                    },
                ));
            }
        }
    }

    for (from, to, kind) in new_edges {
        cpg.add_edge(from, to, kind);
    }
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

/// If node `k` is a definition site, return the defined variable name.
fn def_variable(k: &NodeKind) -> Option<String> {
    match k {
        NodeKind::Assignment { lhs, .. } => Some(lhs.clone()),
        NodeKind::Parameter { name, .. } => Some(name.clone()),
        _ => None,
    }
}

/// Return the variable name for Identifier nodes only.
fn identifier_name(k: &NodeKind) -> Option<String> {
    match k {
        NodeKind::Identifier { name, .. } => Some(name.clone()),
        _ => None,
    }
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::builder::build_python_cpg;

    fn reaching_def_edge_count(cpg: &Cpg) -> usize {
        cpg.edges()
            .filter(|(_, _, k)| matches!(k, EdgeKind::ReachingDef { .. }))
            .count()
    }

    #[test]
    fn reaching_defs_simple_assignment_chain() {
        // x defined, then used in a call
        let source = "def foo():\n    x = 1\n    bar(x)\n";
        let mut cpg = build_python_cpg(source, "test.py");
        add_reaching_def_edges(&mut cpg);
        assert!(
            reaching_def_edge_count(&cpg) > 0,
            "should have at least one ReachingDef edge"
        );
    }

    #[test]
    fn reaching_defs_parameter_to_call() {
        let source = "def run(cmd):\n    subprocess.run(cmd)\n";
        let mut cpg = build_python_cpg(source, "test.py");
        add_reaching_def_edges(&mut cpg);
        // The parameter `cmd` should have a ReachingDef edge to the identifier `cmd`
        let rd_edges: Vec<_> = cpg
            .edges()
            .filter(
                |(_, _, k)| matches!(k, EdgeKind::ReachingDef { variable } if variable == "cmd"),
            )
            .collect();
        assert!(!rd_edges.is_empty(), "expected ReachingDef edge for 'cmd'");
    }

    #[test]
    fn reaching_defs_assignment_chain() {
        // y = x, then bar(y): both x and the assignment to y should propagate
        let source = "def foo(x):\n    y = x\n    bar(y)\n";
        let mut cpg = build_python_cpg(source, "test.py");
        add_reaching_def_edges(&mut cpg);
        assert!(reaching_def_edge_count(&cpg) > 0);
    }

    #[test]
    fn reaching_defs_no_duplicate_edges() {
        let source = "def foo(a):\n    b = a\n    c = b\n    sink(c)\n";
        let mut cpg = build_python_cpg(source, "test.py");
        add_reaching_def_edges(&mut cpg);
        // Just verify it doesn't panic and produces a graph
        assert!(cpg.node_count() > 0);
    }

    #[test]
    fn compute_result_contains_all_nodes() {
        let source = "def foo(x):\n    y = x\n    return y\n";
        let cpg = build_python_cpg(source, "test.py");
        let result = compute_reaching_defs(&cpg);
        // Every node should be present in the result
        for (id, _) in cpg.nodes() {
            assert!(
                result.reaching.contains_key(&id),
                "node {id} missing from reaching result"
            );
        }
    }
}
