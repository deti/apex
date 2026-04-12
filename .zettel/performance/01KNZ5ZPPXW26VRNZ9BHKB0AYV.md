---
id: 01KNZ5ZPPXW26VRNZ9BHKB0AYV
title: "Customer Behavior Model Graph (CBMG) — Menascé's Workload Model"
type: literature
tags: [cbmg, menasce, workload-characterization, markov-chain, user-behavior, performance-modeling, capacity-planning]
links:
  - target: 01KNZ6QBH0YZYKPZNYDCZD5P2B
    type: related
  - target: 01KNZ4VB6J3AB4QA4YZVDPMFWY
    type: related
  - target: 01KNZ4VB6JHJSARKD8E9XVGVRC
    type: related
  - target: 01KNZ5ZPVXKFXJPF3MNWTYXMZ1
    type: related
  - target: 01KNZ5ZPSEK679QQYMHXF16WFF
    type: related
  - target: 01KNZ56MS9HQJ2HJ2ADJ7MBMAX
    type: related
  - target: 01KNZ6QBKG64F2WEP3PNJK61JH
    type: related
  - target: 01KNWE2Q9YZBAR140ZX5P36TQ5
    type: related
created: 2026-04-11T21:08:24.413214+00:00
modified: 2026-04-11T21:08:24.413221+00:00
source: "https://cs.gmu.edu/~menasce/ebook/toc.html"
---

# Customer Behavior Model Graph (CBMG)

*From Daniel A. Menascé and colleagues at George Mason University, published throughout the early-2000s capacity-planning literature; most accessible presentation in "Scaling for E-Business" (Menascé & Almeida, 2000) and "Capacity Planning for Web Services" (Menascé & Almeida, 2001). The CBMG is a foundational concept for server-side workload characterization.*

## What the CBMG is

A **Customer Behavior Model Graph** is a state-transition graph (formally a discrete-time Markov chain with an absorbing state) that describes the navigational pattern of a group of customers as observed from the server side. Nodes correspond to states — typically pages or API endpoints a user can be in during a session. Edges are transitions with associated probabilities. There is an implicit absorbing "exit" state to represent session end.

Menascé's canonical example is an online bookstore with states like Home, Login, Search, View Product, Add to Cart, Checkout, Pay, Logout. The CBMG captures, for this population of users: "from Search, 0.4 go to View Product, 0.3 go back to Home, 0.1 go to Login, 0.2 exit."

Formally: **CBMG = (V, E, P)** where V is the set of states, E ⊆ V×V is the set of transitions, and P is the transition-probability matrix. The steady-state visit ratios to each state are derived by solving the Markov equations for the chain.

## Why this matters for performance test generation

The CBMG is, as far as I know, the cleanest formal model of a **realistic user workload** in the entire server-side performance literature. It encodes three things that every other workload model lacks:

1. **Session structure.** A user is not a bag of independent requests; they follow a path.
2. **Transition probabilities from observation.** Probabilities are estimated from real access logs or session logs, not declared by hand.
3. **Per-population parameterisation.** Different customer clusters can have different CBMGs — registered users vs. guests, mobile vs. desktop — so the overall workload is a mixture.

For anyone trying to generate load tests from first principles, the CBMG is the model to implement. A generator that takes access logs, clusters sessions, fits a per-cluster CBMG, and emits a load test with per-cluster Markov-chain-driven virtual users is a far more realistic test than anything OpenAPI-spec-driven.

## The methodology Menascé recommends

From Chapter 6 of "Capacity Planning for Web Services":

1. **Monitor.** Collect raw session logs for a representative time window.
2. **Characterise.** Define the set of parameters that capture the workload (session length, requests per session, think-time distribution, resource usage per state).
3. **Cluster.** Group sessions into populations with similar behaviour using k-means or similar. The cluster count is a hyper-parameter chosen by how well clusters separate.
4. **Fit.** Estimate transition probabilities per cluster by counting observed transitions.
5. **Validate.** Check that simulated traffic from the CBMG matches observed distributions (visit counts, session length). Key test: Kolmogorov-Smirnov on session-length distribution.
6. **Calibrate.** Tune parameters if the validation step fails.
7. **Forecast.** Apply growth projections (chapter 12) to predict how the workload changes.

This methodology predates modern ML tools but maps cleanly onto them: the clustering step is HDBSCAN or similar; the validation step is goodness-of-fit statistical testing; the forecasting step is time-series forecasting on traffic metrics.

## Adoption beyond the academic literature

The single biggest mainstream artefact derived from the CBMG is **TPC-W**, the Transaction Processing Performance Council's benchmark for e-commerce web services. TPC-W's workload specification is explicitly constructed as a CBMG with Browsing, Shopping, and Ordering mix classes. TPC-W was retired in 2005 but remains the cleanest publicly documented CBMG-based workload.

Modern load-test tools (k6, Gatling, JMeter) do *not* implement CBMG primitives natively. Engineers can hand-encode Markov chains in user-defined scripts but there is no tool that says "here is my CBMG, go generate load."

IEEE published a paper in 2007 ("Analyzing Customer Behavior Model Graph (CBMG) using Markov Chains") that formalises the analytical properties. A 2018 Åbo Akademi paper ("Identifying worst-case user scenarios for performance testing of web applications using Markov-chain workload models") extends the CBMG idea to *worst-case* scenario generation — finding the paths through the graph that maximise resource utilisation, with applications to stress testing.

## Failure modes and critiques

1. **Memoryless assumption.** A standard DTMC is memoryless — the next state depends only on the current state, not on history. Real users often have multi-step memory: after viewing three similar products, they are more likely to buy. Variable-length Markov chains (VLMC) and higher-order DTMCs (IEEE 2007 paper) address this at the cost of more parameters.
2. **Static model.** A fitted CBMG captures the distribution at a point in time. It does not track drift. Re-fitting is periodic, not continuous.
3. **State granularity trade-off.** Too-fine state (every URL) yields a sparse matrix with poor estimation. Too-coarse state (every URL family) loses information about which specific endpoint is hit. Choosing the state definition is the crucial design decision and there is no general answer.
4. **Steady-state assumption.** Markov-chain analysis is steady-state — the visit ratios are the long-run proportions. Real traffic has strong time-of-day patterns that violate steady-state; you need multiple CBMGs per time bucket.
5. **Requires server-side logging to work well.** Client-side RUM only sees pages that loaded successfully; it misses abandoned flows.
6. **Cluster-count and clustering-algorithm choice is a lever.** Different clustering choices give meaningfully different CBMGs, and the authors' recommended methodology is light on guidance for picking between them.

## The modern path this opens

If you combine the CBMG methodology with:

- Modern clustering (HDBSCAN, Gaussian mixtures) for cleaner cluster separation,
- Distributed traces instead of access logs for richer state features (user ID, tenant, feature flags),
- An LLM for suggesting state granularity and validating the fit,
- A k6 extension for per-VU Markov-chain execution,

you arrive at a practical, production-deployable CBMG load-test generator. This is, in my view, the single highest-leverage gap in the load-testing tool space. Menascé's methodology is correct, the data is now universally available via tracing, and the last missing piece is a tool that ties it all together.

## Citations

- Menascé's CBMG page: https://cs.gmu.edu/~menasce/ebook/toc.html
- O'Reilly excerpt from Scaling for E-Business: https://www.oreilly.com/library/view/scaling-for-e-business/0130863289/0130863289_ch02lev1sec4.html
- IEEE 2007 analysis paper: https://ieeexplore.ieee.org/document/4283675/
- Menascé workload-characterization methodology paper: https://cs.brown.edu/~rfonseca/pubs/menasce99e-com-char.pdf
- Fractal characterization of web workloads: https://cs.gmu.edu/~menasce/papers/web-engineering.pdf
- Åbo Akademi worst-case Markov paper: https://www.sciencedirect.com/science/article/abs/pii/S0167739X18301341
- Capacity Planning for Web Services (book site): https://cs.gmu.edu/~menasce/webservices/toc.html