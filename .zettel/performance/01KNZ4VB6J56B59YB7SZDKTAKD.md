---
id: 01KNZ4VB6J56B59YB7SZDKTAKD
title: "Production Trace Derivation vs Synthetic Workload Distributions"
type: concept
tags: [trace-replay, workload-model, synthetic-workload, shadow-traffic, realism, privacy]
links:
  - target: 01KNZ4VB6J3AB4QA4YZVDPMFWY
    type: related
  - target: 01KNZ4VB6JSR9RJ0RTWXB9P6FV
    type: related
  - target: 01KNZ4VB6J22PTMXAYQ3V2WYAZ
    type: related
  - target: 01KNZ4VB6JHJSARKD8E9XVGVRC
    type: related
  - target: 01KNZ5F5C3YS1EYDCVFQ7TQS9H
    type: related
  - target: 01KNZ5F4WQ8VSNJBFYJFDSX7NT
    type: related
  - target: 01KNZ5F52X5746A9ASY0W6DKDS
    type: related
  - target: 01KNZ5ZPVXKFXJPF3MNWTYXMZ1
    type: related
  - target: 01KNWE2Q9YZBAR140ZX5P36TQ5
    type: related
created: 2026-04-12
modified: 2026-04-12
source: "multiple; Arlitt 2000; Tang et al. 2007 traffic replay; Google's traffic shadowing"
---

# Production Trace Derivation vs Synthetic Workload Distributions

## Two approaches to realism

A load test needs a workload. There are two ways to get one, and they sit at opposite ends of a realism-vs-reproducibility trade-off:

1. **Synthetic**: derive distributions (arrival rate, request mix, session length, think time) from analytics or traces, and generate traffic that *statistically matches* those distributions. The generator is a small parameter set (e.g. "Poisson arrivals at λ, 60/30/10 read/search/write mix, Geometric session length, exponential think time"). Reproducible, compact, inspectable. Loses all correlations not explicit in the parameter set.

2. **Trace replay**: capture real production requests verbatim and replay them against the SUT. Keeps every correlation, burst, hot key, and temporal pattern. Reproduces the workload exactly.

Both are valid and serve different purposes. Understanding which to use when — and what each loses — is one of the most leveraged decisions in load-test design.

## Synthetic: strengths and limitations

**Strengths**
- Parameter-driven: you can sweep "load from 500 to 5000 req/s at steps of 500" by changing one number.
- No privacy concerns: no real customer data.
- Small: a 10-line config describes a 10-hour test.
- Portable: same workload definition runs in any tool that takes the same parameters.
- Analytically tractable: you can reason about the workload using queueing theory.

**Limitations**
- Whatever you didn't parameterise is missing. If the synthetic model uses Poisson arrivals, the self-similarity is lost (see `01KNZ4VB6JSR9RJ0RTWXB9P6FV`). If the synthetic model uses uniform inputs, the hot-key skew is lost.
- Correlations between fields are implicitly independent unless explicitly modelled. Real workloads have lots of correlations (users who hit endpoint A usually follow with B; requests at 9am are overwhelmingly reads; writes at midnight come from batch jobs).
- Over-simplification is invisible. A "realistic" synthetic workload that passes tests may still miss bugs that only a real workload would trigger.

## Trace replay: strengths and limitations

**Strengths**
- Every bit of real workload structure preserved — arrivals, burstiness, self-similarity, key skew, correlations, diurnal patterns, anomalies.
- Catches bugs that the test designer didn't know to parameterise.
- Defensible: "we tested with real production traffic" is the strongest possible claim.

**Limitations**
- **Privacy**: real requests contain PII, credentials, business data. You cannot replay them against a test environment without sanitisation, which itself can break the workload (stripping a customer ID may change the query plan).
- **Volume**: a day of real traffic at a busy service is terabytes of captures. Storage and replay infrastructure is non-trivial.
- **Temporal mismatch**: traces are from a past period. If the code has evolved (new endpoints, removed endpoints, changed field shapes), a replay crashes on unknown requests.
- **Environmental mismatch**: the trace expects responses from systems that don't exist in the test environment (deleted data, different DB state). A "reply" that depends on state isn't a replay; it's a flaky simulation.
- **Not parameter-sweepable**: you can't easily generate "traffic at 2x" by replaying 2x faster — you get the same correlations squeezed into half the time, not "more users".
- **Scale**: replaying 10% of traffic is easy; replaying 10x is not (you have to either loop-replay or synthesise additional load on top).

## The hybrid: trace-derived distributions + synthetic generator

The common middle ground:

1. Capture a production trace for a day.
2. Extract *distributions* from it: endpoint mix histogram, think-time CDF, session-length tail, arrival-rate time series, hot-key frequency distribution.
3. Build a synthetic generator that samples from these distributions.
4. Verify generator output statistically matches the trace on the key moments.

This approach keeps the realism-per-axis (each distribution is real) while losing cross-axis correlations. It's almost always better than "Poisson with uniform inputs" and much cheaper to run than trace replay. It is the default approach for most mature load-testing practices.

Barford & Crovella's SURGE (SIGMETRICS 1998) is the canonical example: it derives a heavy-tailed file-size distribution, a session-length distribution, a popularity distribution, and a think-time distribution from real web traces, and generates synthetic traffic that matches on all four. A Poisson-based generator would miss all of these characteristics.

## Shadow traffic: the third approach

There is a third option that avoids the realism trade-off entirely: **run the test environment against real production traffic**.

- Route a fraction of prod traffic to the test environment (using a mirror, a sidecar, a service-mesh shadow rule).
- The test environment processes the request; the result is compared against the prod result but discarded for the user (the user sees only the prod response).
- The test environment is evaluated on its SLI/SLO without affecting users.

This is the *highest fidelity* form of testing — the workload is literally production. It has its own limitations:

- Side effects: a test environment that writes to a real DB can't safely process mirrored write traffic. Typically shadow is read-only.
- Cost: the shadow environment is running at the shadow percentage forever, not only during tests.
- Interpretation: the shadow environment may lag prod, so latency numbers are relative to prod, not absolute.

Google, Facebook, Netflix, Cloudflare all use shadow traffic heavily for pre-release validation.

## Anti-patterns

1. **Test workload = "100 requests to /health".** The simplest possible workload. Useful for smoke tests; tells you nothing about production.

2. **Uniform synthetic inputs.** `user_id = rand(0, 1M)` for every request. All users equally likely. Real user distributions are heavy-tailed; hot users are 100x more active. Uniform inputs under-test the hot path and over-test the cold path.

3. **Trace replay without sanitising**. Legal risk, data leak risk.

4. **Trace replay with over-sanitising**. Replacing every string field with "X". Destroys query plan selectivity, destroys cache hit rates. Sanitise by replacing with *realistic dummies preserving shape and cardinality*, not with constants.

5. **Synthetic generator that the designer "tuned until numbers looked right"**. Post-hoc tuning to match a metric often means the generator matches that metric and nothing else.

6. **Outdated trace**. A 6-month-old trace replayed against a 2-week-old build. Endpoints added, endpoints removed, field shapes changed. The replay crashes or silently drops mismatched requests, invalidating the result.

7. **Loop-replay of a short trace.** Replaying 10 minutes of trace on a 2-hour loop. Caches are warm after the first pass; results are not comparable to a real 2-hour workload.

## Which to choose

| Situation | Recommended |
|---|---|
| New service, no prod traffic yet | Synthetic, based on intent (design spec) |
| Existing service, pre-release verification | Synthetic from trace-derived distributions |
| Pre-prod final check | Shadow traffic |
| Legal-sensitive data | Synthetic from sanitised distributions |
| Need to explore scale beyond current prod | Synthetic (trace can't tell you about 10x) |
| Reproducing a specific incident | Trace replay of the incident window |
| Capacity planning at projected future volume | Synthetic, parameter sweep |
| Bug that "only happens in prod" | Shadow traffic, or trace replay of the window |

## Adversarial reading

- "Realism" has no single metric. A workload can be perfectly realistic on rate and utterly wrong on hotness; the other way around; perfectly realistic on both and wrong on latency perception. "Realistic" means something different for every performance question. Be explicit about which axis you are matching.
- Trace replay is often *less* useful than it sounds because the prod environment has state the test env doesn't. The reply sees a different set of rows, different cache contents, different lock contention. You are testing against "close to prod workload, far from prod state".
- Synthetic generators age well; trace captures age quickly. A 1-year-old synthetic spec is usually still runnable; a 1-year-old trace is probably not.

## References

- Barford, P., Crovella, M. — "Generating Representative Web Workloads" — SIGMETRICS 1998 — the SURGE generator.
- Arlitt, M. — "Characterizing Web User Sessions" — SIGMETRICS 2000.
- Tang, W. et al. — "MediSyn: A synthetic streaming media service workload generator" — NOSSDAV 2003.
- Traverso, M., et al. — "Presto: SQL on Everything" — discusses trace-replay use at Facebook.
- Kejariwal, A., Allspaw, J. — *The Art of Capacity Planning*, 2nd ed., O'Reilly 2017 — chapters on workload derivation.
- Environment parity note — `01KNZ4VB6J22PTMXAYQ3V2WYAZ`.
