---
id: 01KNZ4VB6JB4Q5H3NPS72MZZ2A
title: "Coordinated Omission — Gil Tene's 'How NOT to Measure Latency'"
type: literature
tags: [coordinated-omission, latency, gil-tene, hdrhistogram, percentiles, load-testing, measurement-bias]
links:
  - target: 01KNZ4VB6JX0CQ5RFAZDJTQMCS
    type: related
  - target: 01KNWE2QA5VP0K80TMSABACKWT
    type: extends
  - target: 01KNWE2QACWYZJXRJ8QE78T043
    type: extends
  - target: 01KNWGA5GS097K0SDS74JJ97X6
    type: related
  - target: 01KNZ4VB6J4TER1QCE9CKABBED
    type: related
  - target: 01KNZ4VB6JPYBYW64S7NNYS1CM
    type: related
  - target: 01KNZ4VB6JCJY0S4JYW2C3CHTR
    type: related
  - target: 01KNZ56MSNRKGMDGT9B745HVEB
    type: related
  - target: 01KNZ6FPTDYDB44VYN8Z4DQ7F4
    type: related
  - target: 01KNZ6WJ2XM5XSX3QEGH986VMD
    type: related
  - target: 01KNZ56MPVSZD05KM395ZKAM5J
    type: related
  - target: 01KNZ56MSFAM3EFBB1B48JKEWT
    type: related
  - target: 01KNZ5F8TVW837C1YJKKXFH504
    type: related
  - target: 01KNWE2Q9YZBAR140ZX5P36TQ5
    type: related
created: 2026-04-12
modified: 2026-04-12
source: "https://www.infoq.com/presentations/latency-response-time/"
---

# Coordinated Omission — Gil Tene, "How NOT to Measure Latency"

*Source: Gil Tene, "How NOT to Measure Latency" — InfoQ recording of Strange Loop 2015 variant, originally given ~2013. https://www.infoq.com/presentations/latency-response-time/*

*Gil Tene is the co-founder and CTO of Azul Systems, author of the JVM Zing pauseless garbage collector, and author of HdrHistogram. His talk "How NOT to Measure Latency" has been given dozens of times since 2013 at Strange Loop, LLVM dev meeting, QCon, JavaOne, and elsewhere; it is the canonical reference for this failure mode.*

## The bug in one paragraph

Almost every load-generation tool measures latency by recording the difference between "we sent the request" and "we got the response". In a **closed-loop** generator (which is almost all of them; see `01KNZ4VB6JX0CQ5RFAZDJTQMCS`), the client waits for the previous response before issuing the next. If the server takes a long time to respond, the client *does not send* the requests it was supposed to send during that stall. Those never-sent requests never contribute latency measurements. The measurement system is *coordinating* with the system it is measuring, *omitting* the samples that would have revealed the problem. Hence: **coordinated omission**.

## The canonical example

Gil's slide: a system where you are supposed to send a request every second for 100 seconds, and the server takes 100 seconds to serve one request, then instantly serves the rest. What a naive closed-loop tool reports:

- Sends request 1 at t=0.
- Waits 100 seconds for response.
- Sends request 2 at t=100, gets instant response.
- Sends request 3 at t=100, gets instant response.
- ... through request 100 at t=100.

The tool records 1 sample at 100 s and 99 samples at ~0 s. Histogram: `mean = 1 s, median = 0 s, p99 = 0 s, p99.99 = 100 s`. The only interesting value is buried in the one sample that happened to hit the bad time.

What actually happened in user-experience terms: at t = 1 s a user expected a response — they waited 99 s. At t = 2 s another user expected a response — they waited 98 s. At t = 50 s a user waited 50 s. The *actual* experienced latencies across the 100 user-seconds were uniformly spread from 100 s down to 0 s. True mean ≈ 50 s, true median = 50 s, true p99 = 99 s, true p99.99 = 99.99 s. The tool reported mean 1 s, median 0 s. **Off by 50x on the mean, and the median is off by infinity (0 vs 50).**

## Why this happens in every tool

The bug is not "bad code" — the naive record-to-response-time logic is genuinely what most benchmarking frameworks do:

```
for each virtual user:
    loop:
        t0 = now()
        send(request)
        response = recv()
        t1 = now()
        record_latency(t1 - t0)
        think(think_time)
```

Every measurement is post-hoc and relative to the moment the user *was able* to send. When the system stalls, the user is stuck inside `recv()` and is not issuing the requests that would experience the stall. The tool's rate and the system's capacity are *coordinated* via the closed loop.

## Coordinated omission is a specific instance of a broader failure

**Sampling can only record events that happen.** If events are suppressed by the measurement apparatus itself — because the tool stops firing during the slow period — no correction in post-processing can recover them. You cannot statistically infer a tail from samples that were structurally censored.

Schroeder et al. NSDI 2006 (`01KNZ4VB6JX0CQ5RFAZDJTQMCS`) framed this as a workload-modeling problem — closed models understate open-model behavior. Coordinated omission is the *latency metric* side of the same coin: the tail gets hidden.

## The correction: service-time vs response-time

Gil's recommended fix has two parts:

**Part 1 — Use a real open-model load generator.** If your tool is open (issues requests on a schedule independent of completions), it physically cannot coordinate-omit. wrk2, Gatling open-injection mode, k6 constant-arrival-rate executor, httperf, Vegeta, and Gil's own Response-time-correcting Grinder variant are designed for this.

**Part 2 — If your tool is closed, apply "Intended" scheduling.** For each request, record not just `(t1 - t0)` but `(t1 - t_scheduled)` where `t_scheduled` was when the request *should* have gone out according to the target rate. When the system is keeping up, t_scheduled ≈ t0 and the two numbers agree. When the system stalls, t_scheduled is earlier than t0 and the correctly-computed latency includes the stall. This is what HdrHistogram's `recordValueWithExpectedInterval()` API does: given an expected inter-sample interval, it back-fills synthetic samples into the histogram corresponding to the requests that *would have been issued* during a stall.

Pseudocode of the correction:

```
record_corrected(measured_latency, expected_interval):
    h.record(measured_latency)
    if measured_latency > expected_interval:
        missing = measured_latency - expected_interval
        while missing > 0:
            h.record(missing)  // synthetic sample for a request that would have seen this much delay
            missing -= expected_interval
```

This is a *heuristic* back-fill. It assumes the system was stalled uniformly during the gap. It is not perfect, but on a ramp-or-stall it is drastically closer to the truth than naive measurement.

## Why averages lie

With coordinated omission, mean latency is dominated by the many fast-path samples that were all collected while the system was idle or fast, and by the single (or few) stall samples. The huge bulk of the slow-path experience — the "what would have happened to users trying to use the system during the stall" — is *not in the histogram*. Averages over a histogram that's missing its right tail are meaningless.

But even without coordinated omission, averages of latency distributions are generally misleading because the distribution is heavy-tailed (log-normal, Weibull, Pareto in many real systems). The mean is pulled hard by the tail. Gil's other favourite slide: "your average is always less than your 50th percentile is always less than your 90th" is *wrong* — for right-skewed distributions, mean > median. A benchmark that reports only "average response time" hides both the skew and the tail.

## The HdrHistogram connection

HdrHistogram (separate note, `01KNZ4VB6JPYBYW64S7NNYS1CM`) was built in large part to make correct latency measurement possible in practice:

1. **It is fast enough** (3–6 ns per record on 2014 hardware) that you can record *every single latency sample* without sampling, eliminating the "too slow to record in hot loops" excuse.
2. **It has constant memory** — you don't have to decide in advance what the range of interest is or truncate outliers.
3. **It has the `recordValueWithExpectedInterval` API** for the corrected-closed-loop case.
4. **It is lossless under merge** — histograms from multiple clients or multiple measurement periods combine without losing information.

Before HdrHistogram, typical tools either (a) stored every sample in a giant array and sorted at the end (slow, memory-hungry), or (b) used t-digest/quantile sketches with bounded relative error (fast but lossy and hard to combine), or (c) recorded only the average and max (lossy in the most damaging way). HdrHistogram is a sweet spot for latency specifically.

## Adversarial reading

- **Correction via `recordValueWithExpectedInterval` is a model, not ground truth.** It assumes a uniform schedule that was interrupted. If the intended traffic was bursty, the correction can itself be wrong. The right answer is always "use an open-model generator", and the correction is a second-best for legacy closed-loop tools.
- **Coordinated omission is a problem in the *generator*, not in the SUT.** If your production system is being called by real users at an independent rate, production latency histograms do not suffer from coordinated omission (assuming you record `entry_to_server - request_arrival_at_edge`, which server-side instrumentation normally does). The bug is specific to *load tests with virtual users in a closed loop*.
- **Naming.** Gil coined "coordinated omission" around 2013. The underlying closed-loop issue was known in operations research decades earlier (see Schroeder et al. 2006 for the survey). Gil's contribution is the precise naming, the back-fill correction, and popularising HdrHistogram as the tool.

## Relevance to APEX G-46

1. If APEX adds any form of load generation (out of scope for G-46 but a likely phase-2 extension), it must default to open-model generation.
2. Any APEX-generated latency report must distinguish *measured* from *intended* latency, or simply disallow closed-loop measurement.
3. APEX's "SLO verification" feature (part of the G-46 spec) must use percentile-aware assertions, never average-based assertions. The spec already says this; coordinated omission is the justification.

## References

- Tene, G. — "How NOT to Measure Latency" — InfoQ video, 2015 — [infoq.com/presentations/latency-response-time](https://www.infoq.com/presentations/latency-response-time/)
- Tene, G. — HdrHistogram project — [github.com/HdrHistogram/HdrHistogram](https://github.com/HdrHistogram/HdrHistogram)
- wrk2 — Will Glozer / Gil Tene — the open-model rewrite of wrk specifically designed to avoid coordinated omission — [github.com/giltene/wrk2](https://github.com/giltene/wrk2)
- Schroeder, Wierman, Harchol-Balter — open-vs-closed note — `01KNZ4VB6JX0CQ5RFAZDJTQMCS`
- HdrHistogram note — `01KNZ4VB6JPYBYW64S7NNYS1CM`
