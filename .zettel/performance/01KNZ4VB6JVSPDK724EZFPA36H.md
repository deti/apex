---
id: 01KNZ4VB6JVSPDK724EZFPA36H
title: "Meier et al. 2007 — Microsoft p&p Performance Testing Guidance for Web Applications"
type: literature
tags: [meier-2007, microsoft, patterns-practices, guide, workflow, taxonomy, reference-text]
links:
  - target: 01KNZ4VB6JY38THW04Z3MMGBZ3
    type: related
  - target: 01KNZ4VB6JTC1Z9CGYN4Q1CCA6
    type: related
  - target: 01KNZ4VB6JCKKSRJ6FE6ST9183
    type: related
  - target: 01KNZ4VB6J29PS11RZNH5K0E47
    type: related
  - target: 01KNZ4VB6JY3QDARVD4N06HR6X
    type: related
  - target: 01KNZ4VB6JJ702SZ7R31SMAJG2
    type: related
  - target: 01KNZ4VB6JRSN6YXB4KC63Y90K
    type: related
  - target: 01KNZ4VB6JK3TC0S5YZWHNNDEV
    type: related
  - target: 01KNZ4VB6JKC337NWTGFZRA8GF
    type: related
  - target: 01KNWE2Q9YZBAR140ZX5P36TQ5
    type: related
created: 2026-04-12
modified: 2026-04-12
source: "https://learn.microsoft.com/en-us/previous-versions/msp-n-p/bb924375(v=pandp.10)"
---

# Meier et al. — Performance Testing Guidance for Web Applications (Microsoft p&p 2007)

*Primary source: J.D. Meier, Carlos Farre, Prashant Bansode, Scott Barber, Dennis Rea — "Performance Testing Guidance for Web Applications" — Microsoft patterns & practices, September 2007. Archived at https://learn.microsoft.com/en-us/previous-versions/msp-n-p/bb924375(v=pandp.10). Fetched 2026-04-12.*

## Why this is the canonical reference for the test taxonomy

The Microsoft patterns & practices "Performance Testing Guidance for Web Applications" is a ~300-page free guide that has been the de facto reference for web performance testing vocabulary since 2007. Although archived and never updated past 2010, its taxonomy and workflow are the foundation every subsequent textbook builds on:

- ISTQB Foundation Syllabus (Performance Testing specialisation) uses Meier's categories.
- Jez Humble's *Continuous Delivery* (2010) cites it directly.
- Scott Barber (one of the authors) spun off *Beyond Performance Testing* with similar conceptual structure.
- k6, Gatling, and JMeter documentation teach the Meier taxonomy without always citing it.

The guide was produced at Microsoft patterns & practices, the same group that produced "Application Architecture Guide", and follows their "activities with inputs, outputs, steps" style.

## Structure

The guide has eight parts and 18 chapters:

**Part I — Introduction to Performance Testing**
- Ch. 1 Fundamentals
- Ch. 2 **Types of Performance Testing** — the definitive taxonomy (load, stress, capacity, spike, endurance, component, smoke, unit, validation, investigation). Quoted extensively in the per-type notes in this vault.
- Ch. 3 Risks Addressed

**Part II — Exemplar Approaches** (four chapters on how to structure a testing cycle)
- Ch. 4 Core activities
- Ch. 5 Iteration-based (waterfall-ish)
- Ch. 6 Agile
- Ch. 7 CMMI / regulated

**Part III — Environment**
- Ch. 8 **Evaluating Systems to Increase Effectiveness** — environment parity, discussed in `01KNZ4VB6J22PTMXAYQ3V2WYAZ`.

**Part IV — Acceptance Criteria**
- Ch. 9 Determining Objectives
- Ch. 10 Quantifying End-User Response Time Goals
- Ch. 11 Consolidating Criteria

**Part V — Plan and Design**
- Ch. 12 **Modeling Application Usage** — think-time, session, workload model parameters.
- Ch. 13 Individual User Data and Variances

**Part VI — Execute**
- Ch. 14 Test Execution

**Part VII — Analyze and Report**
- Ch. 15 **Key Mathematic Principles for Performance Testers** — Little's Law, percentile interpretation, the basics of statistics for perf.
- Ch. 16 Reporting Fundamentals

**Part VIII — Techniques**
- Ch. 17 Load-Testing Web Applications
- Ch. 18 Stress-Testing Web Applications

## The central taxonomy (Chapter 2) — verbatim summary

The guide defines *performance testing* as the umbrella concept and then enumerates:

| Type | Question | Subset of |
|---|---|---|
| Performance test | Determine or validate speed, scalability, stability (general) | — |
| Load test | Behaviour under normal and peak load | Performance test |
| Stress test | Behaviour beyond normal/peak | Performance test |
| Capacity test | How many users/transactions at SLO | Performance test |
| **Endurance test** | Behaviour over extended time at normal load | Subset of load test |
| **Spike test** | Behaviour under repeated short overloads | Subset of stress test |
| Component test | Performance of a specific architectural component | Performance test |
| Smoke test | Initial "does it run" check | — |
| Unit test | Performance test of a code module | Performance test |
| Validation test | Comparison against stated expectations | Performance test |
| Investigation | Information-gathering about performance characteristics | — |

Note the hierarchical relationships: endurance is a subset of load; spike is a subset of stress. These are Meier's classifications; other taxonomies (ISTQB glossary, ISO/IEC/IEEE 29119) treat endurance and spike as siblings. The classification matters for cost allocation and coverage-calibration decisions.

## The workflow (chapters 4–16)

Meier et al.'s core activities for a performance test:

1. **Identify test environment.**
2. **Identify performance acceptance criteria.**
3. **Plan and design tests.**
4. **Configure test environment.**
5. **Implement test design.**
6. **Execute tests.**
7. **Analyze, report, retest.**

Each activity has inputs (e.g. "application knowledge, hardware inventory, user profiles"), outputs (e.g. "documented workload model, test scripts, baseline SLIs"), and steps. The activity-based framing is what made the guide practical for regulated environments.

## Why it matters for APEX

- APEX's G-46 spec (the vault root) is missing most of the Meier taxonomy. G-46 focuses on *input generation* (worst-case inputs, complexity estimation, ReDoS) and on *single-function profiling*. It has no notion of workload modelling, SLO acceptance criteria, or execution/analysis/reporting. Any extension of APEX into "load testing territory" should start from the Meier workflow.
- The per-type notes in this vault (load, stress, spike, soak, capacity, volume, configuration, isolation) are directly derived from Meier Ch. 2 and the subsequent chapters.
- Meier Ch. 15 "Key Mathematic Principles" covers Little's Law and basic percentile interpretation — weaker than this vault's treatment (because it pre-dates HdrHistogram and Gil Tene's coordinated-omission work) but the vocabulary is compatible.

## What Meier et al. does not cover well

The guide was written 2006–2007 and is missing everything that emerged after:

- **Coordinated omission** (Tene 2013). Meier's latency guidance assumes single-run means and percentiles are valid; no mention of the generator-level bias.
- **HdrHistogram** (Tene 2012). Meier's reporting chapter assumes either raw samples or fixed-bin histograms.
- **Open vs closed workload models** (Schroeder 2006). Meier is agnostic about which to use and doesn't explain the stakes. Schroeder's paper was contemporaneous.
- **Statistical rigor** (Georges 2007, Mytkowicz 2009, Curtsinger 2013). Meier treats statistical variance naively.
- **SRE methodology** (Google 2016 SRE book). Meier talks about SLAs but not SLOs, and not in the error-budget framing.
- **Cloud / containerisation / microservices.** Written for web applications deployed on bare servers or IIS, not for fleet-of-microservices-on-Kubernetes.
- **Self-similar traffic** (Leland 1994). Arrival distribution choices are thinly treated.

A practitioner using Meier as their sole reference will get the vocabulary right and the workflow structure right but will miss two decades of measurement-rigor developments. The vault treats Meier as the taxonomy hub and augments with post-2010 literature for measurement correctness.

## References

- Meier, J.D., Farre, C., Bansode, P., Barber, S., Rea, D. — "Performance Testing Guidance for Web Applications" — Microsoft patterns & practices, September 2007 — [learn.microsoft.com/previous-versions/msp-n-p/bb924375(v=pandp.10)](https://learn.microsoft.com/en-us/previous-versions/msp-n-p/bb924375(v=pandp.10))
- Barber, S. — *Beyond Performance Testing: A Series* — PerfTestPlus, 2004–2007.
- ISO/IEC/IEEE 29119-4:2015 — Software testing — Test techniques.
- ISTQB Foundation Syllabus for Performance Testing — ISTQB 2018.
