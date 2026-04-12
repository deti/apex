---
id: 01KNZ67FD9WNQDTFVMEXQ0PRRV
title: "Welch's t-test for Performance Regression — Why It Sometimes Fails"
type: permanent
tags: [welch-t-test, statistics, regression-detection, adversarial, benchmark-statistics, heavy-tails, non-normal]
links:
  - target: 01KNZ67FDMCEA0MKZ8GZ841NDT
    type: related
  - target: 01KNZ67FDY7FH782T6V34Y2CFT
    type: related
  - target: 01KNZ4VB6JR9DSJA90V0WAW1TF
    type: related
  - target: 01KNZ67FBPEC378X6KZ79305T0
    type: related
  - target: 01KNZ6T75SFGWWGZPNF1AXR09K
    type: related
  - target: 01KNWE2Q9YZBAR140ZX5P36TQ5
    type: related
created: 2026-04-11T21:12:39.081532+00:00
modified: 2026-04-11T21:12:39.081534+00:00
---

A foundational statistical test repeatedly invoked for "is this benchmark significantly slower than baseline?" — and repeatedly misapplied. This note covers when Welch's t-test is the right tool for performance regression detection and, more importantly, when it is not.

## What Welch's t-test is

Welch's t-test is a modification of Student's t-test that does **not** assume the two samples being compared have equal variances. Given two independent samples with means `x̄₁, x̄₂`, variances `s₁², s₂²`, and sizes `n₁, n₂`, the test statistic is:

```
t = (x̄₁ - x̄₂) / sqrt(s₁²/n₁ + s₂²/n₂)
```

The degrees of freedom are approximated by the Welch–Satterthwaite equation. Under the null hypothesis of equal population means and the assumption of approximately normal samples, `t` follows a Student's t-distribution.

This is the default statistical test in many performance tools: **criterion.rs**, **Google Benchmark** (optionally), and any tool that reports a p-value from a pairwise comparison is usually using something in this family.

## Why it's attractive for perf regression

- **Closed-form.** No permutation test, no MCMC. Runs in microseconds on a thousand samples.
- **Familiar.** Every statistics 101 textbook covers it, so reviewers and engineers don't need to learn a new method to reason about the output.
- **Reasonable power** when assumptions are met.

## When it fails on performance data

Benchmark timings routinely violate every assumption Welch's t-test makes:

### 1. Non-normality — the dominant failure mode

Latency and throughput distributions are almost never normal. Common shapes:
- **Right-skewed** with a long tail: the underlying operation has a fast path and a slow path (cache hit/miss, allocator slow path, GC pause).
- **Multimodal**: different code paths produce distinct peaks (JIT tier transitions, thermal throttling kicking in).
- **Heavy-tailed**: power-law or log-normal, driven by GC, IO, scheduler.

The t-test is fairly robust to mild non-normality for `n > 30`, but heavy tails make the **variance estimates** themselves unreliable, which propagates into the denominator of the t statistic and inflates false-positive rates. Running a Shapiro-Wilk test on your benchmark samples before using Welch's t-test is a cheap sanity check most teams skip.

### 2. Non-independence — the sneakiest failure

The t-test assumes samples within a group are i.i.d. Benchmark samples are commonly *not* independent:
- **Warm-up effects**: early iterations are slower because caches are cold.
- **Thermal drift**: the CPU gets hotter over a long run and frequency-scales down.
- **Allocator state**: successive iterations share heap state, so a fragmentation-induced slowdown manifests as serial correlation.
- **Coordinated omission**: the measurement loop skips samples when the system is overloaded, which systematically removes the worst samples and biases both the mean and the variance estimate downward.

Positive autocorrelation makes the t-test **optimistic** — it thinks the effective sample size is higher than it really is and declares significance when none exists. Criterion.rs's HTML report has an autocorrelation plot specifically to flag this; most custom benchmark harnesses don't.

### 3. Unequal variance is handled, unequal shapes are not

Welch's correction addresses unequal variances but still assumes the two distributions are normal (just with different spreads). If the baseline is unimodal and the candidate is bimodal (e.g., a new code path introduces a rare slow case), Welch reports no significant difference in means while the user-visible experience has gotten dramatically worse. The right question is often not "did the mean shift?" but "did the tail change?" — which the mean-based t-test cannot answer.

### 4. Multiple-comparisons without correction

A PR touches 500 benchmarks. At `p < 0.05` you expect 25 false alarms by chance. Without Bonferroni or FDR correction, the t-test floods the sheriff queue. Most CI integrations forget this.

## What to use instead

- **Mann-Whitney U / Wilcoxon rank-sum** for non-normal but unimodal distributions (see dedicated note).
- **Kolmogorov-Smirnov** when you care about the whole distribution shape, not just the mean.
- **Bootstrap confidence intervals** for heavy-tailed data where closed-form variance estimates are unreliable. Criterion.rs uses bootstrap for its confidence intervals.
- **Change-point detection** (E-divisive, PELT) when you have a long time series rather than two pointwise samples — this is a fundamentally better framing for CI perf data (cf. Daly et al. 2020).
- **Effect size** (Cohen's d, Cliff's delta) instead of p-values when sample sizes are large and statistical significance is easy but practical significance is what you care about.

## When Welch's t-test is actually fine

- The benchmark is CPU-bound, single-threaded, and has no GC/allocation in the hot loop.
- You've warmed up, pinned CPU, disabled frequency scaling, and your samples look Gaussian on a QQ plot.
- `n` is large (>50 per side).
- You've corrected for multiple comparisons across the suite.
- You accept that it's a coarse "mean shifted" detector and not a "distribution changed" detector.

For microbenchmarks that fit this profile, Welch's test is the right choice and runs in a millisecond.

## Adversarial takeaway

If a paper or blog post says "we used a t-test to detect performance regressions" and provides no diagnostics for normality, independence, or effect size, treat the conclusions with suspicion. The t-test is the aspirin of statistics — it works for most headaches, but you want a doctor for a tumour. The Daly/Hunter papers explicitly chose non-parametric change-point detection over t-tests for CI benchmark histories precisely because the assumptions fail at scale.

## Connections

- Mytkowicz et al. 2009 "Producing Wrong Data" — shows how bias sneaks into benchmark data even before the stats stage.
- Chen & Revels 2016 — Julia BenchmarkTools uses minimum + MAD rather than mean + SD, for exactly this reason.
- Hunter (Fleming et al. 2023) — uses t-test at a confirmation step after E-divisive detects a candidate.
- Criterion.rs analysis — see bheisler.github.io/criterion.rs.
