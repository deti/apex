//! Per-function taint summaries for compositional inter-procedural analysis.
//!
//! A taint summary describes how taint flows through a function: which parameters
//! can reach the return value, which parameters reach sensitive sinks, etc.
//! Summaries enable O(n) inter-procedural taint analysis instead of re-analyzing
//! callees at each call site.

use crate::{Cpg, EdgeKind, NodeId, NodeKind};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Endpoint in a taint flow (source or sink within a function).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum FlowEndpoint {
    /// Function parameter by index.
    Parameter(u32),
    /// Function return value.
    Return,
    /// Global or module-level variable.
    Global(String),
}

/// A single taint flow within a function.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaintFlow {
    /// Where taint enters.
    pub source: FlowEndpoint,
    /// Where taint exits.
    pub sink: FlowEndpoint,
    /// Whether a sanitizer was found on the path.
    pub sanitized: bool,
}

/// Per-function taint summary.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaintSummary {
    /// Function name this summary describes.
    pub function: String,
    /// File containing this function.
    pub file: String,
    /// All taint flows through this function.
    pub flows: Vec<TaintFlow>,
    /// Content hash for cache invalidation.
    pub content_hash: u64,
}

impl TaintSummary {
    /// Does any unsanitized flow go from `source` to `sink`?
    pub fn has_flow(&self, source: &FlowEndpoint, sink: &FlowEndpoint) -> bool {
        self.flows
            .iter()
            .any(|f| &f.source == source && &f.sink == sink && !f.sanitized)
    }

    /// All unsanitized flows from a given source.
    pub fn flows_from(&self, source: &FlowEndpoint) -> Vec<&TaintFlow> {
        self.flows
            .iter()
            .filter(|f| &f.source == source && !f.sanitized)
            .collect()
    }
}

/// Compute a content hash for cache invalidation (FNV-1a).
pub fn hash_content(content: &str) -> u64 {
    let mut hash: u64 = 0xcbf29ce484222325;
    for byte in content.as_bytes() {
        hash ^= *byte as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

/// Known sanitizer function name fragments.
const SANITIZERS: &[&str] = &[
    "sanitize", "escape", "encode", "clean", "validate", "filter", "strip",
];

/// Build a taint summary for a method node in the CPG.
///
/// Walks the CPG to find data flows from parameters to return values,
/// checking for sanitizer calls along the way.
pub fn summarize_function(cpg: &Cpg, method_node: NodeId) -> Option<TaintSummary> {
    let method_kind = cpg.node(method_node)?;
    let (func_name, file) = match method_kind {
        NodeKind::Method { name, file, .. } => (name.clone(), file.clone()),
        _ => return None,
    };

    // Collect parameters (children of method via AST edges)
    let params: Vec<(NodeId, u32)> = cpg
        .edges_from(method_node)
        .iter()
        .filter_map(|&&(_, to, ref kind)| {
            if matches!(kind, EdgeKind::Ast) {
                if let Some(NodeKind::Parameter { index, .. }) = cpg.node(to) {
                    return Some((to, *index));
                }
            }
            None
        })
        .collect();

    // Collect return nodes (children of method via AST edges)
    let returns: Vec<NodeId> = cpg
        .edges_from(method_node)
        .iter()
        .filter_map(|&&(_, to, ref kind)| {
            if matches!(kind, EdgeKind::Ast) && matches!(cpg.node(to)?, NodeKind::Return { .. }) {
                return Some(to);
            }
            None
        })
        .collect();

    let mut flows = Vec::new();

    for &(param_id, param_idx) in &params {
        // BFS forward from param through ReachingDef and Cfg edges.
        // Each queue entry carries (node, sanitized_on_this_path).
        let mut visited = std::collections::HashSet::new();
        let mut queue = std::collections::VecDeque::new();
        queue.push_back((param_id, false));
        visited.insert(param_id);

        while let Some((current, mut path_sanitized)) = queue.pop_front() {
            // Check if current is a sanitizer call
            if let Some(NodeKind::Call { name, .. }) = cpg.node(current) {
                if SANITIZERS.iter().any(|s| name.to_lowercase().contains(s)) {
                    path_sanitized = true;
                }
            }

            // Check if we reached a return
            if returns.contains(&current) {
                flows.push(TaintFlow {
                    source: FlowEndpoint::Parameter(param_idx),
                    sink: FlowEndpoint::Return,
                    sanitized: path_sanitized,
                });
            }

            // Follow ReachingDef and Cfg edges forward
            for &(_, to, ref kind) in cpg.edges_from(current) {
                if matches!(kind, EdgeKind::ReachingDef { .. } | EdgeKind::Cfg)
                    && visited.insert(to)
                {
                    queue.push_back((to, path_sanitized));
                }
            }
        }
    }

    Some(TaintSummary {
        function: func_name,
        file,
        flows,
        content_hash: 0, // caller can set this
    })
}

/// Apply a taint summary at a call site.
///
/// Given a call node and the summary of the called function, determine
/// which arguments' taint flows to the call's result.
pub fn apply_summary_at_callsite(
    cpg: &Cpg,
    call_node: NodeId,
    summary: &TaintSummary,
) -> Vec<TaintFlow> {
    let mut applied = Vec::new();

    // Get argument nodes for this call
    let args: Vec<(NodeId, u32)> = cpg
        .edges_from(call_node)
        .iter()
        .filter_map(|&&(_, to, ref kind)| {
            if let EdgeKind::Argument { index } = kind {
                Some((to, *index))
            } else {
                None
            }
        })
        .collect();

    // For each flow in the summary, check if the corresponding argument exists
    for flow in &summary.flows {
        if let FlowEndpoint::Parameter(idx) = &flow.source {
            if args.iter().any(|(_, arg_idx)| arg_idx == idx) {
                applied.push(flow.clone());
            }
        }
    }

    applied
}

/// Cache for taint summaries with content-hash invalidation.
#[derive(Debug, Default)]
pub struct SummaryCache {
    cache: HashMap<String, TaintSummary>,
}

impl SummaryCache {
    pub fn new() -> Self {
        Default::default()
    }

    /// Get a cached summary if the content hash matches.
    pub fn get(&self, function: &str, content_hash: u64) -> Option<&TaintSummary> {
        self.cache
            .get(function)
            .filter(|s| s.content_hash == content_hash)
    }

    /// Insert or update a summary.
    pub fn insert(&mut self, summary: TaintSummary) {
        self.cache.insert(summary.function.clone(), summary);
    }

    /// Invalidate a specific entry.
    pub fn invalidate(&mut self, function: &str) {
        self.cache.remove(function);
    }

    /// Number of cached summaries.
    pub fn len(&self) -> usize {
        self.cache.len()
    }

    /// Whether the cache is empty.
    pub fn is_empty(&self) -> bool {
        self.cache.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Cpg, EdgeKind, NodeKind};

    #[test]
    fn flow_endpoint_equality() {
        assert_eq!(FlowEndpoint::Parameter(0), FlowEndpoint::Parameter(0));
        assert_ne!(FlowEndpoint::Parameter(0), FlowEndpoint::Parameter(1));
        assert_ne!(FlowEndpoint::Parameter(0), FlowEndpoint::Return);
        assert_eq!(FlowEndpoint::Return, FlowEndpoint::Return);
    }

    #[test]
    fn taint_flow_sanitized() {
        let flow = TaintFlow {
            source: FlowEndpoint::Parameter(0),
            sink: FlowEndpoint::Return,
            sanitized: true,
        };
        assert!(flow.sanitized);
        assert_eq!(flow.source, FlowEndpoint::Parameter(0));
        assert_eq!(flow.sink, FlowEndpoint::Return);
    }

    #[test]
    fn summary_has_flow_unsanitized() {
        let summary = TaintSummary {
            function: "foo".into(),
            file: "test.py".into(),
            flows: vec![TaintFlow {
                source: FlowEndpoint::Parameter(0),
                sink: FlowEndpoint::Return,
                sanitized: false,
            }],
            content_hash: 0,
        };
        assert!(summary.has_flow(&FlowEndpoint::Parameter(0), &FlowEndpoint::Return));
    }

    #[test]
    fn summary_has_flow_sanitized_returns_false() {
        let summary = TaintSummary {
            function: "foo".into(),
            file: "test.py".into(),
            flows: vec![TaintFlow {
                source: FlowEndpoint::Parameter(0),
                sink: FlowEndpoint::Return,
                sanitized: true,
            }],
            content_hash: 0,
        };
        assert!(!summary.has_flow(&FlowEndpoint::Parameter(0), &FlowEndpoint::Return));
    }

    #[test]
    fn summary_flows_from_parameter() {
        let summary = TaintSummary {
            function: "foo".into(),
            file: "test.py".into(),
            flows: vec![
                TaintFlow {
                    source: FlowEndpoint::Parameter(0),
                    sink: FlowEndpoint::Return,
                    sanitized: false,
                },
                TaintFlow {
                    source: FlowEndpoint::Parameter(1),
                    sink: FlowEndpoint::Return,
                    sanitized: false,
                },
            ],
            content_hash: 0,
        };
        let from_p0 = summary.flows_from(&FlowEndpoint::Parameter(0));
        assert_eq!(from_p0.len(), 1);
        assert_eq!(from_p0[0].source, FlowEndpoint::Parameter(0));
    }

    #[test]
    fn summary_empty_flows() {
        let summary = TaintSummary {
            function: "foo".into(),
            file: "test.py".into(),
            flows: vec![],
            content_hash: 0,
        };
        assert!(!summary.has_flow(&FlowEndpoint::Parameter(0), &FlowEndpoint::Return));
        assert!(summary.flows_from(&FlowEndpoint::Parameter(0)).is_empty());
    }

    #[test]
    fn hash_content_deterministic() {
        let h1 = hash_content("hello world");
        let h2 = hash_content("hello world");
        assert_eq!(h1, h2);
    }

    #[test]
    fn hash_content_different() {
        let h1 = hash_content("hello");
        let h2 = hash_content("world");
        assert_ne!(h1, h2);
    }

    #[test]
    fn summarize_function_basic() {
        let mut cpg = Cpg::new();
        let method = cpg.add_node(NodeKind::Method {
            name: "foo".into(),
            file: "test.py".into(),
            line: 1,
        });
        let param = cpg.add_node(NodeKind::Parameter {
            name: "x".into(),
            index: 0,
        });
        let ret = cpg.add_node(NodeKind::Return { line: 2 });

        cpg.add_edge(method, param, EdgeKind::Ast);
        cpg.add_edge(method, ret, EdgeKind::Ast);
        cpg.add_edge(
            param,
            ret,
            EdgeKind::ReachingDef {
                variable: "x".into(),
            },
        );

        let summary = summarize_function(&cpg, method).unwrap();
        assert_eq!(summary.function, "foo");
        assert_eq!(summary.file, "test.py");
        assert_eq!(summary.flows.len(), 1);
        assert!(!summary.flows[0].sanitized);
        assert_eq!(summary.flows[0].source, FlowEndpoint::Parameter(0));
        assert_eq!(summary.flows[0].sink, FlowEndpoint::Return);
    }

    #[test]
    fn summarize_function_with_sanitizer() {
        let mut cpg = Cpg::new();
        let method = cpg.add_node(NodeKind::Method {
            name: "bar".into(),
            file: "test.py".into(),
            line: 1,
        });
        let param = cpg.add_node(NodeKind::Parameter {
            name: "x".into(),
            index: 0,
        });
        let sanitizer = cpg.add_node(NodeKind::Call {
            name: "html_escape".into(),
            line: 2,
        });
        let ret = cpg.add_node(NodeKind::Return { line: 3 });

        cpg.add_edge(method, param, EdgeKind::Ast);
        cpg.add_edge(method, ret, EdgeKind::Ast);
        cpg.add_edge(
            param,
            sanitizer,
            EdgeKind::ReachingDef {
                variable: "x".into(),
            },
        );
        cpg.add_edge(
            sanitizer,
            ret,
            EdgeKind::ReachingDef {
                variable: "x".into(),
            },
        );

        let summary = summarize_function(&cpg, method).unwrap();
        assert_eq!(summary.flows.len(), 1);
        assert!(summary.flows[0].sanitized);
    }

    #[test]
    fn summarize_function_no_params() {
        let mut cpg = Cpg::new();
        let method = cpg.add_node(NodeKind::Method {
            name: "noop".into(),
            file: "test.py".into(),
            line: 1,
        });
        let ret = cpg.add_node(NodeKind::Return { line: 2 });
        cpg.add_edge(method, ret, EdgeKind::Ast);

        let summary = summarize_function(&cpg, method).unwrap();
        assert!(summary.flows.is_empty());
    }

    #[test]
    fn summarize_function_non_method() {
        let mut cpg = Cpg::new();
        let call = cpg.add_node(NodeKind::Call {
            name: "foo".into(),
            line: 1,
        });
        assert!(summarize_function(&cpg, call).is_none());
    }

    #[test]
    fn apply_summary_at_callsite_basic() {
        let mut cpg = Cpg::new();
        let call = cpg.add_node(NodeKind::Call {
            name: "foo".into(),
            line: 1,
        });
        let arg = cpg.add_node(NodeKind::Identifier {
            name: "x".into(),
            line: 1,
        });
        cpg.add_edge(call, arg, EdgeKind::Argument { index: 0 });

        let summary = TaintSummary {
            function: "foo".into(),
            file: "test.py".into(),
            flows: vec![TaintFlow {
                source: FlowEndpoint::Parameter(0),
                sink: FlowEndpoint::Return,
                sanitized: false,
            }],
            content_hash: 0,
        };

        let applied = apply_summary_at_callsite(&cpg, call, &summary);
        assert_eq!(applied.len(), 1);
        assert_eq!(applied[0].source, FlowEndpoint::Parameter(0));
    }

    #[test]
    fn cache_insert_and_get() {
        let mut cache = SummaryCache::new();
        assert!(cache.is_empty());

        let summary = TaintSummary {
            function: "foo".into(),
            file: "test.py".into(),
            flows: vec![],
            content_hash: 42,
        };
        cache.insert(summary);
        assert_eq!(cache.len(), 1);
        assert!(cache.get("foo", 42).is_some());
        assert_eq!(cache.get("foo", 42).unwrap().function, "foo");
    }

    #[test]
    fn cache_invalidation_by_hash() {
        let mut cache = SummaryCache::new();
        let summary = TaintSummary {
            function: "foo".into(),
            file: "test.py".into(),
            flows: vec![],
            content_hash: 42,
        };
        cache.insert(summary);

        // Wrong hash returns None
        assert!(cache.get("foo", 99).is_none());

        // Explicit invalidation
        cache.invalidate("foo");
        assert!(cache.get("foo", 42).is_none());
        assert!(cache.is_empty());
    }
}
