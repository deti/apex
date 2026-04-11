---
id: 01KNWE2QA2KV1NN8QH32RA5EPA
title: Empirical Computational Complexity Estimation
type: concept
tags: [complexity, profiling, performance, goldsmith, trend-profiler]
links:
  - target: 01KNWE2Q9YZBAR140ZX5P36TQ5
    type: extends
  - target: 01KNWE2QA0Z52H8VVFAMSA7KGA
    type: related
  - target: 01KNWE2QA5VP0K80TMSABACKWT
    type: related
  - target: 01KNWE2QABV7943DKAXTARJHXA
    type: related
  - target: 01KNWE2QACWYZJXRJ8QE78T043
    type: related
  - target: 01KNWEGYBB4AAEFYMR3Y29EZ49
    type: extends
created: 2026-04-10
modified: 2026-04-10
---

# Empirical Computational Complexity Estimation

**Empirical computational complexity** is the practice of **measuring** a program's asymptotic cost from observations of actual runs, rather than deriving it by analysing source code. Coined formally by Goldsmith, Aiken, and Wilkerson in their ESEC/FSE 2007 paper "Measuring Empirical Computational Complexity", which introduced the *Trend Profiler* tool.

## Motivation

Static complexity analysis is **undecidable in general** and fragile in practice — loops with data-dependent bounds, virtual dispatch, callbacks, caching, amortised data structures, and system calls all confound it. Meanwhile, end users rarely care about a theoretical worst case; they care about the *actual* growth rate the code exhibits on realistic workloads.

Empirical complexity inverts the problem: **run the code**, **vary a workload size parameter** `n`, **record a cost signal** `c(n)`, and **fit** the observations to a small set of canonical complexity models.

## Method (Goldsmith et al.)

1. Identify workload "features" that scale with problem size — e.g. list length, string length, file size, node count. Multiple features may be observed simultaneously.
2. Run the target function on a sequence of inputs of growing size `{n₁, n₂, ..., n_k}`.
3. For each run, record a cost: basic-block executions, wall-clock time, CPU time, instructions retired, allocations, peak RSS.
4. For each plausible model `f ∈ { 1, log n, n, n log n, n², n³, 2ⁿ }`, fit coefficients `a, b` such that `c(n) ≈ a·f(n) + b` via least-squares regression on `(n, c(n))` pairs.
5. Rank models by goodness-of-fit (R² or residual sum of squares, adjusted for model degrees of freedom to avoid over-fitting higher-order polynomials).
6. Report the **best-fitting** model as the estimated complexity class, with a confidence level.

Trend Profiler's key insight: fit models in **log-log space** where polynomial complexities become straight lines of different slopes (slope 1 = linear, 2 = quadratic, ...) and exponential complexities become curves that straighten only in semi-log. This gives a visual and numerical separation criterion.

## Practical refinements

- **Sampling density** — use geometric growth (`n = 10, 20, 40, 80, ..., 10K`) rather than linear; geometric spacing spans more orders of magnitude cheaply.
- **Multiple repetitions per `n`** — compute median to reduce noise from GC, JIT, system jitter.
- **Noise filtering** — discard outliers beyond 1.5×IQR before fitting.
- **Phase transitions** — code that is `O(n)` below a threshold and `O(n²)` above is common (e.g. small-vector optimisation, hash-table resize storms, GC promotion). A single global fit will miss this; a **piecewise** fit or a change-point detector is needed.
- **Non-determinism** — randomised algorithms (quicksort with random pivots) need expected-case fitting with confidence intervals, not single-point estimates.
- **Input generation matters** — the estimated complexity reflects the *input distribution used*. Worst-case complexity requires adversarial inputs (e.g. from a resource-guided fuzzer). Goldsmith et al. note this prominently.

## Tools and descendants

- **Trend Profiler** (Goldsmith, Aiken, Wilkerson — 2007) — the original.
- **AlgoProf** (Zaparanuks, Hauswirth — PLDI 2012) — per-routine empirical complexity with improved model selection.
- **COZ** (Curtsinger, Berger — SOSP 2015) — causal profiling; complementary: identifies *which* functions contribute to overall slowdown, not *what class* they belong to.
- **aprof / aprof-plot** (Coppa, Demetrescu, Finocchi — ICSE 2012) — input-sensitive profiling for recursive workloads.
- **perfplot** (Python) — a practical reimplementation for library microbenchmarks.

## Why APEX needs it

The G-46 spec requires APEX to classify each target function into one of `{ O(1), O(n), O(n log n), O(n²), O(n³), O(2ⁿ) }` with ≥70% accuracy on benchmarks of known complexity. This complements resource-guided fuzzing: fuzzing finds *a* worst-case input; empirical complexity fits tell you the *growth rate* the function will exhibit as attackers scale the input. A function that's fast on today's inputs but quadratic will eventually be exploited.

## Known limits

- Phase-transition behaviour (hidden threshold) is systematically under-detected.
- Sub-linear differences (n vs n log n) are hard to distinguish without very large `n`.
- Cache effects and TLB pressure can make linear look super-linear at inflection points.
- Cost of measurement (profiling overhead) must be subtracted or amortised out.

## References

- Goldsmith, Aiken, Wilkerson — "Measuring Empirical Computational Complexity" — ESEC/FSE 2007 — [DOI 10.1145/1287624.1287681](https://doi.org/10.1145/1287624.1287681)
- Zaparanuks, Hauswirth — "Algorithmic Profiling" — PLDI 2012
- Coppa, Demetrescu, Finocchi — "Input-Sensitive Profiling" — ICSE 2012
- Curtsinger, Berger — "COZ: Finding Code that Counts with Causal Profiling" — SOSP 2015
