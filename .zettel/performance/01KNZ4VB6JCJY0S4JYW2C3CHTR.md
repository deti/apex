---
id: 01KNZ4VB6JCJY0S4JYW2C3CHTR
title: "Percentiles vs Averages — Why Latency Means Lie"
type: concept
tags: [percentiles, latency, statistics, tail-latency, measurement, distribution]
links:
  - target: 01KNZ4VB6JPYBYW64S7NNYS1CM
    type: related
  - target: 01KNZ4VB6JB4Q5H3NPS72MZZ2A
    type: related
  - target: 01KNZ4VB6JCD7A2BMXFN1AWGP4
    type: related
  - target: 01KNWE2Q9YZBAR140ZX5P36TQ5
    type: related
created: 2026-04-12
modified: 2026-04-12
source: "multiple; Gil Tene 'How NOT to Measure Latency', Google Dean & Barroso 'The Tail at Scale' CACM 2013"
---

# Percentiles vs Averages — Why Latency Means Lie

## The one-sentence version

Latency distributions in real systems are right-skewed and heavy-tailed, so the mean is not the middle; the mean does not describe "typical user experience"; and the mean alone cannot be used for SLO enforcement, regression detection, or capacity planning.

## The shape of real latency distributions

Production request latency almost never looks like a normal distribution. It typically looks like one of:

- **Log-normal**: a long right tail, mode to the left of the mean, ratio mean/median often 1.5 to 4. Typical for simple CPU-bound work plus GC pauses or minor lock contention.
- **Weibull / Pareto-like**: extremely heavy tail, ratio p99/p50 often 10x to 100x. Typical for systems with queueing, cache effects, or compound failures.
- **Multi-modal**: a fast "cache hit" peak, a slower "cache miss" peak, an even slower "cold start / network retry" peak. The mean is a fiction somewhere between peaks that describes no real request.

For all three shapes, the arithmetic mean is *not* the median. For right-skewed distributions, mean > median. The mean is dragged to the right by the tail. A single 60-second pause out of a million fast requests moves the mean by 60 μs — enough to blow an SLO even though 999,999 users had a perfect experience.

## Why averages are actively misleading

1. **Composition failure.** A page view that makes 10 backend calls with p50 = 1 ms each has a p50 page load that is *not* 10 ms. Backend calls with independent latencies combine such that the slowest of the 10 dominates the aggregate. Dean & Barroso ("The Tail at Scale", CACM 2013) showed that if each backend has p99 = 1 s and a service fans out to 100 backends, 63 % of requests hit at least one p99 tail. Averaging tells you nothing useful about the composite experience.

2. **Users don't experience an average.** "Mean latency is 50 ms" does not mean any user saw 50 ms. Some saw 5 ms, some saw 5 s. SLOs are phrased about "most users" ("99% of requests complete in < 200 ms"), not "an average user". Only percentile assertions translate directly to SLO language.

3. **Averages are not robust.** A single outlier moves the mean. A single outlier does not move the 50th percentile. Stability of the metric across re-runs is essential for regression detection, and the mean is the least-stable summary statistic of a heavy-tailed distribution.

4. **Averages hide bimodality.** If 80 % of requests take 1 ms (cache hit) and 20 % take 1 s (cache miss), the mean is 200 ms — a latency that *no user ever experiences*. The two peaks are the story; the average is statistical fiction. Only a histogram or percentile report reveals the bimodality.

## Which percentiles, and why

The standard set in SRE practice:

- **p50 (median)**: "typical" user. Describes the fast path.
- **p90**: outer edge of common cases. Still happens hundreds of times per minute in a busy service.
- **p99**: the slowest 1%. In a service doing 1000 req/s, that's 10 per second — a persistent hot complaint if bad.
- **p99.9**: the slowest 1 in 1000. In a service doing 1000 req/s that's 1/s. In a service doing 10/s, that's once every 100 seconds — still user-visible.
- **p99.99**: tail of interest only at very high volume. Users who hit this threshold often abandon.

Rule of thumb (Dean & Barroso): measure one percentile beyond the one you SLO on. If your SLO is p99, measure and alert on p99.9, because a growing p99.9 predicts a p99 blowout.

## Where percentiles fail

1. **Not additive.** p99(A + B) ≠ p99(A) + p99(B) in general. This is the composition problem above. You cannot sum percentiles to compute end-to-end budgets. You must either:
   a) compose histograms (HdrHistogram supports this by addition, see `01KNZ4VB6JPYBYW64S7NNYS1CM`), or
   b) allocate individual-stage budgets that are strict enough to make the worst-case composition work, or
   c) measure end-to-end directly and back-propagate.

2. **Not averageable across time.** The average of yesterday's p99 and today's p99 is *not* the p99 over both days. To get a two-day p99 you need the two-day histogram. This is why Prometheus `histogram_quantile()` operates on bucket counts, not on previously-computed quantiles, and why stacking `avg(p99) over time` is a standard rookie mistake.

3. **Not averageable across hosts.** mean over fleet of per-host p99 ≠ fleet p99. Same reason.

4. **Requires a histogram / full distribution.** Computing p99 correctly from a stream requires either the whole sample set, or a histogram data structure, or an approximation sketch with known error bounds. See HdrHistogram note for the right way.

5. **Noisy in the tail.** p99.99 from 10 000 samples is the 10th-worst sample — a single outlier. CI width at the p99.99 level requires order-statistic bootstrapping or a very large sample count.

## The correct metric set for a load test

Minimum useful set, in order of importance:

1. **Latency histogram** (HdrHistogram or equivalent) over the measurement window. Everything else is derivable.
2. **p50, p90, p99, p99.9, max** — computed from the histogram.
3. **Throughput (actual, not offered)** — because latency without throughput is meaningless.
4. **Error rate**, broken down by error class.
5. **Max concurrency observed**, for Little's Law sanity check.
6. **Ramp / steady-state markers**, so later analysis can slice to the steady period.

What should *not* be the headline metric: mean latency, 95th percentile by itself, or a single "response time" number.

## The Dean & Barroso tail-amplification result

From "The Tail at Scale" (Jeff Dean, Luiz Barroso, CACM 2013) — a load-balanced service with N independent backends has an aggregate tail that grows with N. If each backend has a probability q of being in a "slow" state for a given request, the probability the whole service experiences at least one slow backend is 1 − (1−q)^N.

- N = 1, q = 0.01 → 1% slow.
- N = 10, q = 0.01 → 9.6% slow.
- N = 100, q = 0.01 → 63.4% slow.
- N = 1000, q = 0.01 → 99.996% slow.

That is: in a large fan-out service, a per-backend p99 of 1 s translates to a service-level median of 1 s. Tail latency is *not* the rare case; it is the common case at scale. The implication for load testing: measuring only p50 at small fan-out dramatically underestimates production experience at realistic fan-out.

## Adversarial reading

- **Percentiles still lose information.** The full histogram has the whole story; any fixed percentile set is a projection. For anomaly detection and regression analysis, compare histograms (via Kolmogorov–Smirnov, Earth Mover's distance, or simply plotting CDFs) rather than scalars.
- **Percentile choice is arbitrary.** Why p99 and not p99.5 or p99.9? The answer is "because it's what teams agreed to SLO on". If your SLO is p99.5, measure p99.5 plus one level deeper (p99.9).
- **Averages are fine for cumulative metrics.** Request *count*, bytes *transferred*, CPU *seconds consumed* — these are sums and averages are well-defined. The rule "averages lie" is specific to distributional quantities like latency.

## References

- Dean, J. & Barroso, L. — "The Tail at Scale" — CACM 56(2), Feb 2013 — [cacm.acm.org/magazines/2013/2/160173-the-tail-at-scale/fulltext](https://cacm.acm.org/magazines/2013/2/160173-the-tail-at-scale/fulltext)
- Tene, G. — "How NOT to Measure Latency" — coordinated omission note `01KNZ4VB6JB4Q5H3NPS72MZZ2A`
- HdrHistogram note — `01KNZ4VB6JPYBYW64S7NNYS1CM`
- Schwartz, B. — "Why percentiles don't work the way you think" — VividCortex blog, 2015 — one of the standard treatments of non-additivity and composition pitfalls.
