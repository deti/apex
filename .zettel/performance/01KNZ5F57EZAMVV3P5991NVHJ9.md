---
id: 01KNZ5F57EZAMVV3P5991NVHJ9
title: Diffy (Twitter) — Shadow Traffic with Response Diffing
type: literature
tags: [diffy, twitter, shadow-traffic, response-diffing, regression-testing, test-generation]
links:
  - target: 01KNZ5F4WQ8VSNJBFYJFDSX7NT
    type: related
  - target: 01KNZ5F52X5746A9ASY0W6DKDS
    type: related
  - target: 01KNZ5F59R6M5ATD9X2YW87XAC
    type: related
  - target: 01KNZ72G5SVY6JH66N7BP825C6
    type: related
  - target: 01KNWE2Q9YZBAR140ZX5P36TQ5
    type: related
created: 2026-04-11T20:59:22.222507+00:00
modified: 2026-04-11T20:59:22.222513+00:00
source: "https://github.com/twitter-archive/diffy"
---

# Diffy — Twitter's Shadow Traffic Response Differ

Diffy is an open-source tool originally developed at Twitter (now twitter-archive/diffy) to validate a new version of a service against a trusted baseline *using real production traffic*. It is built on Finagle (Twitter's Scala RPC stack). The premise is simple and powerful:

> If two implementations of the service return "similar" responses for a sufficiently large and diverse set of requests, then the two implementations can be treated as equivalent, and the newer implementation is regression-free.

## Architecture

Diffy sits as a reverse proxy in front of three backends:

- **Primary.** The current production code (the trusted reference).
- **Candidate.** The new build under test.
- **Secondary.** Another instance of the current production code — used as a *noise baseline*.

For every incoming request (which can be live production traffic, shadow traffic from an upstream mirror, or synthetic traffic), Diffy multicasts to all three. It compares candidate-vs-primary and secondary-vs-primary. Any difference in the candidate that also appears between two primaries is likely noise (non-determinism, clock skew, order-sensitive responses) and is filtered out. Differences unique to the candidate are flagged as regressions.

This noise-cancellation step is the key contribution. Without it, response diffing on a real service generates thousands of false positives an hour.

## What Diffy actually detects

Diffy's output is a UI listing endpoints with candidate-only differences, ranked by how often the difference appears and which fields differ. Engineers then investigate.

The tool is *primarily correctness-focused* — it finds behavioural regressions, not performance ones. But:

- **Perf regressions often manifest as error responses** that Diffy surfaces (new 500s, new 429s, added latency triggering timeouts).
- **Diffy records wall-clock latency** for each side, so latency deltas are visible in the same UI.
- **As a workload-generation tool**, pointing production traffic at Diffy is equivalent to running a load test of the candidate, with the additional oracle of "response shape matches primary."

## Adoption

As of the last Twitter release (around 2015-2016), Diffy was used in production by Twitter itself, Airbnb, Baidu, and Bytedance according to the README. Since then the original repo has been archived; various forks exist. Airbnb in particular has published extensively on their internal descendant of Diffy used for pricing service rewrites (one of the best case studies in the genre).

## Failure modes

1. **Non-determinism dominates.** Timestamps in responses, UUIDs generated server-side, ordered lists returned unordered, floating-point results with different rounding, cache-affected fields — all flood the diff output. The secondary-baseline trick handles *repeatable* non-determinism but not cases where two calls to the same primary return different things (e.g., because of pagination tokens, time-based TTLs).
2. **Side effects.** Multicasting real POSTs to three backends means three database writes. For stateful services Diffy is unusable without a major engineering effort to make the backends idempotent-or-read-only.
3. **Database isolation is hard.** To use Diffy safely you need three copies of the database (or one shared read-only snapshot). Most teams don't have this.
4. **Scale.** Diffy is a stateful proxy that holds response bodies in memory for diffing. Throughput is limited by Diffy itself, not the backends. Not a tool for >10k RPS without horizontal scaling of the Diffy layer.
5. **Archived upstream.** The Twitter repo is frozen. Dependencies on old Finagle versions are a maintenance headache for adopters.
6. **Diff semantics are syntactic.** Field-by-field comparison misses semantic equivalences (sorted list vs. unsorted, struct vs. json-serialised struct). Configuration knobs exist but require per-service tuning.

## Related and successor work

- **Mixpanel's regression testing with production traffic** (engineering blog, ca. 2021) — describes a Diffy-style system used to validate analytics pipeline refactors.
- **LinkedIn's "Dark Canary"** and related internal tooling — commonly mentioned in talks but not open-sourced.
- **Airbnb's diffing frameworks** — multiple internal tools extending the Diffy idea for their pricing and search services.
- **KrakenD API Gateway** shipped a traffic-mirroring feature explicitly citing the Diffy pattern as prior art.
- **Microsoft's engineering playbook** has a shadow-testing chapter that walks through the Diffy pattern for cloud services.

## Why Diffy still matters conceptually

Diffy is the best published example of **using real production traffic as both a workload AND an oracle**. Most traffic replay tools (GoReplay, VPC mirroring) only give you the workload. Diffy's contribution is adding the "is the new version right?" question on top — the closest thing we have to solving the oracle problem for perf/behaviour regressions at the system level.

For APEX-adjacent tooling, Diffy's architecture is a reference point: any tool that wants to combine shadow traffic with a regression-detection signal is reinventing Diffy's core loop.

## Citations

- https://github.com/twitter-archive/diffy
- Mixpanel engineering blog: https://engineering.mixpanel.com/regression-testing-with-production-traffic-at-mixpanel-fc424eec4401
- Microsoft engineering playbook (shadow testing): https://microsoft.github.io/code-with-engineering-playbook/automated-testing/shadow-testing/
- KrakenD shadow testing: https://www.krakend.io/blog/krakend-shadow-testing/