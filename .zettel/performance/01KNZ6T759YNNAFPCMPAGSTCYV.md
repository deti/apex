---
id: 01KNZ6T759YNNAFPCMPAGSTCYV
title: Bencher.dev — Continuous Benchmarking with Configurable Statistical Thresholds
type: literature
tags: [bencher, continuous-benchmarking, thresholds, t-test, z-score, iqr, percentage, ci-cd, github-actions, open-source]
links:
  - target: 01KNZ6T74XZWE3RYQ86DZ2WREJ
    type: related
  - target: 01KNZ6WHSTDKHSQERTN6XZC90B
    type: related
  - target: 01KNZ67FD9WNQDTFVMEXQ0PRRV
    type: related
  - target: 01KNZ67FDY7FH782T6V34Y2CFT
    type: related
  - target: 01KNZ67FCARB8N2V5KPN8TY1PG
    type: related
  - target: 01KNZ4VB6J5ZW3JERZNDNGP7GD
    type: related
  - target: 01KNWGA5GQ08MFV0XJXX3MTFC3
    type: related
  - target: 01KNWE2Q9YZBAR140ZX5P36TQ5
    type: related
created: 2026-04-11T21:22:53.225694+00:00
modified: 2026-04-11T21:22:53.225695+00:00
---

*Sources: bencher.dev/docs/explanation/thresholds/, bencher.dev/docs/how-to/github-actions/, bencher.dev/docs/explanation/bencher-run/, github.com/bencherdev/bencher.*

Bencher.dev is an open-source continuous benchmarking tool that sits in the same design space as CodSpeed and benchmark-action but takes a third approach: instead of a noise-free proxy (CodSpeed) or a naive pairwise diff (benchmark-action), it provides a **configurable catalog of statistical threshold types** that the user explicitly picks based on the noise properties of their benchmark.

## The "six threshold types" menu

Bencher's documentation lists six threshold types, each representing a different statistical model for what "regressed" means. The user specifies one (or more) via the `--threshold-test` flag on `bencher run`. The six:

### 1. Static

A fixed numeric lower and/or upper bound.

> "If a new Metric is below a set Lower Boundary or above a set Upper Boundary an Alert is generated."

Use case: constant metrics like code coverage percentage, binary size, or a latency SLO that should never be exceeded. Does not adapt to historical baseline.

### 2. Percentage

A percentage deviation from the historical mean.

Use case: "alert if the new measurement is more than 5% above the historical mean." Adapts to mean drift over time but not to variance. Fails in exactly the ways described in the Welch's t-test note — if noise is 10% and your threshold is 5%, you get constant false positives.

### 3. t-test (Student's t-test)

> "Measures confidence intervals for whether new metrics deviate from historical means. Works best with small sample sizes and independent benchmark runs."

A proper statistical test with a null hypothesis: "the new measurement is consistent with the historical distribution." Requires the historical samples to be approximately normal. Bencher's documentation correctly flags the assumption — this test is good for small samples (`n < 30`) when you can believe normality.

### 4. z-score

> "Calculates standard deviations from the mean using z-scores. Requires at least 30 historical metrics and independent runs with no extreme outliers."

Functionally similar to t-test but uses the population variance (asymptotic) rather than the sample variance. Bencher explicitly warns that z-score requires `n ≥ 30` and independence and "no extreme outliers." Outliers break it.

### 5. Interquartile Range (IQR)

A non-parametric threshold: "alert if new measurement exceeds median + k × IQR."

> "Robust against outliers without requiring large sample sizes."

This is the robust-statistics choice. Uses medians and the IQR rather than means and standard deviations. Works on skewed or heavy-tailed distributions. The `k` parameter tunes sensitivity — `k = 1.5` is the conventional "outlier" threshold from Tukey's boxplot; `k = 3` is very conservative.

### 6. Delta Interquartile Range (Δ-IQR)

> "Extends IQR by measuring average percentage changes in the interquartile range, providing more sensitivity to relative variations."

A variant of IQR that normalizes by the IQR itself, so that changes in benchmark variance (not just in the median) are detected. More sensitive than plain IQR for benchmarks whose distribution shape changes.

## Why "six threshold types" is an interesting design choice

Most CI benchmark tools pick **one** statistical model and bake it in — Criterion.rs picks bootstrap, CodSpeed picks deterministic counts, benchmark-action picks percentage. Bencher defers the choice to the user. This is honest — different benchmarks have different noise properties and no single test is correct for all — but it also pushes the responsibility of statistical literacy onto the user. Most users do not have strong intuitions about when to use IQR vs. z-score vs. t-test, and the Bencher docs acknowledge this by guiding users toward IQR as the most robust default.

The fact that Bencher explicitly distinguishes these six threshold types is valuable: it makes the trade-offs visible, which forces users to think about noise characteristics before choosing a gate. Teams that reach for Bencher's `z-score` without reading the docs get the same false positives teams always get from assuming Gaussian benchmark noise.

## GitHub Actions integration

Bencher integrates with GitHub Actions via the `bencher run` CLI:

```yaml
- uses: bencherdev/bencher@main
- run: bencher run \
    --project my-project \
    --branch "$GITHUB_HEAD_REF" \
    --testbed ubuntu-latest \
    --threshold-measure latency \
    --threshold-test t_test \
    --threshold-max-sample-size 64 \
    --threshold-upper-boundary 0.95 \
    --err \
    --github-actions "$GITHUB_TOKEN" \
    -- cargo bench
```

Flags of note:
- `--threshold-test` picks which of the six models.
- `--threshold-max-sample-size` bounds the history window used for the threshold calculation.
- `--threshold-upper-boundary` is the confidence level (for t-test/z-score) or the multiplier (for IQR).
- `--err` causes the CLI to exit non-zero if any alert fires (so the GitHub Actions job fails).
- `--github-actions` posts a PR comment with the results.

## Adversarial commentary

- **Threshold selection is still on the user.** The menu is honest but puts the cognitive load on devs who rarely have the stats background to pick well. Contrast CodSpeed's "instructions don't lie, use 0% threshold" simplicity.
- **Wall-clock noise still present.** Bencher runs your benchmark on your CI runner, so it inherits all the GitHub Actions noise that wall-clock benchmarking has. The threshold types are mitigations, not cures.
- **No change-point detection in the threshold menu.** Rolling-window threshold checks catch step regressions but not slow drifts composed of multiple sub-threshold shifts. A proper time-series model (MongoDB / Hunter style) would complement the menu.
- **The `percentage` threshold is often the wrong default.** Newcomers pick it because it's easy to explain, and then wonder why noisy benchmarks flood the alert channel. IQR should be the recommended starting point.
- **Effect size is absent.** Like Kayenta, no Cliff's delta or Cohen's d.

## Connections

- Welch's t-test / Mann-Whitney / Cliff's delta notes — the stats underlying Bencher's threshold choices.
- CodSpeed — the "deterministic proxy" alternative that avoids needing thresholds.
- benchmark-action/github-action-benchmark — the "percentage threshold" alternative, simpler but noisier.
- Argo Rollouts / Flagger — production deployment gating, orthogonal.

## References

- bencher.dev/docs/explanation/thresholds/
- github.com/bencherdev/bencher — Apache 2.0 licensed, Rust.
