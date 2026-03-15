//! Dependency Graph — visualize and analyze package dependencies.

use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize)]
pub struct DepNode {
    pub name: String,
    pub version: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DepGraphReport {
    pub nodes: Vec<DepNode>,
    pub edges: Vec<(String, String)>, // (from, to)
    pub cycles: Vec<Vec<String>>,
    pub fan_in: HashMap<String, usize>,  // how many depend on this
    pub fan_out: HashMap<String, usize>, // how many this depends on
}

/// Parse Cargo.toml to extract dependencies.
pub fn analyze_cargo(target: &Path) -> DepGraphReport {
    let cargo_path = target.join("Cargo.toml");
    let mut nodes = Vec::new();
    let mut edges = Vec::new();
    let mut fan_in: HashMap<String, usize> = HashMap::new();
    let mut fan_out: HashMap<String, usize> = HashMap::new();

    if let Ok(content) = std::fs::read_to_string(&cargo_path) {
        if let Ok(value) = content.parse::<toml::Value>() {
            let pkg_name = value
                .get("package")
                .and_then(|p| p.get("name"))
                .and_then(|n| n.as_str())
                .unwrap_or("unknown")
                .to_string();

            nodes.push(DepNode {
                name: pkg_name.clone(),
                version: value
                    .get("package")
                    .and_then(|p| p.get("version"))
                    .and_then(|v| v.as_str())
                    .map(String::from),
            });

            for section in &["dependencies", "dev-dependencies", "build-dependencies"] {
                if let Some(deps) = value.get(section).and_then(|d| d.as_table()) {
                    for (dep_name, _dep_val) in deps {
                        nodes.push(DepNode {
                            name: dep_name.clone(),
                            version: None,
                        });
                        edges.push((pkg_name.clone(), dep_name.clone()));
                        *fan_out.entry(pkg_name.clone()).or_default() += 1;
                        *fan_in.entry(dep_name.clone()).or_default() += 1;
                    }
                }
            }
        }
    }

    // Workspace members
    let workspace_tomls = find_workspace_tomls(target);
    for wt in &workspace_tomls {
        if let Ok(content) = std::fs::read_to_string(wt) {
            if let Ok(value) = content.parse::<toml::Value>() {
                let pkg_name = value
                    .get("package")
                    .and_then(|p| p.get("name"))
                    .and_then(|n| n.as_str())
                    .unwrap_or("unknown")
                    .to_string();

                nodes.push(DepNode {
                    name: pkg_name.clone(),
                    version: None,
                });

                if let Some(deps) = value.get("dependencies").and_then(|d| d.as_table()) {
                    for (dep_name, _) in deps {
                        edges.push((pkg_name.clone(), dep_name.clone()));
                        *fan_out.entry(pkg_name.clone()).or_default() += 1;
                        *fan_in.entry(dep_name.clone()).or_default() += 1;
                    }
                }
            }
        }
    }

    // Deduplicate nodes
    let mut seen = HashSet::new();
    nodes.retain(|n| seen.insert(n.name.clone()));

    // Ensure every package has an entry in both fan_in and fan_out (zero-degree nodes
    // would otherwise be missing, making callers unable to distinguish "absent" from "0").
    for node in &nodes {
        fan_in.entry(node.name.clone()).or_insert(0);
        fan_out.entry(node.name.clone()).or_insert(0);
    }

    // Simple cycle detection via DFS
    let cycles = detect_cycles(&edges);

    DepGraphReport {
        nodes,
        edges,
        cycles,
        fan_in,
        fan_out,
    }
}

fn find_workspace_tomls(root: &Path) -> Vec<PathBuf> {
    let mut result = Vec::new();
    if let Ok(entries) = std::fs::read_dir(root) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                let cargo = path.join("Cargo.toml");
                if cargo.exists() {
                    result.push(cargo);
                }
                // One level deeper for crates/ pattern
                if let Ok(sub_entries) = std::fs::read_dir(&path) {
                    for sub in sub_entries.flatten() {
                        let sub_cargo = sub.path().join("Cargo.toml");
                        if sub.path().is_dir() && sub_cargo.exists() {
                            result.push(sub_cargo);
                        }
                    }
                }
            }
        }
    }
    result
}

/// Detect all cycles using Tarjan's SCC algorithm. Every SCC with more than
/// one node represents a cycle. This correctly finds cycles reachable from
/// already-visited nodes (fixes Bug 14).
fn detect_cycles(edges: &[(String, String)]) -> Vec<Vec<String>> {
    let mut adj: HashMap<&str, Vec<&str>> = HashMap::new();
    for (from, to) in edges {
        adj.entry(from.as_str()).or_default().push(to.as_str());
        // Ensure destination nodes appear in the adjacency map even if they
        // have no outgoing edges, so Tarjan visits every node.
        adj.entry(to.as_str()).or_default();
    }

    let mut state = TarjanState {
        index_counter: 0,
        stack: Vec::new(),
        on_stack: HashSet::new(),
        index: HashMap::new(),
        lowlink: HashMap::new(),
        sccs: Vec::new(),
    };

    for &node in adj.keys() {
        if !state.index.contains_key(node) {
            tarjan_strongconnect(node, &adj, &mut state);
        }
    }

    // Return SCCs with more than one node (each is a cycle).
    // Also detect self-loops (single node with an edge to itself).
    let self_loops: HashSet<&str> = edges
        .iter()
        .filter(|(f, t)| f == t)
        .map(|(f, _)| f.as_str())
        .collect();

    state
        .sccs
        .into_iter()
        .filter(|scc| scc.len() > 1 || (scc.len() == 1 && self_loops.contains(scc[0].as_str())))
        .collect()
}

struct TarjanState<'a> {
    index_counter: usize,
    stack: Vec<&'a str>,
    on_stack: HashSet<&'a str>,
    index: HashMap<&'a str, usize>,
    lowlink: HashMap<&'a str, usize>,
    sccs: Vec<Vec<String>>,
}

fn tarjan_strongconnect<'a>(
    node: &'a str,
    adj: &HashMap<&'a str, Vec<&'a str>>,
    state: &mut TarjanState<'a>,
) {
    let idx = state.index_counter;
    state.index.insert(node, idx);
    state.lowlink.insert(node, idx);
    state.index_counter += 1;
    state.stack.push(node);
    state.on_stack.insert(node);

    if let Some(neighbors) = adj.get(node) {
        for &next in neighbors {
            if !state.index.contains_key(next) {
                tarjan_strongconnect(next, adj, state);
                let next_low = state.lowlink[next];
                let node_low = state.lowlink.get_mut(node).unwrap();
                if next_low < *node_low {
                    *node_low = next_low;
                }
            } else if state.on_stack.contains(next) {
                let next_idx = state.index[next];
                let node_low = state.lowlink.get_mut(node).unwrap();
                if next_idx < *node_low {
                    *node_low = next_idx;
                }
            }
        }
    }

    // If node is a root of an SCC, pop the SCC from the stack.
    if state.lowlink[node] == state.index[node] {
        let mut scc = Vec::new();
        loop {
            let w = state.stack.pop().unwrap();
            state.on_stack.remove(w);
            scc.push(w.to_string());
            if w == node {
                break;
            }
        }
        state.sccs.push(scc);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_cycles_finds_cycle() {
        let edges = vec![
            ("a".into(), "b".into()),
            ("b".into(), "c".into()),
            ("c".into(), "a".into()),
        ];
        let cycles = detect_cycles(&edges);
        assert!(!cycles.is_empty());
    }

    #[test]
    fn detect_cycles_no_cycle() {
        let edges = vec![("a".into(), "b".into()), ("b".into(), "c".into())];
        let cycles = detect_cycles(&edges);
        assert!(cycles.is_empty());
    }

    #[test]
    fn empty_edges_no_cycles() {
        let cycles = detect_cycles(&[]);
        assert!(cycles.is_empty());
    }

    /// Regression test for Bug 14: cycles reachable only through
    /// already-visited nodes must still be detected.
    #[test]
    fn detect_cycles_through_visited_nodes() {
        // Graph: a -> b -> c (no cycle), a -> d -> e -> d (cycle via d-e)
        // The old DFS would visit a->b->c first, then skip d because it might
        // be reached from a different root traversal. With Tarjan's SCC, the
        // d<->e cycle is always found regardless of traversal order.
        let edges = vec![
            ("a".into(), "b".into()),
            ("b".into(), "c".into()),
            ("a".into(), "d".into()),
            ("d".into(), "e".into()),
            ("e".into(), "d".into()),
        ];
        let cycles = detect_cycles(&edges);
        // Must find the d<->e cycle
        let has_de_cycle = cycles.iter().any(|c| c.contains(&"d".to_string()) && c.contains(&"e".to_string()));
        assert!(has_de_cycle, "Expected d<->e cycle, found: {:?}", cycles);
    }

    /// Test that a cycle reachable only from an already-visited shared node
    /// is still detected. This is the core Bug 14 scenario.
    #[test]
    fn detect_cycles_shared_entry_point() {
        // x -> y -> z -> y (cycle), x -> w (no cycle)
        // The shared entry point x leads to both a cycle and a non-cycle path.
        let edges = vec![
            ("x".into(), "y".into()),
            ("y".into(), "z".into()),
            ("z".into(), "y".into()),
            ("x".into(), "w".into()),
        ];
        let cycles = detect_cycles(&edges);
        let has_yz_cycle = cycles.iter().any(|c| c.contains(&"y".to_string()) && c.contains(&"z".to_string()));
        assert!(has_yz_cycle, "Expected y<->z cycle, found: {:?}", cycles);
    }

    #[test]
    fn detect_self_loop() {
        let edges = vec![("a".into(), "a".into())];
        let cycles = detect_cycles(&edges);
        assert!(!cycles.is_empty(), "Expected self-loop cycle");
    }

    #[test]
    fn dep_graph_report_serializes() {
        let report = DepGraphReport {
            nodes: vec![DepNode {
                name: "test".into(),
                version: Some("1.0".into()),
            }],
            edges: vec![],
            cycles: vec![],
            fan_in: HashMap::new(),
            fan_out: HashMap::new(),
        };
        let json = serde_json::to_string(&report).unwrap();
        assert!(json.contains("test"));
    }
}
