---
id: 01KNZ4VB6JY38THW04Z3MMGBZ3
title: "Load Testing — Verifying Behaviour Under Expected Load"
type: concept
tags: [load-testing, taxonomy, performance-testing, slo, meier-2007, istqb]
links:
  - target: 01KNZ4VB6JTC1Z9CGYN4Q1CCA6
    type: related
  - target: 01KNZ4VB6JCKKSRJ6FE6ST9183
    type: related
  - target: 01KNZ4VB6J29PS11RZNH5K0E47
    type: related
  - target: 01KNZ4VB6JVSPDK724EZFPA36H
    type: references
  - target: 01KNZ4VB6JJ51X8KRGKY6VH2W8
    type: related
  - target: 01KNZ4VB6JY3QDARVD4N06HR6X
    type: related
  - target: 01KNZ4VB6JJ702SZ7R31SMAJG2
    type: related
  - target: 01KNZ4VB6JRSN6YXB4KC63Y90K
    type: related
  - target: 01KNZ4VB6JK3TC0S5YZWHNNDEV
    type: related
  - target: 01KNZ4VB6JKC337NWTGFZRA8GF
    type: related
  - target: 01KNZ4VB6JX0CQ5RFAZDJTQMCS
    type: related
  - target: 01KNWE2Q9YZBAR140ZX5P36TQ5
    type: related
created: 2026-04-12
modified: 2026-04-12
source: "Meier, Farre, Bansode, Barber, Rea — Performance Testing Guidance for Web Applications — Microsoft patterns & practices, 2007, Chapter 2"
---

# Load Testing — Verifying Behaviour Under Expected Load

## Question answered

*"Will the system meet its SLOs under normal and peak production load?"*

Load testing is the type of performance test closest to everyday engineering practice. It is the baseline check that a service, under realistic traffic volume and mix, stays within its agreed response-time, throughput, and resource-utilisation budgets.

## Canonical definition (Meier et al. 2007, ch. 2)

From *Performance Testing Guidance for Web Applications* (Meier, Farre, Bansode, Barber, Rea — Microsoft patterns & practices, 2007), Chapter 2 "Types of Performance Testing":

> *Load test — To verify application behavior under normal and peak load conditions. Load testing is conducted to verify that your application can meet your desired performance objectives; these performance objectives are often specified in a service level agreement (SLA). A load test enables you to measure response times, throughput rates, and resource-utilization levels, and to identify your application's breaking point, assuming that the breaking point occurs below the peak load condition.*

Key properties of this definition:

1. The target load is *expected production load* — both a "normal" operating point and a "peak" (end-of-month, holiday, post-announcement). The load is not arbitrary.
2. The acceptance criteria are *SLO-based*: response time, throughput, and resource utilisation targets. Without explicit targets, there is nothing to verify.
3. The breaking point is *an output* of the test, not a design parameter. Stress testing (separate note) pushes past the breaking point deliberately; load testing stops at peak.
4. Endurance testing is defined by Meier et al. as *a subset of load testing* — sustained load rather than peak. Others (ISTQB) treat endurance/soak as a sibling.

## SLOs as test oracles

A load test without SLOs produces a report nobody knows how to interpret — "response time was 500 ms, is that good?". SLOs supply the oracle. Typical SLO shape:

> *99 % of login requests complete in under 300 ms when offered 1000 req/s.*

This has five parts, all of which the load test must realise:

- **Endpoint scope** (login requests).
- **Percentile target** (p99, *not* average).
- **Latency budget** (300 ms).
- **Offered load level** (1000 req/s).
- **Time window / duration** (implicit — typically "for the duration of the test, stationary, post-warmup").

The test passes if and only if, during a steady-state interval of the test, the p99 of login latency is < 300 ms while sustained throughput is ≥ 1000 req/s. Anything else is either a pass-by-coincidence or a fail.

## The workload must reflect production, not be a convenient synthetic

A load test using a single endpoint, zero cache churn, all-happy-path requests, and zero think time tests "how fast can this server serve the exact same request repeatedly". That is a microbenchmark, not a load test. A realistic load test needs:

- **Endpoint mix proportional to production.** 60 % reads, 30 % searches, 8 % writes, 2 % admin — derived from production traces or analytics.
- **Cardinality of inputs.** Different user IDs, different session tokens, different search terms. All-same-input tests are defeated by caching.
- **Realistic session shape.** Login → browse → add-to-cart → checkout, not pick-one-endpoint-at-random.
- **Think time.** Real users do not hammer endpoints. Real load-per-request comes from many simultaneous users doing small amounts of work each.
- **Arrival model.** Open model matching production arrival rate (see Schroeder et al. `01KNZ4VB6JX0CQ5RFAZDJTQMCS`). Closed "virtual users" tests answer a different question.

## Fit in the SDLC

1. **Design phase** — SLO targets are set. Back-of-envelope Little's Law check: does the architecture plausibly support target load?
2. **Integration / staging phase** — first load tests against staging environment. Catches gross misses (order-of-magnitude too slow).
3. **Pre-release** — final load test against a production-parity environment with realistic data volume. Goes into release checklist.
4. **Continuous** — automated load tests in CI, comparing against a baseline. Regressions gate the merge. This is the weak spot in most orgs: continuous load testing is rare because environment cost and test duration are high.
5. **Post-release** — production traffic replay and shadow testing. Arguably the most trustworthy form, because the environment and workload are real.

## Common anti-patterns

1. **"Load testing" a single endpoint with JMeter and calling it done.** Covers the smallest interesting case; tells you nothing about the real system. Fix: realistic mix.
2. **Running against a scaled-down environment.** A staging environment with 1 % of production data caches everything in RAM; production with 100x data is disk-bound. Fix: production-parity data volume. See environment parity note.
3. **Ignoring ramp-up and measuring steady state from t=0.** Includes cold-cache, JIT warm-up, connection pool initialisation. Fix: explicit steady-state window.
4. **Using coordinated-omission-vulnerable tools (JMeter default, classic wrk, Gatling default).** See `01KNZ4VB6JB4Q5H3NPS72MZZ2A`. Fix: open-model generator.
5. **Reporting averages only.** See percentile note `01KNZ4VB6JCJY0S4JYW2C3CHTR`. Fix: percentile-based pass/fail.
6. **"Peak load" = 10x daily average without looking at the real peak.** Real peaks are often 50x average or higher (flash sales, viral events). Fix: trace analysis, not guesswork.
7. **Reusing the same test inputs each run.** Cache warm on second run; regressions hidden. Fix: generate fresh input IDs per run.

## Acceptance criteria checklist

A well-scoped load test has a written, pre-registered answer to each of:

- What workload mix is being used? Where does the mix come from?
- What is the target throughput, and for how long is it sustained?
- Is the workload generator open or closed? Why?
- What is the SLO target (p50? p99? both? for which endpoints?)?
- How is steady state defined and how much of the run contributes to the measurement?
- What is the warm-up period? How is it excluded?
- What counts as a "fail" — a single breach, a sustained breach, a percentage of iterations?
- Is the test environment production-parity? In what dimensions is it not?

If any of these has no answer, the test result is unlabelled data, not a decision.

## References

- Meier, J.D., Farre, C., Bansode, P., Barber, S., Rea, D. — "Performance Testing Guidance for Web Applications", Microsoft patterns & practices, Sep 2007, Ch. 2 — [learn.microsoft.com/previous-versions/msp-n-p/bb924357(v=pandp.10)](https://learn.microsoft.com/en-us/previous-versions/msp-n-p/bb924357(v=pandp.10))
- ISO/IEC 25010 — Performance efficiency characteristic (time behaviour, resource utilisation, capacity) — [iso.org/standard/35733.html](https://www.iso.org/standard/35733.html)
- ISTQB Glossary — "load testing" entry — [glossary.istqb.org](https://glossary.istqb.org/)
- Schroeder et al. — Open-vs-closed workload models — `01KNZ4VB6JX0CQ5RFAZDJTQMCS`
- Coordinated omission — `01KNZ4VB6JB4Q5H3NPS72MZZ2A`
- Percentiles vs averages — `01KNZ4VB6JCJY0S4JYW2C3CHTR`
