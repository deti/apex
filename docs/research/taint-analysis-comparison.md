# Taint Analysis: APEX vs Production Tools

Research comparison of APEX's `apex-cpg` taint analysis against production-grade tools.
Date: 2026-03-21.

---

## 1. APEX Current State (apex-cpg)

### What APEX Has

| Capability | Module | Status |
|---|---|---|
| Line-based Python CPG builder | `builder.rs` | Working, regex-based |
| Go/JS CPG builders | `builder.rs` | Stubs via `CpgBuilder` trait |
| Reaching definitions (iterative MOP) | `reaching_def.rs` | Working, intra-procedural |
| Backward taint BFS from sinks to sources | `taint.rs` | Working, intra-procedural |
| SSA form with phi nodes | `ssa.rs` | Working, single function |
| Dominator tree (Cooper-Harvey-Kennedy) | `ssa.rs` | Working |
| Configurable source/sink/sanitizer rules | `taint_rules.rs` | Python + JS defaults |
| Runtime-extensible taint specs (LLM-injectable) | `taint_store.rs` | Working |
| Per-function taint summaries | `taint_summary.rs` | Working, basic |
| Summary application at call sites | `taint_summary.rs` | Working, basic |
| Summary cache with content-hash invalidation | `taint_summary.rs` | Working |
| Type-annotation taint propagation | `type_taint.rs` | Working, rule-based |
| Taint flow triage/scoring | `taint_triage.rs` | Working, heuristic |
| CPG merge (multi-file) | `lib.rs` (`Cpg::merge`) | Working, ID-remapping |

### What APEX Lacks

1. **Tree-sitter or real parser** -- the CPG builder is line-based regex, missing nested expressions, decorators, comprehensions, multi-line statements
2. **True inter-procedural analysis** -- summaries exist but are not wired into the main taint engine; `find_taint_flows()` only operates within a single function's CPG
3. **Call graph construction** -- no call graph; summaries require manual call-site resolution
4. **Field sensitivity** -- no tracking of `obj.field` taint through object attributes
5. **Context sensitivity** -- no call-string or object-sensitive analysis
6. **Alias analysis / points-to** -- no heap modeling
7. **Path sensitivity** -- all paths are explored regardless of feasibility
8. **Inter-file dataflow** -- `Cpg::merge` exists but reaching-def analysis does not cross file boundaries

### Architecture Assessment

The CPG data structure (`Vec<(NodeId, NodeKind)>` + `Vec<(NodeId, NodeId, EdgeKind)>`) is flat and uses linear scans for lookups (`node()` is O(n), `edges_from()`/`edges_to()` are O(e)). This is fine for single-function analysis but will not scale to whole-program graphs.

---

## 2. Production Tool Comparison

### 2.1 Joern

**Architecture**: Scala, OverflowDB (in-memory property graph), Scala REPL for queries.

| Feature | Joern | APEX |
|---|---|---|
| Parser | Language-specific frontends (JavaParser, GHidra, fuzzyC, etc.) | Line-based regex |
| CPG schema | 20+ node types, 15+ edge types (AST, CFG, CDG, DDG, PDG, REACHING_DEF, CALL, etc.) | 8 node types, 4 edge types |
| Graph storage | OverflowDB with adjacency indices | Flat Vec, linear scan |
| Inter-procedural | Pre-computed call graph, method summaries, overapproximation for unresolvable calls | Summaries exist but disconnected |
| Query language | Scala DSL (`.call.name("exec").argument(1).reachableBy(...)`) | Rust function calls only |
| Call resolution | Pre-generated call graph; falls back to overapproximation or user-defined semantics | None |
| Field sensitivity | Property access tracking via PDG | None |
| Multi-language | C/C++, Java, Python, JS/TS, Go, PHP, Ruby, Kotlin, Swift, C# | Python (partial), Go/JS (stubs) |

**Key gap**: Joern's pre-generated call graph and method summary system is what enables inter-procedural taint. Without it, analysis is confined to individual functions. APEX has the `TaintSummary` struct but does not automatically chain summaries across call sites during the main analysis pass.

**What Joern's inter-procedural engine does**:
- When taint reaches a call site for an internally defined method, it descends into the callee's CPG
- For external/unresolvable methods, it overapproximates (taint propagates through all parameters to return)
- Users can define custom semantic stubs to override overapproximation, improving precision

### 2.2 CodeQL

**Architecture**: QL (Datalog-derived language), relational database of program facts, whole-program analysis.

| Feature | CodeQL | APEX |
|---|---|---|
| Parser | Extractor per language (full semantic parse) | Line-based regex |
| IR | Relational database (snapshots) | In-memory graph |
| Dataflow | Global (whole-program) via fixpoint, local (intra-procedural) for precision | Intra-procedural reaching defs |
| Taint vs data flow | Explicit distinction: data flow preserves values, taint adds derivation edges | Combined |
| Query language | QL (Datalog + ADTs + recursion) | Rust API |
| Flow state | Per-data-value flow labels for precision (e.g., "tainted-but-length-checked") | None |
| Context sensitivity | Configurable per-query | None |

**Key insight**: CodeQL's power comes from the QL language being purpose-built for relational queries over program facts. Taint tracking is expressed as a Datalog-style query: define `isSource()`, `isSink()`, `isAdditionalTaintStep()`, and the engine computes global reachability. This is fundamentally more expressive than APEX's BFS-backward approach.

**What whole-program analysis adds**: CodeQL can track data from an HTTP handler's `request` parameter through 15 function calls, across 8 files, through a database abstraction layer, to a SQL query -- all in a single query. APEX would need to manually chain summaries across all those boundaries.

### 2.3 Semgrep

**Architecture**: OCaml, pattern-based matching, AST-level analysis.

| Feature | Semgrep OSS | Semgrep Pro | APEX |
|---|---|---|---|
| Parser | Tree-sitter (40+ languages) | Tree-sitter | Line-based regex |
| Taint mode | Intra-procedural | Inter-procedural (intra-file and inter-file) | Intra-procedural |
| Rule format | YAML (pattern-based) | YAML | Rust code |
| Analysis | AST pattern matching + dataflow on IL | + cross-function, cross-file | Reaching defs + BFS |
| Constant propagation | Yes | Yes | No |
| Path sensitivity | No | No | No |

**Key insight**: Semgrep demonstrates that intra-procedural taint with good pattern matching covers ~70% of real vulnerabilities. The jump from intra-procedural to inter-procedural (Semgrep Pro) catches the remaining ~30% but requires significantly more engineering. APEX is at the same level as Semgrep OSS's taint mode, but with a weaker parser.

### 2.4 Bearer

**Architecture**: Go (not Rust), tree-sitter parsing, rule-based.

| Feature | Bearer | APEX |
|---|---|---|
| Parser | Tree-sitter | Line-based regex |
| Points-to analysis | Local variable points-to | None |
| Constant propagation | String constant propagation | None |
| Taint model | Source/sink/sanitizer with data flow | Same model |
| Focus | Data sensitivity (PII/PHI tracking) | Security vulnerabilities |

**Key insight**: Bearer uses tree-sitter for parsing and adds points-to analysis for local variables, which is one step beyond APEX. Its main differentiation is tracking data classifications (PII, PHI) through flows, not just taint.

### 2.5 Snyk Code (DeepCode AI)

**Architecture**: Hybrid symbolic AI + neural ML, cloud-based.

| Feature | Snyk Code | APEX |
|---|---|---|
| Parser | Full semantic parse | Line-based regex |
| Dataflow | Inter-procedural "SecurityFlow" rules | Intra-procedural |
| AI component | Event graphs + symbolic AI + genAI for fix suggestions | LLM-injectable specs via TaintSpecStore |
| Taint model | Source/sink/sanitizer with contextual dataflow | Same core model |
| Analysis | Cloud-based, proprietary | Local, open source |

**Key insight**: Snyk Code's innovation is the hybrid approach -- symbolic rules catch known patterns, while neural models identify novel taint paths. APEX's `TaintSpecStore` (LLM-injectable specs) is a step in this direction but operates at the rule level, not the analysis level.

### 2.6 Academic: FlowDroid / IFDS / Doop

| Tool | Technique | Key Contribution |
|---|---|---|
| FlowDroid | IFDS-based taint for Android | Context-, flow-, field-, object-sensitive; 93% recall, 86% precision on DroidBench |
| IFDS/IDE | Distributive function framework | O(E*D^3) for interprocedural distributive problems; foundation for modern tools |
| Doop | Datalog-based points-to + taint | Declarative specification of whole-program analysis; 10x faster than prior art |

**Key insight**: IFDS provides the theoretical foundation for sound inter-procedural analysis. The key property is that taint analysis is a distributive problem (taint of variable X does not depend on taint of variable Y), so IFDS gives you context-sensitive results in polynomial time. APEX's BFS-backward approach is an ad-hoc approximation of what IFDS formalizes.

---

## 3. Gap Analysis

### Critical Gaps (blocking real-world effectiveness)

| Gap | Impact | Effort to Close |
|---|---|---|
| **Line-based parser** | Misses nested expressions, decorators, multi-line, list comprehensions, string interpolation | Medium -- adopt tree-sitter (Rust bindings exist) |
| **No call graph** | Cannot follow taint across function boundaries | Medium -- build from CPG Call nodes + name resolution |
| **Summaries not wired in** | `TaintSummary` exists but `find_taint_flows()` ignores it | Low -- modify BFS to consult summary cache at Call nodes |
| **Graph storage O(n)** | `node()` lookup is linear scan; will not scale past ~10K nodes | Low -- switch to `HashMap<NodeId, NodeKind>` |

### Important Gaps (precision and coverage)

| Gap | Impact | Effort to Close |
|---|---|---|
| **No field sensitivity** | Cannot distinguish `request.args` from `request.method` | Medium -- extend NodeKind with field access chains |
| **No constant propagation** | Cannot eliminate impossible paths or match string patterns | Medium -- add constant folding pass |
| **No alias analysis** | `a = b; taint(a)` does not taint `b` | High -- requires points-to analysis |
| **No path sensitivity** | Reports flows through infeasible paths | High -- requires constraint solving |

### Nice-to-Have Gaps (competitive differentiation)

| Gap | Impact | Effort to Close |
|---|---|---|
| **No query language** | Users cannot write custom taint queries | High -- design + implement DSL |
| **No flow state** | Cannot express "tainted-but-validated" precisely | Medium -- add state labels to taint propagation |
| **Limited language support** | Only Python works; Go/JS are stubs | High per language -- parser + rules + tests |

---

## 4. Can Tree-sitter Replace the Line-Based Builder?

**Yes, and it should.** The current builder in `builder.rs` uses regex patterns like:

```
def name(params):  ->  Method + Parameter nodes
name(args)         ->  Call + Argument nodes
lhs = rhs          ->  Assignment + Identifier nodes
```

This breaks on:
- Nested calls: `foo(bar(x))` -- only the outer call is captured
- Multi-line statements: `x = (very_long_expression\n    + continuation)`
- Decorators: `@app.route("/path")` is ignored
- Comprehensions: `[f(x) for x in user_input]`
- String interpolation: `f"SELECT * FROM {table}"` -- the taint through `table` is missed
- Method chaining: `request.args.get("id")` -- parsed as a single name, not a chain

Tree-sitter provides:
- Correct AST for all syntactically valid code
- Rust bindings (`tree-sitter` crate, mature)
- Grammars for 40+ languages (maintained by the community)
- Incremental parsing (re-parse only changed regions)
- Error recovery (produces partial ASTs for broken code)

**Migration path**:
1. Add `tree-sitter` and `tree-sitter-python` as dependencies
2. Implement a `TreeSitterCpgBuilder` that walks the tree-sitter CST and emits the same `NodeKind`/`EdgeKind` types
3. Keep the existing `PythonCpgBuilder` as a fallback
4. Gradually extend `NodeKind` to support richer constructs (field access, comprehensions, etc.)
5. Add `tree-sitter-javascript`, `tree-sitter-go`, etc. for multi-language support

**Estimated effort**: 2-3 days for Python, 1 day per additional language (for basic CPG construction).

---

## 5. What Does Inter-Procedural Taint Add?

### The problem inter-procedural taint solves

Consider this real-world pattern:

```python
# file: routes.py
@app.route("/search")
def search():
    query = request.args.get("q")
    results = db.search(query)         # taint crosses into db.search
    return render(results)

# file: db.py
def search(term):
    return cursor.execute(             # SQL injection sink
        f"SELECT * FROM items WHERE name = '{term}'"
    )
```

- **Intra-procedural analysis** (APEX today): Finds nothing. In `search()`, `query` flows into `db.search()` but there is no sink in this function. In `db.search()`, `term` is a parameter (marked as source) that flows to `cursor.execute()` -- this IS found, but only if parameters are treated as sources.
- **Inter-procedural analysis**: Connects `request.args.get("q")` in `routes.py` through `db.search()` to `cursor.execute()` in `db.py`. Reports a single, actionable finding with the full trace.

### What it costs

- Call graph construction (name resolution, method resolution order for OOP)
- Summary computation for every function
- Handling of dynamic dispatch, closures, callbacks
- Significantly more memory and analysis time
- Higher false-positive rate from overapproximation of unresolvable calls

### Practical value

Based on published benchmarks and Semgrep's own data:
- Intra-procedural taint finds ~65-75% of real injection vulnerabilities
- Inter-procedural (intra-file) adds ~15-20%
- Inter-file analysis adds ~5-10%
- The remaining ~5% requires alias analysis, reflection handling, etc.

APEX's current approach of treating all parameters as tainted sources is actually a sound overapproximation that catches many inter-procedural cases within a single function -- it just cannot provide the end-to-end trace.

---

## 6. Closing the Gap: Practical Roadmap

### Phase 1: Parser upgrade (highest ROI)

Switch from line-based regex to tree-sitter for the CPG builder.

**Why first**: Every other improvement (field sensitivity, better call resolution, richer taint specs) depends on having an accurate AST. The line-based parser is the single biggest source of false negatives.

**Deliverable**: `TreeSitterPythonCpgBuilder` implementing `CpgBuilder` trait, passing all existing tests plus new tests for nested calls, multi-line, decorators, comprehensions.

### Phase 2: Wire summaries into the taint engine (low effort, high impact)

Connect `TaintSummary` to `find_taint_flows()`:
1. When backward BFS reaches a `Call` node, look up the callee's summary in `SummaryCache`
2. If the summary says "parameter 0 flows to return unsanitized", continue BFS through the call
3. If no summary exists, overapproximate (all args taint return)

**Deliverable**: Modified `find_taint_flows()` that accepts a `&SummaryCache` parameter and chains through call sites.

### Phase 3: Graph storage upgrade

Replace `Vec<(NodeId, NodeKind)>` with `HashMap<NodeId, NodeKind>` and build adjacency lists for edges.

**Deliverable**: O(1) node lookup, O(degree) edge iteration. Enables scaling to whole-program graphs.

### Phase 4: Call graph construction

Build a call graph from CPG `Call` nodes by resolving callee names:
1. Direct name match (`foo()` -> function `foo`)
2. Qualified name resolution (`module.foo()` -> function `foo` in module `module`)
3. For unresolvable calls, mark as "external" and apply overapproximation

**Deliverable**: `CallGraph` struct with `callers(fn)` and `callees(fn)` methods.

### Phase 5: Field sensitivity

Extend `NodeKind` to track field access chains:
```rust
NodeKind::FieldAccess {
    base: String,      // "request"
    field: String,     // "args"
    line: u32,
}
```

This enables distinguishing `request.args` (tainted) from `request.method` (safe).

### Phase 6: Constant propagation

Add a constant-folding pass that propagates literal values through assignments, enabling:
- Elimination of false positives where a "tainted" variable actually holds a constant
- String concatenation tracking for SQL injection patterns

---

## 7. Competitive Positioning

| Dimension | APEX Today | After Phase 1-3 | After Phase 1-6 | Joern | CodeQL | Semgrep Pro |
|---|---|---|---|---|---|---|
| Parser quality | Low (regex) | High (tree-sitter) | High | High | Very High | High |
| Intra-procedural taint | Yes | Yes | Yes | Yes | Yes | Yes |
| Inter-procedural taint | No (summaries unused) | Basic (summary chaining) | Good (call graph) | Yes | Yes | Yes |
| Field sensitivity | No | No | Yes | Yes | Yes | Partial |
| Multi-language | 1 (Python partial) | 1 (Python full) | 3+ | 12+ | 12+ | 30+ |
| Query language | No | No | No | Scala DSL | QL | YAML |
| Speed | Fast (small graphs) | Fast | Fast | Medium | Slow | Fast |
| Self-contained binary | Yes (5MB) | Yes | Yes | No (JVM) | No (CLI + DB) | No (cloud) |

APEX's competitive advantage is the self-contained binary with no runtime dependencies. The path to being a credible taint analysis tool requires phases 1-3 (parser + summary wiring + graph storage). Phases 4-6 bring it to parity with Joern's core capabilities for Python.

---

## Sources

- [Joern: How Interprocedural Data-flow Works](https://joern.io/blog/interproc-dataflow-2024/)
- [Joern: Custom Data-Flow Semantics](https://docs.joern.io/dataflow-semantics/)
- [Joern: Code Property Graph](https://docs.joern.io/code-property-graph/)
- [CPG Specification 1.1](https://cpg.joern.io/)
- [CodeQL: About Data Flow Analysis](https://codeql.github.com/docs/writing-codeql-queries/about-data-flow-analysis/)
- [CodeQL: Using Flow State](https://codeql.github.com/docs/codeql-language-guides/using-flow-labels-for-precise-data-flow-analysis/)
- [Semgrep: Taint Analysis](https://semgrep.dev/docs/writing-rules/data-flow/taint-mode/overview)
- [Semgrep: Dataflow Analysis Engine Overview](https://semgrep.dev/docs/writing-rules/data-flow/data-flow-overview)
- [Bearer CLI: How the CLI Works](https://docs.bearer.com/explanations/workflow/)
- [Snyk: Taint Analysis with Contextual Dataflow](https://snyk.io/blog/analyze-taint-analysis-contextual-dataflow-snyk-code/)
- [Snyk: DeepCode AI](https://snyk.io/platform/deepcode-ai/)
- [FlowDroid Paper (PLDI 2014)](https://www.bodden.de/pubs/far+14flowdroid.pdf)
- [Doop: Declarative Points-to Analysis](https://github.com/plast-lab/doop)
- [Doop: Using Datalog for Fast and Easy Program Analysis](https://yanniss.github.io/doop-datalog2.0.pdf)
