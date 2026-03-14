# APEX Research Integration — Master Implementation Plan

**Date:** 2026-03-14
**Scope:** 49 techniques from 46 papers across 5 subsystems
**Subsystem plans:** `docs/research/plans/apex-{synth,fuzz,agent,coverage-symbolic,security}-plan.md`

---

## Cross-Subsystem Dependency Map

```
                         apex-core
                   ┌───── LlmClient trait ─────┐
                   │   ExecutionResult ext.     │
                   │   BranchCandidate ext.     │
                   │                            │
         ┌────────┼────────┬───────────┬────────┤
         │        │        │           │        │
         v        v        v           v        v
    apex-synth  apex-fuzz  apex-agent  apex-cov  apex-security
    (8 tech)    (10 tech)  (8 tech)    (12 tech) (11 tech)
         │                    │           │        │
         │                    │           │        │
         │                    ├───────────┘        │
         │                    │ MutationScore      │
         │                    │ OracleGap          │
         └────────────────────┴────────────────────┘
                              │
                           apex-cpg
                   (shared by agent, security,
                    synth slicing)
```

**Shared prerequisites (must come first):**
1. `LlmClient` trait in `apex-core/src/llm.rs` — used by synth, fuzz, coverage, security
2. `ExecutionResult` extensions — `input: Vec<u8>`, semantic signals
3. `MutationOperator` / `MutationKind` types in `apex-coverage` — used by agent, coverage

---

## Phase 0 — Foundation (Week 1)

Shared infrastructure that unblocks everything else. No feature work yet.

| # | Task | Crate | Blocks | Est. |
|---|------|-------|--------|------|
| 0.1 | `LlmClient` trait + `AnthropicClient` impl | apex-core | synth, fuzz, cov, security | 1d |
| 0.2 | Fix `observe()` corpus feedback (P1) | apex-fuzz | all fuzz techniques | 0.5d |
| 0.3 | Fix `mutate_with_index()` on MOptScheduler (P2) | apex-fuzz | all fuzz techniques | 0.5d |
| 0.4 | Extend `ExecutionResult` with `input` field | apex-core | fuzz corpus, semantic | 0.5d |
| 0.5 | `MutationOperator` + `MutationKind` types | apex-coverage | oracle gap, ACH, adversarial | 1d |
| 0.6 | CPG wiring into apex-detect pipeline | apex-detect | all security techniques | 1d |
| 0.7 | `TaintSpecStore` (runtime-extensible) | apex-cpg | IRIS, taint flow detector | 1d |

**Gate:** `cargo test --workspace` passes. All existing behavior unchanged.

---

## Phase 1 — Quick Wins (Weeks 2-3)

Low-complexity, no cross-subsystem dependencies. Each is independent — all parallelizable.

### 1A: Coverage & Index (apex-coverage, apex-index)

| # | Technique | Paper | New Files | Est. |
|---|-----------|-------|-----------|------|
| 1.1 | Oracle Gap Metric | Mind the Gap | `mutation.rs`, `oracle_gap.rs` | 3d |
| 1.2 | Flaky Detection (coverage instability) | FlaKat | `flaky.rs` in apex-index | 2d |
| 1.3 | Semantic Feedback Signals | arXiv:2511.03995 | `semantic.rs` in apex-coverage | 2d |

### 1B: Fuzzing (apex-fuzz)

| # | Technique | Paper | New Files | Est. |
|---|-----------|-------|-----------|------|
| 1.4 | Thompson Sampling Seed Scheduling | T-Scheduler | `thompson.rs` | 2d |
| 1.5 | DEzzer Mutation Scheduling | DEzzer/JSS 2025 | `de_scheduler.rs` | 3d |
| 1.6 | Semantic Feedback in Fuzzer | arXiv:2511.03995 | `semantic_feedback.rs` | 2d |

### 1C: Synthesis (apex-synth)

| # | Technique | Paper | New Files | Est. |
|---|-----------|-------|-----------|------|
| 1.7 | Core abstractions: `PromptStrategy` trait, `GapHistory`, `GapClassifier` | — | `strategy.rs`, `classify.rs` | 2d |
| 1.8 | Code Elimination from Prompts | Xu 2026 | `eliminate.rs` | 2d |
| 1.9 | Extract CoverUp into `CoverUpStrategy` | — | `coverup.rs` | 1d |

### 1D: Agent (apex-agent)

| # | Technique | Paper | New Files | Est. |
|---|-----------|-------|-----------|------|
| 1.10 | `BranchClassifier` + S2F categories | S2F | `classifier.rs` | 2d |
| 1.11 | Thompson Strategy Bandit | T-Scheduler ext. | `bandit.rs` | 2d |
| 1.12 | `MutationGuide` (oracle gap tracking) | Meta ACH | `mutation_guide.rs` | 2d |

### 1E: Security (apex-cpg, apex-detect)

| # | Technique | Paper | New Files | Est. |
|---|-----------|-------|-----------|------|
| 1.13 | `find_taint_flows_with_store()` | IRIS | modify `taint.rs` | 1d |
| 1.14 | Type-Based Taint Tracking | arXiv:2504.18529 | `type_taint.rs` | 2d |
| 1.15 | ML Taint Triage (scoring) | arXiv:2510.20739 | `taint_triage.rs` | 2d |

**Gate:** `cargo test --workspace` passes. All Phase 1 items have unit tests.

---

## Phase 2 — Synthesis Pipeline Upgrade (Weeks 3-5)

Depends on Phase 1C abstractions.

### 2A: Prompt Strategies (apex-synth)

| # | Technique | Paper | New Files | Depends On | Est. |
|---|-----------|-------|-----------|------------|------|
| 2.1 | Counter-Example Feedback | TELPA | `counter_example.rs` | 1.7 (GapHistory) | 3d |
| 2.2 | Co-Evolutionary Gen/Repair | TestART/YATE | `repair.rs` | 1.7 (strategy trait) | 3d |
| 2.3 | Method Slicing | HITS | `slice.rs` | 1.7 + CFG data | 4d |
| 2.4 | Path-Enumeration Prompting | SymPrompt | `path_enum.rs` | 1.7 + CFG data | 4d |
| 2.5 | NL Constraint Fallback | PALM | `nl_constraint.rs` | 1.7 (GapHistory) | 3d |
| 2.6 | `SynthPipeline` orchestrator | — | `pipeline.rs` | all 2.1-2.5 | 3d |
| 2.7 | Agentic File-Level Gen | TestForge | `agentic.rs` | 2.6 | 5d |

### 2B: Fuzz LLM Foundation (apex-fuzz)

| # | Technique | Paper | New Files | Depends On | Est. |
|---|-----------|-------|-----------|------------|------|
| 2.8 | LLAMAFUZZ (LLM mutations) | LLAMAFUZZ | `llm_mutator.rs` | 0.1 (LlmClient) | 4d |
| 2.9 | Fuzz4All (autoprompting) | Fuzz4All | `autoprompt.rs` | 0.1 + 1.4 | 3d |
| 2.10 | Grammar-based fuzzing | FANDANGO-RS | `grammar_mutator.rs` | existing grammar.rs | 4d |

### 2C: Agent Composites (apex-agent)

| # | Technique | Paper | New Files | Depends On | Est. |
|---|-----------|-------|-----------|------------|------|
| 2.11 | DeepGo Transition Table | DeepGo | `transition.rs` | 1.10 | 3d |
| 2.12 | Graphuzz BFS Scorer | Graphuzz | `scorer.rs` | 1.10 | 3d |
| 2.13 | Trace-Guided DGF Filter | arXiv:2510.23101 | `trace_filter.rs` | 1.10 | 2d |
| 2.14 | Fitness Landscape Analysis | arXiv:2502.00169 | `landscape.rs` | 1.12 | 3d |

### 2D: Security Detectors (apex-detect)

| # | Technique | Paper | New Files | Depends On | Est. |
|---|-----------|-------|-----------|------------|------|
| 2.15 | LLM-Inferred Taint Specs | IRIS | `llm_spec_infer.rs` | 0.6, 0.7 | 3d |
| 2.16 | CPG Backward Slicing | LLMxCPG | `slice.rs` (apex-cpg) | 0.6 | 2d |
| 2.17 | LLM Taint Validator | LLMxCPG | `llm_taint_validator.rs` | 2.16 | 3d |
| 2.18 | SAST FP Reduction | SAST-Genius | `triage.rs` | 0.1 | 3d |
| 2.19 | `SpecMiner` trait | Caruca et al. | `spec.rs` (apex-cpg) | — | 1d |

**Gate:** `cargo test --workspace` passes. Synth pipeline routes gaps correctly.

---

## Phase 3 — Analysis & Solver Upgrades (Weeks 5-7)

Deeper changes to coverage, symbolic, and security pipelines.

### 3A: Coverage Intelligence (apex-coverage, apex-index)

| # | Technique | Paper | New Files | Depends On | Est. |
|---|-----------|-------|-----------|------------|------|
| 3.1 | Metamorphic Adequacy (mutation-first) | Meta ACH | extend `mutation.rs` | 1.1 | 2d |
| 3.2 | Rank Aggregation TCP | arXiv:2412.00015 | `prioritize.rs` | 1.1, 1.3 | 3d |
| 3.3 | Slice-Based Change Impact | arXiv:2508.19056 | `change_impact.rs` | 3.2 | 2d |
| 3.4 | Dead Code + LLM Validation | DCE-LLM | `dead_code.rs` | 0.1, 1.2 | 4d |
| 3.5 | LLM Flaky Test Repair | FlakyFix | `flaky_repair.rs` | 0.1, 1.2 | 4d |

### 3B: Solver Upgrades (apex-symbolic, apex-concolic)

| # | Technique | Paper | New Files | Depends On | Est. |
|---|-----------|-------|-----------|------------|------|
| 3.6 | Diverse SMT Solutions | PanSampler | `diversity.rs` | — | 4d |
| 3.7 | LLM as Concolic Solver | Cottontail | `llm_solver.rs` | 0.1 | 5d |
| 3.8 | Fitness Landscape Adaptation | arXiv:2502.00169 | `landscape.rs` | 1.1 | 3d |
| 3.9 | AutoBug Path Decomposition | AutoBug | `path_decomp.rs` | 3.7 | 5d |

### 3C: Fuzz Advanced (apex-fuzz)

| # | Technique | Paper | New Files | Depends On | Est. |
|---|-----------|-------|-----------|------------|------|
| 3.10 | SeedMind (LLM seed generators) | SeedMind | `seedmind.rs` | 0.1, 2.8 | 5d |
| 3.11 | HGFuzzer (directed greybox) | HGFuzzer | `hgfuzzer.rs` | 0.1, 2.8 | 4d |
| 3.12 | FOX (stochastic control) | FOX | `control.rs` | 1.4, 1.5 | 5d |

### 3D: Security Spec Mining (apex-detect, apex-sandbox)

| # | Technique | Paper | New Files | Depends On | Est. |
|---|-----------|-------|-----------|------------|------|
| 3.13 | Syscall Spec Mining | Caruca | `spec_miner.rs`, `python_audit.rs` | 2.19 | 4d |
| 3.14 | Data Transform Spec Mining | Beyond Bools | `spec_mining.rs` (apex-index) | 2.19 | 3d |
| 3.15 | CEGAR Spec Mining | SmCon | `cegar.rs` | 3.13, 3.14 | 4d |
| 3.16 | DeepDFA Dataflow Features | DeepDFA | `deepdfa.rs` (apex-cpg) | 0.6 | 4d |

**Gate:** `cargo test --workspace` passes. Solver portfolio includes LLM backend.

---

## Phase 4 — Advanced Capabilities (Weeks 7-10)

Complex techniques, ML infrastructure, orchestrator integration.

### 4A: Agent Orchestrator (apex-agent)

| # | Technique | Paper | New Files | Depends On | Est. |
|---|-----------|-------|-----------|------------|------|
| 4.1 | S2F Router (replaces recommend_strategy) | S2F | `router.rs` | 1.10, 2.14 | 2d |
| 4.2 | Adversarial Test-Mutant Loop | AdverTest | `adversarial.rs` | 1.12, 2.2 | 5d |
| 4.3 | Wire all into `orchestrator.rs` | — | modify `orchestrator.rs` | all agent | 3d |

### 4B: ML Security Detectors (feature-gated)

| # | Technique | Paper | New Files | Feature Flag | Depends On | Est. |
|---|-----------|-------|-----------|-------------|------------|------|
| 4.4 | IPAG + HAGNN (GNN vuln detect) | IPAG/HAGNN | `ipag.rs`, `hagnn_detector.rs` | `gnn` | 3.16 | 5d |
| 4.5 | Vul-LMGNNs (dual encoder) | Vul-LMGNNs | `dual_encoder.rs` | `ml` | 4.4 | 4d |
| 4.6 | VulnDetectionPipeline orchestrator | — | `vuln_pipeline.rs` | — | all security | 3d |

### 4C: Binary Fuzzing (feature-gated)

| # | Technique | Paper | New Files | Feature Flag | Est. |
|---|-----------|-------|-----------|-------------|------|
| 4.7 | LibAFL QEMU Backend | BAR 2024 | `qemu_backend.rs` | `libafl-qemu` | 7d |

**Gate:** Full integration test suite. `cargo test --workspace` + `cargo test --workspace --features "gnn,ml"`.

---

## Summary

| Phase | Techniques | New Files | Est. LOC | Timeline |
|-------|-----------|-----------|----------|----------|
| 0 Foundation | 7 prereqs | 3 | ~300 | Week 1 |
| 1 Quick Wins | 15 techniques | 15 | ~2,500 | Weeks 2-3 |
| 2 Pipeline Upgrade | 19 techniques | 19 | ~5,000 | Weeks 3-5 |
| 3 Analysis & Solvers | 16 techniques | 16 | ~4,500 | Weeks 5-7 |
| 4 Advanced | 7 techniques | 7 | ~2,500 | Weeks 7-10 |
| **Total** | **49 + 7 prereqs** | **~60 new files** | **~14,800** | **~10 weeks** |

With 2-3 developers working in parallel (each subsystem is largely independent after Phase 0), this compresses to **5-6 weeks**.

---

## Feature Flag Strategy

| Flag | Techniques | Heavy Deps |
|------|-----------|------------|
| default | 38 techniques | none beyond existing |
| `gnn` | +3 (HAGNN, dual encoder, IPAG) | `ort` (ONNX Runtime) |
| `ml` | +3 (DeepDFA, Vul-LMGNNs, dual encoder) | `ort`, `tokenizers` |
| `libafl-qemu` | +1 (LibAFL QEMU binary fuzzing) | `libafl_qemu` (Linux-only) |
| LLM optional | 11 techniques degrade gracefully | `reqwest` (already present) |

All LLM-dependent techniques (IRIS, LLMxCPG, SAST triage, flaky repair, dead code, synth strategies, fuzz LLM mutations) work without LLM when `ANTHROPIC_API_KEY` is not set — they fall back to rule-based analysis or skip gracefully.

---

## Critical Path

```
Phase 0.1 (LlmClient) ──┬──> Phase 1 (all subsystems, parallel)
Phase 0.2-0.3 (fuzz fix) ┘          │
                                     ├──> Phase 2 (pipeline upgrades)
                                     │         │
                                     │         ├──> Phase 3 (solvers, spec mining)
                                     │         │         │
                                     │         │         └──> Phase 4 (orchestrator, ML)
                                     │         │
                                     └─────────┘
```

**Longest path:** 0.1 → 1.7 → 2.1-2.6 → 2.7 (agentic) = ~4 weeks
**Second longest:** 0.2 → 1.4 → 2.8 → 3.10/3.12 = ~4 weeks
**Parallel track:** 0.6 → 1.13-1.15 → 2.15-2.18 → 3.13-3.16 → 4.4-4.6 = ~5 weeks

---

## Test Strategy

Each technique must have:
1. **Unit tests** — pure function tests, no I/O, no LLM calls
2. **Mock tests** — LLM-dependent code tested with mock responses
3. **Integration tests** — end-to-end with real files but mocked LLM

Test files follow pattern: `crates/{crate}/tests/{technique}_test.rs`

LLM-dependent tests use `#[ignore]` attribute and run only when `APEX_LLM_TESTS=1` is set.

---

## How To Start

1. Read the subsystem plan for the technique you're implementing
2. Implement Phase 0 first (shared infrastructure)
3. Pick any Phase 1 technique — they're all independent
4. Follow the build sequence in the subsystem plan
5. Write tests before or alongside implementation
6. Run `cargo test --workspace` after each technique
