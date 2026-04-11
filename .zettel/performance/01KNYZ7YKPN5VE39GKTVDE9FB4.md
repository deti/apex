---
id: 01KNYZ7YKPN5VE39GKTVDE9FB4
title: "Google SRE Book: Service Level Objectives"
type: literature
tags: [sre, google, slo, sli, sla, percentiles, error-budget]
links:
  - target: 01KNWE2QA5VP0K80TMSABACKWT
    type: references
  - target: 01KNWE2QACWYZJXRJ8QE78T043
    type: related
  - target: 01KNWE2Q9YZBAR140ZX5P36TQ5
    type: related
created: 2026-04-10
modified: 2026-04-10
source: "https://sre.google/sre-book/service-level-objectives/"
---

# Google SRE Book — Service Level Objectives Chapter

*Source: https://sre.google/sre-book/service-level-objectives/ — fetched 2026-04-10.*

This chapter is the canonical reference for the SLI/SLO/SLA taxonomy that underpins performance assertion in APEX G-46's "SLO verification mode". The authors are Google's Site Reliability Engineering team.

## Core Definitions

**Service Level Indicator (SLI)** — a quantitative measurement of service aspects users experience. Common examples include request latency, error rate, system throughput, and availability.

**Service Level Objective (SLO)** — a target value or range for an SLI. The text notes: *"A natural structure for SLOs is thus SLI ≤ target, or lower bound ≤ SLI ≤ upper bound."*

**Service Level Agreement (SLA)** — an explicit or implicit contract with users that specifies consequences for missing SLOs. The book clarifies: *"An easy way to tell the difference between an SLO and an SLA is to ask 'what happens if the SLOs aren't met?'"* — if someone has to refund you money, it's an SLA. If the oncall gets paged, it's an SLO.

## Key Measurement Principles

**Percentiles Over Averages**: The chapter emphasises using percentile distributions rather than arithmetic means. Averages can obscure tail latencies — the text explains that a system averaging 50 ms response time might have 5% of requests taking 20 times longer. This is why most SLO reports focus on p95, p99, and p99.9.

**Aggregation Matters**: How data is collected significantly impacts interpretation. Averaging across different time windows can hide performance spikes and burst behaviour. An SLO measured over 5-minute windows will treat a 30-second catastrophic burst differently from one measured over 1-second windows.

**Service Categories**: Different system types prioritise different SLIs:

- **User-facing systems** — availability, latency, throughput
- **Storage systems** — latency, availability, durability
- **Big data pipelines** — throughput, end-to-end latency

## Choosing Effective SLOs

The chapter recommends:

- Starting with user needs, not what's easily measurable
- Keeping definitions simple and standardised
- Avoiding absolute perfection targets
- Maintaining **error budgets** allowing controlled SLO misses
- Using safety margins between internal and published targets

**Publishing Impact**: Making SLOs public sets user expectations but requires careful consideration to prevent over-reliance on services or building unsustainable operational demands.

## Relevance to APEX G-46

The SLO chapter is the theoretical grounding for APEX's `apex perf --slo "parse:100ms:10KB"` CLI mode. Several of its insights should propagate to APEX:

1. **Report percentiles, not means.** APEX's SLO verification output should include median, p95, p99 and the tail shape, not just pass/fail against a scalar threshold.
2. **SLOs are lower bounds on measurement sophistication.** An SLO of "100 ms p95" is not verifiable with a single 10-iteration benchmark — you need enough samples to have a confident p95. Criterion.rs-style bootstrap CIs on the tail quantiles are the right methodology.
3. **Error budgets are useful for regression detection.** If an APEX baseline file records past SLO-window tail performance, a new run burning down the budget is a more principled regression signal than "is the median 2x bigger?".
4. **The SLO is a user-facing thing.** When APEX flags an SLO violation, the Finding should be in user-readable terms: "the `parse` function took 340 ms at p95 for 10 KB inputs, violating the declared SLO of 100 ms."

## References (from the chapter)

- Beyer, Jones, Petoff, Murphy — "Site Reliability Engineering" — O'Reilly, 2016
- Beyer, Murphy, Rensin, Kawahara, Thorne — "The Site Reliability Workbook" — O'Reilly, 2018
- Adya et al. — "Auto-sharding for datacenter applications" — OSDI 2016 (example cited in the book)
