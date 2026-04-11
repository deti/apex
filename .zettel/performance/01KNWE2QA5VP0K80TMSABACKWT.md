---
id: 01KNWE2QA5VP0K80TMSABACKWT
title: Performance SLO Assertion and Regression Detection
type: concept
tags: [slo, performance, regression, ci, sre]
links:
  - target: 01KNWE2Q9YZBAR140ZX5P36TQ5
    type: extends
  - target: 01KNWE2QA2KV1NN8QH32RA5EPA
    type: related
  - target: 01KNWE2QACWYZJXRJ8QE78T043
    type: related
  - target: 01KNWGA5GQ08MFV0XJXX3MTFC3
    type: related
  - target: 01KNWGA5GYY1GE3G957BDNKX3D
    type: related
  - target: 01KNZ2ZDM8YHSYRFSJ399TWDWY
    type: references
  - target: 01KNZ2ZDMA4SCFK7QSPAJTPTGP
    type: references
created: 2026-04-10
modified: 2026-04-12
---

# Performance SLO Assertion and Regression Detection

A **Service Level Objective (SLO)** is a quantitative, testable statement about a system's performance — e.g. "the `/search` endpoint must return in under 200ms at the 99th percentile for requests up to 10KB." Google's SRE book formalised SLOs as the middle ground between **SLIs** (Service Level Indicators: raw measurements) and **SLAs** (Service Level Agreements: contractual, legally binding promises). SLOs are what you test; SLAs are what you pay for when you miss.

## Why SLOs matter for APEX

Google's SRE team reports that approximately **70% of production outages** are caused by changes to live systems rather than pre-existing correctness bugs. A silent performance regression — a function that used to take 10ms now takes 200ms — is the archetypal change-induced outage. Functional tests pass; users complain; oncall gets paged.

Traditional testing frameworks (JUnit, pytest) express correctness as assertions (`assertEquals`). They do not natively express *performance* as assertions. A few (Hypothesis, Criterion, JMH) support performance notation but treat it as advisory, not failing.

APEX's G-46 spec introduces a principled SLO-assertion mode:

```
apex perf --slo "parse:100ms:10KB" --target ./src
```

The command means: the function `parse` must complete in ≤100 ms for inputs ≤10 KB. APEX will:

1. **Generate boundary inputs** — inputs at exactly 10 KB plus a sweep inside the region.
2. **Generate over-boundary inputs** — inputs above 10 KB to characterise the degradation curve (does the function fail gracefully or fall off a cliff?).
3. **Measure** — time each run with multiple iterations and compute median / p95 / p99.
4. **Assert** — pass if p95 ≤ 100 ms; fail and produce a Finding otherwise.
5. **Report the degradation curve** — not just pass/fail, but a plot/table showing how latency scales past the SLO.

## Regression detection (baseline comparison)

SLO mode tests against an absolute threshold. **Regression detection** tests against a **relative baseline** — last week's measurements, or the main branch's numbers before this PR:

```
apex run --perf-baseline ./baseline.json --target ./src
```

The baseline file records, per function, the median / p95 / p99 of the relevant resource signals. A new run compares against it. A regression is flagged when:

- Median time grows by ≥ *k* × (default k=2.0)
- Or peak memory grows by ≥ *k* × (default k=2.0)
- Or allocation count grows by ≥ *k* × (default k=2.0)
- Or p99 shifts beyond the baseline's p99 + 3·stdev (statistical).

The G-46 acceptance criterion: 2× slowdown detected with **<10% false positives**. Achieving that requires statistical methods — single-shot measurements are too noisy.

## Statistical techniques to combat noise

- **Multiple iterations** — run each input *n* times (5–30) and take the median.
- **Warm-up runs** — discard the first *k* iterations to let JITs, caches, and page faults stabilise.
- **Outlier filtering** — drop measurements beyond 1.5·IQR.
- **Confidence intervals** — report 95% CI, not point estimates.
- **Change-point detection** — pelt / binary segmentation on a time series of per-commit measurements.
- **CPU pinning and frequency locking** — reduce jitter caused by thermal throttling, core migration, interrupts.
- **Criterion-rs / BenchmarkTools.jl methodology** — bootstrap-based CIs and t-tests for regression detection.

## Related: error budgets and burn-rate

Google SRE formalises the **error budget**: the allowed fraction of time the SLO may be violated per window (e.g. 99.9% availability → 43.2 min budget per month). **Burn-rate alerts** fire when the budget is being consumed too quickly. APEX doesn't target live monitoring — that's out of scope per the spec — but the same arithmetic is useful for deciding whether a CI regression should block merge (hot burn) or just warn (slow burn).

## Why this is hard

- **"Fast" is not a testable property** without a concrete SLO. Users either provide the SLO or rely on super-linear-scaling heuristics.
- **Hardware variance** — CI runners differ by generation, frequency, and contention. Absolute numbers are not portable; ratios (baseline vs. candidate on the same runner) are.
- **JIT warmup** — managed languages (JVM, .NET, V8) have steady-state performance that differs from cold start. The spec explicitly lists JIT warmup as *out of scope* for G-46.
- **False positives from flaky tests** — a slow CI runner, a noisy neighbour VM, a GC pause. Multi-iteration medians and statistical tests are the primary defences.

## References

- Beyer, Jones, Petoff, Murphy — "Site Reliability Engineering" — O'Reilly 2016 — [sre.google/sre-book](https://sre.google/sre-book/)
- Beyer, Murphy, Rensin, Kawahara, Thorne — "The Site Reliability Workbook" — O'Reilly 2018 — [sre.google/workbook](https://sre.google/workbook/)
- Gregg — "Systems Performance" 2nd ed. — Pearson 2020
- Criterion.rs — statistical benchmarking for Rust — [github.com/bheisler/criterion.rs](https://github.com/bheisler/criterion.rs)
- JMH — Java Microbenchmark Harness — [openjdk.org/projects/code-tools/jmh](https://openjdk.org/projects/code-tools/jmh/)
