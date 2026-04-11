---
id: 01KNZ2ZDN167V89N6CR6ZNRWYR
title: "Criterion.rs Statistical Methodology (Bootstrap, Tukey, T-test)"
type: literature
tags: [criterion, rust, bootstrap, tukey, statistics, outliers, t-test]
links:
  - target: 01KNWGA5GQ08MFV0XJXX3MTFC3
    type: extends
  - target: 01KNZ2ZDMC9YQR6MDJ9FZJGSEZ
    type: references
  - target: 01KNWE2QACWYZJXRJ8QE78T043
    type: related
created: 2026-04-12
modified: 2026-04-12
source: "https://bheisler.github.io/criterion.rs/book/analysis.html"
---

# Criterion.rs — Statistical Methodology Deep-Dive

*Source: https://bheisler.github.io/criterion.rs/book/analysis.html — fetched 2026-04-12.*

This is the statistical reference sheet for what Criterion.rs does to every benchmark before reporting a number. If APEX wants to produce honest performance findings, this is the checklist to match.

## The measurement model

Criterion treats each benchmark as a black-box function `f(iters: u64) -> duration`. It calls `f` with various values of `iters` and observes `duration`. The model is:

```
duration = noise + overhead + iters × time_per_iter
```

The **unknown** is `time_per_iter`. Criterion estimates it via the slope of a linear regression through the `(iters, duration)` pairs. This is the Criterion innovation relative to simple mean-based benchmarkers — the regression slope subtracts off the fixed per-sample overhead automatically.

## The benchmark phases

Each run proceeds through three phases:

1. **Warmup** — run the function for a fixed time (default 3 s) and discard the results. Ensures JIT, cache, and branch predictor are in steady state.
2. **Measurement** — collect N samples, each at a different `iters` count. Default N = 100, spread over the measurement_time (default 5 s). The `iters` counts are chosen so each sample takes ~the same wall-clock time, which gives the regression enough dynamic range in `iters`.
3. **Analysis** — apply the statistical pipeline below.

Default total time per benchmark: ~8 seconds. Configurable up or down.

## Phase 1 — Bootstrap resampling

From the N = 100 collected samples, Criterion generates **100,000 bootstrap resamples** by sampling with replacement. For each resample it computes:

- Mean
- Median
- Standard deviation
- Median absolute deviation (MAD)
- Slope (from the linear regression)

The bootstrap produces an empirical distribution for each estimator. The mean of the distribution is the point estimate; the 2.5th and 97.5th percentiles give a 95% confidence interval.

**Why bootstrap?** Because we don't know the true distribution of `duration` — it's *not* normal (it has a sharp lower bound and a long upper tail due to OS scheduling). Bootstrap resampling is distribution-free: it makes no assumption about the shape of the underlying distribution, only that the samples are representative.

## Phase 2 — Outlier detection via Tukey's method

Criterion applies a **modified Tukey's method** to classify each observed sample:

| Range | Classification |
|---|---|
| `[Q1 - 1.5·IQR, Q3 + 1.5·IQR]` | Normal |
| `[Q1 - 3·IQR, Q1 - 1.5·IQR]` or `[Q3 + 1.5·IQR, Q3 + 3·IQR]` | Mild outlier |
| Outside `[Q1 - 3·IQR, Q3 + 3·IQR]` | Severe outlier |

Where `Q1`, `Q3` are the first and third quartiles and `IQR = Q3 - Q1` is the interquartile range. Stock Tukey uses 1.5 only; Criterion adds the 3·IQR fence for severe outliers.

**Crucially, outliers are NOT removed from the analysis.** Criterion reports them separately ("1 severe outlier (1%)") but leaves them in the regression input. The rationale:

> Outliers are part of the real-world distribution of the function's runtime. Removing them biases the estimator toward the idealised noise-free case, which is not what users actually observe. If the function has a 1% chance of running 3x slower because of GC / OS / I/O, users care about that 1% — the 99th-percentile case — just as much as the median.

This is a subtle but important point. JMH has the same philosophy. BenchmarkTools.jl takes the opposite position (min-is-truth). APEX's empirical complexity estimator needs to pick a philosophy and stick to it.

## Phase 3 — Comparison against baseline

If a `target/criterion/<name>/base/` directory exists from a previous run, Criterion runs the **change analysis**:

1. Compute the mean slope from the *current* bootstrap distribution.
2. Compute the mean slope from the *stored* baseline bootstrap distribution.
3. Compute the observed difference: `Δ = new_slope - baseline_slope`.
4. Compute a **bootstrap T-statistic**: for each paired bootstrap resample, compute the same `Δ`, producing a distribution of Δs under the null hypothesis that the current run is no different from the baseline.
5. Compute the fraction of the bootstrap Δs that are more extreme than the observed Δ. This is the **p-value**.
6. If `p < 0.05` AND `|Δ| > noise_threshold` (default 2%), report a change.

The output line looks like:

```
Change: [-1.234% +0.123% +2.345%] (p = 0.34 > 0.05)
No change in performance detected.
```

Where `[lo, mid, hi]` is the 95% CI of `Δ` and `mid` is the point estimate.

## The noise threshold

Default: 2% (configurable via `noise_threshold`). The rationale: even in the absence of a real change, wall-clock benchmarks have ~1% run-to-run variation due to CPU temperature, page cache state, memory bandwidth contention with other processes, etc. Reporting any change smaller than 2% as "significant" produces a flood of false positives in CI.

The user can tune this. For very quiet targets (pure-arithmetic kernels on a dedicated machine), 0.5% is achievable. For noisy targets (networked services, I/O-heavy), 5-10% is more realistic.

## Why this matters for APEX G-46

The Criterion.rs methodology is the **right shape** for any honest performance-regression detector. APEX's G-46 implementation should mirror it step by step:

1. **Warmup** — run the target a few times before measuring. Critical for JIT languages (Java, JS, Julia, .NET) and for avoiding cold-cache bias in any language.
2. **Linear regression over iteration counts** — not just a mean of N independent calls. The regression separates per-iteration cost from fixed overhead.
3. **Bootstrap CI, not normal-theory CI** — wall-clock distributions are not normal. Normal-theory gives wrong CIs.
4. **Tukey outlier classification without removal** — report outliers, don't hide them.
5. **T-test with noise threshold for change detection** — don't fire alarms on 0.5% changes; do fire them on 10% changes above the floor.
6. **Explicit p-value and CI in the output** — users should see whether a change is likely real.

## Practical APEX design decisions

- **Sample count**: 100 is the minimum for a stable CI; 1000 is comfortable; 10000 saturates. APEX should default to 100 for fast CI mode and 1000 for the "thorough" mode.
- **Bootstrap resample count**: 10,000 is fine for a 95% CI; 100,000 is Criterion's default and is slightly better for the tails. APEX can default to 10,000 to save CPU in CI.
- **Noise threshold**: 5% for CI mode (tolerant), 2% for thorough mode. Configurable per benchmark.
- **Baseline storage**: a JSON file per benchmark, version-controlled alongside the code. Criterion's JSON shape is a fine template.
- **Regression direction**: only report *slowdowns* in CI mode (don't fail the build on speedups). In thorough mode, report both.

## Relevance to APEX G-46

Everything above is a direct input to APEX's `apex perf` implementation design. The statistical core of the tool is essentially "Criterion.rs generalised to languages beyond Rust". Where the source language has a native Criterion-equivalent (Julia's BenchmarkTools.jl, Python's pytest-benchmark, Go's testing.B, Java's JMH), APEX should emit benchmark harnesses for it; where there isn't one (bash, Ruby), APEX needs to implement the methodology itself.

## References

- Criterion.rs book — "Analysis" chapter — [bheisler.github.io/criterion.rs/book/analysis.html](https://bheisler.github.io/criterion.rs/book/analysis.html)
- Tukey — "Exploratory Data Analysis" — Addison-Wesley 1977 (original source for the method)
- Efron, Tibshirani — "An Introduction to the Bootstrap" — Chapman & Hall 1993
- Criterion.rs README — `01KNZ2ZDMC9YQR6MDJ9FZJGSEZ`
