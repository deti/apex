---
id: 01KNZ68KDQCP6HGTQEBGQW26VC
title: PerfGen — LLM-Assisted Performance Benchmark Generation for Big Data Analytics
type: literature
tags: [perfgen, benchmark-generation, big-data, grey-box-fuzzing, llm, arxiv-2024, test-generation]
links:
  - target: 01KNZ5SM642DR52PJ1CDNEZ101
    type: related
  - target: 01KNZ6QBKG64F2WEP3PNJK61JH
    type: related
  - target: 01KNZ67FBPEC378X6KZ79305T0
    type: related
  - target: 01KNZ68KG6N588XFB9A13H7RQZ
    type: related
  - target: 01KNWE2Q9YZBAR140ZX5P36TQ5
    type: related
created: 2026-04-11T21:13:15.959121+00:00
modified: 2026-04-11T21:13:15.959128+00:00
source: "https://arxiv.org/abs/2412.04687"
---

# PerfGen — Automated Performance Benchmark Generation for Big Data Analytics

*ArXiv:2412.04687 (December 2024). One of very few published papers specifically on performance-test *generation* (rather than test execution). Targets Apache Spark and similar big-data analytics frameworks.*

## Problem PerfGen addresses

Big data analytics jobs exhibit performance symptoms that are deeply input-dependent:

- **Computational skew.** A UDF is expensive for some rows and cheap for others.
- **Data skew.** A join or group-by has highly non-uniform key distribution.
- **Memory skew.** Certain input patterns cause a single executor to OOM while others spread work evenly.

These symptoms are hard to trigger in a regression test because they require specific input shapes. Generic benchmarks (TPC-H) miss them. Random data generation misses them too — the symptoms are triggered by *structured* input patterns, not uniform noise.

## Approach

PerfGen uses a **phased grey-box fuzzing approach**:

1. **User specifies a performance symptom of interest** via a "performance monitor template" — effectively a rule like "OOM in Spark executor X" or "job stage Y takes >10× median runtime."
2. **Phase 1: intermediate-state fuzzing.** The tool fuzzes directly at the stage boundary — if the symptom manifests during stage 3 of a 5-stage pipeline, PerfGen fuzzes the *intermediate output of stage 2* rather than the pipeline's top-level input. This is much more sample-efficient because you're fuzzing closer to where the symptom occurs.
3. **Phase 2: pseudo-inverse back-propagation.** Once a symptom-causing intermediate input is found, PerfGen needs to map it back to a top-level input that would produce that intermediate. For this, the paper uses an **LLM to generate a pseudo-inverse function** — code that reverses the pipeline stage from intermediate back to top-level. The LLM approach is necessary because real pipeline stages are arbitrary UDFs with no analytic inverse.
4. **Benchmark generation.** The generated top-level input is the benchmark — when you run the pipeline on it, the symptom fires reproducibly.

## Claimed results

- **43× speedup** compared to a naive fuzzing approach that directly fuzzes top-level input. Much of this comes from the intermediate-state fuzzing step.
- Successful symptom triggering on multiple Apache Spark workloads.

## What's novel

1. **Phased fuzzing targeted at performance symptoms.** Most fuzzers search for crashes or memory errors. PerfGen searches for *perf symptoms*, which means the fitness function is a resource-usage measurement (OOM, execution time) rather than a crash signal. This is rare.
2. **LLM as pseudo-inverse oracle.** The LLM is used for a specific, verifiable sub-task: write a function that maps intermediate state back to top-level input. The result can be validated (run the LLM's proposed inverse, check if its output produces the intermediate). This is a good pattern — LLM for code generation on a narrow problem with an objective oracle.
3. **Targets big data specifically.** Most perf research targets low-level code (libraries, compilers). PerfGen targets distributed analytics pipelines, where the performance bugs are structurally different (data-flow-driven, not control-flow-driven).

## Adversarial reading

1. **Monitor templates are user-supplied.** The user has to know what kind of symptom they care about. For incident-driven benchmarking this is fine — you know what broke — but it does not discover *new* symptom classes.
2. **Pseudo-inverse may not exist.** If the pipeline stage loses information (projection, aggregation), a true inverse is impossible. The LLM produces a plausible but potentially incorrect reverse. The paper validates by running forward; when the validation fails, the approach falls back to top-level fuzzing.
3. **Scope: Spark-specific.** The paper's evaluation is on Spark. Porting to Flink, Presto, or other frameworks is non-trivial.
4. **LLM cost.** Each pseudo-inverse generation is an LLM call. For many iterations the cost adds up.
5. **No workload-model integration.** The generated benchmarks trigger symptoms; they don't necessarily resemble production workloads. A benchmark that OOMs on a pathological 1-row input is a reproducer, not a realistic load test. Useful for regression but not for capacity planning.

## Relation to the rest of the vault

- Conceptually closest to **PerfFuzz** (already present): both are coverage/symptom-guided fuzzers targeted at performance. PerfFuzz works at the function level; PerfGen works at the pipeline-stage level.
- The LLM pseudo-inverse technique is a specific instance of the broader **LLM-as-targeted-generator** pattern I've discussed in the LLM-frontier note.
- The "phase 1 find intermediate, phase 2 back-propagate" idea is reminiscent of **WISE** (already present), which uses symbolic execution to find worst-case inputs by working backwards from an exit condition.

## What would be next

A natural follow-on: use distributed traces as the intermediate-state capture (rather than Spark-specific instrumentation), making the approach framework-agnostic. Traces naturally contain the stage boundaries and the data volumes at each stage. LLM-pseudo-inverse to map traced intermediate state back to API-level inputs would give you a general-purpose perf-symptom reproducer. This is a plausible APEX-adjacent direction.

## Citations

- ArXiv: https://arxiv.org/abs/2412.04687
- HTML version: https://arxiv.org/html/2412.04687
- Literature review summary: https://www.themoonlight.io/en/review/perfgen-automated-performance-benchmark-generation-for-big-data-analytics