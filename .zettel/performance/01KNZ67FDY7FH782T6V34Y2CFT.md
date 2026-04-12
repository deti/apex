---
id: 01KNZ67FDY7FH782T6V34Y2CFT
title: "Effect Size for Performance Regression — Cohen's d & Cliff's Delta"
type: permanent
tags: [effect-size, cohen-d, cliff-delta, vargha-delaney, statistics, regression-detection, adversarial]
links:
  - target: 01KNZ67FD9WNQDTFVMEXQ0PRRV
    type: related
  - target: 01KNZ67FDMCEA0MKZ8GZ841NDT
    type: related
  - target: 01KNZ4VB6JR9DSJA90V0WAW1TF
    type: related
  - target: 01KNZ67FBPEC378X6KZ79305T0
    type: related
  - target: 01KNWE2Q9YZBAR140ZX5P36TQ5
    type: related
created: 2026-04-11T21:12:39.102526+00:00
modified: 2026-04-11T21:12:39.102527+00:00
---

Statistical significance (a small p-value) tells you whether a difference probably exists. **Effect size** tells you whether the difference is big enough to care about. For performance regression detection, effect size is usually the more useful quantity — and it's the one most CI gates fail to report.

## Why p-values alone are not enough for CI perf gating

Consider a nightly benchmark run with `n = 1000` samples per side on a stable CI runner. The t-test or Mann-Whitney U will happily declare a 0.2% slowdown statistically significant at `p < 0.001`. Shipping this as an alert is wrong because:
- 0.2% is inside the run-to-run noise on every CI system that isn't a custom bare-metal perf lab.
- Humans don't care about 0.2% shifts on a single benchmark — triaging them would drown the sheriff rotation.
- Large `n` makes significance cheap. At `n = 10000`, even 0.02% becomes significant.

The fix is to **gate on effect size**, not on p-value alone. Report both: "statistically significant at `p < 0.001` AND effect size exceeds 0.3 Cohen's d".

## Cohen's d — the parametric default

Cohen's d is the standardised mean difference:

```
d = (x̄₁ - x̄₂) / s_pooled
```

where `s_pooled` is the pooled standard deviation of the two samples. It expresses the difference in means in units of the within-group standard deviation. Conventional interpretation (Cohen 1988):
- `|d| ≈ 0.2`: small effect
- `|d| ≈ 0.5`: medium
- `|d| ≈ 0.8`: large

For perf regressions, teams typically set a threshold around `d > 0.3` to combine with a significance test.

### Why Cohen's d fails on perf data

Same reason Welch's t-test fails: it assumes roughly Gaussian, well-behaved distributions with meaningful standard deviations. Heavy-tailed or multimodal benchmark data has standard deviations dominated by the tails, which makes `d` noisy and unreliable.

## Cliff's delta — the non-parametric alternative

Cliff's delta is a rank-based effect size that pairs naturally with the Mann-Whitney U test:

```
δ = (#{(i,j) : x₁ᵢ > x₂ⱼ} - #{(i,j) : x₁ᵢ < x₂ⱼ}) / (n₁ × n₂)
```

In words: count all pairs of observations where sample 1's value exceeds sample 2's, minus pairs where sample 2 exceeds sample 1, normalized by the total number of pairs. The result is in `[-1, 1]`:
- `δ = +1`: every observation in sample 1 is larger than every observation in sample 2 (complete dominance).
- `δ = 0`: equal probability of either sample being larger (no difference).
- `δ = -1`: complete reverse dominance.

Romano et al. (2006) conventional thresholds:
- `|δ| < 0.147`: negligible
- `|δ| < 0.33`: small
- `|δ| < 0.474`: medium
- `|δ| ≥ 0.474`: large

### Why Cliff's delta is better for benchmark data

- **No distributional assumptions.** Works on anything with an ordering.
- **Robust to outliers.** An extreme value changes a rank by 1, not by its magnitude.
- **Directly interpretable.** `δ = 0.6` means "in 60% more pair comparisons, the candidate was slower than the baseline than vice versa." This is something you can explain to a reviewer without a stats lecture.
- **Pairs with Mann-Whitney.** If you're already using M-W for significance, Cliff's delta is literally reusing the same rank computations.

## Vargha-Delaney A-hat

A closely related non-parametric effect size is the **Vargha-Delaney A₁₂** statistic, which equals `P(X₁ > X₂) + 0.5 × P(X₁ = X₂)`. It lives in `[0, 1]` with 0.5 meaning no effect. Arcuri & Briand (2014) strongly recommend it for software engineering experiments because its interpretation is direct: "A₁₂ = 0.7 means 70% of the time, a random draw from sample 1 is larger than a random draw from sample 2." A-hat and Cliff's delta are linearly related: `A₁₂ = (δ + 1) / 2`.

## Common language effect size (CLES)

McGraw & Wong (1992) proposed CLES as an interpretation-friendly effect size: "the probability that a random observation from group 1 exceeds a random observation from group 2". For normally distributed data, `CLES = Φ(d / √2)`. For non-normal data, Vargha-Delaney A-hat is exactly the CLES.

## The CI gate recipe

The practical recommendation (aligned with Arcuri & Briand 2014 and the MongoDB/Hunter practice):

1. Collect `n ≥ 20` samples per benchmark.
2. Run Mann-Whitney U for significance. Correct for multiple comparisons across the suite (Benjamini-Hochberg FDR at 0.05 is standard).
3. Compute Cliff's delta (or A-hat) for effect size.
4. Alert if **both** significance and effect size thresholds are crossed.
5. Report both quantities in the alert, so the human sheriff can sanity-check.

## Adversarial commentary

- **Effect-size thresholds are still arbitrary.** Cohen's categories (small/medium/large) come from social-science research, not computer systems. A `d = 0.2` "small" effect on a 100ms latency is 20ms of user-visible slowness — possibly catastrophic. Domain-specific thresholds matter more than textbook labels.
- **Cliff's delta can saturate.** If the baseline and candidate have *any* overlap, `δ < 1`; if they don't, `δ = 1` regardless of how much faster the baseline is. For highly-separated distributions, Cliff's delta loses resolution and you should fall back to quantile-based comparisons.
- **Effect sizes are still pointwise.** They compare two samples, not a time series. For CI histories, change-point detection is still the right framing.

## Connections

- Welch's t-test (parametric significance, dedicated note).
- Mann-Whitney U (non-parametric significance, dedicated note).
- Arcuri & Briand 2014 "A Hitchhiker's guide" — key methodological paper in SE.
- Kayenta's Mann-Whitney judge — does not currently return effect sizes, a real gap.

## References

- Cohen, J. (1988). *Statistical Power Analysis for the Behavioral Sciences*.
- Cliff, N. (1993). *Dominance statistics: Ordinal analyses to answer ordinal questions*. Psych. Bulletin.
- Vargha, A. & Delaney, H. D. (2000). *A critique and improvement of the CL common language effect size statistics*. JEBS.
- Romano, J. et al. (2006). *Appropriate statistics for ordinal level data*. Florida Assn. of Institutional Research.
- Arcuri, A. & Briand, L. (2014). *A Hitchhiker's guide to statistical tests*. STVR.
