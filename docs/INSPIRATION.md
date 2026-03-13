# Mechanism Inspiration Sources

APEX integrates fundamental analysis mechanisms from established security and testing tools.
This document records what was adopted, from where, and how it maps to APEX's architecture.

## Branch Distance (EvoMaster / Korel)

**Source:** EvoMaster's `HeuristicsForJumps.java` + `TruthnessUtils`
**Paper:** Korel, "Automated Software Test Data Generation" (1990)

**Mechanism:** Continuous [0,1] fitness for branch conditions instead of binary covered/not-covered.
- `x == 42` with `x = 40` scores `1 - normalize(|40-42|)` = 0.33 instead of binary 0
- `normalize(d) = d / (d + 1)` — maps any distance to [0,1)

**APEX location:** `crates/apex-coverage/src/heuristic.rs`
**Integration:** `CoverageOracle.record_heuristic()` / `best_heuristic()` in `crates/apex-coverage/src/oracle.rs`

## Gradient Descent Constraint Solving (Angora)

**Source:** Angora's `fuzzer/src/search/gd.rs` + `grad.rs`
**Paper:** Chen & Chen, "Angora: Efficient Fuzzing by Principled Search" (S&P 2018)

**Mechanism:** Treat branch conditions as distance functions, compute partial derivatives
via finite differences (perturb input byte by +/-1, measure distance change), descend toward
zero distance. Solves numeric constraints 10-100x faster than SMT solvers.
- Exponential step size search: try 1, 2, 4, 8, ... until distance stops improving
- Falls back to Z3 for non-numeric / complex constraints

**APEX location:** `crates/apex-symbolic/src/gradient.rs`
**Integration:** `PortfolioSolver::with_gradient_first()` in `crates/apex-symbolic/src/portfolio.rs`

## Code Property Graph (Joern / ShiftLeft)

**Source:** Joern CPG schema + REACHING_DEF pass + backward reachability
**Paper:** Yamaguchi et al., "Modeling and Discovering Vulnerabilities with Code Property Graphs" (S&P 2014)

**Mechanism:** Unified graph combining AST + CFG + data-dependency (REACHING_DEF) edges.
Taint analysis via backward BFS from sinks following ReachingDef edges to sources.
- MOP (Meet-Over-all-Paths) iterative dataflow for reaching definitions
- Sanitizer nodes cut taint propagation during backward traversal
- Source/sink/sanitizer tables for Python security patterns

**APEX location:** `crates/apex-cpg/` (new crate)
- `src/lib.rs` — NodeKind, EdgeKind, Cpg graph structure
- `src/builder.rs` — Python source → CPG construction
- `src/reaching_def.rs` — iterative gen/kill fixpoint
- `src/taint.rs` — backward reachability + source/sink tables

## LLM-Guided Test Refinement (CoverUp)

**Source:** CoverUp's `improve_coverage()` loop + AST segment extraction
**Paper:** Pizzorno & Berger, "CoverUp: Coverage-Guided LLM-Based Test Generation" (2024)

**Mechanism:** Closed-loop generate-run-measure-refine cycle:
1. Extract code segment around uncovered branch (with line-number tags)
2. Prompt LLM to generate a test covering it
3. Run test, measure coverage
4. If error: feed error back to LLM, retry
5. If no coverage gain: feed "still missing lines X-Y" back, retry
6. Up to 3 attempts per gap

**APEX location:**
- `crates/apex-synth/src/llm.rs` — LlmSynthesizer with `fill_gap()` loop
- `crates/apex-synth/src/segment.rs` — `extract_segment()` + `clean_error_output()`

## Priority-Based Exploration (Owi + EvoMaster)

**Source:** Owi's `Prio` module + EvoMaster's `Archive.chooseTarget()`
**Paper:** Various (Owi: WASM symbolic execution; EvoMaster: REST API testing)

**Mechanism:** Composite priority for selecting which uncovered branch to focus on:
- **Rarity** (Owi): `1 / (hit_count + 1)` — prefer code reached by fewer inputs
- **Depth penalty** (Owi): `1 / ln(1 + depth)` — penalize deeply nested paths
- **Proximity** (EvoMaster): use branch distance heuristic as priority signal
- **Staleness bonus**: boost branches stuck without progress to rotate strategies

Strategy routing: high proximity → gradient solver, medium → fuzzer, low/stalled → LLM synthesis.

**APEX location:**
- `crates/apex-agent/src/priority.rs` — `target_priority()`, `recommend_strategy()`
- `crates/apex-agent/src/cache.rs` — `SolverCache` with negation inference (from Owi)

## Solver Caching with Negation Inference (Owi)

**Source:** Owi's solver cache
**Context:** WASM parallel symbolic execution engine

**Mechanism:** Cache SAT/UNSAT results keyed by constraint string.
Negation inference: if `(not C)` is cached as UNSAT, infer `C` is SAT without querying
the solver. Reduces redundant solver calls during exploration.

**APEX location:** `crates/apex-agent/src/cache.rs`

## CWE ID Mapping (Bearer / Industry Standard)

**Source:** Bearer's finding-to-CWE mapping pattern
**Standard:** MITRE CWE (Common Weakness Enumeration)

**Mechanism:** Every security finding carries `cwe_ids: Vec<u32>` for compliance reporting
(SOC2, HIPAA, PCI-DSS). Mapping table from detection category to CWE:

| Category | CWE |
|----------|-----|
| OS command injection | CWE-78 |
| XSS | CWE-79 |
| SQL injection | CWE-89 |
| Code injection (eval/exec) | CWE-94 |
| Buffer overflow | CWE-120 |
| Certificate validation | CWE-295 |
| Weak hash | CWE-328 |
| Deserialization | CWE-502 |
| Hardcoded credentials | CWE-798 |
| Path traversal | CWE-22 |

**APEX location:**
- `crates/apex-detect/src/finding.rs` — `cwe_ids` field on `Finding`
- `crates/apex-detect/src/detectors/security_pattern.rs` — `cwe` field on `SecurityPattern`
