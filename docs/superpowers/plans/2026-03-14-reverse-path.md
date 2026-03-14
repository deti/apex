<!-- status: DONE -->
# Reverse Path Analysis (`apex-reach`) Implementation Plan

> **For agentic workers:** REQUIRED: Use fleet crew agents (intelligence crew) for Tasks 1-6, security-detect crew for Task 7, platform crew for Task 8. Tasks 1-3 are sequential (build foundation), Tasks 4-6 are parallel (per-language extractors), Tasks 7-8 are parallel (integration). Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** New crate `apex-reach` that traces backwards from any code region to entry points via a call graph with three granularity levels.

**Architecture:** Core data structures (graph.rs) → engine (engine.rs) → per-language extractors (extractors/*.rs) → integration with apex-detect and apex-cli. Each task produces a compilable, testable increment.

**Tech Stack:** Rust, regex, std collections (HashMap, BTreeMap, VecDeque)

---

## File Map

| File | Responsibility |
|------|---------------|
| `crates/apex-reach/Cargo.toml` | Crate manifest |
| `crates/apex-reach/src/lib.rs` | Public API re-exports |
| `crates/apex-reach/src/graph.rs` | FnId, FnNode, CallEdge, CallGraph + indices |
| `crates/apex-reach/src/entry_points.rs` | EntryPointKind enum |
| `crates/apex-reach/src/engine.rs` | ReversePathEngine, TargetRegion, Granularity, ReversePath |
| `crates/apex-reach/src/extractors/mod.rs` | CallGraphExtractor trait |
| `crates/apex-reach/src/extractors/rust.rs` | Rust call graph extraction |
| `crates/apex-reach/src/extractors/python.rs` | Python call graph extraction |
| `crates/apex-reach/src/extractors/javascript.rs` | JS/TS call graph extraction |
| `Cargo.toml` (workspace root) | Add `crates/apex-reach` to members |

---

## Task 1: Crate scaffold + core data structures

**Crew:** intelligence
**Files:**
- Create: `crates/apex-reach/Cargo.toml`
- Create: `crates/apex-reach/src/lib.rs`
- Create: `crates/apex-reach/src/graph.rs`
- Create: `crates/apex-reach/src/entry_points.rs`
- Modify: `Cargo.toml` (workspace root — add member)

- [ ] **Step 1: Create Cargo.toml**

```toml
[package]
name = "apex-reach"
version.workspace = true
edition.workspace = true
authors.workspace = true
license.workspace = true
repository.workspace = true
description = "Reverse path analysis — call graph + backward traversal to entry points"

[dependencies]
apex-core = { path = "../apex-core" }
regex = "1"

[dev-dependencies]
tempfile = "3"
```

- [ ] **Step 2: Add to workspace**

In root `Cargo.toml`, add `"crates/apex-reach"` to `members` list.

- [ ] **Step 3: Create entry_points.rs**

```rust
/// Classification of function entry points.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EntryPointKind {
    Test,
    Main,
    HttpHandler,
    PublicApi,
    CliEntry,
}

impl std::fmt::Display for EntryPointKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Test => write!(f, "test"),
            Self::Main => write!(f, "main"),
            Self::HttpHandler => write!(f, "http"),
            Self::PublicApi => write!(f, "api"),
            Self::CliEntry => write!(f, "cli"),
        }
    }
}
```

- [ ] **Step 4: Create graph.rs with core types + tests**

```rust
use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
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
            self.by_name.entry(node.name.clone()).or_default().push(node.id);
            for line in node.start_line..=node.end_line {
                self.by_file_line.insert((node.file.clone(), line), node.id);
            }
        }

        for (idx, edge) in self.edges.iter().enumerate() {
            self.callers_of.entry(edge.callee).or_default().push(idx);
            self.callees_of.entry(edge.caller).or_default().push(idx);
        }
    }

    /// Resolve a file:line to the containing function.
    pub fn fn_at(&self, file: &PathBuf, line: u32) -> Option<FnId> {
        self.by_file_line.get(&(file.clone(), line)).copied()
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
        self.nodes.iter().filter(|n| n.entry_kind.is_some()).collect()
    }
}
```

Tests:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn make_node(id: u32, name: &str, file: &str, start: u32, end: u32, entry: Option<EntryPointKind>) -> FnNode {
        FnNode { id: FnId(id), name: name.into(), file: PathBuf::from(file), start_line: start, end_line: end, entry_kind: entry }
    }

    fn make_edge(caller: u32, callee: u32, line: u32) -> CallEdge {
        CallEdge { caller: FnId(caller), callee: FnId(callee), call_site_line: line, call_site_block: None }
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
        // bar's callers should be [foo]
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
        let mut g = CallGraph::default();
        g.nodes = vec![
            make_node(0, "main", "src/main.rs", 1, 10, Some(EntryPointKind::Main)),
            make_node(1, "helper", "src/lib.rs", 1, 5, None),
        ];
        assert_eq!(g.entry_points().len(), 1);
        assert_eq!(g.entry_points()[0].name, "main");
    }
}
```

- [ ] **Step 5: Create lib.rs**

```rust
pub mod entry_points;
pub mod graph;

pub use entry_points::EntryPointKind;
pub use graph::{CallEdge, CallGraph, FnId, FnNode};
```

- [ ] **Step 6: Run tests, commit**

Run: `cargo test -p apex-reach`

```bash
git add crates/apex-reach/ Cargo.toml
git commit -m "feat: scaffold apex-reach crate with core data structures"
```

---

## Task 2: Reverse path engine

**Crew:** intelligence
**Files:**
- Create: `crates/apex-reach/src/engine.rs`
- Modify: `crates/apex-reach/src/lib.rs` (add module)

- [ ] **Step 1: Create engine.rs with types**

```rust
use std::collections::{HashSet, VecDeque};
use std::path::PathBuf;
use apex_core::types::BranchId;
use crate::graph::{CallGraph, FnId};
use crate::entry_points::EntryPointKind;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Granularity {
    Function,
    Block,
    Line,
}

#[derive(Debug, Clone)]
pub enum TargetRegion {
    Branch(BranchId),
    Function(String),
    FileLine(PathBuf, u32),
    Sink(String),
}

#[derive(Debug, Clone)]
pub struct ReversePath {
    pub target: FnId,
    pub entry_point: FnId,
    pub chain: Vec<(FnId, u32)>,  // (function, call_site_line)
    pub entry_kind: EntryPointKind,
    pub depth: usize,
    pub granularity: Granularity,
}
```

- [ ] **Step 2: Implement ReversePathEngine**

```rust
pub struct ReversePathEngine {
    graph: CallGraph,
    max_depth: usize,
}

impl ReversePathEngine {
    pub fn new(graph: CallGraph) -> Self {
        Self { graph, max_depth: 20 }
    }

    pub fn with_max_depth(graph: CallGraph, max_depth: usize) -> Self {
        Self { graph, max_depth }
    }

    pub fn graph(&self) -> &CallGraph { &self.graph }

    /// Resolve a TargetRegion to FnId(s).
    fn resolve_target(&self, target: &TargetRegion) -> Vec<FnId> {
        match target {
            TargetRegion::FileLine(file, line) => {
                self.graph.fn_at(file, *line).into_iter().collect()
            }
            TargetRegion::Function(name) => self.graph.fns_named(name).to_vec(),
            TargetRegion::Sink(name) => self.graph.fns_named(name).to_vec(),
            TargetRegion::Branch(branch_id) => {
                // Resolve via file_id → file path → line
                // For now, fall back to empty (requires file_paths map from context)
                Vec::new()
            }
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

    pub fn shortest_path_to_entry(
        &self,
        target: &TargetRegion,
    ) -> Option<ReversePath> {
        let mut paths = self.paths_to_entry(target, Granularity::Function);
        paths.sort_by_key(|p| p.depth);
        paths.into_iter().next()
    }

    pub fn reachable_entries(
        &self,
        target: &TargetRegion,
    ) -> Vec<(FnId, EntryPointKind)> {
        self.paths_to_entry(target, Granularity::Function)
            .into_iter()
            .map(|p| (p.entry_point, p.entry_kind))
            .collect()
    }

    /// Core BFS backward traversal.
    fn bfs_backward(
        &self,
        start: FnId,
        filter_kind: Option<EntryPointKind>,
        granularity: Granularity,
        results: &mut Vec<ReversePath>,
    ) {
        // BFS state: (current_fn, path_so_far)
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

            let caller_edges = self.graph.callers_of.get(&current);
            let edge_indices = match caller_edges {
                Some(indices) => indices,
                None => continue,
            };

            for &edge_idx in edge_indices {
                let edge = &self.graph.edges[edge_idx];

                // Granularity filtering for Block level
                if granularity == Granularity::Block {
                    if edge.call_site_block.is_none() {
                        continue; // skip edges without block info
                    }
                }

                let caller_id = edge.caller;
                if visited.contains(&caller_id) {
                    continue;
                }
                visited.insert(caller_id);

                let mut new_path = path.clone();
                new_path.push((caller_id, edge.call_site_line));

                // Check if caller is an entry point
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
```

- [ ] **Step 3: Write engine tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::*;

    fn build_test_graph() -> CallGraph {
        // main -> handler -> db_query -> execute
        // test_create -> db_query -> execute
        let mut g = CallGraph::default();
        g.nodes = vec![
            FnNode { id: FnId(0), name: "main".into(), file: "src/main.rs".into(), start_line: 1, end_line: 10, entry_kind: Some(EntryPointKind::Main) },
            FnNode { id: FnId(1), name: "handler".into(), file: "src/api.rs".into(), start_line: 1, end_line: 15, entry_kind: Some(EntryPointKind::HttpHandler) },
            FnNode { id: FnId(2), name: "db_query".into(), file: "src/db.rs".into(), start_line: 1, end_line: 10, entry_kind: None },
            FnNode { id: FnId(3), name: "execute".into(), file: "src/db.rs".into(), start_line: 12, end_line: 20, entry_kind: None },
            FnNode { id: FnId(4), name: "test_create".into(), file: "tests/test.rs".into(), start_line: 1, end_line: 8, entry_kind: Some(EntryPointKind::Test) },
        ];
        g.edges = vec![
            CallEdge { caller: FnId(0), callee: FnId(1), call_site_line: 5, call_site_block: None },
            CallEdge { caller: FnId(1), callee: FnId(2), call_site_line: 10, call_site_block: None },
            CallEdge { caller: FnId(2), callee: FnId(3), call_site_line: 8, call_site_block: None },
            CallEdge { caller: FnId(4), callee: FnId(2), call_site_line: 3, call_site_block: None },
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
        // Should find: main->handler->db_query->execute, test_create->db_query->execute, handler (http entry)
        assert!(paths.len() >= 2);
        let entry_kinds: Vec<_> = paths.iter().map(|p| p.entry_kind).collect();
        assert!(entry_kinds.contains(&EntryPointKind::Main));
        assert!(entry_kinds.contains(&EntryPointKind::Test));
    }

    #[test]
    fn shortest_path_to_entry() {
        let g = build_test_graph();
        let engine = ReversePathEngine::new(g);
        let path = engine.shortest_path_to_entry(
            &TargetRegion::Function("execute".into()),
        ).unwrap();
        // test_create->db_query->execute is shorter (depth 2) than main->handler->db_query->execute (depth 3)
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
    }

    #[test]
    fn file_line_target_resolves() {
        let g = build_test_graph();
        let engine = ReversePathEngine::new(g);
        let paths = engine.paths_to_entry(
            &TargetRegion::FileLine("src/db.rs".into(), 15),
            Granularity::Function,
        );
        assert!(!paths.is_empty()); // line 15 is inside execute (12-20)
    }

    #[test]
    fn handles_cycles() {
        let mut g = CallGraph::default();
        g.nodes = vec![
            FnNode { id: FnId(0), name: "test_a".into(), file: "t.rs".into(), start_line: 1, end_line: 5, entry_kind: Some(EntryPointKind::Test) },
            FnNode { id: FnId(1), name: "a".into(), file: "a.rs".into(), start_line: 1, end_line: 5, entry_kind: None },
            FnNode { id: FnId(2), name: "b".into(), file: "b.rs".into(), start_line: 1, end_line: 5, entry_kind: None },
        ];
        // a <-> b (mutual recursion) + test_a -> a
        g.edges = vec![
            CallEdge { caller: FnId(0), callee: FnId(1), call_site_line: 2, call_site_block: None },
            CallEdge { caller: FnId(1), callee: FnId(2), call_site_line: 3, call_site_block: None },
            CallEdge { caller: FnId(2), callee: FnId(1), call_site_line: 3, call_site_block: None },
        ];
        g.build_indices();
        let engine = ReversePathEngine::new(g);
        let paths = engine.paths_to_entry(
            &TargetRegion::Function("b".into()),
            Granularity::Function,
        );
        // Should find test_a->a->b, not infinite loop
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
        // Only depth-1 paths (direct callers of execute = db_query, which is not an entry)
        // So no paths found at depth 1
        assert!(paths.is_empty());
    }

    #[test]
    fn reachable_entries_returns_unique() {
        let g = build_test_graph();
        let engine = ReversePathEngine::new(g);
        let entries = engine.reachable_entries(
            &TargetRegion::Function("execute".into()),
        );
        assert!(entries.len() >= 2);
    }
}
```

- [ ] **Step 4: Update lib.rs**

```rust
pub mod engine;
pub mod entry_points;
pub mod graph;

pub use engine::{Granularity, ReversePath, ReversePathEngine, TargetRegion};
pub use entry_points::EntryPointKind;
pub use graph::{CallEdge, CallGraph, FnId, FnNode};
```

- [ ] **Step 5: Run tests, commit**

Run: `cargo test -p apex-reach`

```bash
git add crates/apex-reach/
git commit -m "feat: reverse path engine with BFS backward traversal"
```

---

## Task 3: Extractor trait + scaffold

**Crew:** intelligence
**Files:**
- Create: `crates/apex-reach/src/extractors/mod.rs`
- Modify: `crates/apex-reach/src/lib.rs`

- [ ] **Step 1: Create extractor trait**

```rust
use apex_core::types::Language;
use std::collections::HashMap;
use std::path::PathBuf;
use crate::graph::CallGraph;

pub mod rust;
pub mod python;
pub mod javascript;

pub trait CallGraphExtractor: Send + Sync {
    fn language(&self) -> Language;
    fn extract(&self, sources: &HashMap<PathBuf, String>) -> CallGraph;
}

/// Build a call graph using the appropriate extractor for the language.
pub fn build_call_graph(
    sources: &HashMap<PathBuf, String>,
    lang: Language,
) -> CallGraph {
    match lang {
        Language::Rust => rust::RustExtractor.extract(sources),
        Language::Python => python::PythonExtractor.extract(sources),
        Language::JavaScript => javascript::JsExtractor.extract(sources),
        _ => CallGraph::default(), // unsupported language
    }
}
```

- [ ] **Step 2: Create stub extractors for each language**

Each file (`rust.rs`, `python.rs`, `javascript.rs`) gets a stub:

```rust
use super::CallGraphExtractor;
use crate::graph::CallGraph;
use apex_core::types::Language;
use std::collections::HashMap;
use std::path::PathBuf;

pub struct RustExtractor;

impl CallGraphExtractor for RustExtractor {
    fn language(&self) -> Language { Language::Rust }
    fn extract(&self, _sources: &HashMap<PathBuf, String>) -> CallGraph {
        CallGraph::default() // stub
    }
}
```

- [ ] **Step 3: Update lib.rs, run tests, commit**

```bash
git commit -m "feat: add CallGraphExtractor trait and stub extractors"
```

---

## Task 4: Rust extractor

**Crew:** intelligence
**Files:**
- Modify: `crates/apex-reach/src/extractors/rust.rs`

Implement full Rust call graph extraction:
- **Function detection:** `(?:pub\s+)?(?:async\s+)?fn\s+(\w+)`, track `impl` blocks for `Type::method` naming
- **Call detection:** `(\w+)\s*\(`, `self\.(\w+)\s*\(`, `(\w+)::(\w+)\s*\(`
- **Entry points:** `#[test]`, `#[tokio::test]` on preceding line → Test; `fn main()` → Main; `#[get(`, `#[post(` → HttpHandler; `pub fn` at brace depth 0 → PublicApi; `#[derive(Parser)]` / `clap::` → CliEntry
- **Block detection:** Split at `if`, `else`, `for`, `while`, `match`, `loop` — assign sequential block IDs

Tests should cover:
- Simple function + call
- `impl` block methods
- `async fn`
- Entry point detection for all 5 types
- Nested functions / closures (should be separate nodes)
- Cross-file call resolution

- [ ] **Steps 1-5: Tests first, implement, verify, commit**

Run: `cargo test -p apex-reach rust`

```bash
git commit -m "feat: Rust call graph extractor with entry point detection"
```

---

## Task 5: Python extractor

**Crew:** intelligence
**Files:**
- Modify: `crates/apex-reach/src/extractors/python.rs`

- **Function detection:** `^(\s*)def\s+(\w+)\s*\(` — indentation level determines if method (indented) or free function (column 0). Class methods: `ClassName.method`
- **Call detection:** `(\w+)\s*\(`, `self\.(\w+)\s*\(`, `(\w+)\.(\w+)\s*\(`
- **Entry points:** `def test_\w+` / `class Test\w+` → Test; `if __name__` → Main; `@app.route` / `@router.get` decorator on preceding line → HttpHandler; file is `__init__.py` + `def` at indent 0 → PublicApi; `argparse` / `click.command` → CliEntry
- **Block detection:** Track indentation level — blocks split at `if`/`elif`/`else`/`for`/`while`/`try`/`except`

Tests: same categories as Rust but with Python idioms (decorators, indentation-based scoping, class methods).

- [ ] **Steps 1-5: Tests, implement, verify, commit**

```bash
git commit -m "feat: Python call graph extractor with decorator entry points"
```

---

## Task 6: JavaScript/TypeScript extractor

**Crew:** intelligence
**Files:**
- Modify: `crates/apex-reach/src/extractors/javascript.rs`

- **Function detection:** `function\s+(\w+)\s*\(`, `(?:const|let|var)\s+(\w+)\s*=\s*(?:async\s+)?\(` (arrow fn), `(\w+)\s*\(` inside `class` body (methods)
- **Call detection:** `(\w+)\s*\(`, `(\w+)\.(\w+)\s*\(`, `require\s*\(\s*['"](\w+)['"]\s*\)\.(\w+)`
- **Entry points:** `describe(` / `it(` / `test(` → Test; `require.main` → Main; `app.get(` / `router.post(` → HttpHandler; `export default function` / `module.exports` → PublicApi; `commander` / `yargs` → CliEntry
- **Block detection:** Brace-based splitting at `if`/`else`/`for`/`while`/`switch`

Tests: arrow functions, class methods, `require()` calls, Express-style routes, Jest/Mocha patterns.

- [ ] **Steps 1-5: Tests, implement, verify, commit**

```bash
git commit -m "feat: JS/TS call graph extractor with Express + test entry points"
```

---

## Task 7: Integration with apex-detect (security enrichment)

**Crew:** security-detect
**Files:**
- Modify: `crates/apex-detect/src/context.rs` (add `reverse_path_engine` field)
- Modify: `crates/apex-detect/src/finding.rs` (add `Evidence::ReachabilityChain` variant)
- Modify: `crates/apex-detect/Cargo.toml` (add `apex-reach` dependency)

- [ ] **Step 1: Add apex-reach dependency**

In `crates/apex-detect/Cargo.toml`:
```toml
apex-reach = { path = "../apex-reach" }
```

- [ ] **Step 2: Add field to AnalysisContext**

```rust
pub reverse_path_engine: Option<Arc<apex_reach::ReversePathEngine>>,
```

Update `test_default()` to set this to `None`.

- [ ] **Step 3: Add Evidence variant**

```rust
Evidence::ReachabilityChain {
    tool: String,
    paths: Vec<String>,
}
```

- [ ] **Step 4: Run tests, commit**

Run: `cargo test -p apex-detect`

```bash
git commit -m "feat: wire apex-reach into AnalysisContext for detector enrichment"
```

---

## Task 8: CLI `reach` subcommand

**Crew:** platform
**Files:**
- Modify: `crates/apex-cli/src/lib.rs` (add `Reach` command variant + handler)
- Modify: `crates/apex-cli/Cargo.toml` (add `apex-reach` dependency)

- [ ] **Step 1: Add apex-reach dependency**

In `crates/apex-cli/Cargo.toml`:
```toml
apex-reach = { path = "../apex-reach" }
```

- [ ] **Step 2: Add Reach command to CLI enum**

```rust
Reach(ReachArgs),
```

```rust
#[derive(clap::Args)]
pub struct ReachArgs {
    #[arg(long)]
    pub target: String,  // "file:line" format
    #[arg(long)]
    pub lang: String,
    #[arg(long, default_value = "function")]
    pub granularity: String,  // "function", "block", "line"
    #[arg(long)]
    pub entry_kind: Option<String>,  // "test", "http", "main", "api", "cli"
    #[arg(long)]
    pub output: Option<String>,
}
```

- [ ] **Step 3: Implement reach handler**

```rust
async fn run_reach(args: ReachArgs, cfg: &ApexConfig) -> Result<()> {
    let target_path = PathBuf::from(&args.target.split(':').next().unwrap_or(""));
    let line: u32 = args.target.split(':').nth(1).and_then(|s| s.parse().ok()).unwrap_or(1);
    let lang = args.lang.parse::<Language>()?;

    // Build source cache
    let source_cache = build_source_cache(&target_path.parent().unwrap_or(&target_path), lang);

    // Build call graph
    let graph = apex_reach::extractors::build_call_graph(&source_cache, lang);

    // Build engine
    let engine = apex_reach::ReversePathEngine::new(graph);

    // Parse granularity
    let granularity = match args.granularity.as_str() {
        "block" => apex_reach::Granularity::Block,
        "line" => apex_reach::Granularity::Line,
        _ => apex_reach::Granularity::Function,
    };

    // Query
    let target = apex_reach::TargetRegion::FileLine(target_path, line);
    let paths = engine.paths_to_entry(&target, granularity);

    // Output
    if paths.is_empty() {
        println!("No paths found to entry points.");
        return Ok(());
    }

    println!("Found {} paths to entry points:\n", paths.len());
    for path in &paths {
        if let Some(entry_node) = engine.graph().node(path.entry_point) {
            println!("  {} ({})", entry_node.name, path.entry_kind);
            for (fn_id, line) in &path.chain {
                if let Some(node) = engine.graph().node(*fn_id) {
                    println!("    → {} ({}:{})", node.name, node.file.display(), line);
                }
            }
            println!();
        }
    }

    Ok(())
}
```

- [ ] **Step 4: Wire into command dispatch**

In `run_cli()`:
```rust
Commands::Reach(args) => run_reach(args, cfg).await,
```

- [ ] **Step 5: Run tests, commit**

Run: `cargo test -p apex-cli`

```bash
git commit -m "feat: add apex reach CLI subcommand"
```

---

## Dispatch Plan

```
Sequential (foundation):
  Task 1: scaffold + data structures
  Task 2: reverse path engine
  Task 3: extractor trait + stubs

Parallel (per-language extractors — independent files):
  ├── Task 4: Rust extractor
  ├── Task 5: Python extractor
  └── Task 6: JS/TS extractor

Parallel (integration — different crates):
  ├── Task 7: apex-detect integration (security crew)
  └── Task 8: apex-cli reach command (platform crew)
```

Tasks 1-3 must complete before 4-8. Tasks 4-6 are fully parallel. Tasks 7-8 are parallel with each other and with 4-6 (they only depend on 1-3).

---

## Summary

| Task | Crew | Files | What |
|------|------|-------|------|
| 1 | Intelligence | graph.rs, entry_points.rs, lib.rs, Cargo.toml | Core data structures + indices |
| 2 | Intelligence | engine.rs | BFS backward traversal engine |
| 3 | Intelligence | extractors/mod.rs + stubs | Trait + build_call_graph dispatcher |
| 4 | Intelligence | extractors/rust.rs | Rust fn/call/entry extraction |
| 5 | Intelligence | extractors/python.rs | Python def/call/entry extraction |
| 6 | Intelligence | extractors/javascript.rs | JS/TS function/call/entry extraction |
| 7 | Security | apex-detect context.rs, finding.rs | Reachability enrichment for detectors |
| 8 | Platform | apex-cli lib.rs | `apex reach` CLI subcommand |
