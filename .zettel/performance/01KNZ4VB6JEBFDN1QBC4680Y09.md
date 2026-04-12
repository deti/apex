---
id: 01KNZ4VB6JEBFDN1QBC4680Y09
title: "Chen & Revels 2016 — Robust Benchmarking in Noisy Environments"
type: literature
tags: [benchmarking, chen-revels, minimum-estimator, noise, benchmarktools-jl, julia, statistical-validity]
links:
  - target: 01KNZ4VB6J5ZW3JERZNDNGP7GD
    type: related
  - target: 01KNZ4VB6JR9DSJA90V0WAW1TF
    type: related
  - target: 01KNZ4VB6JZWDCTRVCP1R5V3GA
    type: related
  - target: 01KNWE2QACWYZJXRJ8QE78T043
    type: related
  - target: 01KNZ4VB6JF8CBPEK1YNFDTDAT
    type: related
  - target: 01KNWE2Q9YZBAR140ZX5P36TQ5
    type: related
created: 2026-04-12
modified: 2026-04-12
source: "Chen, J., Revels, J. — 'Robust benchmarking in noisy environments' — arXiv:1608.04295, HPEC 2016"
---

# Chen & Revels — Robust Benchmarking in Noisy Environments

*Source: Chen, J. & Revels, J. — "Robust benchmarking in noisy environments" — [arXiv:1608.04295](https://arxiv.org/abs/1608.04295) — Proceedings of the 20th Annual IEEE High Performance Extreme Computing Conference (HPEC), 2016.*

## The thesis in one sentence

**Use the sample minimum, not the mean or median, as the estimator of "true" runtime when benchmarking on noisy hardware** — because OS jitter, thermal effects, and contention make *every* sample at or above the true minimum, never below, so the minimum is the uncontaminated estimate.

This is the methodology baked into **BenchmarkTools.jl**, the benchmarking framework used in the Julia language's CI and every mature Julia package. It has been the default since ~2016 and has proven robust enough to gate the Julia compiler itself.

## Why means and medians fail

Consider a short piece of code whose "true" runtime — its CPU-and-cache-behaviour ideal — is some constant T. Every measurement gives an observed value T + noise, where noise:

- Is *always ≥ 0*. You cannot finish faster than T.
- Is caused by any of: OS scheduler preempting, context switch, cache pollution from another tenant, page fault, interrupt, clock resolution rounding up, branch predictor cold-start, JIT re-optimisation.
- Is *highly skewed*: most samples are close to T; a few are 10x T or more.
- Has *no fixed distribution*: the skew depends on machine state, which varies.

Given this structure, what does the mean measure? It measures T + E[noise]. But E[noise] is a moving target that changes with machine state and with how long you've been running. Two runs of the same benchmark, on the same machine, minutes apart, give different mean values — not because the code changed but because the noise distribution changed. **The mean is unstable under precisely the conditions you want it stable.**

The median is better (outlier-robust) but still contaminated: the median is T + median(noise), and median(noise) is still positive, still unstable, still somewhere above T.

## The minimum as an estimator of T

Chen & Revels' key observation: under the "noise always non-negative" assumption, **T = min(observed samples)** is a consistent estimator of T. It is:

- **Biased downward toward T.** Every sample is ≥ T; the minimum of many samples is the closest approach to T observed.
- **Monotone improving with sample size.** More samples → more chances to hit the clean path → minimum converges to T.
- **Robust to outliers.** A 10x outlier doesn't affect the minimum; it is invisible to the estimator.
- **Stable across runs.** Two runs on the same machine give the same minimum if the underlying T is the same — noise spikes don't move the minimum, only the upper tail.

The bias is downward: you never overstate performance, only approach it from above. For regression detection, this is exactly the property you want: "we measured the fastest this code got on this hardware, and yesterday it was 10 ns faster". No false positives from transient spikes; slow drift is visible.

## Caveats Chen & Revels document

1. **The minimum is *not* the right estimator of all quantities.** If you want to know "what is the expected runtime of this code on this machine in realistic conditions", the minimum is an underestimate — real runs will experience noise. The minimum is a *lower bound on true runtime*, which is the right thing for regression detection but not for capacity planning.

2. **Minimum is sensitive to sample count.** Min(5 samples) is biased farther from T than min(500 samples). Chen & Revels show that with reasonable sample counts (50–500) the minimum converges fast.

3. **Minimum fails for extremely short operations.** If the noise floor is dominated by clock resolution (~100 ns on a typical system), the minimum hits the clock resolution, not the true T. For sub-microsecond operations you need batching: measure N iterations and divide, not one iteration.

4. **Minimum fails when the "noise" has a genuine latency component, not just CPU contention.** A benchmark that hits the disk cold has a latency floor from the disk itself. The minimum finds that floor; it does not tell you the cached-path performance. This is a workload-characterisation question, not a statistical one.

## The BenchmarkTools.jl workflow

BenchmarkTools.jl (the Julia package that operationalises Chen & Revels' method) runs each benchmark as follows:

1. **Warm up**: execute N iterations before measurement to stabilise JIT, caches, branch predictors.
2. **Evaluate iterations needed**: run the benchmark briefly to estimate how many inner iterations fit in a target time budget (typical default 1 ms).
3. **Collect samples**: run "sample" = (batch of N inner iterations) several thousand times if possible, capturing per-sample wall-time.
4. **Report**: minimum, median, mean, max, standard deviation, and GC time overhead — with "min" flagged as the primary estimator.
5. **Compare**: Julia's regression-gating CI compares `min(benchmark on PR) - min(benchmark on base)` against a small threshold, typically 5 %.

The default configuration is tuned such that the method catches 5 % regressions reliably on shared-runner CI hardware where mean-based methods would need 10x more runs to reach the same sensitivity.

## Why it's less well-known outside Julia

BenchmarkTools.jl was the first major benchmarking framework to adopt the minimum-based estimator as its default. Python's `timeit` uses minimum *of several runs of a loop* (which is a similar idea but less rigorous). Google's `benchmark` library uses mean with outlier filtering. JMH uses mean with confidence intervals. Criterion.rs uses a hybrid — reports mean and median but also provides a "typical" estimate via robust bootstrap.

The Chen & Revels argument is *convincing and implementable* but hasn't percolated into all frameworks, partly because the mean has a familiar statistical story and the minimum requires justification each time it's explained.

## When to prefer minimum vs other estimators

| Goal | Estimator |
|---|---|
| Detect regressions in CI | **minimum** (Chen & Revels) |
| Predict production runtime | mean or median (of a realistic run) |
| Predict p99 latency | histogram-based percentile |
| Compare two algorithms' best-case performance | minimum |
| Compare two algorithms' worst-case performance | maximum, or p99 |
| Build a performance model | mean (plus regressions on workload params) |

Regression gating uses "minimum" not because it's the most physically meaningful number but because it's the *most stable* number across re-runs — which is what regression detection needs.

## Adversarial reading

- The minimum is only the right estimator when noise is strictly additive and strictly non-negative. Microbenchmarks satisfy this; end-to-end latency tests do not (latency can be lower than expected if caches are warm or if the fast path is exercised). For whole-system tests use percentile-based analysis instead.
- The "noise" floor on modern hardware includes intrinsic variability from CPU frequency scaling, thermal throttling, and turbo boost. "Minimum" captures the best-case turbo state. If your CPU is throttling for physical reasons, min-based regression can be a false positive.
- Minimum does not tell you about the *upper tail* of runtime, which is often what matters. Complement minimum with maximum or a high percentile to catch "fast most of the time, occasionally terrible" regressions.

## Relevance to APEX

- APEX's complexity estimation runs functions at many input sizes and fits a curve to the runtime data. The data points should be minimums (Chen & Revels), not means, to get the cleanest-possible signal that isolates algorithmic behaviour from noise.
- APEX's regression gating (spec item 6: "flag regressions exceeding 2x") is a coarse minimum-based check already in spirit.

## References

- Chen, J., Revels, J. — "Robust benchmarking in noisy environments" — [arXiv:1608.04295](https://arxiv.org/abs/1608.04295) — HPEC 2016.
- JuliaCI BenchmarkTools.jl — [github.com/JuliaCI/BenchmarkTools.jl](https://github.com/JuliaCI/BenchmarkTools.jl)
- Stabilizer note — `01KNZ4VB6JZWDCTRVCP1R5V3GA` — complementary method for randomising layout so that parametric tests (mean-based) apply.
- Statistical validity note — `01KNZ4VB6JR9DSJA90V0WAW1TF`.
