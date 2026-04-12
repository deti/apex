---
id: 01KNZ4VB6JJ51X8KRGKY6VH2W8
title: "ISO/IEC 25010 — Performance Efficiency Quality Characteristic"
type: reference
tags: [iso-25010, squa-re, quality-model, performance-efficiency, time-behaviour, resource-utilisation, capacity, standards]
links:
  - target: 01KNZ4VB6JY38THW04Z3MMGBZ3
    type: related
  - target: 01KNZ4VB6JVSPDK724EZFPA36H
    type: related
  - target: 01KNZ4VB6JK3TC0S5YZWHNNDEV
    type: related
  - target: 01KNWE2Q9YZBAR140ZX5P36TQ5
    type: related
created: 2026-04-12
modified: 2026-04-12
source: "ISO/IEC 25010:2011 Systems and software engineering — Systems and software Quality Requirements and Evaluation (SQuaRE) — System and software quality models"
---

# ISO/IEC 25010 — Performance Efficiency Quality Characteristic

## Where it sits

ISO/IEC 25010:2011 is the "quality model" chapter of the SQuaRE (Systems and software Quality Requirements and Evaluation) family of standards (ISO/IEC 2500n). It defines a hierarchy of quality characteristics for software products. The top level has eight characteristics:

1. Functional suitability
2. **Performance efficiency** ← this note
3. Compatibility
4. Usability
5. Reliability
6. Security
7. Maintainability
8. Portability

A 2023 revision, ISO/IEC 25010:2023, updates the model with minor additions (a ninth "Safety" characteristic and small reorganisations of sub-characteristics) but the Performance efficiency structure below is unchanged.

## The three sub-characteristics

ISO/IEC 25010 decomposes "Performance efficiency" into three sub-characteristics:

### 1. Time behaviour

> *Degree to which the response and processing times and throughput rates of a product or system, when performing its functions, meet requirements.*

This is the target of load testing, stress testing, and most latency-focused performance testing. The standard uses "response time", "processing time" (total compute), and "throughput" as the three primary sub-metrics. The classical "p99 < 300 ms at 1 k req/s" SLO is exactly a time-behaviour requirement.

### 2. Resource utilisation

> *Degree to which the amounts and types of resources used by a product or system, when performing its functions, meet requirements.*

Resource here covers CPU, memory, disk, network bandwidth, battery, thermal budget, financial cost. A system that meets response-time targets but only by burning 95 % of every CPU is resource-inefficient. This sub-characteristic is what the USE method (Utilisation, Saturation, Errors) directly addresses.

### 3. Capacity

> *Degree to which the maximum limits of a product or system parameter meet requirements.*

Parameters here include max concurrent users, max storage volume, max data throughput, max transaction rate. This is what capacity testing is for. Note that the standard uses "capacity" differently from the operational-research sense: ISO 25010 capacity is a *requirement* (e.g. "supports 100 000 concurrent users"), not an *observed metric*.

## Why the separation matters

Performance efficiency is a *group* of three characteristics rather than a single one because they exchange with each other:

- You can trade time for resources: more caching reduces latency at the cost of memory.
- You can trade capacity for time: vertical partitioning reduces per-partition latency at the cost of total capacity per machine.
- You can trade resources for capacity: compression saves storage at the cost of CPU.

An SLO that only constrains time behaviour silently rewards wasteful resource use. A well-specified performance requirement constrains all three: "p99 < 300 ms at 1 k req/s, with CPU utilisation < 70 % and memory resident < 8 GB". All three numbers must be verified simultaneously; load testing a service and reporting only latency misses two thirds of the quality characteristic.

## Fit in the test taxonomy

The Meier et al. 2007 P&P guide (the primary taxonomy source in this vault) maps its performance-test types to ISO 25010 implicitly:

| 25010 sub-characteristic | Meier test types |
|---|---|
| Time behaviour | Performance test, load test, endurance (soak) |
| Resource utilisation | Stress test (when monitoring CPU/mem/disk), load test with resource gates |
| Capacity | Capacity test, scalability test, volume test |

No single test type covers all three sub-characteristics, which is why a complete performance-testing campaign includes several.

## Why the standard matters for APEX

- APEX's G-46 spec (the root note) talks about "SLO assertions" without grounding them in a quality model. 25010 supplies the grounding: SLOs should cover all three sub-characteristics, not just time.
- When APEX generates performance test assertions, the assertion DSL should naturally express (time, resource, capacity) triplets rather than just latency bounds.
- 25010 provides consistent vocabulary for reporting Findings to users who work in regulated environments (medical, automotive, aerospace, banking) where SQuaRE is part of procurement requirements.

## Adversarial reading

- ISO/IEC 25010 is a *terminology* standard, not a *methodology* standard. It tells you what to call things but not how to measure them. That methodology job falls to ISO/IEC 25023 (Measurement of system and software product quality), which gives example metrics but is largely ignored in practice because its examples are unsophisticated (e.g. "number of processes" as a resource metric).
- The standard does not address *percentile* semantics. "Response time meets requirements" leaves open whether that means average, p50, p99, or worst case. In practice this is specified separately, in individual SLOs.
- Unlike CWE (which APEX uses for security findings), 25010 is behind a paywall (~CHF 138 per copy). Most practitioners know it second-hand through Wikipedia and through CISQ summaries.

## References

- ISO/IEC 25010:2011 — Systems and software engineering — Systems and software Quality Requirements and Evaluation (SQuaRE) — System and software quality models — [iso.org/standard/35733.html](https://www.iso.org/standard/35733.html) (paywalled)
- ISO/IEC 25010:2023 — second edition — [iso.org/standard/78176.html](https://www.iso.org/standard/78176.html)
- ISO/IEC 25023:2016 — Measurement of system and software product quality.
- CISQ (Consortium for Information and Software Quality) — free summary documents aligned with 25010 — [it-cisq.org](https://www.it-cisq.org/)
- Meier et al. — Microsoft p&p Performance Testing Guidance.
