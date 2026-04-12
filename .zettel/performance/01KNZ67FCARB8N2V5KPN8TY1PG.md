---
id: 01KNZ67FCARB8N2V5KPN8TY1PG
title: "Hunter 2023 — Change Point Detection for Performance Regressions (Fleming et al., ICPE 2023)"
type: literature
tags: [hunter, change-point-detection, icpe-2023, e-divisive-means, regression-detection, open-source, ci-cd]
links:
  - target: 01KNZ67FBPEC378X6KZ79305T0
    type: related
  - target: 01KNZ67FCZ2BPSAM6QEAQ0RX2P
    type: related
  - target: 01KNZ6T759YNNAFPCMPAGSTCYV
    type: related
  - target: 01KNZ6T721S1YTYHGZE1AS1Y43
    type: related
  - target: 01KNZ4VB6J5ZW3JERZNDNGP7GD
    type: related
  - target: 01KNWE2Q9YZBAR140ZX5P36TQ5
    type: related
created: 2026-04-11T21:12:39.050253+00:00
modified: 2026-04-11T21:12:39.050258+00:00
---

*Source: Matt Fleming, Piotr Kołaczkowski, Ishita Kumar, Shaunak Das, Sean McCarthy, Pushkala Pattabhiraman, Henrik Ingo — "Hunter: Using Change Point Detection to Hunt for Performance Regressions" — Proceedings of the 2023 ACM/SPEC International Conference on Performance Engineering (ICPE '23), April 2023. arXiv:2301.03034, DOI 10.1145/3578244.3583719.*

Hunter is the direct descendant of the Daly et al. 2020 MongoDB approach (see dedicated note). It was developed at DataStax (by an author overlap with MongoDB: Henrik Ingo) as an **open-source, reusable tool** for change-point detection over time series of performance measurements. It is currently used to monitor Apache Cassandra and other DataStax-supported systems.

## What Hunter adds over Daly et al. 2020

The core algorithm is still E-divisive means, but Hunter replaces the **permutation-based significance test** with a **Student's t-test**. The motivation is directly operational:

> "The primary modification replaces randomized permutation-based significance testing with Student's t-tests to ensure deterministic results."

Permutation tests are stochastic (they randomly shuffle labels) and can produce slightly different p-values on repeated runs of the same history. For a CI dashboard that is consulted by multiple humans over days, this non-determinism is confusing — "yesterday Hunter said this was a change-point, today it doesn't." Replacing permutation with a t-test makes the output deterministic at the cost of re-introducing a (weak) normality assumption at the segment-comparison step. Fleming et al. argue that after E-divisive has already identified a candidate boundary, the distributions on either side are typically concentrated enough that the t-test assumption is acceptable.

Additional improvements:
- **Detection of closely-spaced change points.** Vanilla E-divisive can miss a second change-point if it falls close to a first one; Hunter adjusts the recursion to recover these.
- **Evaluation against PELT and DYNP.** The authors compare against two established change-point algorithms using artificially injected latency changes on real time series, showing Hunter's modified E-divisive has competitive detection rates with better behaviour in closely-spaced cases.
- **Operational lessons.** The paper includes a section on what it takes to maintain a CPD system across multiple teams with individual ownership of their performance responsibilities. Key lesson: giving each team a fence around their own benchmarks (so team A's noisy benchmark doesn't desensitise team B's triage) matters as much as the algorithm.

## The open-source artifact

Hunter is distributed as a Python tool (github.com/datastax-labs/hunter) and is a pragmatic choice for any team that wants MongoDB-style CPD without writing their own. It integrates with Graphite, CSV, and InfluxDB data sources.

## Adversarial commentary

- **Still retrospective.** Hunter, like Daly et al., needs post-change data to establish a new regime. Useless for PR-level gating; excellent for nightly/post-merge detection.
- **The t-test substitution is a trade-off**, not a free lunch. It buys determinism but reintroduces the Gaussian assumption that non-parametric methods were specifically chosen to avoid. In benchmarks with heavy tails or multimodality, the t-test p-values are optimistic. The mitigation is that the t-test here is used only to confirm a candidate already flagged by E-divisive, not to find candidates de novo.
- **No causal attribution.** Like any CPD, Hunter tells you the range; it doesn't tell you the commit. Bisection (cf. Chrome Pinpoint) is a separate problem.
- **Assumes a single time series per benchmark.** Multi-metric benchmarks (latency + throughput + memory) are handled by running CPD independently on each series and reconciling alerts downstream, which loses the correlation structure.

## Connections

- Direct successor to Daly et al. 2020.
- Uses the same algorithmic core as MongoDB's `signal-processing-algorithms`.
- Contrasts with PELT (Killick et al. 2012) and Bayesian change-point.
- Complementary to canary analysis (Kayenta) which does *prospective* gating, not retrospective detection.

## Reference

Fleming, M., Kołaczkowski, P., Kumar, I., Das, S., McCarthy, S., Pattabhiraman, P., Ingo, H. (2023). *Hunter: Using Change Point Detection to Hunt for Performance Regressions*. ICPE 2023. arXiv:2301.03034.
