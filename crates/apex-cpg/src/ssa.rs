//! SSA (Static Single Assignment) intermediate representation for the CPG.
//!
//! Transforms the CPG into SSA form where every use of a variable directly
//! points to its single definition. This eliminates the need for iterative
//! fixpoint dataflow analysis — def-use and use-def chains are explicit.

use std::collections::{HashMap, HashSet};

use crate::{Cpg, EdgeKind, NodeId, NodeKind};

/// SSA variable — a variable with a version number.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SsaVar {
    pub name: String,
    pub version: u32,
}

impl SsaVar {
    pub fn new(name: &str, version: u32) -> Self {
        Self {
            name: name.to_string(),
            version,
        }
    }

    pub fn display(&self) -> String {
        format!("{}_{}", self.name, self.version)
    }
}

/// A phi node: `x_3 = phi(x_1, x_2)` at a control flow join point.
#[derive(Debug, Clone)]
pub struct PhiNode {
    /// The variable being defined (e.g. x_3).
    pub result: SsaVar,
    /// The source versions (e.g. x_1, x_2).
    pub sources: Vec<SsaVar>,
    /// CPG node where the phi is placed.
    pub block_id: NodeId,
}

/// SSA form of a function.
#[derive(Debug, Clone)]
pub struct SsaFunction {
    pub name: String,
    /// node -> SSA var it defines.
    pub definitions: HashMap<NodeId, SsaVar>,
    /// node -> SSA vars it uses.
    pub uses: HashMap<NodeId, Vec<SsaVar>>,
    pub phi_nodes: Vec<PhiNode>,
    /// var -> nodes that use it.
    pub def_use_chains: HashMap<SsaVar, Vec<NodeId>>,
    /// (node, var_name) -> defining SSA var.
    pub use_def_chains: HashMap<(NodeId, String), SsaVar>,
}

/// Version counter for SSA renaming.
struct VersionCounter {
    counters: HashMap<String, u32>,
}

impl VersionCounter {
    fn new() -> Self {
        Self {
            counters: HashMap::new(),
        }
    }

    fn next(&mut self, name: &str) -> u32 {
        let counter = self.counters.entry(name.to_string()).or_insert(0);
        let version = *counter;
        *counter += 1;
        version
    }

    fn current(&self, name: &str) -> Option<u32> {
        self.counters
            .get(name)
            .and_then(|c| if *c == 0 { None } else { Some(c - 1) })
    }
}

/// Compute dominator tree using the iterative algorithm from Cooper, Harvey, Kennedy.
///
/// Returns: node -> immediate dominator.
pub fn compute_dominators(cpg: &Cpg, entry: NodeId) -> HashMap<NodeId, NodeId> {
    let cfg_successors = build_cfg_successors(cpg);
    let cfg_predecessors = build_cfg_predecessors(cpg);
    let nodes = reachable_nodes(&cfg_successors, entry);

    let order: HashMap<NodeId, usize> = nodes.iter().enumerate().map(|(i, &n)| (n, i)).collect();

    let mut idom: HashMap<NodeId, NodeId> = HashMap::new();
    idom.insert(entry, entry);

    let mut changed = true;
    while changed {
        changed = false;
        for &node in &nodes {
            if node == entry {
                continue;
            }
            let preds = cfg_predecessors.get(&node).cloned().unwrap_or_default();
            if preds.is_empty() {
                continue;
            }

            // Find first predecessor with a dominator
            let mut new_idom = None;
            for &p in &preds {
                if idom.contains_key(&p) {
                    new_idom = Some(p);
                    break;
                }
            }

            if let Some(mut dom) = new_idom {
                for &p in &preds {
                    if p == dom {
                        continue;
                    }
                    if idom.contains_key(&p) {
                        dom = intersect(&idom, p, dom, &order);
                    }
                }
                if idom.get(&node) != Some(&dom) {
                    idom.insert(node, dom);
                    changed = true;
                }
            }
        }
    }

    idom
}

fn intersect(
    idom: &HashMap<NodeId, NodeId>,
    mut a: NodeId,
    mut b: NodeId,
    order: &HashMap<NodeId, usize>,
) -> NodeId {
    while a != b {
        while order.get(&a) > order.get(&b) {
            a = *idom.get(&a).unwrap_or(&a);
        }
        while order.get(&b) > order.get(&a) {
            b = *idom.get(&b).unwrap_or(&b);
        }
    }
    a
}

/// Compute dominance frontiers from the dominator tree.
pub fn compute_dominance_frontiers(
    cpg: &Cpg,
    _entry: NodeId,
    idom: &HashMap<NodeId, NodeId>,
) -> HashMap<NodeId, HashSet<NodeId>> {
    let cfg_predecessors = build_cfg_predecessors(cpg);
    let mut frontiers: HashMap<NodeId, HashSet<NodeId>> = HashMap::new();

    for (&node, preds) in &cfg_predecessors {
        if preds.len() >= 2 {
            // Join point
            for &pred in preds {
                let mut runner = pred;
                while Some(&runner) != idom.get(&node) {
                    frontiers.entry(runner).or_default().insert(node);
                    if let Some(&idom_runner) = idom.get(&runner) {
                        if idom_runner == runner {
                            break;
                        }
                        runner = idom_runner;
                    } else {
                        break;
                    }
                }
            }
        }
    }

    frontiers
}

/// Convert a function in the CPG to SSA form.
pub fn convert_to_ssa(cpg: &Cpg, method_node: NodeId) -> SsaFunction {
    let method_name = match cpg.node(method_node) {
        Some(NodeKind::Method { name, .. }) => name.clone(),
        _ => "unknown".to_string(),
    };

    // 1. Find all assignments and their variables
    let assignments = find_assignments(cpg, method_node);

    // 2. Compute dominators and frontiers
    let idom = compute_dominators(cpg, method_node);
    let frontiers = compute_dominance_frontiers(cpg, method_node, &idom);

    // 3. Determine where phi nodes are needed
    let phi_locations = compute_phi_locations(&assignments, &frontiers);

    // 4. Rename variables (assign SSA versions)
    let mut counter = VersionCounter::new();
    let mut definitions = HashMap::new();
    let mut uses = HashMap::new();
    let mut phi_nodes = Vec::new();

    // Create initial versions for parameters
    for (id, kind) in cpg.nodes() {
        if let NodeKind::Parameter { name, .. } = kind {
            // Check if this parameter belongs to our method
            let is_child = cpg
                .edges_from(method_node)
                .iter()
                .any(|(_, to, ek)| *to == id && matches!(ek, EdgeKind::Ast));
            if is_child {
                let version = counter.next(name);
                definitions.insert(id, SsaVar::new(name, version));
            }
        }
    }

    // Create versions for assignments
    for (node_id, var_name) in &assignments {
        let version = counter.next(var_name);
        definitions.insert(*node_id, SsaVar::new(var_name, version));
    }

    // Create phi nodes
    for (var_name, locations) in &phi_locations {
        for &loc in locations {
            let version = counter.next(var_name);
            let result = SsaVar::new(var_name, version);
            // Sources are all prior versions from predecessors
            let sources: Vec<SsaVar> = (0..version).map(|v| SsaVar::new(var_name, v)).collect();
            phi_nodes.push(PhiNode {
                result,
                sources,
                block_id: loc,
            });
        }
    }

    // Build use chains — for each Identifier node, link to its defining version
    let mut use_def_chains = HashMap::new();
    let mut def_use_chains: HashMap<SsaVar, Vec<NodeId>> = HashMap::new();

    for (id, kind) in cpg.nodes() {
        if let NodeKind::Identifier { name, .. } = kind {
            if let Some(version) = counter.current(name) {
                let ssa_var = SsaVar::new(name, version);
                uses.entry(id)
                    .or_insert_with(Vec::new)
                    .push(ssa_var.clone());
                use_def_chains.insert((id, name.clone()), ssa_var.clone());
                def_use_chains.entry(ssa_var).or_default().push(id);
            }
        }
    }

    SsaFunction {
        name: method_name,
        definitions,
        uses,
        phi_nodes,
        def_use_chains,
        use_def_chains,
    }
}

/// Query: trace a use back to its definition through SSA chains.
pub fn trace_def_use(ssa: &SsaFunction, node: NodeId, var_name: &str) -> Option<SsaVar> {
    ssa.use_def_chains
        .get(&(node, var_name.to_string()))
        .cloned()
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn build_cfg_successors(cpg: &Cpg) -> HashMap<NodeId, Vec<NodeId>> {
    let mut succs: HashMap<NodeId, Vec<NodeId>> = HashMap::new();
    for (from, to, kind) in cpg.edges() {
        if matches!(kind, EdgeKind::Cfg) {
            succs.entry(*from).or_default().push(*to);
        }
    }
    succs
}

fn build_cfg_predecessors(cpg: &Cpg) -> HashMap<NodeId, Vec<NodeId>> {
    let mut preds: HashMap<NodeId, Vec<NodeId>> = HashMap::new();
    for (from, to, kind) in cpg.edges() {
        if matches!(kind, EdgeKind::Cfg) {
            preds.entry(*to).or_default().push(*from);
        }
    }
    preds
}

fn reachable_nodes(successors: &HashMap<NodeId, Vec<NodeId>>, entry: NodeId) -> Vec<NodeId> {
    let mut visited = HashSet::new();
    let mut stack = vec![entry];
    let mut result = Vec::new();
    while let Some(node) = stack.pop() {
        if visited.insert(node) {
            result.push(node);
            if let Some(succs) = successors.get(&node) {
                for &s in succs {
                    stack.push(s);
                }
            }
        }
    }
    result
}

fn find_assignments(cpg: &Cpg, method_node: NodeId) -> Vec<(NodeId, String)> {
    let method_nodes = collect_ast_descendants(cpg, method_node);
    let mut assignments = Vec::new();
    for (id, kind) in cpg.nodes() {
        if method_nodes.contains(&id) {
            if let NodeKind::Assignment { lhs, .. } = kind {
                assignments.push((id, lhs.clone()));
            }
        }
    }
    assignments
}

/// Recursively collect all AST descendants of a node (including the node itself).
fn collect_ast_descendants(cpg: &Cpg, root: NodeId) -> HashSet<NodeId> {
    let mut result = HashSet::new();
    let mut stack = vec![root];
    while let Some(node) = stack.pop() {
        if result.insert(node) {
            for &(_, to, ref kind) in cpg.edges_from(node) {
                if matches!(kind, EdgeKind::Ast) {
                    stack.push(to);
                }
            }
        }
    }
    result
}

fn compute_phi_locations(
    assignments: &[(NodeId, String)],
    frontiers: &HashMap<NodeId, HashSet<NodeId>>,
) -> HashMap<String, HashSet<NodeId>> {
    let mut phi_locs: HashMap<String, HashSet<NodeId>> = HashMap::new();

    for (node, var_name) in assignments {
        if let Some(frontier) = frontiers.get(node) {
            for &frontier_node in frontier {
                phi_locs
                    .entry(var_name.clone())
                    .or_default()
                    .insert(frontier_node);
            }
        }
    }

    phi_locs
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Cpg, EdgeKind, NodeKind};

    #[test]
    fn ssa_var_display_format() {
        let v0 = SsaVar::new("x", 0);
        assert_eq!(v0.display(), "x_0");
        let v1 = SsaVar::new("x", 1);
        assert_eq!(v1.display(), "x_1");
        let named = SsaVar::new("result", 3);
        assert_eq!(named.display(), "result_3");
    }

    #[test]
    fn version_counter_increments() {
        let mut vc = VersionCounter::new();
        assert_eq!(vc.next("x"), 0);
        assert_eq!(vc.next("x"), 1);
        assert_eq!(vc.next("x"), 2);
        assert_eq!(vc.current("x"), Some(2));
    }

    #[test]
    fn version_counter_separate_variables() {
        let mut vc = VersionCounter::new();
        assert_eq!(vc.next("x"), 0);
        assert_eq!(vc.next("y"), 0);
        assert_eq!(vc.next("x"), 1);
        assert_eq!(vc.current("x"), Some(1));
        assert_eq!(vc.current("y"), Some(0));
        assert_eq!(vc.current("z"), None);
    }

    #[test]
    fn compute_dominators_simple_chain() {
        // A -> B -> C
        let mut cpg = Cpg::new();
        let a = cpg.add_node(NodeKind::Method {
            name: "f".into(),
            file: "t.py".into(),
            line: 1,
        });
        let b = cpg.add_node(NodeKind::Call {
            name: "g".into(),
            line: 2,
        });
        let c = cpg.add_node(NodeKind::Call {
            name: "h".into(),
            line: 3,
        });
        cpg.add_edge(a, b, EdgeKind::Cfg);
        cpg.add_edge(b, c, EdgeKind::Cfg);

        let idom = compute_dominators(&cpg, a);
        assert_eq!(idom[&a], a); // entry dominates itself
        assert_eq!(idom[&b], a); // A dominates B
        assert_eq!(idom[&c], b); // B dominates C
    }

    #[test]
    fn compute_dominators_diamond() {
        //     A
        //    / \
        //   B   C
        //    \ /
        //     D
        let mut cpg = Cpg::new();
        let a = cpg.add_node(NodeKind::Method {
            name: "f".into(),
            file: "t.py".into(),
            line: 1,
        });
        let b = cpg.add_node(NodeKind::Call {
            name: "b".into(),
            line: 2,
        });
        let c = cpg.add_node(NodeKind::Call {
            name: "c".into(),
            line: 3,
        });
        let d = cpg.add_node(NodeKind::Call {
            name: "d".into(),
            line: 4,
        });
        cpg.add_edge(a, b, EdgeKind::Cfg);
        cpg.add_edge(a, c, EdgeKind::Cfg);
        cpg.add_edge(b, d, EdgeKind::Cfg);
        cpg.add_edge(c, d, EdgeKind::Cfg);

        let idom = compute_dominators(&cpg, a);
        assert_eq!(idom[&a], a);
        assert_eq!(idom[&b], a);
        assert_eq!(idom[&c], a);
        assert_eq!(idom[&d], a); // A dominates the join point D
    }

    #[test]
    fn dominance_frontier_at_join_point() {
        //     A
        //    / \
        //   B   C
        //    \ /
        //     D
        let mut cpg = Cpg::new();
        let a = cpg.add_node(NodeKind::Method {
            name: "f".into(),
            file: "t.py".into(),
            line: 1,
        });
        let b = cpg.add_node(NodeKind::Assignment {
            lhs: "x".into(),
            line: 2,
        });
        let c = cpg.add_node(NodeKind::Assignment {
            lhs: "x".into(),
            line: 3,
        });
        let d = cpg.add_node(NodeKind::Identifier {
            name: "x".into(),
            line: 4,
        });
        cpg.add_edge(a, b, EdgeKind::Cfg);
        cpg.add_edge(a, c, EdgeKind::Cfg);
        cpg.add_edge(b, d, EdgeKind::Cfg);
        cpg.add_edge(c, d, EdgeKind::Cfg);

        let idom = compute_dominators(&cpg, a);
        let frontiers = compute_dominance_frontiers(&cpg, a, &idom);

        // B and C should have D in their dominance frontiers
        assert!(
            frontiers.get(&b).map_or(false, |f| f.contains(&d)),
            "B should have D in its dominance frontier"
        );
        assert!(
            frontiers.get(&c).map_or(false, |f| f.contains(&d)),
            "C should have D in its dominance frontier"
        );
    }

    #[test]
    fn convert_linear_function() {
        // method -> param(x) -> assign(x=1) -> return
        let mut cpg = Cpg::new();
        let m = cpg.add_node(NodeKind::Method {
            name: "linear".into(),
            file: "t.py".into(),
            line: 1,
        });
        let p = cpg.add_node(NodeKind::Parameter {
            name: "x".into(),
            index: 0,
        });
        let a = cpg.add_node(NodeKind::Assignment {
            lhs: "x".into(),
            line: 2,
        });
        let r = cpg.add_node(NodeKind::Return { line: 3 });

        cpg.add_edge(m, p, EdgeKind::Ast);
        cpg.add_edge(m, a, EdgeKind::Ast);
        cpg.add_edge(m, a, EdgeKind::Cfg);
        cpg.add_edge(a, r, EdgeKind::Cfg);

        let ssa = convert_to_ssa(&cpg, m);
        assert_eq!(ssa.name, "linear");
        // No branches => no phi nodes
        assert!(ssa.phi_nodes.is_empty());
        // Parameter gets version 0, assignment gets version 1
        assert_eq!(ssa.definitions[&p], SsaVar::new("x", 0));
        assert_eq!(ssa.definitions[&a], SsaVar::new("x", 1));
    }

    #[test]
    fn convert_function_with_branch() {
        //   method
        //    |
        //   if_ctrl
        //   / \
        //  a1  a2    (x = ... in each branch)
        //   \ /
        //   join     (use of x — should see phi node placed here)
        let mut cpg = Cpg::new();
        let m = cpg.add_node(NodeKind::Method {
            name: "branching".into(),
            file: "t.py".into(),
            line: 1,
        });
        let ctrl = cpg.add_node(NodeKind::ControlStructure {
            kind: crate::CtrlKind::If,
            line: 2,
        });
        let a1 = cpg.add_node(NodeKind::Assignment {
            lhs: "x".into(),
            line: 3,
        });
        let a2 = cpg.add_node(NodeKind::Assignment {
            lhs: "x".into(),
            line: 4,
        });
        let join = cpg.add_node(NodeKind::Identifier {
            name: "x".into(),
            line: 5,
        });

        cpg.add_edge(m, ctrl, EdgeKind::Ast);
        cpg.add_edge(m, a1, EdgeKind::Ast);
        cpg.add_edge(m, a2, EdgeKind::Ast);
        cpg.add_edge(m, ctrl, EdgeKind::Cfg);
        cpg.add_edge(ctrl, a1, EdgeKind::Cfg);
        cpg.add_edge(ctrl, a2, EdgeKind::Cfg);
        cpg.add_edge(a1, join, EdgeKind::Cfg);
        cpg.add_edge(a2, join, EdgeKind::Cfg);

        let ssa = convert_to_ssa(&cpg, m);
        assert_eq!(ssa.name, "branching");

        // Both assignments should define x with different versions
        assert!(ssa.definitions.contains_key(&a1));
        assert!(ssa.definitions.contains_key(&a2));
        assert_ne!(ssa.definitions[&a1].version, ssa.definitions[&a2].version);

        // Phi node should exist at the join point
        assert!(
            !ssa.phi_nodes.is_empty(),
            "should have phi node at join point"
        );
        let phi = &ssa.phi_nodes[0];
        assert_eq!(phi.block_id, join);
        assert_eq!(phi.result.name, "x");
        assert!(phi.sources.len() >= 2, "phi should have >= 2 sources");
    }

    #[test]
    fn parameter_gets_version_zero() {
        let mut cpg = Cpg::new();
        let m = cpg.add_node(NodeKind::Method {
            name: "f".into(),
            file: "t.py".into(),
            line: 1,
        });
        let p = cpg.add_node(NodeKind::Parameter {
            name: "y".into(),
            index: 0,
        });
        cpg.add_edge(m, p, EdgeKind::Ast);

        let ssa = convert_to_ssa(&cpg, m);
        assert_eq!(ssa.definitions[&p], SsaVar::new("y", 0));
    }

    #[test]
    fn assignment_gets_next_version() {
        let mut cpg = Cpg::new();
        let m = cpg.add_node(NodeKind::Method {
            name: "f".into(),
            file: "t.py".into(),
            line: 1,
        });
        let p = cpg.add_node(NodeKind::Parameter {
            name: "x".into(),
            index: 0,
        });
        let a = cpg.add_node(NodeKind::Assignment {
            lhs: "x".into(),
            line: 2,
        });
        cpg.add_edge(m, p, EdgeKind::Ast);
        cpg.add_edge(m, a, EdgeKind::Ast);
        cpg.add_edge(m, a, EdgeKind::Cfg);

        let ssa = convert_to_ssa(&cpg, m);
        // Parameter x gets version 0
        assert_eq!(ssa.definitions[&p].version, 0);
        // Assignment x gets version 1
        assert_eq!(ssa.definitions[&a].version, 1);
    }

    #[test]
    fn trace_def_use_finds_definition() {
        let mut cpg = Cpg::new();
        let m = cpg.add_node(NodeKind::Method {
            name: "f".into(),
            file: "t.py".into(),
            line: 1,
        });
        let p = cpg.add_node(NodeKind::Parameter {
            name: "x".into(),
            index: 0,
        });
        let ident = cpg.add_node(NodeKind::Identifier {
            name: "x".into(),
            line: 2,
        });
        cpg.add_edge(m, p, EdgeKind::Ast);
        cpg.add_edge(m, ident, EdgeKind::Cfg);

        let ssa = convert_to_ssa(&cpg, m);
        let traced = trace_def_use(&ssa, ident, "x");
        assert!(traced.is_some());
        assert_eq!(traced.unwrap().name, "x");
    }

    #[test]
    fn phi_node_has_correct_sources() {
        // Two assignments to same variable converging at a join
        let mut cpg = Cpg::new();
        let m = cpg.add_node(NodeKind::Method {
            name: "f".into(),
            file: "t.py".into(),
            line: 1,
        });
        let a1 = cpg.add_node(NodeKind::Assignment {
            lhs: "z".into(),
            line: 2,
        });
        let a2 = cpg.add_node(NodeKind::Assignment {
            lhs: "z".into(),
            line: 3,
        });
        let join = cpg.add_node(NodeKind::Identifier {
            name: "z".into(),
            line: 4,
        });

        cpg.add_edge(m, a1, EdgeKind::Ast);
        cpg.add_edge(m, a2, EdgeKind::Ast);
        cpg.add_edge(m, a1, EdgeKind::Cfg);
        cpg.add_edge(m, a2, EdgeKind::Cfg);
        cpg.add_edge(a1, join, EdgeKind::Cfg);
        cpg.add_edge(a2, join, EdgeKind::Cfg);

        let ssa = convert_to_ssa(&cpg, m);
        assert!(!ssa.phi_nodes.is_empty(), "should have phi nodes");
        let phi = &ssa.phi_nodes[0];
        assert_eq!(phi.result.name, "z");
        // The phi sources should include the versions from a1 and a2
        assert!(
            phi.sources.len() >= 2,
            "phi should have at least 2 source versions"
        );
    }

    #[test]
    fn cfg_successors_from_edges() {
        let mut cpg = Cpg::new();
        let a = cpg.add_node(NodeKind::Call {
            name: "a".into(),
            line: 1,
        });
        let b = cpg.add_node(NodeKind::Call {
            name: "b".into(),
            line: 2,
        });
        let c = cpg.add_node(NodeKind::Call {
            name: "c".into(),
            line: 3,
        });
        cpg.add_edge(a, b, EdgeKind::Cfg);
        cpg.add_edge(a, c, EdgeKind::Cfg);
        // Non-CFG edge should be ignored
        cpg.add_edge(b, c, EdgeKind::Ast);

        let succs = build_cfg_successors(&cpg);
        assert_eq!(succs[&a].len(), 2);
        assert!(
            !succs.contains_key(&b),
            "AST edge should not appear in CFG successors"
        );
    }

    #[test]
    fn reachable_nodes_traversal() {
        let mut cpg = Cpg::new();
        let a = cpg.add_node(NodeKind::Call {
            name: "a".into(),
            line: 1,
        });
        let b = cpg.add_node(NodeKind::Call {
            name: "b".into(),
            line: 2,
        });
        let c = cpg.add_node(NodeKind::Call {
            name: "c".into(),
            line: 3,
        });
        let d = cpg.add_node(NodeKind::Call {
            name: "d".into(),
            line: 4,
        }); // unreachable
        cpg.add_edge(a, b, EdgeKind::Cfg);
        cpg.add_edge(b, c, EdgeKind::Cfg);

        let succs = build_cfg_successors(&cpg);
        let reached = reachable_nodes(&succs, a);
        assert!(reached.contains(&a));
        assert!(reached.contains(&b));
        assert!(reached.contains(&c));
        assert!(!reached.contains(&d), "d should be unreachable");
    }

    #[test]
    fn empty_cpg_produces_empty_ssa() {
        let mut cpg = Cpg::new();
        let m = cpg.add_node(NodeKind::Method {
            name: "empty".into(),
            file: "t.py".into(),
            line: 1,
        });

        let ssa = convert_to_ssa(&cpg, m);
        assert_eq!(ssa.name, "empty");
        assert!(ssa.definitions.is_empty());
        assert!(ssa.uses.is_empty());
        assert!(ssa.phi_nodes.is_empty());
        assert!(ssa.def_use_chains.is_empty());
        assert!(ssa.use_def_chains.is_empty());
    }
}
