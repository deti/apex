---
id: 01KNWGA5GQ08MFV0XJXX3MTFC3
title: "Tool: Criterion.rs (Statistical Rust Benchmarking)"
type: literature
tags: [tool, criterion, rust, benchmarking, statistics, bootstrap, regression-detection]
links:
  - target: 01KNWE2QA5VP0K80TMSABACKWT
    type: related
  - target: 01KNWE2QACWYZJXRJ8QE78T043
    type: references
created: 2026-04-10
modified: 2026-04-10
source: "https://bheisler.github.io/criterion.rs/book/analysis.html"
---

# Criterion.rs — Statistics-Driven Microbenchmarking in Rust

*Source: https://bheisler.github.io/criterion.rs/book/analysis.html — fetched 2026-04-10.*

The canonical reference for how a modern benchmark harness should handle noise. Criterion.rs is to Rust what JMH is to Java: a statistical-methodology framework that treats each benchmark as a small experiment rather than a single-shot timing.

## Four Phases

Criterion.rs benchmarks progress through four distinct phases: **Warmup**, **Measurement**, **Analysis**, and **Comparison**.

## Warmup Phase

"The routine is executed once, then twice, four times and so on until the total accumulated execution time is greater than the configured warm up time." This primes CPUs, OS caches, and JIT compilers before actual performance measurement begins. In a Rust context there is no JIT, but warmup still matters for CPU frequency stabilisation, icache/dcache warmth, and branch predictor state.

## Measurement Phase

During measurement, the tool collects performance data across multiple samples. "Each sample consists of one or more (typically many) iterations of the routine." Importantly, iteration counts increase systematically: `iterations = [d, 2d, 3d, ..., Nd]`, where `d` is a scaling factor derived from warmup estimates.

The increasing-iteration pattern is not arbitrary — it enables the **linear-regression method**: plot `(total_iterations, total_time)` and fit a line. The slope is the per-iteration cost, the intercept is the fixed overhead (loop setup, timing call latency). This decouples the real per-iteration cost from timing noise.

## Analysis Phase

### Outlier Detection (Tukey's Method)

Criterion.rs employs "a modified version of Tukey's Method" for outlier classification. Standard fences exist at **±1.5 × IQR** from quartiles; **additional severe outlier fences sit at ±3 × IQR**. Critically, "outlier samples are _not_ dropped from the data."

The categories Criterion.rs reports are:
- **low mild** / **high mild**: between 1.5× and 3× IQR
- **low severe** / **high severe**: beyond 3× IQR

Not dropping outliers is a deliberate choice: it preserves the dataset's full variance so downstream consumers can decide what to do. Criterion instead flags the outlier count in the report as a noise-quality signal.

### Bootstrap & Regression

"A large number of bootstrap samples are generated from the measured samples. A line is fitted to each of the bootstrap samples" to establish confidence intervals around iteration time estimates. This produces distributions of means, standard deviations, medians, and median absolute deviations.

Bootstrap resampling lets Criterion produce **distribution-free confidence intervals** — no Gaussian assumption, no central limit theorem reliance on small-N samples. Default: 100,000 bootstrap resamples, 95% CI.

## Comparison Phase

"The new and old bootstrap samples are compared and their T score is calculated using a T-test." A configurable noise threshold filters insignificant variations (e.g. ±1%) to distinguish genuine regressions from measurement fluctuations.

This is exactly the methodology APEX G-46 needs for its regression-detection mode: compare the candidate run's bootstrap distribution against the baseline's, apply a T-test, report the change only if it crosses the significance and the practical-relevance threshold.

## Relevance to APEX G-46

Criterion.rs's methodology is essentially the reference implementation for "2× regression detection with <10% false positives" — the G-46 acceptance criterion. Specifically:

1. **Warmup strategy** — geometric iteration growth to stabilise thermal and cache state.
2. **Linear regression for per-iteration cost** — decouples fixed overhead from real work.
3. **Tukey outlier fences (1.5× / 3× IQR)** — for reporting, not data removal.
4. **Bootstrap CIs** — distribution-free, robust to non-normal noise.
5. **T-test + noise threshold** — combines statistical and practical significance.

APEX's resource-measurement layer should adopt this whole framework rather than reinventing it. A Rust implementation could even *literally use* Criterion.rs as a library.
