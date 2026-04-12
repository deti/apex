---
id: 01KNZ4VB6JMSSE4E40PBE23S3M
title: "Dean & Barroso — The Tail at Scale (CACM 2013)"
type: literature
tags: [tail-at-scale, jeff-dean, luiz-barroso, cacm-2013, tail-latency, fan-out, hedged-requests, google]
links:
  - target: 01KNZ4VB6JCJY0S4JYW2C3CHTR
    type: related
  - target: 01KNZ4VB6JCD7A2BMXFN1AWGP4
    type: related
  - target: 01KNZ666VE9ZGQ8DKVV36PZ7MZ
    type: related
  - target: 01KNZ4VB6JB4Q5H3NPS72MZZ2A
    type: related
  - target: 01KNZ4VB6JPYBYW64S7NNYS1CM
    type: related
  - target: 01KNZ666VRMB0N00T1E5GRPHT4
    type: related
  - target: 01KNWE2Q9YZBAR140ZX5P36TQ5
    type: related
created: 2026-04-12
modified: 2026-04-12
source: "Dean, J. & Barroso, L.A. — 'The Tail at Scale' — Communications of the ACM 56(2): 74-80, February 2013"
---

# Dean & Barroso — The Tail at Scale

*Source: Dean, J. & Barroso, L.A. — "The Tail at Scale" — Communications of the ACM 56(2):74–80, February 2013. Both authors at Google at the time.*

## The thesis

Services that fan out to many backends — which is all modern user-facing services — experience tail latency amplification so severe that **rare slow requests become common**. The standard "make each backend fast" prescription barely helps, because the relationship between per-backend tail probability and fan-out is exponential. Dean and Barroso argue that (1) tolerating variability in shared environments is inevitable, and (2) tail-tolerance techniques at the service level (hedging, request scheduling) are more effective than attempting to eliminate per-backend tail latency at the source.

## The math

Let q be the probability that a single backend is in a "slow" state (i.e., above some threshold — typically, "above this backend's p99") when serving an individual request. For a service that fans out to N independent backends and needs all to respond, the probability that *at least one* is slow is:

    P(slow) = 1 − (1 − q)^N

For q = 0.01 (1 % slow threshold):

| N | P(slow) |
|---|---|
| 1 | 1 % |
| 10 | 9.6 % |
| 100 | 63.4 % |
| 1 000 | 99.996 % |

Concretely: if every backend is at p99 = 1 s (1 % of requests take > 1 s) and the service fans out to 100 backends, the *median* (p50) of service-level latency is > 1 s. The rare tail of one backend is the common case of the composite service.

## Why it's counter-intuitive

Engineers are trained to think "1 % tail is rare". The intuition collapses at fan-out scale. A team that achieves "99 % < 10 ms on each microservice" still sees a user-visible p50 of 100+ ms because every user request touches dozens of microservices.

The conclusion engineers often draw is "we must make every backend's tail tighter". Dean and Barroso show this is quixotic: reducing per-backend q from 1 % to 0.1 % lowers the 100-fan-out P(slow) from 63 % to 10 %, which is improvement but still painful. Reducing to 0.01 % (3 orders of magnitude improvement) lowers P(slow) to 1 %. The returns on tail-reduction are real but require implausible per-backend tightness.

## The alternative: tail-tolerance

Dean and Barroso argue that the winning strategy is not to *eliminate* per-backend tail but to *tolerate* it. Techniques that work at the service level:

### 1. Hedged requests

Issue the primary request. If it hasn't returned within, say, the 95th-percentile latency of that backend, issue a duplicate request to a different backend and take whichever returns first. Cancel the losing request if the system supports it.

**Cost**: a small fraction of extra requests (5 %, if you hedge after p95). **Benefit**: the tail is the minimum of two distributions, so p99 of the hedged service is approximately p95 of the single-backend — a dramatic reduction in tail latency at small cost.

**Applicability**: idempotent read operations. Writes are harder because duplicates are not safe.

### 2. Tied requests

Issue the request to *both* backends simultaneously, with each backend knowing the other is also handling the request. The backend that's ready first runs the operation; the other cancels. More aggressive than hedged requests; lower latency but higher extra-load cost.

Google uses this in its distributed GFS/Colossus reads — the primary and secondary replicas both serve the read, and whichever is first wins.

### 3. Request-queue reordering

Prioritise small/short requests over long ones. Reduces the probability that a short user-facing request is queued behind a long background operation. The classical SRPT scheduling result from queueing theory — Schroeder et al. show (`01KNZ4VB6JX0CQ5RFAZDJTQMCS`) that in open systems SRPT reduces mean response time by a large factor.

### 4. Selective replication

Replicate hot items to multiple backends so a read can pick the fastest. A form of built-in hedging at the data layer.

### 5. Latency-induced probation

Temporarily remove slow backends from the pool — not errors, just slow — and route around them. Returns to the pool after they recover. This is what "latency-aware load balancing" does.

### 6. Micro-partitioning

Split work into many small partitions (more than CPUs), so the scheduler can move work away from slow workers. Reduces straggler effect. Common in MapReduce and Spark.

### 7. Cross-request adaptation

Keep a per-backend latency histogram; route new requests preferentially to the current-fastest. "Join the shortest queue" style adaptive load balancing.

## Where variability comes from

Dean and Barroso enumerate sources of per-backend variability:

- **Shared resources**: other processes on the same host, other tenants in the same VM, OS housekeeping, kernel threads.
- **Daemons**: log rotation, backup, monitoring agents.
- **Global resource sharing**: network bandwidth contention with other flows, shared storage backing, hypervisor scheduling.
- **Maintenance activities**: GC, compaction, log cleanup.
- **Queuing**: at every layer of the stack, every queue introduces a tail.
- **Power limits**: thermal throttling, core parking.
- **Garbage collection**: JVM or CLR GC pauses. Gil Tene's Zing GC was built in large part to address this class of tail.
- **Energy management**: transitions in and out of low-power states.

Many of these are fundamental: in a shared environment you cannot eliminate them. Tail tolerance is the engineering response to that.

## Relationship to load testing

1. **A load test that reports p99 on a microservice misses the tail-at-scale problem.** The service's p99 in isolation is irrelevant; what matters is the *aggregate* p99 when composed with N others. A microservice load test without tail tolerance analysis is incomplete.

2. **Load tests should measure end-to-end latency**, not per-service. If you only have per-service numbers and want end-to-end, you have to compose, and composition is hard (see percentile composition note `01KNZ4VB6JCD7A2BMXFN1AWGP4`).

3. **Testing tail-tolerance mechanisms**: hedged requests, backend probation, SRPT, etc. are themselves features that need load tests. A well-designed load test for a hedged-request system exercises the hedge path (injects artificial slowness into the primary backend) and verifies that service-level p99 is bounded by the backup-path latency.

## The p99 rule of thumb

A useful rule extracted from the paper: **measure one percentile beyond the one you SLO on**. If your SLO is p99, measure and alert on p99.9. Because of the tail amplification effect, a growing p99.9 is a leading indicator of a future p99 blowout. The paper recommends building dashboards and alerting with this in mind.

## Adversarial reading

- The mathematics assumes *independence* between backends. If backends share a root cause of slowness (shared DB, shared cache, correlated GC), the formula overstates the amplification. Real systems often have correlated failures that make tail worse than independence predicts, not better.
- Hedging is not free. The cost is additional backend load; Google's own measurements show a 2x latency improvement at ~5 % extra load, but only if the backend has spare capacity. On saturated backends, hedging increases load and *worsens* tail.
- The paper is 13 years old. Modern proposals (Ousterhout et al., 2020+; Qin et al., 2020) build on this base with more sophisticated latency-aware scheduling, but the fundamental insight has not changed.

## Relevance to APEX

- APEX does not currently model fan-out at all. G-46 addresses single-function performance. A user whose service suffers tail-at-scale problems will not find help in APEX's findings — the right tool for that class is distributed tracing (Jaeger, Dapper-style) plus tail-tolerance analysis at the architecture level.
- APEX's guidance to users can note the tail-at-scale problem in cases where a regression hits p99 hard but p50 slightly — this is the pattern tail amplification creates, and fixing it may require service-level (not function-level) changes.

## References

- Dean, J., Barroso, L.A. — "The Tail at Scale" — CACM 56(2):74–80, February 2013 — [cacm.acm.org/magazines/2013/2/160173-the-tail-at-scale/fulltext](https://cacm.acm.org/magazines/2013/2/160173-the-tail-at-scale/fulltext)
- Barroso, L.A., Hölzle, U. — *The Datacenter as a Computer*, Morgan & Claypool 2013 — broader context on Google datacenter design.
- Percentiles composition math — `01KNZ4VB6JCD7A2BMXFN1AWGP4`.
- Percentiles vs averages — `01KNZ4VB6JCJY0S4JYW2C3CHTR`.
