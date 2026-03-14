use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

use crate::entry_points::EntryPointKind;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FnId(pub u32);

#[derive(Debug, Clone)]
pub struct FnNode {
    pub id: FnId,
    pub name: String,
    pub file: PathBuf,
    pub start_line: u32,
    pub end_line: u32,
    pub entry_kind: Option<EntryPointKind>,
}

#[derive(Debug, Clone)]
pub struct CallEdge {
    pub caller: FnId,
    pub callee: FnId,
    pub call_site_line: u32,
    pub call_site_block: Option<u32>,
}

#[derive(Debug, Default)]
pub struct CallGraph {
    pub nodes: Vec<FnNode>,
    pub edges: Vec<CallEdge>,
    pub callers_of: HashMap<FnId, Vec<usize>>,
    pub callees_of: HashMap<FnId, Vec<usize>>,
    pub by_name: HashMap<String, Vec<FnId>>,
    pub by_file_line: BTreeMap<(PathBuf, u32), FnId>,
}

impl CallGraph {
    /// Build indices from nodes and edges.
    pub fn build_indices(&mut self) {
        self.callers_of.clear();
        self.callees_of.clear();
        self.by_name.clear();
        self.by_file_line.clear();

        for node in &self.nodes {
            self.by_name
                .entry(node.name.clone())
                .or_default()
                .push(node.id);
            for line in node.start_line..=node.end_line {
                self.by_file_line
                    .insert((node.file.clone(), line), node.id);
            }
        }

        for (idx, edge) in self.edges.iter().enumerate() {
            self.callers_of.entry(edge.callee).or_default().push(idx);
            self.callees_of.entry(edge.caller).or_default().push(idx);
        }
    }

    /// Resolve a file:line to the containing function.
    pub fn fn_at(&self, file: &Path, line: u32) -> Option<FnId> {
        self.by_file_line.get(&(file.to_path_buf(), line)).copied()
    }

    /// Resolve a function name to all matching FnIds.
    pub fn fns_named(&self, name: &str) -> &[FnId] {
        self.by_name.get(name).map(|v| v.as_slice()).unwrap_or(&[])
    }

    /// Get node by ID.
    pub fn node(&self, id: FnId) -> Option<&FnNode> {
        self.nodes.iter().find(|n| n.id == id)
    }

    /// All entry points in the graph.
    pub fn entry_points(&self) -> Vec<&FnNode> {
        self.nodes
            .iter()
            .filter(|n| n.entry_kind.is_some())
            .collect()
    }

    /// Total number of functions.
    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }

    /// Total number of call edges.
    pub fn edge_count(&self) -> usize {
        self.edges.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entry_points::EntryPointKind;

    fn make_node(
        id: u32,
        name: &str,
        file: &str,
        start: u32,
        end: u32,
        entry: Option<EntryPointKind>,
    ) -> FnNode {
        FnNode {
            id: FnId(id),
            name: name.into(),
            file: PathBuf::from(file),
            start_line: start,
            end_line: end,
            entry_kind: entry,
        }
    }

    fn make_edge(caller: u32, callee: u32, line: u32) -> CallEdge {
        CallEdge {
            caller: FnId(caller),
            callee: FnId(callee),
            call_site_line: line,
            call_site_block: None,
        }
    }

    #[test]
    fn build_indices_creates_reverse_index() {
        let mut g = CallGraph::default();
        g.nodes = vec![
            make_node(0, "main", "src/main.rs", 1, 10, Some(EntryPointKind::Main)),
            make_node(1, "foo", "src/lib.rs", 1, 5, None),
            make_node(2, "bar", "src/lib.rs", 7, 12, None),
        ];
        g.edges = vec![make_edge(0, 1, 3), make_edge(1, 2, 4)];
        g.build_indices();
        assert_eq!(g.callers_of[&FnId(2)].len(), 1);
        let edge_idx = g.callers_of[&FnId(2)][0];
        assert_eq!(g.edges[edge_idx].caller, FnId(1));
    }

    #[test]
    fn fn_at_resolves_file_line() {
        let mut g = CallGraph::default();
        g.nodes = vec![make_node(0, "foo", "src/lib.rs", 5, 10, None)];
        g.build_indices();
        assert_eq!(g.fn_at(&PathBuf::from("src/lib.rs"), 7), Some(FnId(0)));
        assert_eq!(g.fn_at(&PathBuf::from("src/lib.rs"), 11), None);
    }

    #[test]
    fn entry_points_filters_correctly() {
        let g = CallGraph {
            nodes: vec![
                make_node(0, "main", "src/main.rs", 1, 10, Some(EntryPointKind::Main)),
                make_node(1, "helper", "src/lib.rs", 1, 5, None),
            ],
            ..Default::default()
        };
        assert_eq!(g.entry_points().len(), 1);
        assert_eq!(g.entry_points()[0].name, "main");
    }

    #[test]
    fn fns_named_returns_all_matches() {
        let mut g = CallGraph::default();
        g.nodes = vec![
            make_node(0, "foo", "a.rs", 1, 5, None),
            make_node(1, "foo", "b.rs", 1, 5, None),
            make_node(2, "bar", "c.rs", 1, 5, None),
        ];
        g.build_indices();
        assert_eq!(g.fns_named("foo").len(), 2);
        assert_eq!(g.fns_named("bar").len(), 1);
        assert_eq!(g.fns_named("baz").len(), 0);
    }

    #[test]
    fn node_count_and_edge_count() {
        let g = CallGraph {
            nodes: vec![
                make_node(0, "a", "a.rs", 1, 5, None),
                make_node(1, "b", "b.rs", 1, 5, None),
            ],
            edges: vec![make_edge(0, 1, 3)],
            ..Default::default()
        };
        assert_eq!(g.node_count(), 2);
        assert_eq!(g.edge_count(), 1);
    }
}
