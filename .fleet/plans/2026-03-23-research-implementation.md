<!-- status: ACTIVE -->

# Research Implementation Plan

**Date:** 2026-03-23
**Source:** 12 deep research digs across coverage, instrumentation, fuzzing, concolic, symbolic, CPG, detection, synthesis, orchestrator, sandbox, and reachability.
**Scope:** 15 priority items mapped to 4 phases across 7 crews.

---

## Infrastructure Inventory (Pre-Planning Assessment)

What already exists and what is missing, to inform task scoping.

| Capability | Status | Files |
|-----------|--------|-------|
| tree-sitter feature flag (`treesitter`) | EXISTS in apex-cpg/Cargo.toml | `tree-sitter = 0.24`, `tree-sitter-python = 0.23` |
| tree-sitter Python CPG builder | EXISTS, partial | `crates/apex-cpg/src/ts_python.rs` -- builds CPG from tree-sitter CST |
| tree-sitter JS/Go CPG builders | MISSING | Need `ts_javascript.rs`, `ts_go.rs` |
| CpgBuilder trait | EXISTS | `crates/apex-cpg/src/builder.rs` -- object-safe, `build()` + `language()` |
| Solver trait | EXISTS, extensible | `crates/apex-symbolic/src/traits.rs` -- `solve()`, `solve_batch()`, `set_logic()`, `name()` |
| PortfolioSolver | EXISTS | `crates/apex-symbolic/src/portfolio.rs` -- sequential try, GradientSolver first |
| SolverLogic enum | EXISTS | `QfLia`, `QfAbv`, `QfS`, `Auto` -- already maps to solver strengths |
| EnsembleSync (GALS) | EXISTS | `crates/apex-agent/src/ensemble.rs` -- Mutex-based buffer, periodic drain |
| FuzzStrategy | EXISTS | `crates/apex-fuzz/src/lib.rs` -- MOpt scheduler, corpus, coverage-guided |
| Taint analysis | EXISTS | `crates/apex-cpg/src/taint.rs` -- backward BFS, inter-procedural summaries |
| Taint triage | EXISTS | `crates/apex-cpg/src/taint_triage.rs` -- scorer for finding prioritization |
| ProcessSandbox | EXISTS | `crates/apex-sandbox/src/process.rs` -- subprocess + SHM bitmap, no seccomp |
| FirecrackerSandbox | EXISTS, feature-gated | `crates/apex-sandbox/src/firecracker.rs` |
| seccomp-bpf filter | MISSING | No syscall filtering on ProcessSandbox |
| sandbox-exec (macOS) | MISSING | No macOS sandbox profile |
| Differential coverage | MISSING | No `--diff` flag, no git-aware filtering |
| MC/DC mode | MISSING | No MC/DC instrumentation support |
| Coverage import/export | PARTIAL | `crates/apex-instrument/src/import.rs`, `lcov_export.rs` exist |
| Incremental coverage cache | MISSING | No file-hash caching |
| Dynamic call graph collection | MISSING | No runtime call edge recording |
| LLM triage for detection | MISSING | `taint_triage.rs` exists but no LLM integration |
| CPG-informed synthesis prompts | MISSING | Synthesis prompts use source text only |

---

## File Map

| Crew | Phase | Files Affected |
|------|-------|---------------|
| **security-detect** | 1, 2 | `crates/apex-cpg/src/ts_python.rs`, new `ts_javascript.rs`, new `ts_go.rs`, `crates/apex-cpg/Cargo.toml`, `crates/apex-cpg/src/builder.rs`, `crates/apex-cpg/src/taint_triage.rs`, `crates/apex-detect/src/pipeline.rs` |
| **exploration** | 2, 3 | `crates/apex-fuzz/src/lib.rs`, `crates/apex-fuzz/src/corpus.rs`, `crates/apex-fuzz/src/traits.rs`, `crates/apex-symbolic/src/portfolio.rs`, `crates/apex-symbolic/src/traits.rs`, new `crates/apex-symbolic/src/bitwuzla.rs`, `crates/apex-concolic/src/` |
| **runtime** | 2, 3 | `crates/apex-sandbox/src/process.rs`, new `crates/apex-sandbox/src/seccomp.rs`, new `crates/apex-sandbox/src/macos_sandbox.rs`, `crates/apex-instrument/src/`, `crates/apex-lang/src/` |
| **foundation** | 1, 2 | `crates/apex-coverage/src/oracle.rs`, `crates/apex-coverage/src/lib.rs`, `crates/apex-core/src/types.rs` |
| **intelligence** | 2, 3 | `crates/apex-agent/src/ensemble.rs`, `crates/apex-agent/src/orchestrator.rs`, `crates/apex-synth/src/strategy.rs`, `crates/apex-synth/src/llm.rs` |
| **platform** | 3, 4 | `crates/apex-cli/src/`, `crates/apex-instrument/src/import.rs`, `crates/apex-instrument/src/lcov_export.rs` |
| **mcp-integration** | 4 | `crates/apex-cli/src/mcp.rs`, MCP tool definitions |

---

## Phase 1: CPG Foundation (tree-sitter migration)

**Prerequisite for:** Phase 2 LLM triage, Phase 2 CPG-backed validation, Phase 3 CPG-informed synthesis.
**Duration estimate:** 2-3 weeks.
**Parallelism:** Tasks 1.1-1.3 are independent (one per language). Task 1.4 depends on all three.

### Task 1.1 -- security-detect crew: Complete tree-sitter Python CPG builder
**Files:** `crates/apex-cpg/src/ts_python.rs`, `crates/apex-cpg/src/builder.rs`
**Effort:** 3-4 days
**Context:** `ts_python.rs` already exists with `TreeSitterPythonCpgBuilder` that implements `CpgBuilder`. It parses function definitions, calls, assignments, control structures. Needs:
- [ ] Audit completeness: verify all node types that `PythonCpgBuilder` (regex) handles are also handled by tree-sitter builder
- [ ] Add comprehension handling (list/dict/set comps create implicit scopes)
- [ ] Add decorator handling (already mentioned in doc but verify)
- [ ] Add `with` statement, `try`/`except`, `assert`, `raise`, `yield`
- [ ] Write comparison tests: run both builders on 20+ Python fixtures, assert CPG node/edge counts match or tree-sitter exceeds
- [ ] Run apex-cpg test suite with `--features treesitter`, all tests pass

### Task 1.2 -- security-detect crew: tree-sitter JavaScript CPG builder
**Files:** new `crates/apex-cpg/src/ts_javascript.rs`, `crates/apex-cpg/Cargo.toml`
**Effort:** 3-4 days
**Context:** No JS tree-sitter builder exists. Must add `tree-sitter-javascript` crate as optional dep behind `treesitter` feature.
- [ ] Add `tree-sitter-javascript` to Cargo.toml under `treesitter` feature
- [ ] Implement `TreeSitterJavaScriptCpgBuilder` following `ts_python.rs` pattern
- [ ] Handle: function declarations, arrow functions, class methods, `try`/`catch`, `switch`/`case`, template literals, destructuring, `async`/`await`
- [ ] Write tests with JS fixtures covering nested callbacks, promise chains, class hierarchies
- [ ] Compare against regex-based JS CPG builder (if one exists in `builder.rs`)

### Task 1.3 -- security-detect crew: tree-sitter Go CPG builder
**Files:** new `crates/apex-cpg/src/ts_go.rs`, `crates/apex-cpg/Cargo.toml`
**Effort:** 3-4 days
**Context:** No Go tree-sitter builder exists. Must add `tree-sitter-go` crate.
- [ ] Add `tree-sitter-go` to Cargo.toml under `treesitter` feature
- [ ] Implement `TreeSitterGoCpgBuilder`
- [ ] Handle: func declarations, methods with receivers, goroutine calls, defer, select/case, type switches, multiple return values, interface assertions
- [ ] Write tests with Go fixtures covering goroutines, channels, interface dispatch
- [ ] Compare against regex-based Go CPG builder

### Task 1.4 -- security-detect crew: tree-sitter integration and default swap
**Files:** `crates/apex-cpg/src/lib.rs`, `crates/apex-cpg/src/builder.rs`, `crates/apex-detect/src/pipeline.rs`
**Effort:** 2 days
**Depends on:** 1.1, 1.2, 1.3
- [ ] Add `get_builder(language, use_treesitter)` factory function that returns tree-sitter builder when feature is enabled, regex builder otherwise
- [ ] Wire factory into `DetectorPipeline` and taint analysis entry points
- [ ] Run full apex-detect test suite (361+ tests) with `--features treesitter`
- [ ] Benchmark: measure CPG build time for 10 real-world files per language (regex vs tree-sitter)
- [ ] Document accuracy comparison in a brief markdown note

---

## Phase 2: Core Mechanism Upgrades

**Duration estimate:** 3-4 weeks.
**Parallelism:** All tasks within Phase 2 are independent across crews. Tasks 2.1-2.2 (security-detect) depend on Phase 1 completion. All others can start immediately.

### Task 2.1 -- security-detect crew: LLM triage on CPG slices (noise reduction)
**Files:** `crates/apex-cpg/src/taint_triage.rs`, `crates/apex-detect/src/pipeline.rs`, new `crates/apex-detect/src/llm_triage.rs`
**Effort:** 5-7 days
**Depends on:** Phase 1 (needs accurate CPG for meaningful slices)
**Context:** Current noise rate is 84%. Research shows LLM triage on CPG slices achieves 15-40% F1 improvement (LLMxCPG, USENIX Security 2025). APEX already has `taint_triage.rs` with a scoring model.
- [ ] Implement CPG slice extraction: given a finding (source node, sink node), extract the minimal code slice by following CPG edges (target: 67-91% code reduction per LLMxCPG)
- [ ] Define `LlmTriageRequest` struct: finding metadata + CPG slice text + question template
- [ ] Implement `LlmTriageResponse` parsing: exploitable/not-exploitable/uncertain with confidence
- [ ] Add triage as optional Phase 3 in `DetectorPipeline`: pattern scan -> CPG taint validation -> LLM triage
- [ ] Gate behind `--llm-triage` flag (off by default, requires LLM API key)
- [ ] Write tests with known TP/FP fixtures, assert FP rate drops below 30% on test set
- [ ] Cost tracking: log token usage per finding for cost estimation

### Task 2.2 -- security-detect crew: CPG-backed finding validation
**Files:** `crates/apex-detect/src/pipeline.rs`, `crates/apex-cpg/src/taint.rs`
**Effort:** 3-4 days
**Depends on:** Phase 1 (tree-sitter CPG for accuracy)
- [ ] For each pattern-match finding with identifiable source/sink, validate via CPG taint analysis
- [ ] Findings without taint flow: demote severity to "info"
- [ ] Findings with taint flow: promote confidence, annotate with taint path
- [ ] Add `--validate-findings` flag (default on when tree-sitter CPG available)
- [ ] Write tests: known FP patterns (e.g., `subprocess.run` with hardcoded string) should be demoted

### Task 2.3 -- exploration crew: Bitwuzla solver backend
**Files:** new `crates/apex-symbolic/src/bitwuzla.rs`, `crates/apex-symbolic/Cargo.toml`, `crates/apex-symbolic/src/portfolio.rs`
**Effort:** 3-5 days
**Context:** Solver trait already exists. Bitwuzla is 2.8-5.1x faster than Z3 on QfAbv (bitvector) constraints. `bitwuzla-sys` crate exists on crates.io.
- [ ] Add `bitwuzla-solver` feature flag to apex-symbolic/Cargo.toml
- [ ] Implement `BitwuzlaSolver` struct implementing `Solver` trait
- [ ] Map `SolverLogic::QfAbv` to Bitwuzla, others fall through to Z3
- [ ] Update `PortfolioSolver` to insert Bitwuzla before Z3 when feature enabled
- [ ] Write solver tests: known SAT/UNSAT constraint sets, compare results against Z3
- [ ] Benchmark: measure solve time for 50 constraint sets from apex-symbolic test fixtures

### Task 2.4 -- exploration crew: Ensemble fuzzing (parallel strategies with shared corpus)
**Files:** `crates/apex-agent/src/orchestrator.rs`, `crates/apex-agent/src/ensemble.rs`, `crates/apex-fuzz/src/corpus.rs`
**Effort:** 5-7 days
**Context:** Currently strategies run in parallel within each iteration (`futures::join_all`) but the loop is synchronous. EnsembleSync exists with Mutex-based buffer. Research (EnFuzz, KRAKEN) shows 20-40% coverage improvement from true ensemble parallelism.
- [ ] Refactor `Corpus` to use `Arc<DashMap>` or `Arc<RwLock>` for concurrent access (currently `Mutex<Corpus>` in FuzzStrategy)
- [ ] Add background fuzzer task: spawn as `tokio::spawn` that runs FuzzStrategy continuously, depositing interesting seeds into EnsembleSync
- [ ] LLM-based strategies (SeedMind, LlmSynthesizer) continue in the synchronous loop
- [ ] Background fuzzer reads from shared corpus on sync interval
- [ ] Add `--ensemble` flag to enable parallel mode (default off initially)
- [ ] Write integration test: verify both sync and async paths find coverage, no data races
- [ ] Measure: compare coverage over time with and without ensemble mode on a benchmark target

### Task 2.5 -- foundation crew: Differential coverage (`apex run --diff`)
**Files:** `crates/apex-coverage/src/lib.rs`, new `crates/apex-coverage/src/diff.rs`, `crates/apex-cli/src/`
**Effort:** 3-4 days
**Context:** Research confirms this is a UX win, not a technical coverage change. Implementation: intersect coverage data with `git diff --unified=0`.
- [ ] Implement `DiffFilter` struct that takes a git diff and returns set of (file, line_range) tuples
- [ ] Shell out to `git diff --unified=0 <ref>` to get changed lines
- [ ] Filter coverage gap report to only include gaps in changed lines
- [ ] Add `--diff <ref>` flag to `apex run` (e.g., `apex run --diff HEAD~1`)
- [ ] Add `apex diff` subcommand as shorthand for `apex run --diff HEAD~1`
- [ ] Write tests: mock git diff output, verify filtering is correct
- [ ] Handle edge cases: new files (all lines are "changed"), deleted files (exclude), renamed files

### Task 2.6 -- foundation crew: MC/DC coverage mode
**Files:** `crates/apex-coverage/src/lib.rs`, `crates/apex-coverage/src/oracle.rs`, `crates/apex-instrument/src/rust_cov.rs`, `crates/apex-cli/src/`
**Effort:** 4-5 days
**Context:** LLVM 18+ supports MC/DC via `-fcoverage-mcdc`. Rust nightly has `-Cinstrument-coverage=mcdc`. Differentiator for DO-178C, ISO 26262.
- [ ] Add `CoverageMode` enum: `Line`, `Branch`, `Mcdc` to apex-core types
- [ ] Update `RustCovInstrumentor` to pass `-Cinstrument-coverage=mcdc` when MC/DC mode selected
- [ ] Update `CCoverageInstrumentor` to pass `-fcoverage-mcdc` to clang
- [ ] Parse MC/DC data from `llvm-cov export --format=json` output (the `mcdc` section)
- [ ] Add MC/DC metrics to gap report: condition coverage percentage, independence pairs
- [ ] Add `--mcdc` flag to `apex run`
- [ ] Write tests: compile a Rust fixture with MC/DC, verify condition-level data appears
- [ ] Document limitation: requires LLVM 18+, Rust nightly for Rust targets

### Task 2.7 -- runtime crew: seccomp-bpf sandbox (Linux)
**Files:** new `crates/apex-sandbox/src/seccomp.rs`, `crates/apex-sandbox/src/process.rs`, `crates/apex-sandbox/Cargo.toml`
**Effort:** 2-3 days
**Context:** Near-zero overhead syscall filtering. ~50 lines using `seccompiler` crate. Blocks dangerous syscalls while allowing coverage SHM.
- [ ] Add `seccompiler` as optional dependency behind `seccomp` feature flag
- [ ] Implement `SeccompFilter::apply()` that installs BPF filter before exec
- [ ] Allow list: read, write, open, close, mmap, brk, exit_group, clock_gettime, shm_open, fstat, lseek, rt_sigaction, rt_sigprocmask
- [ ] Block list: execve (except initial exec), ptrace, mount, reboot, kexec_load
- [ ] Wire into `ProcessSandbox::run_seed()` -- apply filter to child process
- [ ] Write test: verify blocked syscalls cause SIGSYS, allowed syscalls work
- [ ] Gate behind `cfg(target_os = "linux")`

### Task 2.8 -- runtime crew: sandbox-exec profile (macOS)
**Files:** new `crates/apex-sandbox/src/macos_sandbox.rs`, `crates/apex-sandbox/src/process.rs`
**Effort:** 2-3 days
**Context:** macOS sandbox-exec with SBPL profile. ~30 lines. Deny network, restrict file writes.
- [ ] Write SBPL profile: deny network-*, deny file-write-* except `/tmp` and target temp dir, allow file-read-*
- [ ] Implement `MacosSandbox::wrap_command()` that prepends `sandbox-exec -f <profile>` to command
- [ ] Wire into `ProcessSandbox` on macOS
- [ ] Write test: verify network access is blocked, file reads work, temp writes work
- [ ] Gate behind `cfg(target_os = "macos")`

### Task 2.9 -- runtime crew: Dynamic call graph collection
**Files:** `crates/apex-sandbox/src/python.rs`, `crates/apex-sandbox/src/javascript.rs`, `crates/apex-instrument/src/python.rs`
**Effort:** 3-4 days
**Context:** Piggyback on test execution. For Python, use `sys.settrace` to record caller/callee pairs. Merge with static CPG call graph.
- [ ] Python: add call edge recording to coverage shim via `sys.settrace` 'call' events
- [ ] Output format: JSON lines `{"caller": "mod.func", "callee": "mod2.func2"}` to a sidecar file
- [ ] JavaScript: use `--trace-function-calls` or equivalent V8 flag if available, or instrument with tree-sitter
- [ ] Add `--collect-callgraph` flag to apex run
- [ ] Implement merger: union of static CPG edges and dynamic edges
- [ ] Write test: run a Python fixture, verify dynamic call edges are captured
- [ ] Feed merged graph into taint analysis for improved precision

---

## Phase 3: Engine Enhancements

**Duration estimate:** 2-3 weeks.
**Parallelism:** All tasks are independent across crews.

### Task 3.1 -- foundation crew: Incremental coverage cache
**Files:** new `crates/apex-coverage/src/cache.rs`, `crates/apex-coverage/src/oracle.rs`
**Effort:** 3-4 days
**Context:** Hash each source file. Only re-instrument changed files. Merge cached coverage for unchanged files. Expected: 1.86x-8.2x speedup for localized changes (iJaCoCo research).
- [ ] Implement `CoverageCache` struct backed by `.apex/cache/coverage/` directory
- [ ] Per-file entry: `{file_hash, coverage_data, timestamp}`
- [ ] On `apex run`: hash source files, load cached coverage for unchanged files, only re-instrument changed files
- [ ] Merge strategy: cached data + fresh data = full report
- [ ] Cache invalidation: clear entry when file hash changes, clear all on `--no-cache`
- [ ] Write tests: run twice, verify second run is faster and produces same results
- [ ] Add `--no-cache` flag to force full re-instrumentation

### Task 3.2 -- exploration crew: SymCC concolic backend (C/Rust)
**Files:** new `crates/apex-concolic/src/symcc.rs`, `crates/apex-concolic/Cargo.toml`
**Effort:** 5-7 days
**Context:** SymCC is 10-100x faster than interpretive concolic for compiled targets. LLVM compiler pass that injects symbolic tracking. Feature-flagged because it requires LLVM.
- [ ] Add `symcc-backend` feature flag
- [ ] Implement `SymCCBackend` that: (a) compiles target with SymCC pass, (b) runs instrumented binary, (c) collects constraint logs, (d) feeds constraints to solver
- [ ] Falls back to StaticConcolicStrategy for non-C/Rust targets
- [ ] Integrate with PortfolioSolver for constraint solving
- [ ] Write tests: verify constraint extraction on a C fixture with known branches
- [ ] Benchmark: compare branch coverage with StaticConcolicStrategy vs SymCC on a C target
- [ ] Document: requires SymCC installed, only for C/C++/Rust targets

### Task 3.3 -- intelligence crew: CPG-informed synthesis prompts
**Files:** `crates/apex-synth/src/strategy.rs`, `crates/apex-synth/src/llm.rs`
**Effort:** 3-4 days
**Depends on:** Phase 1 (tree-sitter CPG)
**Context:** Current prompts include source segment and line numbers. Research (LLMxCPG) shows CPG context improves synthesis. Add data flow and call graph context.
- [ ] When CPG is available, extract: (a) variables flowing to uncovered branch condition, (b) functions called on the path, (c) constraint values needed to reach the branch
- [ ] Add CPG context section to synthesis prompts (after source code section)
- [ ] Implement `CpgPromptEnricher` that queries CPG for a given gap and produces context text
- [ ] A/B test: compare synthesis success rate with and without CPG context on 50 gaps
- [ ] Fall back gracefully when CPG not available (no tree-sitter feature)

### Task 3.4 -- runtime crew: tree-sitter instrumentation (unified probes)
**Files:** `crates/apex-instrument/src/`, new `crates/apex-instrument/src/treesitter.rs`
**Effort:** 5-7 days
**Context:** Use tree-sitter to insert coverage probes at statement/branch boundaries for interpreted languages. Replaces per-language instrumentation (coverage.py, Istanbul).
- [ ] Implement `TreeSitterInstrumentor` generic over language
- [ ] Walk CST, identify statement and branch nodes, insert `__apex_cov(file_id, probe_id)` calls
- [ ] Write instrumented source to temp directory
- [ ] Implement probe runtime: Python (`__apex_cov` as a no-cost function writing to shared array), JS (similar)
- [ ] Map probe IDs back to source locations for reporting
- [ ] Write tests: instrument a Python file, run it, verify probe hits are recorded
- [ ] Compare accuracy with coverage.py on 10 Python projects

---

## Phase 4: Polish and Integration

**Duration estimate:** 1-2 weeks.
**Parallelism:** All tasks independent.

### Task 4.1 -- platform crew: Coverage format import/export
**Files:** `crates/apex-instrument/src/import.rs`, `crates/apex-instrument/src/lcov_export.rs`, `crates/apex-cli/src/`
**Effort:** 2-3 days
**Context:** Partial implementation exists (import.rs, lcov_export.rs). Need to complete and wire to CLI.
- [ ] Complete LCOV import parser (if not already functional)
- [ ] Add Cobertura XML import parser
- [ ] Add JaCoCo XML import parser
- [ ] Add `apex import --format <lcov|cobertura|jacoco> <file>` subcommand
- [ ] Add `apex export --format <lcov|cobertura> <file>` subcommand
- [ ] Write round-trip tests: import LCOV, export LCOV, compare

### Task 4.2 -- exploration crew: Per-branch seed archive for orchestrator
**Files:** `crates/apex-agent/src/orchestrator.rs`, new `crates/apex-agent/src/seed_archive.rs`
**Effort:** 2-3 days
**Context:** MIO algorithm insight -- maintain per-branch best seed (closest heuristic value). Enables targeted driller escalation.
- [ ] Implement `SeedArchive` struct: `HashMap<BranchId, (InputSeed, f64)>` where f64 is heuristic distance
- [ ] On each execution result, update archive if new seed is closer to any branch
- [ ] When driller escalation triggers, use archived seed for the target branch instead of random corpus sample
- [ ] Write tests: verify archive tracks closest seeds correctly
- [ ] Wire into `AgentCluster` orchestrator loop

### Task 4.3 -- security-detect crew: Datalog-style declarative detection rules
**Files:** `crates/apex-cpg/src/taint_rules.rs`, new `crates/apex-cpg/src/rule_compiler.rs`
**Effort:** 3-4 days
**Context:** Instead of full Souffle, adopt declarative rule pattern in YAML. Compile to existing BFS engine. Gets 80% of Datalog benefits without dependency.
- [ ] Define YAML schema for taint rules: sources, sinks, sanitizers, propagators
- [ ] Implement `RuleCompiler` that reads YAML and produces `TaintRuleSet`
- [ ] Bundle default rules for Python, JS, Go as YAML files in `crates/apex-cpg/rules/`
- [ ] Allow user-defined rules via `--rules <path>` flag
- [ ] Write tests: define a custom rule, verify it detects the expected taint flow
- [ ] Migrate 5+ existing hardcoded taint rules to YAML format as proof of concept

### Task 4.4 -- platform crew: CLI integration for new features
**Files:** `crates/apex-cli/src/`
**Effort:** 2-3 days
**Depends on:** Tasks from all prior phases
- [ ] Wire `--diff <ref>` flag
- [ ] Wire `--mcdc` flag
- [ ] Wire `--ensemble` flag
- [ ] Wire `--llm-triage` flag
- [ ] Wire `--collect-callgraph` flag
- [ ] Wire `--validate-findings` flag
- [ ] Wire `--no-cache` flag
- [ ] Update `--help` text for all new flags
- [ ] Write integration tests for new CLI flags (at minimum: parse without error)

---

## Dependency Graph

```
Phase 1 (CPG Foundation)
  1.1 ts_python ──┐
  1.2 ts_javascript┼──> 1.4 integration ──> 2.1 LLM triage
  1.3 ts_go ──────┘                    ──> 2.2 CPG validation
                                       ──> 3.3 CPG synthesis prompts

Phase 2 (Core Upgrades) -- all independent of each other
  2.3 Bitwuzla        (exploration, no deps)
  2.4 Ensemble fuzzing (exploration/intelligence, no deps)
  2.5 Diff coverage    (foundation, no deps)
  2.6 MC/DC            (foundation/runtime, no deps)
  2.7 seccomp-bpf      (runtime, no deps)
  2.8 sandbox-exec     (runtime, no deps)
  2.9 Dynamic callgraph(runtime, no deps)

Phase 3 (Engine Enhancements) -- all independent
  3.1 Incremental cache (foundation, no deps)
  3.2 SymCC backend     (exploration, no deps)
  3.3 CPG synthesis     (intelligence, depends on Phase 1)
  3.4 TS instrumentation(runtime, no deps)

Phase 4 (Polish) -- depends on prior phases
  4.1 Import/export     (platform, no deps)
  4.2 Seed archive      (exploration, no deps)
  4.3 Declarative rules (security-detect, no deps)
  4.4 CLI integration   (platform, depends on all)
```

---

## Crew Assignment Summary

| Crew | Tasks | Total Effort |
|------|-------|-------------|
| **security-detect** | 1.1, 1.2, 1.3, 1.4, 2.1, 2.2, 4.3 | ~22-30 days |
| **exploration** | 2.3, 2.4, 3.2, 4.2 | ~15-22 days |
| **foundation** | 2.5, 2.6, 3.1 | ~10-13 days |
| **runtime** | 2.7, 2.8, 2.9, 3.4 | ~12-17 days |
| **intelligence** | 3.3 | ~3-4 days |
| **platform** | 4.1, 4.4 | ~4-6 days |
| **mcp-integration** | (none in this plan -- MCP tool definitions update after CLI flags land) | 0 |

**Total estimated effort:** ~66-92 person-days across 7 crews.

---

## Prioritization for Parallel Execution

If running with limited crew capacity, execute in this order:

**Must-do (P0):** Tasks 1.1-1.4, 2.1, 2.2 -- tree-sitter CPG + LLM triage. This is the single highest-impact change (84% noise -> ~15%). Everything in the detection pipeline improves when the CPG is accurate.

**Should-do (P1):** Tasks 2.3, 2.4, 2.5, 2.7, 2.8 -- Bitwuzla, ensemble fuzzing, diff coverage, sandbox hardening. These are independently valuable and can ship incrementally.

**Nice-to-have (P2):** Tasks 2.6, 2.9, 3.1, 3.3, 3.4 -- MC/DC, dynamic callgraph, incremental cache, CPG synthesis, TS instrumentation. Each improves a specific vertical.

**Defer (P3):** Tasks 3.2, 4.1, 4.2, 4.3, 4.4 -- SymCC, import/export, seed archive, declarative rules, CLI wiring. These depend on the above or have lower standalone value.

---

## Risk Register

| Risk | Impact | Mitigation |
|------|--------|-----------|
| tree-sitter grammar version mismatch | Build failures | Pin exact versions in Cargo.toml, test in CI |
| Bitwuzla sys crate doesn't compile on macOS | Feature unusable on dev machines | Feature-gate, CI tests on Linux only, cross-compile |
| seccomp filter too restrictive | Breaks language runtimes (Python needs many syscalls) | Start with audit mode (log, don't kill), then tighten |
| LLM triage cost at scale | Expensive for large codebases | Batch findings, use cheapest model first, cache responses |
| Ensemble mode introduces data races | Flaky tests, corrupted corpus | Use DashMap/RwLock, run under TSAN in CI |
| SymCC requires LLVM toolchain at build time | Binary size increase, build complexity | Strict feature gating, no default compilation |

---

## Verification Plan

After each phase:
1. `cargo check --workspace` (compile)
2. `cargo nextest run --workspace` (all tests pass)
3. `cargo clippy --workspace -- -D warnings` (no new warnings)
4. `cargo fmt --check` (formatting)
5. Phase-specific benchmarks (noted per task)
6. CHANGELOG.md updated
