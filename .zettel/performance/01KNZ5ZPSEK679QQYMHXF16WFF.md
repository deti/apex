---
id: 01KNZ5ZPSEK679QQYMHXF16WFF
title: Identifying Worst-Case User Scenarios Using Markov-Chain Workload Models
type: literature
tags: [markov-chain, worst-case-scenarios, workload-model, web-application, capacity-planning, abo-akademi, test-generation]
links:
  - target: 01KNZ5ZPPXW26VRNZ9BHKB0AYV
    type: related
  - target: 01KNZ5ZPVXKFXJPF3MNWTYXMZ1
    type: related
  - target: 01KNZ6QBH0YZYKPZNYDCZD5P2B
    type: related
  - target: 01KNWE2QA8H1GKHCVNHYS5QW1F
    type: related
  - target: 01KNZ6QBKG64F2WEP3PNJK61JH
    type: related
  - target: 01KNWE2Q9YZBAR140ZX5P36TQ5
    type: related
created: 2026-04-11T21:08:24.494525+00:00
modified: 2026-04-11T21:08:24.494530+00:00
source: "https://www.sciencedirect.com/science/article/abs/pii/S0167739X18301341"
---

# Identifying Worst-Case User Scenarios for Performance Testing Using Markov-Chain Workload Models

*Authored by researchers at Åbo Akademi University (Finland) and published in Future Generation Computer Systems (Elsevier, 2018). This is the clearest academic bridge between the CBMG tradition and adversarial workload generation.*

## Core idea

Given a Markov-chain workload model (typically fitted from access logs in the CBMG style), find the **path through the graph** that, when walked by virtual users, generates the highest resource utilisation on the system under test for a sustained period. That path is the "worst-case user scenario" and is the right seed for a stress test.

The paper formalises two algorithms:

1. **Graph-search exact method.** Treats the problem as shortest-path-in-reverse over a weighted graph where edge weights are the negative resource utilisation. Exact but exponential in pathological cases.
2. **Near-optimal heuristic.** A dynamic-programming-with-pruning approach that scales better at the cost of missing a small number of optimal paths.

## Why this is important

This paper is the first to make explicit a connection that seems obvious in retrospect but is missing from almost every commercial perf-testing tool:

> The workload model is not just a recipe for realistic traffic — it's also a recipe for *worst-case* traffic if you know which transitions to weight.

The CBMG tradition (Menascé et al.) emphasises realism: probability-weighted traversal gives you traffic that matches production. This paper flips the optimisation: given the same graph, find the traversal that maximises load. It is the analogue of worst-case complexity fuzzing (SlowFuzz, PerfFuzz — already in this vault) but applied at the *workload* level rather than the *input* level.

The PerfFuzz / SlowFuzz family answers: "what specific inputs to *this one function* trigger worst-case behaviour?" The Åbo Akademi work answers: "what sequence of *user actions* triggers worst-case system behaviour?" These are complementary. A complete performance test generator would run both.

## Methodology details

The paper builds a workload model with three levels:

- **Application model.** The structure of the web application expressed as a state graph (same as CBMG).
- **User behaviour model.** A probabilistic user model overlaid on the application model.
- **Performance model.** Per-state annotations for resource usage (CPU, memory, DB calls per state).

The worst-case search uses the performance-model annotations as the objective: find the path that accumulates the most resource usage. The paper experiments on a simplified e-commerce model and shows the heuristic method achieves >95% of the exact solution's load with orders-of-magnitude less search time.

## Adversarial reading

1. **Requires the performance-model annotation.** Where does "CPU cost of state X" come from? The paper assumes you've already profiled the application. In practice this is the hard part — getting accurate per-state resource costs is itself a significant measurement project.
2. **Graph structure is assumed.** The paper uses hand-built application models. Fitting them from logs is not covered.
3. **Resource-use additive across states.** The objective assumes per-state costs sum across the path. Real systems have non-additive effects (caching, connection pooling, resource contention) that make the true path cost a function of the whole path, not just the states in it.
4. **Small-scale evaluation.** The experimental validation is on toy models. Scaling to a production-scale application with hundreds of states has not been published.
5. **No stochastic arrival integration.** The work finds the worst *single-session* path but does not address what rate of worst-case sessions would overwhelm the system.
6. **Memoryless Markov assumption inherited.** Same limitation as the CBMG: real users have memory.

## Why this closes a real gap

Commercial stress-testing tools typically rely on engineers to hand-select "worst-case scenarios" based on intuition. Intuition is usually wrong — the actual worst case involves a non-obvious sequence (e.g., repeated add-to-cart followed by a failed checkout that re-adds, looping through a cache-busting code path). An algorithmic search over the workload graph is much more likely to find this than a human guessing.

The paper's contribution is a practical recipe: if you have a workload model, you can mechanically find stress scenarios without creative engineer effort.

## Research directions this opens

- Combining worst-case path search with real resource-cost annotations collected via distributed tracing. Jaeger/Tempo gives you the per-operation CPU time and latency; feeding that back into the graph as edge weights would fully automate the annotation step.
- LLM-driven generation of the application-model graph from an OpenAPI spec or a source-code analysis. The Menascé methodology does this by hand; an LLM could plausibly automate it with some accuracy.
- Joint optimisation: find paths that are both *high-load* and *plausible* (high probability in the baseline CBMG), not pathological cases that never occur in real traffic. This is the analog of "realistic counterexamples" in fuzzing.

## Relation to other vault notes

- Worst-case input fuzzing tradition (SlowFuzz, PerfFuzz, Badger) — complementary, operating at function level.
- CBMG / Menascé workload characterisation — provides the graph this paper searches.
- APEX Spec G-46 — target for integrating worst-case workload search.

## Citations

- https://www.sciencedirect.com/science/article/abs/pii/S0167739X18301341
- https://research.abo.fi/en/publications/identifying-worst-case-user-scenarios-for-performance-testing-of-
- https://www.researchgate.net/publication/322871273_Identifying_worst-case_user_scenarios_for_performance_testing_of_web_applications_using_Markov-chain_workload_models