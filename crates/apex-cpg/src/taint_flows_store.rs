//! Taint flow analysis using runtime-extensible `TaintSpecStore`.
//!
//! Implements `find_taint_flows_with_store()` — an IRIS-style taint analysis
//! that accepts a `TaintSpecStore` so LLM-inferred specs can be injected at runtime.

use crate::taint_store::TaintSpecStore;

/// A flat node representation used by taint flow analysis.
#[derive(Debug, Clone)]
pub struct CpgNode {
    pub id: u32,
    pub name: String,
}

/// A source→sink taint flow with the full path of node IDs traversed.
#[derive(Debug, Clone)]
pub struct TaintFlow {
    pub source_node: u32,
    pub sink_node: u32,
    pub path: Vec<u32>,
}

/// Find source→sink flows using runtime-extensible specs from `store`.
///
/// `nodes` is a slice of `CpgNode` (id + name).
/// `edges` is a list of directed data-flow edges `(from_id, to_id)`.
/// Sanitizer nodes block the flow — any path through a sanitizer is pruned.
pub fn find_taint_flows_with_store(
    nodes: &[CpgNode],
    edges: &[(u32, u32)],
    store: &TaintSpecStore,
) -> Vec<TaintFlow> {
    let sources: Vec<u32> = nodes
        .iter()
        .filter(|n| store.is_source(&n.name))
        .map(|n| n.id)
        .collect();
    let sinks: Vec<u32> = nodes
        .iter()
        .filter(|n| store.is_sink(&n.name))
        .map(|n| n.id)
        .collect();
    let sanitizer_ids: std::collections::HashSet<u32> = nodes
        .iter()
        .filter(|n| store.is_sanitizer(&n.name))
        .map(|n| n.id)
        .collect();

    let mut flows = Vec::new();
    for &src in &sources {
        for &sink in &sinks {
            if let Some(path) =
                reachable_without_sanitizer(src, sink, edges, &sanitizer_ids)
            {
                flows.push(TaintFlow {
                    source_node: src,
                    sink_node: sink,
                    path,
                });
            }
        }
    }
    flows
}

fn reachable_without_sanitizer(
    src: u32,
    sink: u32,
    edges: &[(u32, u32)],
    sanitizers: &std::collections::HashSet<u32>,
) -> Option<Vec<u32>> {
    // BFS from src to sink, avoiding sanitizer nodes.
    use std::collections::VecDeque;
    let mut queue = VecDeque::from([(src, vec![src])]);
    let mut visited = std::collections::HashSet::new();
    while let Some((cur, path)) = queue.pop_front() {
        if cur == sink {
            return Some(path);
        }
        if !visited.insert(cur) {
            continue;
        }
        for &(from, to) in edges {
            if from == cur && !sanitizers.contains(&to) {
                let mut new_path = path.clone();
                new_path.push(to);
                queue.push_back((to, new_path));
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::taint_store::TaintSpecStore;

    fn make_node(id: u32, name: &str) -> CpgNode {
        CpgNode { id, name: name.into() }
    }

    #[test]
    fn empty_store_finds_no_flows() {
        let store = TaintSpecStore::new();
        let nodes = vec![make_node(1, "user_input"), make_node(2, "exec")];
        let flows = find_taint_flows_with_store(&nodes, &[], &store);
        assert!(flows.is_empty());
    }

    #[test]
    fn source_to_sink_flow_detected() {
        let mut store = TaintSpecStore::new();
        store.add_source("user_input".into());
        store.add_sink("exec".into());
        let nodes = vec![make_node(1, "user_input"), make_node(2, "exec")];
        let edges = vec![(1u32, 2u32)]; // data flow edge
        let flows = find_taint_flows_with_store(&nodes, &edges, &store);
        assert!(!flows.is_empty());
    }

    #[test]
    fn sanitizer_breaks_flow() {
        let mut store = TaintSpecStore::new();
        store.add_source("user_input".into());
        store.add_sink("exec".into());
        store.add_sanitizer("sanitize".into());
        let nodes = vec![
            make_node(1, "user_input"),
            make_node(2, "sanitize"),
            make_node(3, "exec"),
        ];
        let edges = vec![(1, 2), (2, 3)];
        let flows = find_taint_flows_with_store(&nodes, &edges, &store);
        assert!(flows.is_empty(), "sanitizer should block the flow");
    }
}
