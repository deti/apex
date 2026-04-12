---
id: 01KNZ67FCNDFZMA3XVZ7K6DNKF
title: Daly 2021 — Creating a Virtuous Cycle in Performance Testing at MongoDB (ICPE 2021)
type: literature
tags: [mongodb, performance-testing, ci-cd, icpe-2021, daly, organisational]
links:
  - target: 01KNZ67FBPEC378X6KZ79305T0
    type: related
  - target: 01KNZ67FCARB8N2V5KPN8TY1PG
    type: related
  - target: 01KNZ4VB6J5ZW3JERZNDNGP7GD
    type: related
  - target: 01KNZ6WHSTDKHSQERTN6XZC90B
    type: related
  - target: 01KNWE2Q9YZBAR140ZX5P36TQ5
    type: related
created: 2026-04-11T21:12:39.061084+00:00
modified: 2026-04-11T21:12:39.061086+00:00
---

*Source: David Daly — "Creating a Virtuous Cycle in Performance Testing at MongoDB" — ICPE 2021, arXiv:2101.10231, January 2021.*

Companion piece to Daly et al. 2020 (the change-point detection paper). Where the 2020 paper covers the **algorithm**, this paper covers the **organisation**. Together they are a complete picture of how MongoDB built a CI performance testing practice that stuck.

## The "virtuous cycle" framing

The core thesis: a performance testing system is an investment that only pays off if usage grows over time, and usage grows only if the system produces actionable signal. The positive feedback loop is:

> "Performance test improvements drive impact, which drives more use, which drives further impact and investment in improvements."

The failure mode — which Daly has seen at MongoDB and elsewhere — is the **vicious cycle**: a noisy dashboard nobody looks at, regressions that are detected but not actioned, developers who stop caring because the signal is bad, which leads to less investment in the system, which leads to a noisier dashboard. Once this starts, pulling out requires leadership commitment.

## The three levers MongoDB pulled

1. **Coverage.** Expand the benchmark suite to cover more of the code paths that matter. More coverage = more chances to catch a regression early = more impact.
2. **Signal quality.** Faster and more accurate detection of performance changes — this is where the 2020 change-point detection work lives. Signal quality reduces alert fatigue and keeps humans engaged in triage.
3. **State visibility.** Dashboards and reports that let engineers understand the overall performance landscape of the product, not just answer "did my PR regress anything?"

## Why this matters for toolmakers

The MongoDB experience is the most clearly-written industrial account of why **a good perf CI system is 80% process and 20% algorithm.** You can drop in Hunter or write your own E-divisive implementation, but if:
- benchmarks aren't curated,
- noise isn't controlled at the hardware level,
- a sheriff rotation isn't funded,
- regressions aren't triaged within the same working week they were detected,

…then the algorithm doesn't matter. This is the organisational context that makes the 2020 paper work in practice and is absent from most academic papers on CPD.

## Adversarial commentary

- **Generalisability.** MongoDB had a dedicated performance team of multiple engineers when this was written. The virtuous cycle is harder to bootstrap in a company that doesn't yet have a team — Daly argues you can start small and grow, but doesn't solve the cold-start problem.
- **Metric for the virtuous cycle.** The paper is qualitative. It would be valuable to have a quantitative measure of "health" — e.g., mean time to regression triage, fraction of alerts actionable, ratio of true positives to false positives on the sheriff rotation. Future work.
- **The "impact" feedback loop is slow.** Months to years, not days. For a team in the vicious-cycle trough, the paper gives direction but not a short-term rescue plan.

## Connections

- Sibling to Daly et al. 2020 (change-point detection algorithm).
- Complements Mozilla Perfherder's sheriff rotation — similar organisational pattern.
- Related to Google SRE Workbook's emphasis on operational practices over tools.

## Reference

Daly, D. (2021). *Creating a Virtuous Cycle in Performance Testing at MongoDB*. ICPE 2021. arXiv:2101.10231.
