<!-- status: DONE -->
# Reverse Path Analysis Design — `apex-reach`

## Overview

New crate `apex-reach` providing reverse path analysis: given any code region (uncovered branch, security sink, file:line), trace backwards through the call graph to all entry points that could reach it. Supports three granularity levels (function, block, line) and serves both coverage guidance and security reachability enrichment.

## Motivation

APEX can identify *what* is uncovered or vulnerable, but not *how to get there*. The orchestrator knows branch X is uncovered but can't tell the test generator which call chain reaches it. Detectors find sinks but can't report which HTTP handlers expose them. Reverse path analysis closes this gap with a shared primitive.

## Architecture

```
apex-reach (new crate)
├── extractors/
│   ├── mod.rs          — CallGraphExtractor trait
│   ├── rust.rs         — Rust fn/method/call/entry extraction
│   ├── python.rs       — Python def/call/entry extraction
│   └── javascript.rs   — JS/TS function/call/entry extraction
├── graph.rs            — CallGraph with forward + reverse indices
├── engine.rs           — ReversePathEngine (BFS backward traversal)
├── entry_points.rs     — EntryPointKind enum + classification logic
└── lib.rs              — Public API
```

## Core Data Structures

### Identifiers

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FnId(pub u32);
```

### Function Node

```rust
pub struct FnNode {
    pub id: FnId,
    pub name: String,
    pub file: PathBuf,
    pub start_line: u32,
    pub end_line: u32,
    pub entry_kind: Option<EntryPointKind>,
}
```

### Call Edge

```rust
pub struct CallEdge {
    pub caller: FnId,
    pub callee: FnId,
    pub call_site_line: u32,
    pub call_site_block: Option<u32>,
}
```

### Call Graph

```rust
pub struct CallGraph {
    pub nodes: Vec<FnNode>,
    pub edges: Vec<CallEdge>,
    pub callers_of: HashMap<FnId, Vec<usize>>,  // FnId → edge indices (reverse)
    pub callees_of: HashMap<FnId, Vec<usize>>,  // FnId → edge indices (forward)
    pub by_name: HashMap<String, Vec<FnId>>,     // function name → node IDs
    pub by_file_line: BTreeMap<(PathBuf, u32), FnId>,  // file:line → containing fn
}
```

Both `callers_of` (reverse) and `callees_of` (forward) indices are built at construction time for O(1) lookup.

### Entry Point Classification

```rust
pub enum EntryPointKind {
    Test,
    Main,
    HttpHandler,
    PublicApi,
    CliEntry,
}
```

Detection patterns per language:

| Entry Type | Rust | Python | JS/TS |
|-----------|------|--------|-------|
| Test | `#[test]`, `#[tokio::test]` | `def test_*`, `class Test*` | `describe(`, `it(`, `test(` |
| Main | `fn main()` | `if __name__ == "__main__"` | `require.main === module` |
| HTTP handler | `#[get]`, `#[post]` (actix/axum) | `@app.route`, `@router.get/post` | `app.get(`, `router.post(` |
| Public API | `pub fn` at crate root | functions in `__init__.py` | `export default function`, `module.exports` |
| CLI entry | `clap::Parser` derive | `argparse`, `click.command` | `commander`, `yargs` |

### Query Types

```rust
pub enum Granularity {
    Function,
    Block,
    Line,
}

pub enum TargetRegion {
    Branch(BranchId),
    Function(String),
    FileLine(PathBuf, u32),
    Sink(String),
}

pub struct ReversePath {
    pub target: FnId,
    pub entry_point: FnId,
    pub chain: Vec<(FnId, u32)>,  // (function, call_site_line)
    pub entry_kind: EntryPointKind,
    pub depth: usize,
    pub granularity: Granularity,
}
```

## Call Graph Extractor

```rust
pub trait CallGraphExtractor: Send + Sync {
    fn language(&self) -> Language;
    fn extract(&self, sources: &HashMap<PathBuf, String>) -> CallGraph;
}
```

### Extraction Approach

All extractors use regex + lightweight parsing (not full ASTs), consistent with the detector pattern in `apex-detect`.

**Function detection:** Regex for function/method definitions. Track scope depth (braces for Rust/JS, indentation for Python) to distinguish free functions from methods.

**Call detection:** Regex for function calls (`name(`, `self.method(`, `module::fn(`). Resolve callee by name matching against the `by_name` index. Unresolved calls (to external crates/modules) are recorded but don't contribute to reverse paths.

**Block detection (optional):** Split function bodies at control flow boundaries (`if`/`else`/`for`/`while`/`match`). Each block gets a sequential ID. Call edges within blocks get the block ID annotated.

### Cross-File Resolution

Callee resolution is name-based: `foo()` in file A matches `fn foo()` in file B. For qualified calls (`module::foo()`), strip the module prefix and match against function names. Ambiguous matches (same name in multiple files) produce edges to all candidates — the reverse path engine reports all possible chains.

## Reverse Path Engine

```rust
pub struct ReversePathEngine {
    graph: CallGraph,
    max_depth: usize,
}

impl ReversePathEngine {
    pub fn new(graph: CallGraph) -> Self;
    pub fn with_max_depth(graph: CallGraph, max_depth: usize) -> Self;

    /// All paths from target back to any entry point
    pub fn paths_to_entry(
        &self,
        target: &TargetRegion,
        granularity: Granularity,
    ) -> Vec<ReversePath>;

    /// Paths to a specific entry point kind
    pub fn paths_to_entry_kind(
        &self,
        target: &TargetRegion,
        kind: EntryPointKind,
        granularity: Granularity,
    ) -> Vec<ReversePath>;

    /// Shortest path only
    pub fn shortest_path_to_entry(
        &self,
        target: &TargetRegion,
    ) -> Option<ReversePath>;

    /// All reachable entry points (no path detail)
    pub fn reachable_entries(
        &self,
        target: &TargetRegion,
    ) -> Vec<(FnId, EntryPointKind)>;
}
```

### Algorithm

1. Resolve `TargetRegion` to containing `FnId` via `by_file_line` or `by_name`
2. BFS backward: dequeue function, look up `callers_of[fn_id]`, enqueue callers
3. Track visited set to handle cycles (recursive/mutual recursion)
4. At each step, record `(FnId, call_site_line)` in the path
5. When a node with `entry_kind.is_some()` is reached, emit a `ReversePath`
6. Stop at `max_depth` (default 20)

**Granularity filtering:**
- `Function`: use all edges
- `Line`: use all edges (line metadata always present)
- `Block`: filter to edges whose `call_site_block` matches the block containing the target within each function. Requires block-level data from extraction.

### Performance

Pre-built `callers_of` index makes each BFS step O(1) amortized. Total traversal is O(V+E) for the reachable subgraph. For a workspace with ~500 functions and ~2000 call edges, this is sub-millisecond.

## Integration Points

### 1. AnalysisContext (apex-detect)

```rust
// In context.rs — add field
pub reverse_path_engine: Option<Arc<ReversePathEngine>>,
```

Detectors can enrich findings with reachability chains. A new `Evidence` variant:

```rust
Evidence::ReachabilityChain {
    tool: String,
    paths: Vec<String>,  // formatted path strings
}
```

### 2. ExplorationContext (apex-agent)

```rust
// In orchestrator — when building context for strategies
ctx.suggested_entry_chain = engine.shortest_path_to_entry(
    &TargetRegion::Branch(branch_id)
).map(|r| r.chain);
```

Synthesis strategies use this to generate tests that follow the call chain rather than guessing.

### 3. CLI subcommand (apex-cli)

```
apex reach --target <file:line> --lang <lang> [--granularity function|block|line] [--entry-kind test|http|main|api|cli]
```

Output format:
```
Found 3 paths to entry points:

  test_create_user (test)
    → create_user (src/api.rs:15)
    → db_insert (src/db.rs:38)
    → execute (src/db.rs:42)

  main (main)
    → run_server (src/main.rs:20)
    → handle_post (src/api.rs:55)
    → db_insert (src/db.rs:38)
    → execute (src/db.rs:42)
```

JSON output with `--output-format json` for CI integration.

### 4. Cargo.toml

```toml
[dependencies]
apex-core = { path = "../apex-core" }

[dev-dependencies]
tempfile = "3"
tokio = { version = "1", features = ["full"] }
```

No heavy dependencies. Uses only `apex-core` types (`Language`, `BranchId`).

## Testing Strategy

### Unit tests (per extractor)
- Source snippets → verify correct FnNode extraction, CallEdge extraction, entry point classification
- Edge cases: nested functions, closures/lambdas, decorators, async, generics, trait impls

### Integration tests
- Multi-file fixture project per language (3-4 files with known call chains)
- Build CallGraph, query reverse paths, verify complete chains

### Granularity comparison tests
- Same fixture queried at Function/Block/Line level
- Verify Function returns superset of Block paths

### Dogfood test
- Build CallGraph for APEX itself
- Query: "entry points reaching CoverageOracle::mark_covered"
- Verify it finds test functions + orchestrator::run + CLI run path

## Crate Ownership

**Fleet crew:** This crate will be owned by the **intelligence** crew (alongside apex-agent, apex-synth, apex-rpc) since its primary consumer is the orchestrator for coverage guidance. The security-detect crew consumes it for finding enrichment but doesn't own the code.

## Dependencies

- `apex-core` — `Language`, `BranchId`, `Result`, `ApexError`
- No other APEX crates — the call graph is built from source text, not from CPG or coverage data
- Standard library only: `HashMap`, `BTreeMap`, `VecDeque` (for BFS), `PathBuf`
