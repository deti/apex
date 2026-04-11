---
id: 01KNWE2Q9YZBAR140ZX5P36TQ5
title: "APEX Spec: Performance Test Generation (G-46)"
type: concept
tags: [apex, spec, performance, fuzzing, redos, complexity, cwe-400, cwe-1333]
links:
  - target: 01KNWE2QA0Z52H8VVFAMSA7KGA
    type: extends
  - target: 01KNWE2QA1E37T7GVKSM6EGXXR
    type: extends
  - target: 01KNWE2QA2KV1NN8QH32RA5EPA
    type: extends
  - target: 01KNWE2QA3FA96G8JKN733K0XP
    type: extends
  - target: 01KNWE2QA5VP0K80TMSABACKWT
    type: extends
  - target: 01KNWE2QA6QKE8Z152WX8D9XYB
    type: extends
  - target: 01KNWE2QA700K0Z2W0TWV087JZ
    type: extends
  - target: 01KNWE2QA8H1GKHCVNHYS5QW1F
    type: extends
  - target: 01KNWE2QAAKZH8GGZ172HZ9RHS
    type: extends
  - target: 01KNWE2QABV7943DKAXTARJHXA
    type: extends
  - target: 01KNWE2QACWYZJXRJ8QE78T043
    type: extends
  - target: 01KNWEGYB1B15QGYTRC374Z7DQ
    type: references
  - target: 01KNWEGYB3NXWFB6D4SV4DTD5X
    type: references
  - target: 01KNWEGYB6AVG1FV1EQVYW3K9Q
    type: references
  - target: 01KNWEGYB8807ET2427V3VCRJ3
    type: references
  - target: 01KNWEGYBB4AAEFYMR3Y29EZ49
    type: references
  - target: 01KNWEGYBDMW0V0CFVJ83A4B9J
    type: references
  - target: 01KNWGA5F4W852RG6C5FJCP204
    type: references
  - target: 01KNWGA5F7VA68B8ZPB6XR0RTE
    type: references
  - target: 01KNWGA5F9YTQ01C15QNFHQGPT
    type: references
  - target: 01KNWGA5FCBGJCSJJ3XPH1H1DG
    type: references
  - target: 01KNWGA5FEAC0QN3PK6CAYP7T8
    type: references
created: 2026-04-10
modified: 2026-04-10
source: docs/gaps/specs/performance-test-generation.md
---

# APEX Spec: Performance Test Generation (G-46)

Consolidated gap **G-46 (MEDIUM)** — "No performance test generation." This note summarises the full APEX internal spec that addresses the gap. The authoritative source is `docs/gaps/specs/performance-test-generation.md` inside the APEX repo.

## Problem Statement

APEX today generates only **functional** tests — tests that verify *what* a program computes, not *how efficiently* it computes it. This leaves an entire class of defects invisible:

- **Algorithmic complexity vulnerabilities** — inputs that trigger worst-case time or space.
- **Resource exhaustion** — memory leaks under load, file descriptor starvation, connection pool exhaustion.
- **Denial-of-service attack vectors** — catastrophic regex backtracking (ReDoS), hash collision attacks, XML bomb expansion.
- **Latency/throughput regressions** — code changes that silently breach SLOs.

CWE-400 (Uncontrolled Resource Consumption) entered the 2024 CWE Top 25 at rank 24. Google's SRE research reports ~70% of outages stem from changes to live systems rather than correctness bugs. Performance defects are a first-class, growing category.

## Proposed Capability

A new `apex perf` command plus pipeline additions that deliver:

1. **Worst-case input generation via resource-guided fuzzing** — swap the fuzzer's coverage-maximising feedback for a resource-maximising one (PerfFuzz-style per-edge counts or SlowFuzz-style total instruction count).
2. **Empirical complexity estimation** — run target functions across `n = 10, 100, 1K, 10K, 100K`, fit execution time/memory to O(1), O(n), O(n log n), O(n²), O(n³), O(2ⁿ) via least-squares, and classify each function with a confidence level.
3. **Static + dynamic ReDoS analysis** — scan regex literals for quantifier-nesting patterns, generate worst-case trigger strings, and verify super-linear slowdown dynamically.
4. **Resource profiling during ordinary test execution** — record wall-clock, CPU, peak memory, allocation counts alongside coverage; flag outliers.
5. **Configurable SLO assertions** — `parse: 100ms for 10KB` — APEX generates inputs at and beyond the boundary and verifies compliance.
6. **Performance regression detection in CI** — compare against a saved baseline, flag regressions exceeding (default) 2x.

## Measurable Outcomes

- Worst-case inputs for known-vulnerable benchmarks within 5 minutes of fuzzing.
- ≥80% ReDoS detection on a benchmark regex corpus.
- ≥70% complexity-class identification accuracy on functions of known complexity.
- 2x slowdown detection with <10% false positives.

## Bug Classes Caught

| Bug class | Example |
|---|---|
| Algorithmic complexity vulnerability | Naive string matcher O(n·m) as DoS vector |
| ReDoS (CWE-1333) | `^(a+)+$` exponential on `aaaaaaaa...b` |
| Resource exhaustion (CWE-400) | XML parser without entity depth limits → billion laughs |
| Memory leak under load | Error path allocates buffer, only success path frees |
| Hash collision DoS | HTTP header map with attacker-chosen colliding keys |
| Quadratic accumulation | `result += chunk` in a loop → O(n²) string building |
| Performance regression | Refactor removes memoization → O(n) becomes O(2ⁿ) |

## Theoretical Foundation (core references)

- **SlowFuzz** (Petsios, Zhao, Keromytis, Jana — CCS 2017) — first domain-independent resource-usage-guided evolutionary search. [arXiv:1708.08437](https://arxiv.org/abs/1708.08437)
- **PerfFuzz** (Lemieux, Padhye, Sen, Song — ISSTA 2018) — multi-dimensional per-edge performance feedback; 5–69x improvement over SlowFuzz on the hottest edge. [PDF](https://www.carolemieux.com/perffuzz-issta2018.pdf)
- **Crosby & Wallach** (USENIX Security 2003) — "Denial of Service via Algorithmic Complexity Attacks" — the paper that established the whole class.
- **HotFuzz** (Blair, Mambretti, Arshad et al. — NDSS 2020) — micro-fuzzing for Java algorithmic DoS.
- **Goldsmith, Aiken, Wilkerson** (ESEC/FSE 2007) — "Measuring Empirical Computational Complexity" — Trend Profiler, the template for empirical complexity estimation.
- **Davis, Coghlan, Servant, Lee** (ESEC/FSE 2018) — "The Impact of ReDoS in Practice" — 3.5% of 500K regexes super-linear.

## Competitive Landscape (as recorded in the spec)

| Tool | What it delivers | Gap vs APEX |
|---|---|---|
| PerfFuzz | Multi-dim perf fuzzing for C/C++ via AFL | C/C++ only, LLVM-instrumented, research prototype |
| SlowFuzz | Single-dim resource feedback | C/C++ only, superseded by PerfFuzz |
| HotFuzz | Java object micro-fuzzing for DoS | Java only, no CI integration |
| k6 / Gatling / JMeter | Load testing + latency measurement | No input generation, no complexity analysis, manual test authoring |
| vuln-regex-detector | Ensemble static+dynamic ReDoS | Regex only, no generalised complexity, no test-gen pipeline |
| Hypothesis | Property-based testing + deadlines | No specialised worst-case generation; deadline is a timeout only |

APEX's differentiation: combine worst-case generation (PerfFuzz), multi-language support, resource profiling, complexity estimation, and integration with the broader test-gen / security pipeline. The existing LibAFL + grammar-aware mutation infrastructure is the substrate; the primary change is substituting the coverage feedback with a resource-consumption feedback.

## Known Limitations

- **Noisy measurement** — wall-clock/CPU/memory are affected by system load, GC, JIT, caching. Requires statistical techniques (multiple runs, outlier filtering, CIs).
- **Undecidable in general** — empirical complexity estimation is a heuristic. Phase transitions (linear small, quadratic beyond a threshold) are particularly hard.
- **Hard-to-specify properties** — "should be fast" is not testable. Users must provide SLOs or rely on super-linear scaling heuristics.
- **Language-specific overhead** — Python `sys.settrace` ~10x, Go `pprof` ~5%, C/C++ `perf` <1%. Overhead itself distorts measurement.

## Constraints & Scope

- **In scope**: resource-guided fuzzing, static+dynamic ReDoS, empirical complexity estimation, resource profiling, SLO verification, CLI, standard Finding format.
- **Out of scope**: full load/stress testing with concurrency, real-time prod monitoring, distributed perf testing, JIT warmup/steady-state analysis, formal WCET.
- **Dependency notes**: no strict blockers — existing fuzz infra is the foundation. Benefits from F-08 (Complexity Metrics Suite) which feeds static hints about candidate functions.

## References (spec bibliography, verbatim)

1. Petsios et al., SlowFuzz — CCS 2017 — [arXiv:1708.08437](https://arxiv.org/abs/1708.08437)
2. Lemieux et al., PerfFuzz — ISSTA 2018 — [PDF](https://www.carolemieux.com/perffuzz-issta2018.pdf)
3. Blair et al., HotFuzz — NDSS 2020 — [DOI 10.14722/ndss.2020.24415](https://doi.org/10.14722/ndss.2020.24415)
4. Crosby, Wallach — USENIX Security 2003
5. Goldsmith, Aiken, Wilkerson — ESEC/FSE 2007 — [DOI 10.1145/1287624.1287681](https://doi.org/10.1145/1287624.1287681)
6. Davis et al. — ESEC/FSE 2018 — [DOI 10.1145/3236024.3236027](https://doi.org/10.1145/3236024.3236027)
7. MITRE CWE-400 — [cwe.mitre.org/data/definitions/400.html](https://cwe.mitre.org/data/definitions/400.html)
8. MITRE CWE-1333 — [cwe.mitre.org/data/definitions/1333.html](https://cwe.mitre.org/data/definitions/1333.html)
9. MITRE/CISA 2024 CWE Top 25 — [cwe.mitre.org/top25/archive/2024/2024_cwe_top25.html](https://cwe.mitre.org/top25/archive/2024/2024_cwe_top25.html)
