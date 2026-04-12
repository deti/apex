---
id: 01KNZ6FPTDYDB44VYN8Z4DQ7F4
title: "Coordinated Omission — Gil Tene, wrk2, HdrHistogram"
type: permanent
tags: [coordinated-omission, gil-tene, latency, hdrhistogram, wrk2, benchmark-bias, measurement-bias]
links:
  - target: 01KNZ4VB6JB4Q5H3NPS72MZZ2A
    type: related
  - target: 01KNZ4VB6JPYBYW64S7NNYS1CM
    type: related
  - target: 01KNZ56MSNRKGMDGT9B745HVEB
    type: related
  - target: 01KNZ6WJ2XM5XSX3QEGH986VMD
    type: related
  - target: 01KNZ56MPVSZD05KM395ZKAM5J
    type: related
  - target: 01KNWE2Q9YZBAR140ZX5P36TQ5
    type: related
created: 2026-04-11T21:17:08.813179+00:00
modified: 2026-04-11T21:17:08.813181+00:00
---

*Source: Gil Tene — "How NOT to Measure Latency" — Strange Loop conference talk. Also: github.com/giltene/wrk2, HdrHistogram, and numerous follow-ups (Martin Thompson, ScyllaDB, psy-lob-saw blog).*

Coordinated omission is a specific form of measurement bias that affects load-generating benchmarks for latency-sensitive systems. Gil Tene coined the term around 2013 and argued that **the bias is present in almost every popular load generator** — wrk, ab, JMeter, many in-house tools. Teams using these tools report p99 latencies that are systematically and dramatically optimistic.

## The bug

A load generator intends to simulate a steady request rate, e.g., 1000 rps. It does so with a loop:

```
while not done:
    t_start = now()
    response = send_request()
    t_end = now()
    record(t_end - t_start)
    sleep_until(t_start + interval)
```

Looks fine. But consider what happens when the system under test **stalls**: suppose a 10-second GC pause. The `send_request` call on the one unlucky thread blocks for 10 seconds. During those 10 seconds, the load generator is **not sending new requests**. When the stall ends, it records one high-latency sample (10s) and then races to catch up.

The catastrophe: during the 10-second stall, the generator should have sent **10,000 requests** (at 1000 rps), and those 10,000 requests should all have recorded high latency, because a real client submitting a request at second 5 of the stall would have waited 5 seconds for its response. But the generator only recorded **one** sample — the blocked thread's own request — and then resumed normal operation. The other 9,999 samples that *would have been slow* are never generated.

The histogram of recorded latencies therefore contains one outlier and then a sea of normal samples. The p99 and p99.9 barely move. The reported latency distribution looks fine. **The user-visible latency distribution during the stall was catastrophic.**

Gil Tene's name for this: **coordinated omission** — the measurement loop "coordinates" with the system under test by omitting exactly the samples it most needs to record.

## Why it's pervasive

Coordinated omission occurs whenever:
- A load generator uses a synchronous or tightly-coupled loop.
- The send-rate depends on the response rate (closed-loop).
- Samples are recorded only when a response arrives.

This describes almost every popular load generator. `ab` (Apache Bench), `siege`, naïve `wrk`, JMeter without specific configuration, and most custom harnesses all suffer from it. **YCSB** (Yahoo Cloud Serving Benchmark) had coordinated omission for years and has been retrofitted (see the psy-lob-saw blog posts on fixing it).

## The scale of the distortion

In Tene's Strange Loop talk and the ScyllaDB writeups, examples show:
- A system with a real p99 of 10 seconds reported as 12ms.
- A GC pause of 1 second invisible in the reported histogram.
- p99.9 latencies that cannot exceed a few times the request interval no matter how bad the stall.

In extreme cases, the reported percentiles are wrong by **three orders of magnitude**. This is not a small correction; it is the difference between "our p99 SLO is met" and "our p99 SLO is grossly violated."

## HdrHistogram's correction: `recordValueWithExpectedInterval`

HdrHistogram (Gil Tene's high-dynamic-range histogram library) provides:

```java
histogram.recordValueWithExpectedInterval(value, expectedIntervalBetweenValueSamples);
```

When `value > expectedIntervalBetweenValueSamples`, HdrHistogram synthesises the missing samples by assuming the load generator was blocked. It records additional samples at `value - interval, value - 2*interval, ...` down to `interval`, reconstructing the latencies that would have been observed had the load generator not coordinated with the stall.

This is a **post-hoc correction** and relies on the assumption that the load generator would have continued at the target rate. It's principled but approximate — the real fix is to not let the load generator stall in the first place.

## `wrk2` — the open-loop alternative

Gil Tene forked `wrk` into `wrk2` (github.com/giltene/wrk2) with a fundamental change: instead of a closed-loop "send-and-wait" design, `wrk2` uses an **open-loop constant-throughput** model. The generator schedules requests at fixed time offsets regardless of whether previous requests have completed. If the system under test stalls, `wrk2` records accurate "intended vs actual" latencies for all the queued requests, because the schedule was determined by wall-clock time rather than by responses.

The result: `wrk2` reports accurate tail latencies even in the presence of long GC pauses.

## Implications for CI perf gating

Coordinated omission is primarily a **load testing** (not microbenchmarking) concern, but it matters for CI perf gating in several ways:

1. **Integration / soak tests** that report p99 latency are suspect if they use `ab`, old `wrk`, or custom synchronous harnesses.
2. **Regression detection on p99 metrics** can miss genuine tail-latency regressions if the measurement loop is CO-biased.
3. **SLO-based gates** that check "p99 latency < X" on a load test are only meaningful if the load test is open-loop or uses `recordValueWithExpectedInterval`.
4. **Oracle for performance**: coordinated omission is a direct demonstration that the oracle ("did we meet the SLO?") can produce wrong answers due to measurement bias, even when the statistical test downstream is flawless.

## Adversarial commentary

- **Not every "synchronous" load test has the bug.** If the system never stalls longer than the request interval, the bug is not triggered. CO is a problem specifically when the service has tail behaviour that the benchmark is supposed to characterise.
- **HdrHistogram's correction is a heuristic.** It assumes the load generator would have sent more requests in a steady stream, which is plausible for constant-rate loads but wrong for bursty traffic models.
- **Open-loop load generators can themselves introduce artifacts.** At saturation, the generator queues requests indefinitely and you measure the queueing delay of the generator, not the server. `wrk2` and friends need careful interpretation when the target rate exceeds the service rate.
- **CI perf gates rarely include true open-loop load tests.** The operational complexity of running `wrk2` correctly in CI is higher than running `ab`, so a lot of teams silently rely on biased measurements without realising it.

## Connections

- Mytkowicz et al. 2009 — measurement bias at the microbenchmark layer; coordinated omission is bias at the macro-benchmark layer.
- Welch's t-test (dedicated note) — p-values computed on CO-biased data are wrong about something that is itself wrong.
- SLOs as oracles (dedicated note) — an SLO check is only as accurate as the measurement feeding it.
- `tdigest` and related sketches — have their own CO-corrections, see tdunning/t-digest issue #128.

## References

- Tene, G. "How NOT to Measure Latency" — Strange Loop talk. youtube.com/watch?v=lJ8ydIuPFeU.
- wrk2: github.com/giltene/wrk2.
- HdrHistogram: github.com/HdrHistogram/HdrHistogram.
- psy-lob-saw.blogspot.com/2015/03/fixing-ycsb-coordinated-omission.html.
- ScyllaDB: scylladb.com/2021/04/22/on-coordinated-omission/.
