use std::collections::{HashSet, VecDeque};
use std::path::PathBuf;

use crate::entry_points::EntryPointKind;
use crate::graph::{CallGraph, FnId};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Granularity {
    Function,
    Block,
    Line,
}

#[derive(Debug, Clone)]
pub enum TargetRegion {
    Function(String),
    FileLine(PathBuf, u32),
    Sink(String),
}

#[derive(Debug, Clone)]
pub struct ReversePath {
    pub target: FnId,
    pub entry_point: FnId,
    pub chain: Vec<(FnId, u32)>,
    pub entry_kind: EntryPointKind,
    pub depth: usize,
    pub granularity: Granularity,
}

pub struct ReversePathEngine {
    graph: CallGraph,
    max_depth: usize,
}

impl ReversePathEngine {
    pub fn new(graph: CallGraph) -> Self {
        Self {
            graph,
            max_depth: 20,
        }
    }

    pub fn with_max_depth(graph: CallGraph, max_depth: usize) -> Self {
        Self { graph, max_depth }
    }

    pub fn graph(&self) -> &CallGraph {
        &self.graph
    }

    fn resolve_target(&self, target: &TargetRegion) -> Vec<FnId> {
        match target {
            TargetRegion::FileLine(file, line) => {
                self.graph.fn_at(file, *line).into_iter().collect()
            }
            TargetRegion::Function(name) => self.graph.fns_named(name).to_vec(),
            TargetRegion::Sink(name) => self.graph.fns_named(name).to_vec(),
        }
    }

    pub fn paths_to_entry(
        &self,
        target: &TargetRegion,
        granularity: Granularity,
    ) -> Vec<ReversePath> {
        let starts = self.resolve_target(target);
        let mut results = Vec::new();
        for start in starts {
            self.bfs_backward(start, None, granularity, &mut results);
        }
        results
    }

    pub fn paths_to_entry_kind(
        &self,
        target: &TargetRegion,
        kind: EntryPointKind,
        granularity: Granularity,
    ) -> Vec<ReversePath> {
        let starts = self.resolve_target(target);
        let mut results = Vec::new();
        for start in starts {
            self.bfs_backward(start, Some(kind), granularity, &mut results);
        }
        results
    }

    pub fn shortest_path_to_entry(&self, target: &TargetRegion) -> Option<ReversePath> {
        let mut paths = self.paths_to_entry(target, Granularity::Function);
        paths.sort_by_key(|p| p.depth);
        paths.into_iter().next()
    }

    pub fn reachable_entries(&self, target: &TargetRegion) -> Vec<(FnId, EntryPointKind)> {
        self.paths_to_entry(target, Granularity::Function)
            .into_iter()
            .map(|p| (p.entry_point, p.entry_kind))
            .collect()
    }

    fn bfs_backward(
        &self,
        start: FnId,
        filter_kind: Option<EntryPointKind>,
        granularity: Granularity,
        results: &mut Vec<ReversePath>,
    ) {
        let mut queue: VecDeque<(FnId, Vec<(FnId, u32)>)> = VecDeque::new();
        let mut visited: HashSet<FnId> = HashSet::new();

        // Check if start is itself an entry point
        if let Some(node) = self.graph.node(start) {
            if let Some(kind) = node.entry_kind {
                if filter_kind.is_none() || filter_kind == Some(kind) {
                    results.push(ReversePath {
                        target: start,
                        entry_point: start,
                        chain: vec![(start, node.start_line)],
                        entry_kind: kind,
                        depth: 0,
                        granularity,
                    });
                }
            }
        }

        queue.push_back((start, vec![]));
        visited.insert(start);

        while let Some((current, path)) = queue.pop_front() {
            if path.len() >= self.max_depth {
                continue;
            }

            let edge_indices = match self.graph.callers_of.get(&current) {
                Some(indices) => indices,
                None => continue,
            };

            for &edge_idx in edge_indices {
                let edge = &self.graph.edges[edge_idx];

                if granularity == Granularity::Block && edge.call_site_block.is_none() {
                    continue;
                }

                let caller_id = edge.caller;
                if visited.contains(&caller_id) {
                    continue;
                }
                visited.insert(caller_id);

                let mut new_path = path.clone();
                new_path.push((caller_id, edge.call_site_line));

                if let Some(caller_node) = self.graph.node(caller_id) {
                    if let Some(kind) = caller_node.entry_kind {
                        if filter_kind.is_none() || filter_kind == Some(kind) {
                            results.push(ReversePath {
                                target: start,
                                entry_point: caller_id,
                                chain: new_path.clone(),
                                entry_kind: kind,
                                depth: new_path.len(),
                                granularity,
                            });
                        }
                    }
                }

                queue.push_back((caller_id, new_path));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::*;

    fn build_test_graph() -> CallGraph {
        let mut g = CallGraph::default();
        g.nodes = vec![
            FnNode {
                id: FnId(0),
                name: "main".into(),
                file: "src/main.rs".into(),
                start_line: 1,
                end_line: 10,
                entry_kind: Some(EntryPointKind::Main),
            },
            FnNode {
                id: FnId(1),
                name: "handler".into(),
                file: "src/api.rs".into(),
                start_line: 1,
                end_line: 15,
                entry_kind: Some(EntryPointKind::HttpHandler),
            },
            FnNode {
                id: FnId(2),
                name: "db_query".into(),
                file: "src/db.rs".into(),
                start_line: 1,
                end_line: 10,
                entry_kind: None,
            },
            FnNode {
                id: FnId(3),
                name: "execute".into(),
                file: "src/db.rs".into(),
                start_line: 12,
                end_line: 20,
                entry_kind: None,
            },
            FnNode {
                id: FnId(4),
                name: "test_create".into(),
                file: "tests/test.rs".into(),
                start_line: 1,
                end_line: 8,
                entry_kind: Some(EntryPointKind::Test),
            },
        ];
        g.edges = vec![
            CallEdge {
                caller: FnId(0),
                callee: FnId(1),
                call_site_line: 5,
                call_site_block: None,
            },
            CallEdge {
                caller: FnId(1),
                callee: FnId(2),
                call_site_line: 10,
                call_site_block: None,
            },
            CallEdge {
                caller: FnId(2),
                callee: FnId(3),
                call_site_line: 8,
                call_site_block: None,
            },
            CallEdge {
                caller: FnId(4),
                callee: FnId(2),
                call_site_line: 3,
                call_site_block: None,
            },
        ];
        g.build_indices();
        g
    }

    #[test]
    fn paths_to_entry_finds_all_chains() {
        let g = build_test_graph();
        let engine = ReversePathEngine::new(g);
        let paths = engine.paths_to_entry(
            &TargetRegion::Function("execute".into()),
            Granularity::Function,
        );
        assert!(paths.len() >= 2);
        let entry_kinds: Vec<_> = paths.iter().map(|p| p.entry_kind).collect();
        assert!(entry_kinds.contains(&EntryPointKind::Main));
        assert!(entry_kinds.contains(&EntryPointKind::Test));
    }

    #[test]
    fn shortest_path_to_entry_picks_shallowest() {
        let g = build_test_graph();
        let engine = ReversePathEngine::new(g);
        let path = engine
            .shortest_path_to_entry(&TargetRegion::Function("execute".into()))
            .unwrap();
        assert!(path.depth <= 3);
    }

    #[test]
    fn paths_to_entry_kind_filters() {
        let g = build_test_graph();
        let engine = ReversePathEngine::new(g);
        let paths = engine.paths_to_entry_kind(
            &TargetRegion::Function("execute".into()),
            EntryPointKind::Test,
            Granularity::Function,
        );
        assert!(paths.iter().all(|p| p.entry_kind == EntryPointKind::Test));
        assert!(!paths.is_empty());
    }

    #[test]
    fn file_line_target_resolves() {
        let g = build_test_graph();
        let engine = ReversePathEngine::new(g);
        let paths = engine.paths_to_entry(
            &TargetRegion::FileLine("src/db.rs".into(), 15),
            Granularity::Function,
        );
        assert!(!paths.is_empty());
    }

    #[test]
    fn handles_cycles() {
        let mut g = CallGraph::default();
        g.nodes = vec![
            FnNode {
                id: FnId(0),
                name: "test_a".into(),
                file: "t.rs".into(),
                start_line: 1,
                end_line: 5,
                entry_kind: Some(EntryPointKind::Test),
            },
            FnNode {
                id: FnId(1),
                name: "a".into(),
                file: "a.rs".into(),
                start_line: 1,
                end_line: 5,
                entry_kind: None,
            },
            FnNode {
                id: FnId(2),
                name: "b".into(),
                file: "b.rs".into(),
                start_line: 1,
                end_line: 5,
                entry_kind: None,
            },
        ];
        g.edges = vec![
            CallEdge {
                caller: FnId(0),
                callee: FnId(1),
                call_site_line: 2,
                call_site_block: None,
            },
            CallEdge {
                caller: FnId(1),
                callee: FnId(2),
                call_site_line: 3,
                call_site_block: None,
            },
            CallEdge {
                caller: FnId(2),
                callee: FnId(1),
                call_site_line: 3,
                call_site_block: None,
            },
        ];
        g.build_indices();
        let engine = ReversePathEngine::new(g);
        let paths = engine.paths_to_entry(
            &TargetRegion::Function("b".into()),
            Granularity::Function,
        );
        assert!(!paths.is_empty());
    }

    #[test]
    fn max_depth_limits_search() {
        let g = build_test_graph();
        let engine = ReversePathEngine::with_max_depth(g, 1);
        let paths = engine.paths_to_entry(
            &TargetRegion::Function("execute".into()),
            Granularity::Function,
        );
        // At depth 1, only db_query is reachable — not an entry point
        assert!(paths.is_empty());
    }

    #[test]
    fn reachable_entries_returns_results() {
        let g = build_test_graph();
        let engine = ReversePathEngine::new(g);
        let entries = engine.reachable_entries(&TargetRegion::Function("execute".into()));
        assert!(entries.len() >= 2);
    }

    #[test]
    fn entry_point_is_own_target() {
        let g = build_test_graph();
        let engine = ReversePathEngine::new(g);
        let paths = engine.paths_to_entry(
            &TargetRegion::Function("main".into()),
            Granularity::Function,
        );
        // main is an entry point itself
        assert!(paths.iter().any(|p| p.depth == 0));
    }

    #[test]
    fn unknown_target_returns_empty() {
        let g = build_test_graph();
        let engine = ReversePathEngine::new(g);
        let paths = engine.paths_to_entry(
            &TargetRegion::Function("nonexistent".into()),
            Granularity::Function,
        );
        assert!(paths.is_empty());
    }
}
