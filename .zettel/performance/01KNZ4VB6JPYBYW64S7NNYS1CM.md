---
id: 01KNZ4VB6JPYBYW64S7NNYS1CM
title: "HdrHistogram — High Dynamic Range Histogram for Latency Measurement"
type: literature
tags: [hdrhistogram, latency, percentiles, gil-tene, measurement, histogram, tool]
links:
  - target: 01KNZ4VB6JB4Q5H3NPS72MZZ2A
    type: extends
  - target: 01KNZ4VB6JCJY0S4JYW2C3CHTR
    type: related
  - target: 01KNZ4VB6JCD7A2BMXFN1AWGP4
    type: related
  - target: 01KNZ56MSNRKGMDGT9B745HVEB
    type: related
  - target: 01KNZ56MPVSZD05KM395ZKAM5J
    type: related
  - target: 01KNZ6FPTDYDB44VYN8Z4DQ7F4
    type: related
  - target: 01KNWE2Q9YZBAR140ZX5P36TQ5
    type: related
created: 2026-04-12
modified: 2026-04-12
source: "https://hdrhistogram.github.io/HdrHistogram/"
---

# HdrHistogram — High Dynamic Range Histogram

*Source: https://hdrhistogram.github.io/HdrHistogram/ — fetched 2026-04-12.*
*Original author: Gil Tene (Azul Systems). Ports by Mike Barker (C), Matt Warren (C#), Darach Ennis (Erlang), plus Python, Go, JavaScript, Rust, Swift.*

## What it is

A fixed-memory, O(1)-record, configurable-precision histogram data structure for recording integer-valued samples (typically latency nanoseconds) across a large dynamic range, with correct lossless percentile reporting.

The three design choices that distinguish it from "just make an array and sort at the end":

1. **Bucketing at configurable significant-digit precision** — you declare e.g. "3 significant digits" and the histogram guarantees the reported quantile is accurate to that many digits *of the value at that quantile*, not of the total range. For latency this means 3-sig-digit HdrHistogram reports p99 = 1234 ms as anywhere from 1230 to 1239 ms — 1 % relative error.
2. **High dynamic range** — tracks values from 1 to (up to) 3.6 trillion within a single histogram. A single histogram can span "1 ns to 1 hour" and maintain 1 ns resolution at the low end.
3. **Constant-time record and constant memory** — Tene reports 3–6 ns per `recordValue` on Intel 2014 hardware. No allocation during recording. Memory footprint depends only on declared range × declared precision, not on sample count.

The clever trick: HdrHistogram uses a logarithmic-magnitude outer structure (powers of 2) combined with a linear sub-bucket at each magnitude. So bucket width grows exponentially with value but is constant in *relative* terms. Recording takes a log2 (hardware `lzcnt`) plus an integer offset plus an atomic increment. No hashing, no collisions, no resize.

## Why it exists

Before HdrHistogram (pre-2012), latency measurement in load tests was a mess:

- **Mean and max only.** Hides the distribution entirely. Mean is pulled by the tail, max is a single-sample estimate.
- **Reservoir sampling with sorted array.** Lossy; the reservoir must be small enough to sort; the tail gets under-sampled because it's rare; combining across machines is hard.
- **Approximation sketches (CKMS, t-digest, q-digest).** Fast and bounded-memory but each has a lossiness mode — CKMS drops samples, t-digest centroids blur neighbouring values, q-digest has a hard-to-reason-about error profile. All are hard to combine exactly across observation windows.
- **Store every sample in a big list.** Correct but O(n) memory and O(n log n) to compute quantiles. Collapses under high sample rates.

Tene's observation: latency measurement has a *specific* structure — integer nanoseconds, positive, right-skewed, we mostly care about high quantiles of a heavy-tailed distribution — and that structure is exploited by the log-linear bucketing. Within that structure, HdrHistogram is near-optimal.

## The API surface

```java
Histogram h = new Histogram(3);           // 3 sig digits
h.recordValue(latencyNanos);              // O(1)
long p999 = h.getValueAtPercentile(99.9); // O(buckets)

// Correction for coordinated omission
h.recordValueWithExpectedInterval(latencyNanos, expectedIntervalNanos);

// Merge histograms from multiple threads / machines
globalHist.add(perThreadHist);

// Serialize for transport to a dashboard
ByteBuffer buf = h.encodeIntoCompressedByteBuffer();
```

Key methods:

- `recordValue(v)` — lossless record, atomic-incrementing the right bucket.
- `recordValueWithExpectedInterval(v, expectedInterval)` — if `v > expectedInterval`, back-fills synthetic samples at `v - expectedInterval`, `v - 2*expectedInterval`, ... to approximate the latency experienced by requests that *would have been issued* during the stall. This is Gil's coordinated-omission back-fill heuristic baked into the API.
- `copyCorrectedForCoordinatedOmission(expectedInterval)` — same correction applied post-hoc to an existing histogram.
- `getValueAtPercentile(p)` — O(number of buckets), roughly O(log dynamic range).
- `add(other)` — merge lossless.

## Why "high dynamic range" matters

Real latency distributions span many orders of magnitude. In a web service:

- p50 = 500 μs (cache hit)
- p99 = 50 ms (cold path)
- p99.99 = 5 s (GC pause + network retransmit)
- max = 30 s (client timeout)

That's 6 orders of magnitude between the common case and the rare tail. A fixed-bin-width histogram ("1 ms buckets from 0 to 60 000 ms") gives you 1 ms resolution at p50, which is useless (can't distinguish 500 μs from 1 ms), and 60 000 buckets to store. HdrHistogram with 3 sig digits covers this same range in a few thousand buckets and gives 1 % relative error across the entire range. You can record tail samples without deciding in advance that the tail is interesting.

## Combining with percentile-math correctness

Two important properties for combining data across observation windows and machines:

1. **Exact merge**: two histograms with the same (range, precision) parameters can be merged exactly by per-bucket addition. No loss. Quantiles of the merged histogram are exact (up to the declared precision) across the combined sample set. This is the feature that makes HdrHistogram viable for distributed load tests where each generator records locally and aggregates at the end.
2. **No sample loss on overflow** — if you declare a range of 1 hour and a sample is 2 hours, the histogram reports the sample as "above range" rather than silently clipping, letting you know your declared range was too small. Contrast with reservoir sampling which silently loses samples.

## Adversarial reading

- **Precision is per-value, not per-quantile.** At 3 sig digits, a p99 of 123 ms is ±1 ms, but if p99.99 is 12 345 ms it's ±10 ms. Fine for most uses; surprising if you expect absolute precision.
- **Memory grows linearly in declared precision.** 5 sig digits doubles memory vs 3 sig digits. Most users should stick to 2–3 sig digits; that's 1 % relative error, which is well within other sources of noise.
- **Integer only.** You record integer nanos (or integer microseconds). Fractional values must be scaled. This is a feature — no FP rounding during record — but the scaling can bite if you forget.
- **No time-window support built in.** If you want "p99 over the last 1 minute, updated every 5 seconds", you need to build a sliding-window wrapper that maintains multiple HdrHistograms. Several libraries do this (Dropwizard Metrics' `HdrHistogramReservoir`, Prometheus's native histograms since 2023) but the base API is "one histogram, record forever".
- **Coordinated-omission correction is a heuristic.** The `recordValueWithExpectedInterval` back-fill assumes uniform load; it does not and cannot reconstruct the true tail for bursty workloads. It is better than nothing but worse than using an open-model generator in the first place.

## Why it is the de-facto standard

By 2026, HdrHistogram (or a port of it) is embedded in:

- **wrk2** — Gil's fork of wrk, reporting percentiles with coordinated-omission correction.
- **Dropwizard Metrics** — `HdrHistogramReservoir`.
- **Prometheus native histograms** (since 2023) — based on a similar log-bucket design, interoperable with HdrHistogram serialized form.
- **Apache Cassandra** — internal latency tracking.
- **Netty**, **Aeron**, **LMAX Disruptor** — as the built-in latency instrumentation.
- **k6** — uses OpenMetrics with an HDR-inspired bucket scheme.
- **Go** — `codahale/hdrhistogram-go`.
- **Rust** — `HdrHistogram/HdrHistogram_rust`.

## References

- HdrHistogram home — [hdrhistogram.github.io/HdrHistogram](https://hdrhistogram.github.io/HdrHistogram/)
- Source — [github.com/HdrHistogram/HdrHistogram](https://github.com/HdrHistogram/HdrHistogram)
- Tene, G. "How NOT to Measure Latency" — coordinated omission note `01KNZ4VB6JB4Q5H3NPS72MZZ2A`
- wrk2 — [github.com/giltene/wrk2](https://github.com/giltene/wrk2)
