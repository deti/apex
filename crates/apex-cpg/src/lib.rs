//! Code Property Graph (CPG) for APEX — taint analysis via AST + CFG + REACHING_DEF edges.
//!
//! Inspired by Joern's CPG schema. Provides a graph IR over which reaching-definition
//! dataflow and backward taint reachability are computed.

pub mod architecture;
pub mod builder;
pub use builder::{CpgBuilder, GoCpgBuilder, JsCpgBuilder, PythonCpgBuilder};

#[cfg(feature = "treesitter")]
pub mod ts_python;
#[cfg(feature = "treesitter")]
pub use ts_python::TreeSitterPythonCpgBuilder;
pub mod deepdfa;
pub mod model_loader;
pub mod query;
pub mod reaching_def;
pub mod ssa;
pub mod taint;
pub mod taint_flows_store;
pub mod taint_rules;
pub mod taint_store;
pub mod taint_summary;
pub mod taint_triage;
pub mod type_taint;

pub use taint_flows_store::find_taint_flows_with_store;
pub use taint_rules::TaintRuleSet;
pub use taint_store::TaintSpecStore;
pub use taint_triage::{TaintTriageScorer, TriagedFlow};
pub use type_taint::{TypeTaintAnalyzer, TypeTaintRule};

pub type NodeId = u32;

/// Control structure variants.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum CtrlKind {
    If,
    While,
    For,
    Try,
}

/// The semantic kind of a CPG node.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum NodeKind {
    Method {
        name: String,
        file: String,
        line: u32,
    },
    Parameter {
        name: String,
        index: u32,
    },
    Call {
        name: String,
        line: u32,
    },
    Identifier {
        name: String,
        line: u32,
    },
    Literal {
        value: String,
        line: u32,
    },
    Return {
        line: u32,
    },
    ControlStructure {
        kind: CtrlKind,
        line: u32,
    },
    /// An assignment statement: `lhs = rhs`
    Assignment {
        lhs: String,
        line: u32,
    },
}

/// The kind of a CPG edge.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum EdgeKind {
    /// AST parent → child structural edge.
    Ast,
    /// Control-flow successor edge.
    Cfg,
    /// Data dependency: a definition of `variable` reaches a use.
    ReachingDef { variable: String },
    /// Call node → argument node.
    Argument { index: u32 },
}

use std::collections::HashMap;

/// The Code Property Graph.
///
/// Stores nodes in a `HashMap` for O(1) lookup and maintains adjacency lists
/// for efficient edge traversal. Query helpers return references into the
/// internal storage.
#[derive(Clone)]
pub struct Cpg {
    nodes: HashMap<NodeId, NodeKind>,
    edges: Vec<(NodeId, NodeId, EdgeKind)>,
    /// Forward adjacency: node → indices into `edges` where that node is the source.
    adj_from: HashMap<NodeId, Vec<usize>>,
    /// Reverse adjacency: node → indices into `edges` where that node is the target.
    adj_to: HashMap<NodeId, Vec<usize>>,
    next_id: NodeId,
}

impl Default for Cpg {
    fn default() -> Self {
        Self::new()
    }
}

impl Cpg {
    /// Create an empty CPG.
    pub fn new() -> Self {
        Cpg {
            nodes: HashMap::new(),
            edges: Vec::new(),
            adj_from: HashMap::new(),
            adj_to: HashMap::new(),
            next_id: 0,
        }
    }

    /// Add a node and return its freshly allocated `NodeId`.
    pub fn add_node(&mut self, kind: NodeKind) -> NodeId {
        let id = self.next_id;
        self.next_id += 1;
        self.nodes.insert(id, kind);
        id
    }

    /// Add a directed edge between two nodes.
    pub fn add_edge(&mut self, from: NodeId, to: NodeId, kind: EdgeKind) {
        let idx = self.edges.len();
        self.edges.push((from, to, kind));
        self.adj_from.entry(from).or_default().push(idx);
        self.adj_to.entry(to).or_default().push(idx);
    }

    /// Look up a node by id (O(1) via HashMap).
    pub fn node(&self, id: NodeId) -> Option<&NodeKind> {
        self.nodes.get(&id)
    }

    /// All edges whose source is `id`.
    pub fn edges_from(&self, id: NodeId) -> Vec<&(NodeId, NodeId, EdgeKind)> {
        match self.adj_from.get(&id) {
            Some(indices) => indices.iter().map(|&i| &self.edges[i]).collect(),
            None => Vec::new(),
        }
    }

    /// All edges whose target is `id`.
    pub fn edges_to(&self, id: NodeId) -> Vec<&(NodeId, NodeId, EdgeKind)> {
        match self.adj_to.get(&id) {
            Some(indices) => indices.iter().map(|&i| &self.edges[i]).collect(),
            None => Vec::new(),
        }
    }

    /// Number of nodes in the graph.
    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }

    /// Number of edges in the graph.
    pub fn edge_count(&self) -> usize {
        self.edges.len()
    }

    /// Iterate over all nodes.
    pub fn nodes(&self) -> impl Iterator<Item = (NodeId, &NodeKind)> {
        self.nodes.iter().map(|(&id, k)| (id, k))
    }

    /// Iterate over all edges.
    pub fn edges(&self) -> impl Iterator<Item = &(NodeId, NodeId, EdgeKind)> {
        self.edges.iter()
    }

    /// Merge another CPG into this one, remapping node IDs to avoid collisions.
    pub fn merge(&mut self, other: Cpg) {
        let offset = self.next_id;
        for (id, kind) in other.nodes {
            self.nodes.insert(id + offset, kind);
        }
        for (from, to, kind) in other.edges {
            self.add_edge(from + offset, to + offset, kind);
        }
        self.next_id = offset + other.next_id;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cpg_add_and_query_nodes() {
        let mut cpg = Cpg::new();
        let id = cpg.add_node(NodeKind::Method {
            name: "foo".into(),
            file: "test.py".into(),
            line: 1,
        });
        assert_eq!(id, 0);
        assert_eq!(cpg.node_count(), 1);
        match cpg.node(id).unwrap() {
            NodeKind::Method { name, .. } => assert_eq!(name, "foo"),
            _ => panic!("wrong kind"),
        }
    }

    #[test]
    fn cpg_node_missing_returns_none() {
        let cpg = Cpg::new();
        assert!(cpg.node(99).is_none());
    }

    #[test]
    fn cpg_edges_from_and_to() {
        let mut cpg = Cpg::new();
        let a = cpg.add_node(NodeKind::Literal {
            value: "x".into(),
            line: 1,
        });
        let b = cpg.add_node(NodeKind::Call {
            name: "foo".into(),
            line: 2,
        });
        cpg.add_edge(a, b, EdgeKind::Cfg);

        assert_eq!(cpg.edges_from(a).len(), 1);
        assert_eq!(cpg.edges_to(b).len(), 1);
        assert_eq!(cpg.edges_from(b).len(), 0);
        assert_eq!(cpg.edges_to(a).len(), 0);
        assert_eq!(cpg.edge_count(), 1);
    }

    #[test]
    fn cpg_merge_remaps_ids() {
        let mut cpg1 = Cpg::new();
        cpg1.add_node(NodeKind::Call {
            name: "foo".into(),
            line: 1,
        });
        cpg1.add_node(NodeKind::Call {
            name: "bar".into(),
            line: 2,
        });
        cpg1.add_edge(0, 1, EdgeKind::Cfg);

        let mut cpg2 = Cpg::new();
        cpg2.add_node(NodeKind::Call {
            name: "baz".into(),
            line: 10,
        });
        cpg2.add_node(NodeKind::Call {
            name: "qux".into(),
            line: 11,
        });
        cpg2.add_edge(0, 1, EdgeKind::Cfg);

        cpg1.merge(cpg2);
        assert_eq!(cpg1.node_count(), 4);
        assert_eq!(cpg1.edge_count(), 2);
        // Merged nodes should have remapped IDs (offset by 2)
        assert!(cpg1.node(2).is_some()); // baz
        assert!(cpg1.node(3).is_some()); // qux
                                         // Merged edge should be 2→3, not 0→1
        assert_eq!(cpg1.edges_from(2).len(), 1);
    }

    #[test]
    fn cpg_multiple_edges() {
        let mut cpg = Cpg::new();
        let m = cpg.add_node(NodeKind::Method {
            name: "m".into(),
            file: "f.py".into(),
            line: 1,
        });
        let p = cpg.add_node(NodeKind::Parameter {
            name: "x".into(),
            index: 0,
        });
        let c = cpg.add_node(NodeKind::Call {
            name: "bar".into(),
            line: 2,
        });
        cpg.add_edge(m, p, EdgeKind::Ast);
        cpg.add_edge(m, c, EdgeKind::Ast);
        cpg.add_edge(p, c, EdgeKind::Cfg);

        assert_eq!(cpg.edges_from(m).len(), 2);
        assert_eq!(cpg.edges_to(c).len(), 2);
        assert_eq!(cpg.edge_count(), 3);
    }
}
