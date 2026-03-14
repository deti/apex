//! DeepDFA — extract dataflow feature vectors from CPG nodes.
//! Based on the DeepDFA paper — computes per-node feature vectors
//! encoding reaching-definition and taint-reachability information.

use crate::{Cpg, EdgeKind, NodeId, NodeKind};
use std::collections::HashMap;

/// Number of features per node.
pub const FEATURE_DIM: usize = 8;

// Feature indices
pub const IDX_IS_SOURCE: usize = 0; // 1.0 if Parameter node
pub const IDX_IS_SINK: usize = 1; // 1.0 if Call node
pub const IDX_IS_ASSIGNMENT: usize = 2; // 1.0 if Assignment node
pub const IDX_REACHING_DEF_IN: usize = 3; // count of incoming ReachingDef edges
pub const IDX_REACHING_DEF_OUT: usize = 4; // count of outgoing ReachingDef edges
pub const IDX_CFG_IN: usize = 5; // count of incoming Cfg edges
pub const IDX_CFG_OUT: usize = 6; // count of outgoing Cfg edges
pub const IDX_AST_CHILDREN: usize = 7; // count of outgoing Ast edges

/// Extract a feature vector for every node in the CPG.
///
/// Features encode node type flags and edge counts, suitable for
/// downstream ML models or heuristic scoring.
pub fn extract_dataflow_features(cpg: &Cpg) -> HashMap<NodeId, Vec<f64>> {
    let mut features: HashMap<NodeId, Vec<f64>> = HashMap::new();

    for (id, kind) in cpg.nodes() {
        let mut fv = vec![0.0; FEATURE_DIM];

        // Node type flags
        match kind {
            NodeKind::Parameter { .. } => fv[IDX_IS_SOURCE] = 1.0,
            NodeKind::Call { .. } => fv[IDX_IS_SINK] = 1.0,
            NodeKind::Assignment { .. } => fv[IDX_IS_ASSIGNMENT] = 1.0,
            _ => {}
        }

        // Edge counts
        for edge in cpg.edges_from(id) {
            match &edge.2 {
                EdgeKind::ReachingDef { .. } => fv[IDX_REACHING_DEF_OUT] += 1.0,
                EdgeKind::Cfg => fv[IDX_CFG_OUT] += 1.0,
                EdgeKind::Ast => fv[IDX_AST_CHILDREN] += 1.0,
                _ => {}
            }
        }

        for edge in cpg.edges_to(id) {
            match &edge.2 {
                EdgeKind::ReachingDef { .. } => fv[IDX_REACHING_DEF_IN] += 1.0,
                EdgeKind::Cfg => fv[IDX_CFG_IN] += 1.0,
                _ => {}
            }
        }

        features.insert(id, fv);
    }

    features
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_simple_cpg() -> Cpg {
        let mut cpg = Cpg::new();
        let m = cpg.add_node(NodeKind::Method {
            name: "foo".into(),
            file: "test.py".into(),
            line: 1,
        });
        let p = cpg.add_node(NodeKind::Parameter {
            name: "x".into(),
            index: 0,
        });
        let a = cpg.add_node(NodeKind::Assignment {
            lhs: "y".into(),
            line: 2,
        });
        let c = cpg.add_node(NodeKind::Call {
            name: "sink".into(),
            line: 3,
        });

        cpg.add_edge(m, p, EdgeKind::Ast);
        cpg.add_edge(
            p,
            a,
            EdgeKind::ReachingDef {
                variable: "x".into(),
            },
        );
        cpg.add_edge(
            a,
            c,
            EdgeKind::ReachingDef {
                variable: "y".into(),
            },
        );
        cpg.add_edge(m, a, EdgeKind::Cfg);
        cpg.add_edge(a, c, EdgeKind::Cfg);

        cpg
    }

    #[test]
    fn extract_features_returns_all_nodes() {
        let cpg = make_simple_cpg();
        let features = extract_dataflow_features(&cpg);
        assert_eq!(features.len(), cpg.node_count());
    }

    #[test]
    fn feature_vector_has_expected_dims() {
        let cpg = make_simple_cpg();
        let features = extract_dataflow_features(&cpg);
        for (_, fv) in &features {
            assert_eq!(fv.len(), FEATURE_DIM);
        }
    }

    #[test]
    fn parameter_node_has_source_flag() {
        let cpg = make_simple_cpg();
        let features = extract_dataflow_features(&cpg);
        // Node 1 is the parameter
        let param_features = &features[&1];
        assert!(param_features[IDX_IS_SOURCE] > 0.0);
    }

    #[test]
    fn call_node_has_sink_flag() {
        let cpg = make_simple_cpg();
        let features = extract_dataflow_features(&cpg);
        // Node 3 is the call to "sink"
        let call_features = &features[&3];
        assert!(call_features[IDX_IS_SINK] > 0.0);
    }

    #[test]
    fn empty_cpg_returns_empty_features() {
        let cpg = Cpg::new();
        let features = extract_dataflow_features(&cpg);
        assert!(features.is_empty());
    }

    #[test]
    fn reaching_def_count_populated() {
        let cpg = make_simple_cpg();
        let features = extract_dataflow_features(&cpg);
        // Assignment node (id=2) has 1 incoming reaching def and 1 outgoing
        let assign_features = &features[&2];
        assert!(assign_features[IDX_REACHING_DEF_IN] > 0.0);
        assert!(assign_features[IDX_REACHING_DEF_OUT] > 0.0);
    }
}
