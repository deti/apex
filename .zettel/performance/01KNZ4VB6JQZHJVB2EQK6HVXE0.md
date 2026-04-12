---
id: 01KNZ4VB6JQZHJVB2EQK6HVXE0
title: "SLOs as Test Oracles — Turning Requirements into Pass/Fail"
type: concept
tags: [slo, sla, sli, oracle, acceptance-criteria, sre, google, workflow]
links:
  - target: 01KNZ4VB6JY38THW04Z3MMGBZ3
    type: related
  - target: 01KNZ4VB6JCJY0S4JYW2C3CHTR
    type: related
  - target: 01KNZ4VB6J22PTMXAYQ3V2WYAZ
    type: related
  - target: 01KNWE2QA5VP0K80TMSABACKWT
    type: related
  - target: 01KNZ68KJMZSSAZVFAB3ZNXNTJ
    type: related
  - target: 01KNZ72G2VNNP6JHWQAK0HJTXM
    type: related
  - target: 01KNZ72G5955YGB9B2W61QD2Z4
    type: related
  - target: 01KNZ4VB6JXAZA2TBRCD5DERK9
    type: related
  - target: 01KNWE2Q9YZBAR140ZX5P36TQ5
    type: related
created: 2026-04-12
modified: 2026-04-12
source: "Google SRE Workbook, 'Implementing SLOs' — https://sre.google/workbook/implementing-slos/"
---

# SLOs as Test Oracles — Turning Requirements into Pass/Fail

## The oracle problem

A test "passes" or "fails". Functional tests have an obvious oracle (the output equals the expected output). Performance tests don't — response time is always *some* value, and whether it's "good" depends on what you expect. Without an oracle, performance-test runs produce charts and numbers that different people read differently and nobody commits to a decision on.

SLOs (Service Level Objectives) solve the oracle problem for performance tests by encoding the requirements as quantitative boundaries that the test compares against directly.

## SLI, SLO, SLA

(Following the Google SRE book taxonomy.)

- **SLI (Service Level Indicator)** — a *measurement*. "The p99 latency of the checkout endpoint over the last 30 days, measured at the load balancer." Numeric, continuous, observable.
- **SLO (Service Level Objective)** — a *target* on an SLI. "p99 checkout latency ≤ 400 ms, 99.5 % of the time." The target plus the tolerance on violation.
- **SLA (Service Level Agreement)** — a *contract* that promises an SLO to customers with consequences if breached ("if p99 exceeds 400 ms for more than 1 % of 30-day windows, the customer receives a 10 % credit"). An SLA is an SLO with teeth.

For test-oracle purposes, you want SLOs — unambiguous numeric thresholds your test can check. SLAs include them but add legal and billing stuff that your CI pipeline doesn't need.

## The shape of a well-formed SLO

A production-usable SLO has six parts:

1. **Service scope**: which service, which endpoint, which method.
2. **SLI definition**: which metric, measured how, aggregated how.
3. **Threshold**: the numeric boundary.
4. **Percentile (for latency) or rate (for availability)**.
5. **Measurement window**: over how long, e.g. "over 30 days" or "during the test run".
6. **Objective compliance rate**: "99.5 % of 1-minute windows" — how often the threshold must be met.

Example fully-specified SLO:

> Service: payments-api, endpoint POST /v1/charges.
> SLI: end-to-end latency from request arrival at the load balancer to full response, in milliseconds.
> Threshold: 400 ms.
> Percentile: p99.
> Window: 30 days, rolling.
> Compliance: 99.5 % of 5-minute buckets have p99 ≤ 400 ms.

This SLO has only one interpretation. A load test can take its output and unambiguously decide pass/fail.

## How SLOs become test oracles

The compilation is mechanical:

**Step 1**: identify the SLOs your test is meant to verify. Usually a subset — a load test of the checkout flow verifies the checkout SLO but not the search SLO.

**Step 2**: configure the test workload to match the SLO's *offered-load* condition. If the SLO is "p99 < 400 ms at 1 k req/s", you need a generator that can sustain 1 k req/s *with an open model* (see Schroeder et al. `01KNZ4VB6JX0CQ5RFAZDJTQMCS`).

**Step 3**: collect the SLI during the test at the same granularity as production. Same percentile, same window, same aggregation.

**Step 4**: compare the SLI to the threshold. If within bounds throughout the steady-state window, pass. Otherwise, fail and report the offending percentile / time slice.

**Step 5**: record the result and its metadata (baseline, environment, workload mix, build SHA) for regression analysis.

## Why SLOs (not averages, not maxes) as the oracle

- **Averages lie.** See `01KNZ4VB6JCJY0S4JYW2C3CHTR`. Mean latency is a useless summary of a heavy-tailed distribution.
- **Max is a single sample.** The max of 10 M samples is a rare outlier — a single GC pause, a single packet retransmit. Max-based oracles are noisy and always tripping.
- **Percentile SLOs match user experience.** "p99 < 400 ms" says "no more than 1 % of users see worse than 400 ms" — a statement about user experience that translates directly to support tickets.
- **Percentile SLOs compose with error budgets.** If p99 is meeting target, the error budget (the "1 %" slack) lets SRE / product trade slack for development velocity, which is the whole Google SRE model.

## The error-budget discipline

The Google SRE discipline couples SLOs to release cadence:

1. SLO says "99.5 % availability over 30 days" — error budget is 0.5 % = 3.6 hours of downtime per month.
2. Track the rolling budget consumption. Every minute of exceeded latency or every error consumes budget.
3. When budget is full (we've been meeting SLO), release freely — we have the slack.
4. When budget is empty (we've been missing SLO), freeze releases and invest in reliability work.
5. When budget is recovering, release carefully.

For load testing, the same discipline applies. A PR that breaks the load-test SLO spends the error budget; a PR that improves it earns the budget back. This is the conceptual bridge between "load tests" (a CI check) and "production reliability" (an SRE concern).

## Anti-patterns

1. **SLOs specified in requirements but never translated into tests.** Everyone agrees "p99 < 400 ms". No CI job ever checks it. At production, you discover the SLO was wishful thinking.

2. **Test oracle uses a *different* percentile from the SLO.** SLO says p99; the test script asserts mean < 400 ms. Pass-by-luck.

3. **Single-iteration pass/fail.** Run the test once, check if the p99 < 400 ms, ship. No confidence interval, no repetition, no check on run-to-run variance. A noisy system will flake.

4. **"Soft" SLOs.** "Warning if p99 > 400, error if p99 > 500". The warning threshold is the one you should fail on — the error threshold is a missed SLO. Fix: one hard threshold, matching the production SLO.

5. **SLO based on observed instead of desired performance.** "We measured p99 = 380 ms last month, so the SLO is 400 ms." The SLO should reflect user needs, not current capabilities. Otherwise the SLO tracks the system rather than the system tracking the SLO.

6. **Measuring at the wrong boundary.** The SLO says "end-to-end latency as seen by the client"; the test measures "server-side service latency" after the load balancer, excluding DNS, TLS, network RTT. These are different numbers. Fix: measure at the SLO's specified boundary.

7. **Ignoring availability in the SLO.** A latency-only SLO is happy with "0 % of requests complete in < 400 ms" if the denominator is small (because we errored everything). Fix: latency SLOs must be scoped to successful requests, and paired with an error-rate SLO.

## Adversarial reading

- SLOs encode a snapshot of today's understanding of user needs. They drift — a 400 ms SLO made sense in 2020 when users were on 3G; in 2026 on 5G the expectation is lower. SLO review should be quarterly.
- Strict SLO-as-oracle can miss bugs the SLO didn't anticipate: a new class of error that the SLI doesn't capture. SLOs are necessary but not sufficient oracles; pair with change detection on the full histogram (KS test).
- In practice, what you enforce in CI is usually a *tighter* threshold than production SLO, because the test environment has less noise. "p99 < 300 ms in CI" maps to "p99 < 400 ms in production" with 25 % margin for production-only slowdowns.

## References

- Beyer, B., Jones, C., Petoff, J., Murphy, N. — *Site Reliability Engineering*, O'Reilly 2016 — Chapter 4 "Service Level Objectives" — [sre.google/sre-book/service-level-objectives](https://sre.google/sre-book/service-level-objectives/)
- Beyer, B., Murphy, N., Rensin, D., Kawahara, K., Thorne, S. — *The Site Reliability Workbook*, O'Reilly 2018 — Chapter 2 "Implementing SLOs" — [sre.google/workbook/implementing-slos](https://sre.google/workbook/implementing-slos/)
- Wilkes, J. — "SLOs, SLIs, SLAs, oh my!" talk, 2018 — YouTube.
- Percentiles note — `01KNZ4VB6JCJY0S4JYW2C3CHTR`.
