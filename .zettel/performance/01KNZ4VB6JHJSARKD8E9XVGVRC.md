---
id: 01KNZ4VB6JHJSARKD8E9XVGVRC
title: "Workload Characterisation — Describing What the System Actually Does"
type: concept
tags: [workload-characterisation, brendan-gregg, methodology, analytics, traces, workflow]
links:
  - target: 01KNZ4VB6J08D14Y8P3RWVAABA
    type: extends
  - target: 01KNZ4VB6J56B59YB7SZDKTAKD
    type: related
  - target: 01KNZ4VB6J3AB4QA4YZVDPMFWY
    type: related
  - target: 01KNZ4VB6JHP7W47HM7QREWW53
    type: related
  - target: 01KNZ5ZPPXW26VRNZ9BHKB0AYV
    type: related
  - target: 01KNZ5ZPVXKFXJPF3MNWTYXMZ1
    type: related
  - target: 01KNWE2Q9YZBAR140ZX5P36TQ5
    type: related
created: 2026-04-12
modified: 2026-04-12
source: "Brendan Gregg 'Systems Performance' 2nd ed., §2.5.10; Calzarossa et al. 2016 ACM CSUR"
---

# Workload Characterisation

## The method in one sentence

Before running any performance test, answer four questions about the workload: *Who* is calling the system? *Why* are they calling it? *What* is in the requests? *How* does the load change over time? (Brendan Gregg's summary; see `01KNZ4VB6J08D14Y8P3RWVAABA`).

The four questions cover the inputs to every downstream design choice: the test workload, the test data, the arrival model, the environment, and the acceptance criteria. Skipping workload characterisation — which is the norm — results in load tests that exercise an idealised workload nobody actually runs.

## The four questions expanded

### Who

- Which clients / callers drive the traffic? Humans? Services? Crawlers? Batch jobs?
- How many distinct callers in a typical window?
- Are there dominant callers (top 1 %, top 0.1 %) whose behaviour is critical?
- Are the callers trusted, untrusted, or a mix?
- Do callers have distinct access patterns? Power users vs casual vs machine clients should often be modelled as separate workloads.

### Why

- What are the callers trying to do? Business function, not endpoint.
- Is the workload elastic (callers will go away if it's slow) or inelastic (must complete no matter what)?
- Is there a cost for the caller if the call fails / times out? Retry behaviour matters.

### What

- Which endpoints / methods are called, at what frequency?
- What is the request payload size distribution?
- Which inputs — user IDs, keys, search terms — appear? What's their cardinality and skew?
- What's the response size distribution?
- What's the request-to-response ratio (reads vs writes, cold path vs hot path)?

### How

- Arrival rate over time: constant? Diurnal? Weekly cycles? Event-driven spikes?
- Burstiness: is the arrival process Poisson, self-similar, batched?
- Session shape: how many requests per session, in what order, with what pauses?
- Correlation between callers: independent, or do they trigger in response to the same event?

## Why it's the first step

Workload characterisation is what distinguishes a performance test that generalises from one that doesn't. The test environment, test data, load model, and acceptance criteria all derive from the characterisation. Skipping it produces a test that exercises:

- An endpoint mix the test author thought was realistic (usually wrong).
- Input cardinality the test author thought reasonable (usually too uniform).
- A constant load with no bursts or sessions (always wrong).
- Against a test database that doesn't resemble prod (almost always wrong).

The test passes, ships, and production fails.

## Data sources

- **Access logs / request logs.** Per-request metadata: caller, endpoint, method, timestamp, latency, size. Almost every web-framework / proxy logs this. Anonymise before use.
- **Distributed traces.** OpenTelemetry / Jaeger traces show the full call graph per request: which downstream services are called, how long each took. Essential for understanding microservice workloads.
- **Database query logs / slow query logs.** Show the hottest queries and their inputs.
- **APM dashboards.** Pre-aggregated top endpoints, top callers, traffic patterns. Fast but coarse.
- **Customer analytics.** Usage segments, high-value flows, feature adoption. Maps workload to business impact.
- **Business context.** Sales schedules, marketing campaigns, product launches, known big customers. Not in logs but dominates traffic for their windows.

## Outputs

The concrete artifacts a workload characterisation produces:

1. **Endpoint mix**: a table of (endpoint, fraction of calls, size distribution).
2. **Caller distribution**: histogram of calls per caller (to identify hot callers).
3. **Session model**: average requests per session, session shape, think-time distribution.
4. **Arrival model**: rate over time; peak-to-mean ratio; burst structure.
5. **Input cardinality and skew**: per input field, the distribution of values.
6. **Data volume and growth rate**: how much data does the system hold, how fast is it growing.
7. **Business-impact weighting**: which slices of the workload matter most for SLOs.

Each artifact feeds a specific downstream choice:
- Endpoint mix → test script / scenario definitions.
- Caller distribution → whether to model hot callers separately.
- Session model → session script + think-time in the generator.
- Arrival model → open/closed/partly-open and the arrival-rate curve.
- Input cardinality → test data generation or trace-derived inputs.
- Data volume → environment parity for the DB.
- Business-impact weighting → which SLIs the test must satisfy.

## Typical findings and surprises

Workload characterisations almost always reveal things the team didn't know:

- **An endpoint nobody mentioned** serving 40 % of traffic (often `/health`, `/metrics`, or a badly-cached `/config`).
- **Hot users**. The top 0.1 % of callers generate 30 % of load.
- **Unexpected correlations**. Endpoint X is always followed by endpoint Y; testing them independently misses the cascade.
- **Batch jobs** masquerading as user traffic. A nightly job that issues 1000 req/s for 10 minutes blows up the "mean rate" but is invisible in aggregate.
- **Mismatched SLOs**. The SLO says "p99 < 200 ms" on the API, but 80 % of user-perceived latency is in the client-side bundle load, not the API. Optimising the wrong thing.
- **Dead endpoints**. 15 % of code is for features with 0.001 % of traffic. Carrying cost without value.

## Anti-patterns

1. **Characterising on synthetic assumptions.** "We assume the workload is 80/20 read/write." Source? Not traces. Then it's guesswork.

2. **Characterising on a single day's traffic.** Day-of-week variation is large. Characterise across weeks, at minimum.

3. **Characterising from the dev team's mental model.** The team knows what the code *should* do, not what users *actually* do. Always go to real traces.

4. **Not updating the characterisation.** A year-old characterisation is already wrong; usage shifts.

5. **Over-aggregating.** Mean request size is meaningless if the distribution is multi-modal (small thumbnails vs large reports). Always look at the distribution, not just the mean.

## Relationship to other workflow steps

Workload characterisation is **step 0** of every performance-testing activity. It precedes test design, test environment setup, and SLO specification. Every subsequent step is conditioned on "what workload are we modelling?".

The common failure mode is doing workload characterisation implicitly: the test author picks endpoints and rates based on their memory. The explicit version is almost always more accurate.

## Relevance to APEX

- APEX is a code-level tool, so "workload characterisation" for APEX's purposes is per-function rather than per-service. Which inputs actually hit this function? What are their size and value distributions? This is a trace-driven question for APEX's resource profiler and complexity estimator.
- APEX's ReDoS detector characterises the *input space* of a regex — "what values can trigger this pattern". That is a form of workload characterisation at the regex level.

## References

- Gregg, B. — *Systems Performance*, 2nd ed., Addison-Wesley 2020, §2.5.10 Workload Characterisation.
- Calzarossa, M., Massari, L., Tessera, D. — "Workload Characterization: A Survey Revisited" — ACM Computing Surveys 48(3), 2016.
- Trace-derivation note — `01KNZ4VB6J56B59YB7SZDKTAKD`.
- Methodologies note — `01KNZ4VB6J08D14Y8P3RWVAABA`.
