---
id: 01KNZ4VB6JXAZA2TBRCD5DERK9
title: "Four Golden Signals — Google SRE"
type: literature
tags: [golden-signals, google-sre, monitoring, slo, latency, traffic, errors, saturation]
links:
  - target: 01KNZ4VB6JHP7W47HM7QREWW53
    type: related
  - target: 01KNZ4VB6J6ED6F3YHN1SMDNQ5
    type: related
  - target: 01KNZ4VB6JQZHJVB2EQK6HVXE0
    type: related
  - target: 01KNWE2Q9YZBAR140ZX5P36TQ5
    type: related
created: 2026-04-12
modified: 2026-04-12
source: "https://sre.google/sre-book/monitoring-distributed-systems/"
---

# The Four Golden Signals

*Source: Beyer, Jones, Petoff, Murphy — Site Reliability Engineering, O'Reilly 2016, Chapter 6 "Monitoring Distributed Systems" — https://sre.google/sre-book/monitoring-distributed-systems/*

## The rule

If you can only measure four things about a user-facing system, measure:

1. **Latency** — the time to service a request.
2. **Traffic** — the demand being placed on your system, measured in a system-specific way (req/s for web; I/O ops or bytes/s for storage; transactions/s for DB).
3. **Errors** — the rate of requests that fail, either explicitly (HTTP 500) or implicitly (wrong content) or by policy (exceeded-latency-budget).
4. **Saturation** — how close the system is to its capacity, emphasised for the most constrained resource.

Google SRE: *"If you measure all four golden signals and page a human when one signal is problematic (or, in the case of saturation, nearly problematic), your service will be at least decently covered by monitoring."*

## Why these four

The Golden Signals are a superset of RED Method (Rate, Errors, Duration) and overlap partially with USE Method (Utilisation, Saturation, Errors). The overlap is deliberate: Google SRE wants one list that suffices as a default for user-facing services.

| Golden Signal | RED equivalent | USE equivalent |
|---|---|---|
| Latency | Duration | — (U/S give latency indirectly) |
| Traffic | Rate | — (implicit in utilisation) |
| Errors | Errors | Errors |
| Saturation | — (absent from RED) | Saturation |

The key insights:

- **RED is missing saturation.** This is a known gap (see RED note). Golden Signals fix it.
- **USE is missing request-rate.** USE is resource-centric; it sees "the CPU was 80 % busy" but not "we served 1000 req/s". Golden Signals fix it.
- **All three — Golden Signals, RED, USE — agree on errors.** Errors are universal.

## Critical detail: latency must be split by success

The SRE book is emphatic about this:

> *"an HTTP 500 error triggered due to loss of connection to a database ... might be served very quickly; however, as an HTTP 500 error indicates a failed request, factoring 500s into your overall latency might result in misleading calculations. On the other hand, a slow error is even worse than a fast error!"*

Implications:

1. Latency histogram of *successful* requests and of *errored* requests should be tracked separately.
2. Alerting on aggregate latency is misleading; you get "the average of a very slow 200 and a very fast 500 is fine" confusion.
3. Slow errors are their own category of alarming — they suggest a specific failure mode (timeout-driven) different from fast errors (rejection-driven).

## Saturation — the hardest to measure right

Saturation is the trickiest of the four because most systems don't expose a single "saturation percentage". The book's note:

> *"Many systems degrade in performance before they achieve 100% utilization, so having a utilization target is essential."*

Practical saturation metrics (service-specific):

- **Thread pool**: current in-use / pool max.
- **Connection pool**: active / max.
- **Queue depth**: items in queue; growth over time.
- **Memory**: working set / physical memory (plus pressure indicators).
- **Disk I/O**: iostat `%util` (misleading at 100 %), queue depth, request latency.
- **Network**: bytes/s / NIC bandwidth, retransmit rate.
- **CPU**: run queue length (not utilisation — utilisation is a lagging, ambiguous indicator).

Gregg's USE method note (`01KNZ4VB6JHP7W47HM7QREWW53`) is the canonical reference for turning "saturation" into specific per-resource metrics.

## Signal cardinality and service boundaries

Each signal is a metric family, not a single metric. In practice:

- Latency: histogram per route, per method, per status class.
- Traffic: counter per route, per method, per status class.
- Errors: counter per route, per method, per error class (4xx vs 5xx, specific codes).
- Saturation: gauges per resource.

For a service with 20 endpoints and 5 status classes, that's hundreds of time series per service. Prometheus cardinality budgets need to accommodate this.

## Alerting from Golden Signals

Google SRE recommends alerting on *symptoms* (the four Golden Signals) rather than *causes* (individual log messages, specific exceptions, process restarts). Symptom-based alerting:

- Paging on "latency p99 > 500 ms for 10 minutes" vs paging on "database connection pool utilisation > 80 %".
- The first says "users are unhappy". The second says "a specific thing is happening". The first is user-visible and genuinely requires response; the second may be a false alarm (pool is fine at 80 %, will drain in a minute).

Cause-based alerts are useful as *dashboards and runbooks*, not as pages. The page says "p99 is high"; the runbook says "check the connection pool, the queue depth, the GC pause histogram" — i.e., walk the USE Method.

## Anti-patterns

1. **Alerting on each signal independently with equal weight.** Traffic-rate-dropping by 10 % is expected during off-peak; the same drop at peak is an outage. Alerts must be context-aware (time-of-day thresholds or anomaly detection).

2. **Measuring only on the server side.** Client-observed latency (DNS + TLS + network + server) is what matters to users. Server-side-only measurement undercounts. Where possible, instrument at the load balancer or edge.

3. **Using averages for latency.** Again: see percentile note.

4. **Ignoring saturation because it's hard.** Saturation is the signal that predicts future failure. Skipping it means every incident is a surprise. Fix: invest the measurement work.

5. **No tying to SLOs.** The Golden Signals are the *inputs* to SLOs. If the signals exist but no SLO targets them, they're dashboard decoration, not oracles.

## Relevance to load testing

A load test output should report all four Golden Signals for the SUT:

- **Latency histogram**, split by success/error.
- **Traffic rate** (offered and completed), split by endpoint.
- **Errors**, categorised.
- **Saturation** of the most-stressed resource during the test.

The test *passes* if all four are within bounds during the steady-state window. Reporting only latency (which is the common default) means you get "latency was fine but we quietly shed 20 % of traffic" failures.

## References

- Beyer, B., Jones, C., Petoff, J., Murphy, N. — *Site Reliability Engineering*, O'Reilly 2016 — Chapter 6 "Monitoring Distributed Systems" — [sre.google/sre-book/monitoring-distributed-systems](https://sre.google/sre-book/monitoring-distributed-systems/)
- RED Method note — `01KNZ4VB6J6ED6F3YHN1SMDNQ5`.
- USE Method note — `01KNZ4VB6JHP7W47HM7QREWW53`.
- SLOs as oracles — `01KNZ4VB6JQZHJVB2EQK6HVXE0`.
