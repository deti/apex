---
id: 01KNZ4VB6JCD7A2BMXFN1AWGP4
title: "Percentile Composition Math — Why Averaging Percentiles Is Wrong"
type: concept
tags: [percentiles, math, composition, tail-latency, prometheus, histogram-quantile, fan-out]
links:
  - target: 01KNZ4VB6JCJY0S4JYW2C3CHTR
    type: extends
  - target: 01KNZ4VB6JPYBYW64S7NNYS1CM
    type: related
  - target: 01KNWE2Q9YZBAR140ZX5P36TQ5
    type: related
created: 2026-04-12
modified: 2026-04-12
source: "multiple; Schwartz 2015, Dean & Barroso 2013"
---

# Percentile Composition Math

Percentiles are not numbers you can sum, average, or combine. They are functionals of a distribution, and their arithmetic obeys very different rules than means. The consequences affect every load test, every dashboard, and every SLO calculation that involves more than one measurement window or more than one service.

## The three impossibility rules

### 1. Cannot average percentiles across time

If you have p99 latency at minute 1 and p99 latency at minute 2, `(p99_m1 + p99_m2) / 2` is **not** p99 over the two minutes combined. The combined p99 depends on the actual distribution of *every sample* across the two minutes, not on the two summary percentiles.

Counterexample: minute 1 has 1000 samples, all at 100 ms. Minute 2 has 1000 samples: 990 at 10 ms, 10 at 1000 ms. Minute-1 p99 = 100 ms. Minute-2 p99 = 1000 ms. Average of the two p99s = 550 ms. True combined p99 (the 99th percentile of 2000 samples sorted) = 100 ms (because only 10 of 2000 are above 100, which is ≈ 0.5 % — below the 1 % threshold). The average is 5.5x the true value.

Prometheus's `histogram_quantile(0.99, ...)` operates on histogram *buckets*, merging bucket counts across time windows before computing the quantile. This is correct. `avg_over_time(histogram_quantile(0.99, ...))` computes per-window quantiles first and averages them — **wrong**, one of the most common PromQL mistakes.

### 2. Cannot sum percentiles across stages

If stage A has p99 = 100 ms and stage B has p99 = 50 ms, the end-to-end p99 of A+B is **not** 150 ms.

The correct number depends on whether A and B are independent (in which case end-to-end p99 lies *between* max(p99A, p99B) and p99A + p99B, typically closer to the max for heavy-tailed distributions) or correlated (in which case it can be anywhere from max to p99A + p99B).

Lower bound: the end-to-end p99 ≥ max(p99A, p99B) when stages happen in sequence. The slowest 1 % of A-times cannot contribute less than p99A; likewise for B.

Upper bound: p99A + p99B is always a safe upper bound on end-to-end p99 but is typically loose. For two independent log-normal stages, end-to-end p99 might be 1.3–1.5x of max(p99A, p99B), nowhere near p99A + p99B.

The correct computation is either:

- Compose the two histograms (stage A's histogram + stage B's histogram → end-to-end histogram via convolution) and compute p99 of that. Exact but compute-heavy.
- Measure end-to-end directly and don't try to derive it from the stages.

### 3. Cannot average percentiles across instances

If host A has p99 = 100 ms and host B has p99 = 200 ms, the fleet p99 is **not** 150 ms. It depends on how many requests each host served and on the actual distributions.

If A served 99 % of traffic and B served 1 %, fleet p99 is close to 100 ms. If they split 50/50, fleet p99 is close to 200 ms. The answer comes from combining the sample sets, not from averaging the scalar summaries.

## The tail-amplification rule (Dean & Barroso 2013)

For a service that fans out to N independent backends, the service's tail is *worse* than any individual backend's tail.

If each backend has probability q of being in a "slow" state (p99 or worse), the probability that at least one of N backends is slow on a given request is:

    P(at least one slow) = 1 − (1 − q)^N

For q = 0.01 (each backend has p99 slow 1 % of the time):

- N = 1: 1 % slow
- N = 10: 9.6 % slow
- N = 100: 63.4 % slow
- N = 1000: 99.996 % slow

Therefore a service that fans out to 100 backends with per-backend p99 of 1 s has *median* (p50) latency ≈ 1 s. **Tail latency is the common case at scale**, and the way to keep it in check is not "make each backend faster" (which buys a tiny fraction) but "hedge, retry, limit fan-out, or parallelise" (which changes the structure of the composition).

## Consequences for load-test reporting

1. **Per-window percentiles must come from per-window histograms**, not from averaging.

2. **Fleet-wide percentiles must come from merged histograms**, not from per-host summaries. HdrHistogram's `add()` supports lossless merge. Prometheus's histograms support `sum()` over bucket counts followed by `histogram_quantile`.

3. **End-to-end percentiles must come from end-to-end measurements**, not computed from per-stage percentiles. If you only have per-stage data, you can bound end-to-end but you cannot compute it exactly.

4. **Alert on combined metrics, not averaged ones.** A dashboard that alerts on "avg(p99)" triggers on single-host spikes. A dashboard that alerts on "histogram_quantile(0.99, sum(rate(histogram_bucket[5m])) by (le))" triggers on true fleet-wide tail.

## Prometheus / PromQL specifics

Correct PromQL for a fleet p99 over a 5-minute window:

```promql
histogram_quantile(
  0.99,
  sum by (le) (rate(http_request_duration_seconds_bucket[5m]))
)
```

Incorrect (the mistake to avoid):

```promql
avg by (service) (
  histogram_quantile(0.99, rate(http_request_duration_seconds_bucket[5m]))
)
```

The difference: the correct query sums bucket counts across hosts, *then* computes the quantile. The incorrect query computes quantiles per host *then* averages. The two differ by the fleet imbalance amount.

## Percentile-of-percentiles pitfalls

Avoid these metrics:

- **p99 of p99s across regions.** Meaningless. Each region's p99 is a summary; you can't reason about a p99 of summaries.
- **Average of per-endpoint p99s.** Meaningless. Use merged histograms.
- **Max of p99s across hosts.** At least this is interpretable — "worst-offending host" — but it's a floor on fleet p99, not fleet p99 itself.

## The correct mental model

Treat percentiles as **functions of distributions, not as values that can be manipulated like scalars**. The operations you can do on them are limited:

- **Yes**: compute a percentile from a histogram, from a full sample, from a merged histogram (via lossless merge), from a bootstrap.
- **No**: average, sum, or combine two percentiles to get a third.

If you need a combined percentile, go back to the underlying histogram or samples and re-compute.

## Adversarial reading

- In practice, people do average percentiles all the time because the true answer is expensive to compute and the approximation is "sort-of right" when the distributions are similar. It's a pragmatic hack. But it is wrong, and the magnitude of wrong can be surprising on heterogeneous data (one hot host, one cold host).
- Not all percentile operations are impossible. **Minimum** and **maximum** of percentiles are meaningful (floor and ceiling). **Median of the *distribution*** across instances is not the same as **median of the scalar medians**.
- For alerts, the safest rule is "alert on the combined histogram quantile", and the next-safest is "alert on the max of per-instance quantiles". Never alert on the average.

## References

- Schwartz, B. — "Why percentiles don't work the way you think" — VividCortex blog, 2015.
- Dean, J., Barroso, L. — "The Tail at Scale" — CACM 56(2), 2013.
- Prometheus documentation — `histogram_quantile` correct usage — [prometheus.io/docs/prometheus/latest/querying/functions](https://prometheus.io/docs/prometheus/latest/querying/functions/)
- Percentiles-vs-averages note — `01KNZ4VB6JCJY0S4JYW2C3CHTR`.
- HdrHistogram note — `01KNZ4VB6JPYBYW64S7NNYS1CM`.
