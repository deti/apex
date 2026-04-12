---
id: 01KNZ782C4J9WBCRJ12NHYZN3X
title: Japke et al. 2025 — Optimized Benchmarking Platform for CI/CD Pipelines (Vision)
type: literature
tags: [japke, benchmarking-platform, ci-cd, vision-paper, tu-berlin, ic2e-2025, 2025, future-work]
links:
  - target: 01KNZ782CH21JYMYXTQT9D8W5B
    type: related
  - target: 01KNZ6WHSTDKHSQERTN6XZC90B
    type: related
  - target: 01KNZ68KN59XANY9TX9WE0BYJH
    type: related
  - target: 01KNZ4VB6J5ZW3JERZNDNGP7GD
    type: related
  - target: 01KNWE2Q9YZBAR140ZX5P36TQ5
    type: related
created: 2026-04-11T21:30:27.076394+00:00
modified: 2026-04-11T21:30:27.076395+00:00
---

*Source: Nils Japke, Sebastian Koch, Helmut Lukasczyk, David Bermbach — "Towards an Optimized Benchmarking Platform for CI/CD Pipelines" — arXiv:2510.18640, IEEE International Conference on Cloud Engineering (IC2E) 2025. Partially funded by the German Federal Ministry of Research, Technology and Space (BMFTR) under the Software Campus 3.0 program.*

A recent **vision paper** from TU Berlin's Mobile Cloud Computing group. The contribution is not a new algorithm or tool but a framing of the open problems in CI/CD performance testing and a sketch of what an ideal benchmarking platform would look like. Short paper, but useful as a 2025-era statement of the research frontier.

## The problem the paper names

> "Performance benchmarks... are resource-intensive and time-consuming."

This is the core tension for any CI perf system: benchmarks must be thorough enough to catch regressions but cheap enough to run on every PR. In practice, every CI perf team makes arbitrary trade-offs:
- Run a small "smoke" subset on every PR.
- Run a larger "nightly" suite on main.
- Run a full "release" suite before major releases.

The paper argues that this ad-hoc tiering is a symptom of a missing abstraction: teams are manually picking which benchmarks to run when, with no principled way to optimise for "regressions caught per CI-minute spent."

## The three core problems the authors identify

1. **Composability of optimisation strategies.** Many ideas exist for reducing benchmark cost: prioritisation, test selection, caching, test parallelisation, workload sharing. But these are implemented independently in different tools and don't compose. A CI pipeline can't easily say "run the top-10 regression-prone benchmarks from selection-tool-A, in parallel using runner-tool-B, with cached warm-up state from tool-C" — the integration points don't exist.

2. **Automated result evaluation.** The output of a benchmark run is numbers; the output the CI needs is a pass/fail verdict. The gap between raw numbers and verdict involves statistical analysis, threshold selection, multiple-comparison correction, effect-size gating — and each organisation re-invents this. The authors argue for a **shared abstraction** for benchmark result evaluation with pluggable statistical backends.

3. **Practical usability.** Even the best benchmarking platform is useless if it's hard to integrate into an existing CI. The paper flags that academic benchmarking tools typically require specific environments, language ecosystems, or infrastructure that organisations can't easily adopt. The vision is a platform that is **pipeline-friendly**: configured via YAML, runs on standard CI runners, integrates with Prometheus/Grafana/Datadog, and does not require developers to learn statistics to use.

## The proposed vision

The paper sketches a **cloud-based benchmarking framework** that:
- Handles benchmark scheduling transparently.
- Integrates heterogeneous optimisation strategies (test selection, prioritisation, sharing).
- Provides automated statistical result evaluation with pluggable methods.
- Exposes a simple declarative interface for users.

Crucially, the paper does **not** propose a completed system; it is deliberately a vision paper asking the community to work on these problems.

> "Rather than presenting a fully-realized system, the authors aim to 'stimulate research toward making performance regression detection in CI/CD systems more practical and effective.'"

## Why this is worth reading despite being a vision paper

1. **It names the right problems.** The three-problem framing — composability, evaluation, usability — is concise and resonates with anyone who has built CI perf infrastructure.
2. **It acknowledges that the problem is organisational as well as algorithmic.** Previous vision papers tended to assume that if you had the right statistical test, the rest would follow. This paper explicitly argues that integration and usability matter as much as algorithms.
3. **It maps the 2025 research landscape.** The related-work section points at benchmark prioritisation, test selection for performance tests, and ML-based regression prediction — recent directions that are still immature.

## The open problems the paper flags as future work

In order of how "plausible toolmaker targets" they look:

1. **A standard abstraction for benchmark result evaluation.** A pluggable API where statistical methods (t-test, Mann-Whitney, E-divisive, bootstrap) are interchangeable behind a common interface. Closest current equivalent: Bencher.dev's six threshold types — but tied to one tool.

2. **Benchmark test selection for performance regression.** Analogous to functional-test selection (Rothermel et al., RTS): given a PR, predict which benchmarks are most likely to regress and run only those. ML-based prediction from commit features is an obvious direction. Japke et al.'s group has done some work on this; others include "From Code Changes to Performance Variations" (IEEE 2024) which they cite.

3. **Regression detection for composed benchmarks.** When benchmarks depend on each other (shared setup, shared warmup), evaluating them independently loses information. A principled way to detect regressions across a dependent bench suite is open.

4. **Benchmark cost modelling.** Before running, predict how long a benchmark will take and how confident its result will be; schedule accordingly. Needs data that most CIs don't collect systematically.

5. **Automatic benchmark curation.** Use historical noise/stability data to de-prioritise noisy benchmarks and surface stable-and-regression-sensitive ones. An analogue to flaky-test detection in functional CI.

## Adversarial commentary

- **It is a vision paper.** Short on evidence, long on framing. Don't cite it for empirical results.
- **The "cloud-based platform" framing is commercially loaded.** Any team not on public cloud (on-prem perf labs, bare-metal CI) doesn't benefit from a cloud-first platform. The framing biases the solution space toward managed services.
- **Missing: continuous profiling.** The paper does not mention continuous profiling / production-profile-diff approaches (GWP, Polar Signals, Pyroscope). A complete vision should include post-deploy profile-diff as part of the CI perf gating spectrum.
- **Missing: PR-level microbenchmarks.** The paper focuses on longer-running benchmarks that have CI scheduling problems; it largely ignores CodSpeed / iai-callgrind / Bencher.dev fast microbenchmark gating which is the most adoption-rich area right now.
- **Academic-to-industry gap.** Most production CI perf systems (Mozilla, Google Chrome, Netflix, MongoDB, DataStax) are operated without reference to academic benchmarking frameworks at all. A "vision" for an academic benchmarking platform may not actually map onto what those teams need or would adopt.

## Connections

- Daly et al. 2020 / Hunter 2023 — specific algorithms a platform in this vision would integrate.
- Bencher.dev — closest current match to the "pluggable threshold" piece of the vision.
- Besbes et al. 2025 Perfherder dataset — resource a research agenda in this direction would use.
- Argo Rollouts / Flagger — the deployment-time piece that a full vision must compose with.

## Reference

Japke, N., Koch, S., Lukasczyk, H., Bermbach, D. (2025). *Towards an Optimized Benchmarking Platform for CI/CD Pipelines*. IEEE IC2E 2025. arXiv:2510.18640.
