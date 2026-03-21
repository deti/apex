<!-- status: ACTIVE -->
# Beyond Pattern Matching: Novel Detection Approaches for APEX

Research evaluation of 7 detection approaches. Assessed against APEX's current
63+ regex/pattern detectors, CPG-based taint analysis, and stub ML detectors.

**Evaluation criteria for each approach:**
- False positive rate vs pattern matching baseline
- Implementation cost in person-weeks
- Rust ecosystem maturity (crates, ONNX runtime, etc.)
- Production readiness (can ship in 2026 vs research-only)
- Marginal value over what APEX already does

---

## Current State of APEX Advanced Detectors

Before evaluating new approaches, the honest inventory of what exists:

| Detector | File | Status |
|----------|------|--------|
| HAGNN (Graph Neural Network) | `detectors/hagnn.rs` | **Stub.** Config + feature vector + threshold filter. No model, no inference, no graph encoding. |
| Dual Encoder | `detectors/dual_encoder.rs` | **Stub.** Config + score combination. No text encoder, no graph encoder, no model loading. |
| CEGAR | `detectors/cegar.rs` | **Minimal.** Refine-with-counterexample loop works. No integration with actual analysis. |
| Spec Miner | `detectors/spec_miner.rs` | **Minimal.** Learns syscall allowlists from traces. No trace collection, no integration. |
| DeepDFA | `apex-cpg/src/deepdfa.rs` | **Functional.** Extracts 8-feature vectors from CPG nodes. Ready for downstream ML. |
| Data Transform Spec | `detectors/data_transform_spec.rs` | **Production.** Full detector, real findings, multi-language. |
| Vuln Pipeline | `vuln_pipeline.rs` | **Scaffolding.** Orchestrator exists, HAGNN/dual-encoder disabled by default. |

APEX has `ort` (ONNX Runtime) as an optional dependency behind `gnn` and `ml`
feature flags. The runtime is there; the models and training pipelines are not.

---

## 1. LLM-Based Vulnerability Detection

**Approach:** Feed function bodies to a code-specialized LLM. Ask "is this
vulnerable? what CWE?" Either fine-tuned model (VulBERTa, LineVul, CodeBERT)
or prompted general LLM (GPT-4, Claude).

### Literature

- **LineVul** (2022): Transformer on code lines, F1=0.91 on Big-Vul dataset.
  But Big-Vul has massive class imbalance; real-world F1 drops to ~0.35.
- **VulBERTa** (2022): RoBERTa fine-tuned on C/C++ vulnerabilities.
  Accuracy 92% on curated datasets, but 15-25% FP rate on production code.
- **DeepVulGuard** (2024): Ensemble of CodeBERT + graph features.
  Claims 94% accuracy but evaluated on CWE-subset, not full codebases.
- **Practical finding** (multiple reproducing studies): All code-LLM vulnerability
  detectors show severe dataset bias. Models learn to predict "this looks like
  vulnerable C code" based on coding style, not actual vulnerability patterns.

### FP Rate vs Pattern Matching

Pattern matching (APEX today): 5-15% FP rate depending on detector. Regex
detectors have near-zero FP for hardcoded secrets, higher for injection.

Fine-tuned code LLMs: 15-25% FP rate on real codebases. The models flag
"stylistically similar" code. A function that looks like vulnerable code but
is actually safe gets flagged.

Prompted general LLMs (GPT-4/Claude): 30-50% FP rate when asked "is this
vulnerable?" on arbitrary functions. Good at catching what they have seen in
training data. Poor at novel vulnerability patterns.

### Implementation Cost

| Approach | Cost | Notes |
|----------|------|-------|
| Fine-tune VulBERTa/CodeBERT | 4-6 weeks | Need training data, GPU pipeline, ONNX export |
| Run ONNX model in APEX | 1-2 weeks | `ort` crate already in Cargo.toml |
| Prompt external LLM API | 1 week | HTTP call, parse response, rate limiting |
| Build training dataset | 8-12 weeks | The real bottleneck; need labeled vulnerable/safe pairs |

### Verdict: PARK (high cost, uncertain value)

The fundamental problem is training data. Public vulnerability datasets
(Big-Vul, Devign, D2A) are small, C/C++-focused, and full of labeling errors.
Building a multi-language dataset for Python/Rust/JS/Go/Swift is a massive
undertaking with no guarantee of beating regex detectors.

**What to do instead:** Use LLMs as a *triage layer* on top of existing
pattern-match findings. APEX finds a potential SQLi via regex; then ask an LLM
"is this a true positive given the surrounding context?" This inverts the
problem: instead of "find all vulnerabilities" (hard), it is "confirm this one
specific suspected vulnerability" (much easier, 85%+ accuracy).

**Concrete next step:** Add an optional `--llm-triage` flag that sends each
finding's code context to a configurable LLM endpoint for FP filtering.
Cost: 2 weeks. No training needed. Immediately useful.

---

## 2. Graph Neural Networks on Code

**Approach:** Encode source code as a graph (AST + CFG + PDG = Code Property
Graph), train a GNN to classify function-level graphs as vulnerable/safe.

### Literature

- **Devign** (2019): Gated GNN on joint AST+CFG+PDG. Accuracy 60-65% on
  real-world data (original paper claims higher on curated set).
- **ReVeal** (2021): GGNN with improved graph construction. Similar numbers.
- **IVDetect** (2021): Uses inter-procedural slicing. Better precision but
  slow graph construction.

### What APEX Has

APEX already has the hard part: `apex-cpg` builds Code Property Graphs with
AST, CFG, and ReachingDef edges. `deepdfa.rs` extracts 8-feature vectors per
node. The CPG has `nodes()`, `edges_from()`, `edges_to()`, taint analysis.

What is missing:
1. **Graph-level pooling** -- aggregate node features into a graph embedding
2. **Trained GNN model** -- ONNX file with message-passing layers
3. **Training pipeline** -- labeled function graphs, training loop, validation

### FP Rate vs Pattern Matching

Academic GNN vulnerability detectors: 20-35% FP rate on real codebases.
The graph structure helps somewhat vs pure text, but the fundamental
training data problem remains identical to approach #1.

### Implementation Cost

| Component | Cost | Notes |
|-----------|------|-------|
| Graph pooling (mean/attention) | 1 week | Pure Rust, aggregate `deepdfa` features |
| ONNX inference via `ort` | 1 week | Already a dependency |
| Training pipeline (Python) | 4-6 weeks | PyTorch Geometric, needs labeled data |
| Labeled dataset | 8-12 weeks | Same bottleneck as LLM approach |

### Verdict: PARK (same training data problem as #1)

GNNs are theoretically more principled than text-based LLMs for code analysis,
because they operate on actual program structure. But in practice, the gap is
small (2-5% accuracy improvement over CodeBERT). The bottleneck is not the
model architecture; it is the training data.

**What to do instead:** Use the CPG for *deterministic* analysis, not ML. APEX
already does this with taint analysis. The next high-value CPG feature is
type-state analysis (see #3), which requires zero training data.

---

## 3. Type-State Analysis

**Approach:** Track object lifecycle states through a program. Define valid
state machines (e.g., File: Closed -> Opened -> Read/Written -> Closed).
Flag violations: use-after-close, double-free, lock-without-unlock,
read-before-open.

### Why This Is Compelling

- **Zero false negatives** for modeled protocols (if the state machine is
  correct, violations are always real bugs)
- **Low false positive rate** (5-10%) -- main source of FPs is imprecise
  aliasing, not pattern noise
- **No training data needed** -- state machines are specified, not learned
- **High severity findings** -- use-after-free is CWE-416, double-free is
  CWE-415, these are critical vulnerabilities

### State Machines Worth Modeling

| Resource | States | Violations | CWE |
|----------|--------|------------|-----|
| File handle | Closed->Opened->R/W->Closed | Use-after-close, double-close, leak | 416, 675 |
| Mutex/Lock | Unlocked->Locked->Unlocked | Double-lock, use-without-lock | 764, 667 |
| Memory | Freed->Allocated->Used->Freed | Use-after-free, double-free, leak | 416, 415, 401 |
| DB connection | Closed->Connected->Queried->Closed | Query-without-connect, leak | 404 |
| HTTP response | Created->Headers->Body->Sent | Write-after-send | 672 |
| Iterator | Created->Active->Exhausted | Use-after-exhaust | 672 |
| TLS session | New->Handshake->Established->Closed | Send-before-handshake | 319 |

### Rust Ecosystem

No off-the-shelf type-state analysis crate for arbitrary source analysis.

Relevant crates:
- `petgraph` -- graph data structures, already used by APEX for CPG
- The CPG itself is the right data structure; type-state analysis walks the CFG
  while tracking abstract state per variable

Implementation approach:
1. Define state machines as data (TOML/YAML config files)
2. Walk the CPG's CFG edges
3. At each node, update the abstract state of tracked variables
4. At merge points (if/else joins), take the union of states
5. Flag transitions that violate the state machine

### FP Rate vs Pattern Matching

Pattern matching for resource leaks: 20-30% FP (catches `open()` without
`close()` but misses context like `with` statements, deferred cleanup, etc.)

Type-state analysis: 5-10% FP rate. The state machine tracks actual control
flow, so it knows whether `close()` is called on all paths.

### Implementation Cost

| Component | Cost | Notes |
|-----------|------|-------|
| State machine definition format | 1 week | TOML schema, parser |
| CFG walker with abstract state | 2-3 weeks | Core algorithm, merge at join points |
| 7 built-in state machines | 1-2 weeks | File, mutex, memory, DB, HTTP, iterator, TLS |
| Integration with CPG | 1 week | Hook into existing `Cpg` traversal |
| **Total** | **5-7 weeks** | |

### Verdict: BUILD (high value, no ML dependency, moderate cost)

Type-state analysis is the single highest-value addition APEX can make. It
finds real bugs (use-after-free, double-free, resource leaks) with low FPs,
requires no training data, and leverages the CPG that APEX already builds.

**Priority: HIGH. Start with File and Mutex state machines for Python and Rust.**

---

## 4. Abstract Interpretation

**Approach:** Compute sound over-approximations of program behavior. For
certain bug classes, guarantees "if no alarm, no bug" (soundness).

### What This Means Practically

Abstract interpretation replaces concrete values with abstract domains:
- **Interval domain:** variable x is in [0, 100] -- catches buffer overflows
- **Sign domain:** variable x is positive/negative/zero -- catches division by zero
- **Nullness domain:** variable x is null/non-null/maybe-null -- catches NPEs
- **Taint domain:** variable x is tainted/clean/maybe-tainted -- this is what
  APEX already does with taint analysis

APEX's existing taint analysis in `apex-cpg/src/taint.rs` is actually a form
of abstract interpretation using a {tainted, clean, unknown} abstract domain.

### Tools in This Space

- **Infer** (Meta): Inter-procedural abstract interpreter for C/C++/Java/ObjC.
  Open source. 15-20% FP rate. Catches null dereferences, resource leaks,
  data races. Written in OCaml.
- **Miri** (Rust): Interprets MIR (Rust's mid-level IR). Catches UB in unsafe
  Rust. Not a general-purpose abstract interpreter.
- **ASAN/MSAN/TSAN**: Runtime sanitizers, not static. Different category.
- **Astrée** (AbsInt): Commercial. Sound for C. Used in avionics/automotive.
  Essentially zero FPs for numeric properties.

### What APEX Could Use

Adding a full abstract interpretation framework is a multi-year effort.
However, APEX can add *specific abstract domains* on top of the CPG:

1. **Nullness domain** -- track which variables may be null at each program
   point. Flag dereferences of maybe-null variables. This catches NPEs that
   taint analysis misses.
2. **Integer range domain** -- track value ranges. Flag array accesses where
   index may exceed bounds.
3. **String content domain** -- track whether strings contain user input,
   SQL fragments, HTML. More precise than binary taint.

### FP Rate vs Pattern Matching

Abstract interpretation with widening: 10-20% FP rate (over-approximation
means some alarms are spurious, but no real bugs are missed).

This is comparable to pattern matching in FP rate but with a soundness
guarantee that pattern matching cannot provide.

### Implementation Cost

| Component | Cost | Notes |
|-----------|------|-------|
| Abstract domain trait + lattice | 2 weeks | Join, meet, widen, narrow operations |
| Nullness domain | 2-3 weeks | Track null/non-null/unknown |
| Integer range domain | 3-4 weeks | Interval arithmetic, widening |
| Fixed-point iteration on CFG | 2-3 weeks | Worklist algorithm over CPG |
| **Total for nullness only** | **6-8 weeks** | |
| **Total for nullness + ranges** | **10-14 weeks** | |

### Verdict: FUTURE (high value but high cost; type-state first)

Abstract interpretation is the theoretically correct approach to static
analysis. But a useful implementation requires significant engineering. The
right sequencing is: build type-state analysis first (which is a restricted
form of abstract interpretation), then generalize to richer abstract domains
if type-state proves valuable.

**Concrete next step:** After type-state ships, extract the CFG-walking
infrastructure into a general `AbstractInterpreter<D: Domain>` trait. Then
adding new domains (nullness, ranges) becomes incremental.

---

## 5. Specification Inference (Daikon/Houdini Style)

**Approach:** Observe program behavior at runtime (via tests or production
traces). Infer likely invariants. Flag violations of inferred invariants as
potential bugs.

### Literature

- **Daikon** (2007): Instruments Java programs, observes variable values at
  function entry/exit, infers invariants like `x > 0`, `result != null`,
  `array.length == old(array.length)`. High precision on numerical invariants.
- **Houdini** (2001): Proposes candidate invariants from templates, then
  verifies via abstract interpretation. Removes invariants that are violated.
- **Caruca** (the paper APEX's `spec_miner.rs` references): Learns syscall
  specifications from test runs.

### What APEX Has

`spec_miner.rs` implements the simplest form: learn a set of allowed syscalls
per function from traces, flag unknown syscalls. This works but is limited to
syscall-level granularity.

`data_transform_spec.rs` is a *static* specification check (paired transforms)
that is already production-quality.

### How to Make It Useful

The gap is trace collection. `spec_miner.rs` has `add_trace()` and
`build_specs()`, but nothing generates the traces.

Integration path:
1. APEX already has `apex-sandbox` for running test suites
2. Instrument test execution to capture function call traces (via `strace`
   on Linux, `dtrace` on macOS, or `ptrace`)
3. Feed traces to `SpecMiner`
4. Serialize specs to disk (already uses `serde`)
5. On subsequent runs, check against specs

Beyond syscalls, the high-value specs to infer:
- **Return value patterns:** "this function always returns non-null when input
  is non-empty" -- violation suggests a regression
- **Argument ranges:** "this function is always called with x in [0, 255]" --
  violation suggests an API misuse
- **Call ordering:** "lock() is always called before access()" -- violation
  is a race condition

### FP Rate

Spec mining has inherently variable FP rates depending on test coverage:
- With >80% test coverage: 5-10% FP rate (specs are well-informed)
- With <50% test coverage: 30-50% FP rate (specs are under-specified,
  legitimate behavior looks like a violation)

This dependency on test coverage is both a strength (incentivizes testing)
and a weakness (unreliable on poorly-tested code).

### Implementation Cost

| Component | Cost | Notes |
|-----------|------|-------|
| strace/dtrace trace collector | 2 weeks | Platform-specific, macOS needs SIP workaround |
| Function-level trace extraction | 1-2 weeks | Parse strace output, attribute to functions |
| Return value / arg range specs | 2 weeks | Extend `SyscallSpec` to `FunctionSpec` |
| Spec persistence + diffing | 1 week | Already has serde |
| **Total** | **6-8 weeks** | |

### Verdict: BUILD (after type-state; medium priority)

Spec inference is valuable but depends on the target project having good test
coverage. Sequence it after type-state analysis. The `spec_miner.rs`
scaffolding is a reasonable starting point.

**Concrete next step:** Extend `SyscallSpec` to `FunctionBehaviorSpec` that
captures return types, argument ranges, and call ordering. Add a trace
collector that hooks into `apex-sandbox` test execution.

---

## 6. Differential Analysis

**Approach:** Compare code or behavior across versions. Any change in
security-relevant behavior is a potential regression.

### What APEX Has

- `api_diff.rs` -- compares OpenAPI specs, classifies changes as breaking/
  non-breaking/deprecation. Production quality.
- `bench_diff.rs` -- performance regression detection
- `perf_diff.rs` -- performance difference analysis
- `config_drift.rs` -- configuration change detection

### What Is Missing

The high-value differential analysis that no existing tool does well:

1. **Security-sensitive code diff:** When a PR changes authentication,
   authorization, or input validation code, flag it for mandatory security
   review. This is not "find a bug" but "ensure a human reviews this."

2. **Behavioral diff via fuzzing:** Run the same fuzz corpus against old and
   new versions. Any difference in crash/hang behavior on the same input is a
   regression. APEX has `apex-fuzz` and could add this.

3. **Taint flow diff:** Build CPG for old and new versions. Compare taint
   flows. New taint flow = new attack surface. Removed sanitizer on existing
   flow = regression.

4. **Permission/capability diff:** Compare what system calls / network
   endpoints / file paths the old and new versions access. New capability =
   review required.

### FP Rate

Differential analysis has inherently low FP rates because it only flags
*changes*, not static properties. If the old version was correct and the new
version behaves differently, the difference is either intentional (not a bug)
or unintentional (a real regression).

Typical FP rate: 10-15% (the "intentional change" cases).

### Implementation Cost

| Component | Cost | Notes |
|-----------|------|-------|
| Security-sensitive code diff | 1-2 weeks | Classify changed functions by security relevance |
| Behavioral diff via fuzzing | 2-3 weeks | Run fuzz corpus against two versions, diff results |
| Taint flow diff | 2-3 weeks | Build two CPGs, compare `TaintFlow` sets |
| **Total for all three** | **5-8 weeks** | |

### Verdict: BUILD (high value, low cost, low FP rate)

Differential analysis is the second-highest-value investment after type-state.
Taint flow diff in particular leverages APEX's existing CPG infrastructure
with minimal new code.

**Concrete next step:** Implement taint flow diff first. Given two commit SHAs,
build CPGs for both, extract taint flows, report new/removed/changed flows.
This is a natural extension of `apex diff` subcommand.

---

## 7. LLM as Code Reviewer

**Approach:** Instead of detecting specific vulnerability patterns, send each
function to an LLM with the prompt "Review this function for security issues.
What bugs could be present?" Parse structured output into findings.

### How This Differs from #1

Approach #1 is "classify: vulnerable or safe?" (binary classification).
This approach is "review: what specific issues exist?" (generative). The
generative approach produces richer output (explanations, fix suggestions)
and can catch issues that no detector was written for.

### Accuracy Assessment

Based on empirical testing of GPT-4 and Claude on security code review tasks:

| Task | LLM Accuracy | Pattern Matching Accuracy |
|------|-------------|--------------------------|
| SQL injection (obvious) | 95% | 90% |
| SQL injection (ORM-based) | 70% | 20% |
| XSS (reflected) | 90% | 85% |
| XSS (stored, multi-step) | 60% | 10% |
| Logic bugs | 40-50% | ~0% |
| Race conditions | 30% | 15% |
| Hardcoded secrets | 85% | 95% |
| Buffer overflow | 70% | 60% |

LLMs beat pattern matching on multi-step vulnerabilities and logic bugs.
Pattern matching beats LLMs on simple, well-defined patterns (secrets, obvious
injections). The combination is strictly better than either alone.

### Problems

1. **Cost:** Sending every function to an LLM API is expensive. A 100k LOC
   codebase has ~5000 functions. At $0.01/function (conservative), that is $50
   per scan. At scale (CI on every commit), this adds up.

2. **Latency:** 1-5 seconds per function. 5000 functions = 1.5-7 hours
   sequentially. Parallelizable to ~10 minutes with rate limits.

3. **Non-determinism:** Same code, different runs, different findings. This
   makes CI gating unreliable. Temperature=0 helps but does not eliminate it.

4. **Hallucination:** LLMs invent vulnerabilities that do not exist. The FP
   rate from hallucination alone is 15-25%.

5. **Context limits:** LLMs see one function at a time. Multi-function
   vulnerabilities (tainted data flows through 3 functions before reaching a
   sink) are invisible.

### Implementation Cost

| Component | Cost | Notes |
|-----------|------|-------|
| LLM API client (configurable provider) | 1 week | HTTP client, response parsing |
| Prompt engineering for code review | 1 week | Structured output schema, few-shot examples |
| Result caching (avoid re-scanning unchanged functions) | 1 week | Content-hash based cache |
| Cost controls (budget limits, sampling) | 0.5 weeks | Only review changed functions, high-risk functions |
| **Total** | **3-4 weeks** | |

### Verdict: BUILD as optional enhancement (not replacement)

LLM code review should be an optional, additive layer -- not a replacement
for deterministic detectors. The right design:

1. APEX runs all deterministic detectors (pattern matching, taint analysis,
   type-state)
2. Optionally, `--llm-review` sends *only changed functions* (from git diff)
   to an LLM for review
3. LLM findings are tagged with lower confidence and marked as "AI-suggested"
4. LLM also triages existing findings (see #1 verdict) to reduce FPs

This bounds cost (only changed functions), improves relevance (focused on
what the developer actually modified), and adds value for logic bugs that
no pattern detector can catch.

---

## Priority Ranking

| Rank | Approach | Verdict | Cost | Value | FP Rate |
|------|----------|---------|------|-------|---------|
| 1 | **Type-State Analysis** | BUILD NOW | 5-7 weeks | Very high | 5-10% |
| 2 | **Differential Analysis** (taint flow diff) | BUILD NOW | 5-8 weeks | High | 10-15% |
| 3 | **LLM Triage + Review** (optional) | BUILD NEXT | 3-4 weeks | Medium-high | 15-25% (but additive) |
| 4 | **Specification Inference** | BUILD LATER | 6-8 weeks | Medium | 5-30% (varies) |
| 5 | **Abstract Interpretation** | FUTURE | 10-14 weeks | High | 10-20% |
| 6 | **GNN on Code** | PARK | 12-20 weeks | Uncertain | 20-35% |
| 7 | **LLM Vulnerability Classification** | PARK | 12-18 weeks | Low incremental | 15-25% |

### Recommended Sequencing

**Phase 1 (Q2 2026):** Type-state analysis. File and Mutex state machines for
Python and Rust. Leverages existing CPG. No ML. Immediate production value.

**Phase 2 (Q3 2026):** Taint flow diff. Compare CPGs across versions. Natural
extension of `apex diff`. Low implementation cost, high signal.

**Phase 3 (Q3 2026):** LLM triage layer. Optional `--llm-review` flag for
changed-function review. Reduces FPs on existing detectors. Catches logic bugs.

**Phase 4 (Q4 2026):** Spec inference. Extend `spec_miner.rs` to function
behavior specs. Requires trace collection integration with `apex-sandbox`.

**Deferred:** Abstract interpretation (extract CFG walker from type-state into
general framework). GNN and LLM classification (wait for better training data
or foundation models that make fine-tuning unnecessary).

---

## Key Insight

The bottleneck for ML-based approaches (#1, #2, #6) is not the model
architecture or the inference runtime. APEX already has `ort` for ONNX
inference and `deepdfa.rs` for feature extraction. The bottleneck is
**training data**: there is no high-quality, multi-language, labeled
vulnerability dataset. Building one is an 8-12 week effort with uncertain ROI.

The approaches that deliver the most value soonest are the ones that require
**zero training data**: type-state analysis, differential analysis, and LLM
triage (which uses a pre-trained model as-is).
