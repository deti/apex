//! Code Property Graph (CPG) for APEX — taint analysis via AST + CFG + REACHING_DEF edges.
//!
//! Inspired by Joern's CPG schema. Provides a graph IR over which reaching-definition
//! dataflow and backward taint reachability are computed.

pub mod builder;
pub mod reaching_def;
pub mod taint;

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

/// The Code Property Graph.
///
/// Stores nodes and edges with integer identifiers. Query helpers return
/// references into the internal storage.
pub struct Cpg {
    nodes: Vec<(NodeId, NodeKind)>,
    edges: Vec<(NodeId, NodeId, EdgeKind)>,
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
            nodes: Vec::new(),
            edges: Vec::new(),
            next_id: 0,
        }
    }

    /// Add a node and return its freshly allocated `NodeId`.
    pub fn add_node(&mut self, kind: NodeKind) -> NodeId {
        let id = self.next_id;
        self.next_id += 1;
        self.nodes.push((id, kind));
        id
    }

    /// Add a directed edge between two nodes.
    pub fn add_edge(&mut self, from: NodeId, to: NodeId, kind: EdgeKind) {
        self.edges.push((from, to, kind));
    }

    /// Look up a node by id.
    pub fn node(&self, id: NodeId) -> Option<&NodeKind> {
        self.nodes
            .iter()
            .find(|(nid, _)| *nid == id)
            .map(|(_, k)| k)
    }

    /// All edges whose source is `id`.
    pub fn edges_from(&self, id: NodeId) -> Vec<&(NodeId, NodeId, EdgeKind)> {
        self.edges
            .iter()
            .filter(|(from, _, _)| *from == id)
            .collect()
    }

    /// All edges whose target is `id`.
    pub fn edges_to(&self, id: NodeId) -> Vec<&(NodeId, NodeId, EdgeKind)> {
        self.edges.iter().filter(|(_, to, _)| *to == id).collect()
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
        self.nodes.iter().map(|(id, k)| (*id, k))
    }

    /// Iterate over all edges.
    pub fn edges(&self) -> impl Iterator<Item = &(NodeId, NodeId, EdgeKind)> {
        self.edges.iter()
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
