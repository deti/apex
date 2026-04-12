---
id: 01KNZ4VB6JR9DSJA90V0WAW1TF
title: "Statistical Validity — Confidence Intervals, Effect Size, Run Length"
type: concept
tags: [statistics, confidence-interval, effect-size, bootstrap, order-statistics, run-length, percentile-ci]
links:
  - target: 01KNZ4VB6JEBFDN1QBC4680Y09
    type: related
  - target: 01KNZ4VB6J5ZW3JERZNDNGP7GD
    type: related
  - target: 01KNZ4VB6JCJY0S4JYW2C3CHTR
    type: related
  - target: 01KNZ67FD9WNQDTFVMEXQ0PRRV
    type: related
  - target: 01KNZ67FDMCEA0MKZ8GZ841NDT
    type: related
  - target: 01KNZ67FDY7FH782T6V34Y2CFT
    type: related
  - target: 01KNZ67FBPEC378X6KZ79305T0
    type: related
  - target: 01KNZ67FCARB8N2V5KPN8TY1PG
    type: related
  - target: 01KNWE2QA5VP0K80TMSABACKWT
    type: related
  - target: 01KNWE2Q9YZBAR140ZX5P36TQ5
    type: related
created: 2026-04-12
modified: 2026-04-12
source: "multiple; Hoefler & Belli 'Scientific Benchmarking of Parallel Computing Systems' SC 2015; Georges Buytaert Eeckhout OOPSLA 2007"
---

# Statistical Validity — Confidence Intervals, Effect Size, Run Length

## The question

You ran a load test, measured p99 = 312 ms. You ran it again, got 298 ms. A third time, 325 ms. Is there a regression? Is the system at the SLO limit or safely under it? How many more runs do you need before you can trust the answer?

Without a statistical framework, every load test result is a point estimate of unknown precision. With one, you have confidence intervals, effect sizes, and an objective stopping rule.

## Three separate questions

Performance measurements need to answer three statistical questions, each with its own tools:

1. **Where is the central value?** Mean, median, percentile — a point estimate.
2. **How certain are we?** Confidence interval around the point estimate — a range.
3. **Is a difference meaningful?** Effect size vs significance — a comparison.

Answering only (1) is the default and the source of most performance-testing disputes.

## CIs for means: easy and familiar

For the mean of a sample, the t-distribution gives a confidence interval:

    CI_95 = mean ± t_{0.975, n-1} × s / √n

where s is the sample standard deviation. For n = 30 this is approximately mean ± 2 s/√n. Double the sample size → CI shrinks by 1/√2. So halving the CI needs 4× the runs.

Caveats:
- Assumes samples are approximately Gaussian (central limit theorem applies at n ≳ 30 for most distributions).
- Assumes samples are independent — adjacent runs on the same machine often are not (cache, thermal).
- Applies to the mean, not the percentile.

## CIs for percentiles: the harder problem

You rarely care about the mean of latency; you care about p99. p99 is a *quantile*, and quantile estimators have their own CIs.

Two methods:

### Order-statistic method

For sample size n, the observed percentile p corresponds to order statistic k = ⌈np⌉. A distribution-free CI for the quantile uses the Binomial distribution: if we draw n independent samples from a distribution whose p-th quantile is q, then the number of samples below q is Binomial(n, p). A (1 − α) CI for q is [x_{(L)}, x_{(U)}] where L and U are the Binomial quantiles of α/2 and 1 − α/2.

Worked example: n = 1000 samples, want a 95 % CI for p99. L = ⌈1000 × 0.99 − 1.96 × √(1000 × 0.99 × 0.01)⌉ ≈ 984, U ≈ 997. So the 95 % CI for p99 is between the 984th and 997th sorted sample. That's a 13-sample-wide band, which for heavy-tailed data can be large in value terms.

**Key insight: CI width for tail percentiles grows fast as n shrinks.** 100 samples barely lets you put any CI on p99 at all. 10 000 samples gives a tight p99 estimate but still loose p99.9. For p99.99, you need 100 000+ samples. This is the reason HdrHistogram-style lossless capture matters — you need every sample.

### Bootstrap method

Resample the data with replacement N_boot times (typical N_boot = 10 000). For each resample, compute p99. The 2.5th and 97.5th percentiles of the resulting distribution of resample p99s is a 95 % bootstrap CI.

Bootstrap is more flexible (works for any estimator, including per-minute p99) and makes fewer distributional assumptions. Downside: it's compute-heavy for large samples.

## Effect size vs statistical significance

p < 0.05 says "there probably is a difference". It does not say the difference is important. A huge sample can make trivial differences significant.

**Effect-size metrics:**

- **Cohen's d** = (mean_A − mean_B) / pooled_sd. Scale: d = 0.2 small, 0.5 medium, 0.8 large. Assumes Gaussian.
- **Cliff's delta** = P(A > B) − P(A < B). Non-parametric, in [−1, 1]. |δ| < 0.147 negligible, 0.147–0.33 small, 0.33–0.474 medium, > 0.474 large.
- **A12 VDE** (Vargha-Delaney) = P(A > B) + 0.5 × P(A = B). In [0, 1]. 0.5 is no effect; 0.56 small; 0.64 medium; 0.71 large.
- **Percent change** — the simplest and most communicable. "5 % slower" is what a product team understands.

Hoefler & Belli (SC 2015, "Scientific Benchmarking of Parallel Computing Systems") argue that *every* benchmark report should include effect size and CI, not just p-values. Otherwise you conflate "confidently detected" with "worth reacting to".

## Run-length determination

How long should a test run, and how many iterations? Three answers at different levels of rigour:

### Sequential stopping (rough)

Run N iterations. Compute CI for the metric. If CI width < target precision, stop. Else run more. The simplest adaptive stopping rule.

### Formal sample-size calculation

Given: desired precision δ (absolute) and confidence 1 − α, estimate s (the std dev). Required N:

    N ≥ (z_{α/2} × s / δ)²

For δ = 5 % of mean, α = 0.05, and observed coefficient of variation 10 %, N ≥ (1.96 × 0.10 / 0.05)² ≈ 16. For CoV 20 %, N ≥ 62. For CoV 30 %, N ≥ 138. Most unstable benchmarks have CoV 20–40 %.

### Georges et al. OOPSLA 2007 method

"Statistically rigorous Java performance evaluation" (Georges, Buytaert, Eeckhout, OOPSLA 2007) proposes:

1. **Within-VM iterations**: run the benchmark for many iterations inside a single VM invocation, discarding warm-up iterations, until steady state.
2. **Across-VM invocations**: launch new VMs N times, collect the steady-state mean of each.
3. Compute CI across VM means (capturing JIT-level variability that within-VM iteration can't).
4. Compare two alternatives via CI overlap or Welch's t-test on the across-VM means.

The key insight: within-VM iterations underestimate variance because they don't sample JIT compilation paths. Across-VM invocations are necessary to get a correct variance estimate. JMH's "forks" parameter implements exactly this.

## Steady state detection

Before statistics matter, you must identify the steady-state window. Methods:

- **Fixed warm-up**: discard the first N seconds/iterations. Simple; over-discards or under-discards depending on the workload.
- **CUSUM / change-point detection**: scan the run for a point where the running mean stabilises. More principled.
- **Chow test**: compare regression parameters in different windows for a change-point.
- **JMH's heuristic**: iterations where the running mean delta between consecutive iterations drops below a threshold.

A test that doesn't label its steady-state window is a test with undefined data.

## Anti-patterns

1. **Single-run comparison.** Run A = 100 ms, Run B = 105 ms → "5 % regression". Absent variance, a single-point comparison is noise.

2. **Statistical significance without effect size.** "p < 0.001, regressed!" Regression is 0.2 %, within noise of production. Fix: report effect size.

3. **Effect size without CI.** "Regressed by 5 %." ± what? Fix: report both.

4. **CI for mean when reporting percentile.** "p99 = 312 ms ± 8 ms (95 % CI)". Unless that CI comes from a percentile-aware method (order statistic or bootstrap), it's meaningless.

5. **Not reporting sample size.** "p99 = 312 ms" from 50 samples and from 50 million samples are very different statements of confidence. Always report n.

6. **Assuming Gaussian.** Latency distributions aren't. Use non-parametric tests (Mann-Whitney, bootstrap) unless you've actually checked.

7. **Re-running until the result is favourable.** If you run 10 times and report the best, your effective α is much larger than α. Every "retry" multiplies your type-I error. Fix: pre-register the stopping rule.

## Adversarial reading

- Statistical rigour is expensive — more runs, more compute, more analysis time. Most real-world load tests skip it because the cost of "not statistically rigorous" is paid by future-you in ambiguous regressions.
- "Enough samples" is always a function of your precision target and noise level. Report both, not just the n.
- CI methods for percentiles assume iid samples. Load-test samples often aren't iid (temporal correlation, autocorrelation). True CI widths can be 2–3x the naive calculation. Block bootstrapping addresses this but is rarely used in practice.

## References

- Georges, A., Buytaert, D., Eeckhout, L. — "Statistically Rigorous Java Performance Evaluation" — OOPSLA 2007.
- Hoefler, T., Belli, R. — "Scientific Benchmarking of Parallel Computing Systems" — SC 2015.
- Chen & Revels — minimum-estimator note `01KNZ4VB6JEBFDN1QBC4680Y09`.
- Efron, B., Tibshirani, R. — *An Introduction to the Bootstrap*, Chapman & Hall 1993 — the standard bootstrap reference.
- Conover, W.J. — *Practical Nonparametric Statistics*, Wiley 1999 — order-statistic CI methods.
