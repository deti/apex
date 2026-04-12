---
id: 01KNZ4VB6J08D14Y8P3RWVAABA
title: "Brendan Gregg's Performance Methodologies (USE, Drill-Down, and the Anti-Methodologies)"
type: reference
tags: [brendan-gregg, methodology, use-method, drill-down, workload-characterisation, anti-methodology]
links:
  - target: 01KNZ4VB6JHP7W47HM7QREWW53
    type: related
  - target: 01KNZ4VB6J6ED6F3YHN1SMDNQ5
    type: related
  - target: 01KNZ4VB6JHJSARKD8E9XVGVRC
    type: related
  - target: 01KNWE2Q9YZBAR140ZX5P36TQ5
    type: related
created: 2026-04-12
modified: 2026-04-12
source: "https://www.brendangregg.com/methodology.html"
---

# Brendan Gregg's Performance Methodologies

*Source: https://www.brendangregg.com/methodology.html — fetched 2026-04-12.*
*Reference text: Brendan Gregg, "Systems Performance: Enterprise and the Cloud", 2nd ed., Addison-Wesley 2020, Chapter 2 "Methodologies".*

Brendan Gregg (Intel, ex-Netflix, ex-Sun/Oracle DTrace era) wrote the single most comprehensive catalogue of performance analysis methodologies. The catalogue is useful not just as a list of techniques but as a *vocabulary for disagreeing about how to approach a problem* — one engineer saying "use the USE method" and another saying "no, start with workload characterisation" is a much more productive argument than "no, try perf" vs "no, try strace".

This note summarises the catalogue and pulls out the parts most relevant to load-testing workflow.

## The methodologies (as listed on brendangregg.com/methodology.html)

### Primary methodologies

1. **Problem Statement Method.** Before touching a tool, answer six diagnostic questions: What makes you think there's a performance problem? Has it always been slow? What changed recently? Can you quantify it? Can anyone else reproduce it? What does "fast enough" look like? Often the conversation dissolves the problem before any measurement is needed.

2. **Workload Characterisation.** Describe the workload in concrete terms: *Who* is issuing the load? *Why* is it being called? *What* does the load consist of (request types, sizes, frequencies)? *How* does the load change over time? Answers point at the right investigation target and often reveal that the "problem" is a misunderstanding of what the workload actually is.

3. **Drill-Down Analysis.** Start broadly and narrow. Pick the most interesting breakdown; measure it; drill into the slowest component; repeat until root cause. This is the method for *isolation testing* (see `01KNZ4VB6JRSN6YXB4KC63Y90K`).

4. **USE Method** — *Utilisation, Saturation, Errors* for every resource. Separate note: `01KNZ4VB6JHP7W47HM7QREWW53`. The killer app is completeness: enumerating resources before picking tools catches blind spots.

5. **Thread State Analysis (TSA).** Decompose wall-clock time into OS thread states (executing, runnable/on-queue, uninterruptible sleep, interruptible sleep). Each state has a different investigation path. Built on DTrace/bpftrace.

6. **Latency Analysis.** Decompose a measured latency into layers (app → syscall → kernel → disk), and attribute time to each. Recurse into the slowest. Provides root cause for latency specifically, rather than generic "high CPU".

7. **CPU Profile Method.** Generate a flame graph. Examine any stack consuming > 1 % of CPU. Standard tool-level methodology for CPU-bound problems.

8. **Off-CPU Analysis.** Profile *where threads wait*. The flame graph complement for non-CPU-bound problems (lock wait, I/O wait, sleep).

9. **Scientific Method.** Form a hypothesis about the cause. Design an experiment that would disprove it. Run the experiment. Update the hypothesis. Standard science; under-used in performance analysis because engineers jump from observation to "solution" without going through hypothesis.

10. **Ad Hoc Checklist.** A fixed list of checks, run through in order, stopping at the first abnormal reading. The USE Method is a specific kind of ad-hoc checklist. Others exist for specific environments (Netflix Linux perf checklist).

### Anti-methodologies

The methodologies page includes a counter-list of what *not* to do. These are the ones Gregg sees most often:

1. **Streetlight Anti-Method.** "I'm looking here because the light is better" — diagnosing the problem using whatever tool happens to be familiar, regardless of whether it's the right tool. The engineer who only knows `top` blames CPU for every problem, even when it's I/O or network.

2. **Random Change Anti-Method.** Try random tuning changes until something sticks. Accidentally correct answers leave no understanding and no way to generalise. The "change X → observe Y" loop without a hypothesis is not science.

3. **Blame-Someone-Else Anti-Method.** "Must be the network", "must be the DB", hand-off without evidence. Responsibility diffuses; nothing gets fixed.

4. **Tools Method.** Start with available tools and exhaustively examine their metrics, hoping to find an anomaly. Problem: missing the right tool means missing the problem, and the engineer doesn't know what they're missing. Contrast with USE Method, which enumerates questions before tools.

5. **Drunk-Under-The-Streetlight variant.** Examining only production monitoring dashboards because that's where the charts live, even when the problem is upstream or downstream.

## Applying these to load-testing workflow

A performance test fails; what do you do?

1. **Problem Statement first.** Is the failure real? Is it reproducible? What's the SLO?
2. **Workload Characterisation.** What is the test actually doing? Is it testing the right workload? (Often the test was subtly wrong.)
3. **USE Method on the SUT.** Walk through CPU, memory, disk, network, app-level resources (connection pool, thread pool). Find the saturated resource.
4. **Drill-Down.** From the saturated resource, narrow until you find the specific component or code path.
5. **Latency Analysis and off-CPU** for the specific code path. Flame graphs show where the time goes.
6. **Scientific Method** for validating the suspected fix: hypothesise → change → remeasure.

The failure mode is skipping step 1 and jumping directly to step 6 ("I bet it's GC, let me tune GC") — streetlight anti-method in action.

## Why the catalogue matters

Most orgs don't have one methodology; they have a collection of individual engineers each with a favourite approach. Gregg's catalogue is useful not because one method is best but because it provides a shared vocabulary: "I tried USE and nothing lit up, now doing workload characterisation" is a statement any performance engineer can understand and contribute to. Without the catalogue, the same investigation is a series of un-labelled gestures that nobody can audit.

## Relevance to APEX

- APEX's detectors produce "Findings" — claims about specific performance bugs. Each detector implements an analysis that could be classed using Gregg's methodologies: the complexity estimator is **Workload Characterisation + Latency Analysis**, the ReDoS detector is **Scientific Method** (hypothesis-driven — this regex will backtrack), the resource-profiler is **USE Method applied at the function level**.
- The APEX Finding format should include not just the bug but the methodology that produced it, so users can re-apply the same method when APEX misses a related bug.
- When APEX produces a report, the "how to investigate further" guidance should reference the relevant Gregg methodology — a small amount of shared vocabulary goes a long way.

## Adversarial reading

- Gregg's methodologies are catalogue entries, not prescriptions. Real investigations skip between them, iterate, and combine. The catalogue is a map, not a plan.
- "Problem Statement first" sounds obvious but is the most-skipped step. Engineers like to start with tools because tools are concrete; the problem statement is abstract. Resist.
- USE Method in particular is weak on software resources (connection pools, lock queues, GC generations) — it was designed around hardware. Extend it deliberately.

## References

- Gregg, B. — "Systems Performance: Enterprise and the Cloud", 2nd ed., Addison-Wesley 2020 — Ch. 2 Methodologies.
- Gregg, B. — "The USE Method" — [brendangregg.com/usemethod.html](https://www.brendangregg.com/usemethod.html)
- Gregg, B. — "Linux Performance Analysis in 60,000 Milliseconds" — Netflix Tech Blog — classic ad-hoc checklist.
- Gregg, B. — Flame Graphs — [brendangregg.com/flamegraphs.html](https://www.brendangregg.com/flamegraphs.html)
