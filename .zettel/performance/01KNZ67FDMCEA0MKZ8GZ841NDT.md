---
id: 01KNZ67FDMCEA0MKZ8GZ841NDT
title: Mann-Whitney U & Wilcoxon Rank-Sum for Non-Normal Benchmark Data
type: permanent
tags: [mann-whitney-u, wilcoxon, statistics, non-parametric, regression-detection, benchmark-statistics]
links:
  - target: 01KNZ67FD9WNQDTFVMEXQ0PRRV
    type: related
  - target: 01KNZ67FDY7FH782T6V34Y2CFT
    type: related
  - target: 01KNZ4VB6JR9DSJA90V0WAW1TF
    type: related
  - target: 01KNZ6T721S1YTYHGZE1AS1Y43
    type: related
  - target: 01KNZ67FBPEC378X6KZ79305T0
    type: related
  - target: 01KNWE2Q9YZBAR140ZX5P36TQ5
    type: related
created: 2026-04-11T21:12:39.092035+00:00
modified: 2026-04-11T21:12:39.092037+00:00
---

The Mann-Whitney U test (also called the Wilcoxon rank-sum test — same test, different inventors) is the non-parametric alternative to the two-sample t-test. When benchmark data violates the Gaussian assumption required by Welch's t-test, Mann-Whitney is the workhorse replacement.

## What it tests

Formally, the Mann-Whitney U test checks the null hypothesis that two independent samples come from distributions with the **same median** (more precisely, that `P(X > Y) = 0.5` where `X, Y` are random draws from the two distributions). It makes **no assumption of normality**, only that both samples are drawn from continuous distributions with the same shape (the "shift model").

## How the statistic is computed

1. Pool all observations from both samples.
2. Rank them from smallest to largest (ties get averaged ranks).
3. Sum the ranks for each sample. Let `R₁` = rank sum of sample 1, `n₁` = size of sample 1.
4. Compute `U₁ = R₁ - n₁(n₁+1)/2` and `U₂ = n₁n₂ - U₁`. The test statistic is `U = min(U₁, U₂)`.
5. Under the null, `U` has a known distribution (exact for small `n`, normal approximation for `n > 20`) from which a p-value is derived.

Notice that only the **rank** of each observation matters, not its numerical value. This is what buys robustness to heavy tails: a single catastrophic outlier changes its rank by 1 regardless of whether the slowdown was 2× or 200×.

## Why this is the right default for benchmark data

- **No normality assumption.** Latency distributions are routinely log-normal, multimodal, or heavy-tailed. Mann-Whitney doesn't care.
- **Robust to outliers.** A GC pause that produces a single 100ms sample in a 10μs benchmark would swing a t-test's variance estimate wildly; Mann-Whitney just sees it as "the biggest rank".
- **Works on small samples.** Exact tables exist for `n ≤ 20`; below that, normal approximation for t-tests becomes unreliable too.
- **Easy to compute.** `scipy.stats.mannwhitneyu`, R's `wilcox.test`, etc.

## Where Mann-Whitney also fails

### 1. Shape assumption is sometimes ignored

The "pure" Mann-Whitney tests `P(X > Y) = 0.5`. Many textbooks describe it as a "test for median differences" — this is only strictly true when the two distributions have the **same shape and differ only in location** (a shift). If the shapes differ (e.g., baseline is tight, candidate has a long tail), the test detects the shape difference and calls it a "median shift", which is misleading if you're interpreting the result as "the typical case got slower".

In practice, for perf regression, a shape change that produces higher-ranked samples in the candidate *is* usually a regression, so this is often acceptable. But be aware that a statistically significant Mann-Whitney result does not strictly mean "median got worse".

### 2. No effect size by default

The p-value tells you whether a difference exists, not how big. For perf regression detection with large samples (common in CI), even a 0.1% slowdown can be statistically significant. Pair Mann-Whitney with **Cliff's delta** — a non-parametric effect size — to get both significance and magnitude. (See dedicated note on effect sizes.)

### 3. Multiple comparisons still apply

Same Bonferroni/FDR caveat as with t-tests: running Mann-Whitney across 500 benchmarks at `α = 0.05` gives 25 expected false positives without correction.

### 4. Autocorrelation still matters

Like t-tests, Mann-Whitney assumes independence within each sample. Warm-up effects, thermal drift, and GC state break this. The remedy is the same: pin CPUs, discard warm-up, use blocking, or model the time series directly with change-point detection rather than pointwise comparison.

### 5. Wilcoxon signed-rank is a different beast

Don't confuse Mann-Whitney (independent samples) with **Wilcoxon signed-rank** (paired samples). Signed-rank is for *paired* designs — e.g., "same benchmark, same machine, just the patch applied". Signed-rank is more powerful when the pairing is real but gives nonsense if you apply it to two independent runs.

## In practice: when to reach for Mann-Whitney

- Comparing two benchmark runs and you can't verify Gaussian-ness.
- Latency data with visible right-skew or heavy tails.
- Small samples where the t-test's normal approximation is questionable.
- As a sanity check alongside the t-test: if both agree, you can believe the result; if they disagree, your data is weird and you should look at it with a histogram.

## Which tools use it

- **Netflix Kayenta** uses Mann-Whitney U as the default classifier in its Canary Judge (see dedicated note).
- **criterion.rs** uses a closely-related bootstrap analysis with Cliff's delta for effect size.
- **JMH** (Java Microbenchmark Harness) uses bootstrap confidence intervals but supports Mann-Whitney in downstream comparison tools.
- **signal-processing-algorithms** uses energy statistics rather than rank-based tests — a different non-parametric approach.

## Connections

- Welch's t-test (the parametric alternative, dedicated note).
- Cliff's delta & Cohen's d (effect sizes, dedicated note).
- Kayenta Canary Judge (dedicated note).
- Chen & Revels 2016 Julia BenchmarkTools — uses minimum + MAD, a robust-statistics alternative.

## References

- Mann, H. B. & Whitney, D. R. (1947). *On a test of whether one of two random variables is stochastically larger than the other*. Annals of Mathematical Statistics.
- Arcuri, A. & Briand, L. (2014). *A Hitchhiker's guide to statistical tests for assessing randomized algorithms in software engineering*. STVR — strongly argues for Mann-Whitney + Vargha-Delaney A-hat over t-test + Cohen's d.
