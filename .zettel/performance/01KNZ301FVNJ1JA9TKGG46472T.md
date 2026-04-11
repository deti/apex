---
id: 01KNZ301FVNJ1JA9TKGG46472T
title: "MemLock: Memory Usage Guided Fuzzing"
type: literature
tags: [paper, performance, fuzzing, memory, resource-guided, cwe-400, cwe-789, uncontrolled-recursion, uncontrolled-allocation]
links:
  - target: 01KNWE2Q9YZBAR140ZX5P36TQ5
    type: references
  - target: 01KNWE2QA0Z52H8VVFAMSA7KGA
    type: extends
  - target: 01KNWE2QA6QKE8Z152WX8D9XYB
    type: extends
  - target: 01KNWGA5F4W852RG6C5FJCP204
    type: extends
  - target: 01KNWGA5FEAC0QN3PK6CAYP7T8
    type: extends
  - target: 01KNWGA5FCBGJCSJJ3XPH1H1DG
    type: references
  - target: 01KNWEGYB3NXWFB6D4SV4DTD5X
    type: related
  - target: 01KNWEGYB1B15QGYTRC374Z7DQ
    type: related
  - target: 01KNWGA5GD7A7WXW56682R280K
    type: extends
created: 2026-04-12
modified: 2026-04-12
source: "https://doi.org/10.1145/3377811.3380396"
source_mirror: "https://wcventure.github.io/pdf/ICSE2020_MemLock.pdf"
artifact: "https://github.com/wcventure/MemLock-Fuzz"
venue: ICSE 2020
authors: [Cheng Wen, Haijun Wang, Yuekang Li, Shengchao Qin, Yang Liu, Zhiwu Xu, Hongxu Chen, Xiaofei Xie, Geguang Pu, Ting Liu]
year: 2020
---

# MemLock: Memory Usage Guided Fuzzing

**Authors:** Cheng Wen, Haijun Wang, Yuekang Li, Shengchao Qin, Yang Liu, Zhiwu Xu, Hongxu Chen, Xiaofei Xie, Geguang Pu, Ting Liu
**Venue:** 42nd International Conference on Software Engineering (ICSE 2020), Seoul, July 2020
**DOI:** 10.1145/3377811.3380396
**Artifact:** https://github.com/wcventure/MemLock-Fuzz (MIT)

## Retrieval Notes

The ICSE page and the GitHub artifact repository were accessible; the PDF mirror at wcventure.github.io returns a binary PDF stream not decodable in this sandbox. The body below is synthesized from the ICSE proceedings abstract, the artifact README, the Semantic Scholar summary, and standard secondary accounts of the paper.

## Problem Statement

Excessive memory consumption bugs — uncontrolled allocation (CWE-789) and uncontrolled recursion (CWE-674 / CWE-834) — are a high-impact class of denial-of-service vulnerabilities. Unlike classical memory safety bugs (UAF, OOB write, double-free), they do not produce a crash that coverage-guided fuzzers like AFL can directly latch onto. A 10-line input that allocates 4 GB of memory or recurses to a stack overflow typically looks, from AFL's perspective, like an input that merely survived or timed out — no new edge is reported, so the input is not prioritized.

MemLock attacks this visibility gap by making memory consumption itself a first-class feedback signal.

## Key Insight

Two orthogonal axes of memory-bug severity exist, and they correspond to different program structures:

1. **Stack exhaustion** is driven by call graph depth. Recursive functions with adversary-controlled recursion conditions are the chief source. To detect these, a fuzzer must reward inputs that increase the *maximum recursion depth* observed at runtime.

2. **Heap exhaustion** is driven by allocation statements whose size or count is adversary-controlled. Canonical examples: `malloc(n)` where `n` is attacker-supplied, `realloc` in a loop, or `new T[n]`. To detect these, a fuzzer must reward inputs that increase the *total bytes allocated* during a run.

MemLock treats these as two separate "memory consumption feedback" channels, each of which is combined with standard AFL edge coverage.

## Approach

MemLock layers a memory-consumption-aware fitness on top of AFL's coverage-guided power schedule.

### 1. Static Analysis of Consumption Sites

Before fuzzing, MemLock performs a lightweight static analysis pass (over LLVM IR) that identifies:
- **Stack-consuming statements:** function calls on strongly-connected components of the call graph, i.e. recursive cycles. These are the only statements that can cause unbounded stack growth.
- **Heap-consuming statements:** calls to `malloc`, `calloc`, `realloc`, `new`, and equivalents whose size operand depends on tainted input.

Restricting instrumentation to these sites keeps runtime overhead small and avoids drowning the feedback channel in irrelevant signals.

### 2. Runtime Instrumentation

Two instrumentation modes ship with the tool:
- **`memlock-stack-clang`**: instruments only recursive call sites; maintains a per-execution maximum recursion-depth counter.
- **`memlock-heap-clang`**: instruments only input-dependent allocation sites; maintains a per-execution total-bytes-allocated counter.

At the end of each executed input, the counter is reported alongside AFL's edge map.

### 3. Consumption-Guided Seed Selection

A seed is retained in the corpus if *either* (a) it expands edge coverage in the classical AFL sense, *or* (b) it increases the memory consumption counter beyond the previous maximum observed on that edge set. The power schedule is adjusted so seeds that broke memory records receive disproportionally more energy, accelerating growth along the consumption axis.

This is a direct analogue of PerfFuzz's "max-per-edge" counter idea — but applied to memory instead of execution count.

## Implementation

- Built on AFL 2.52b + LLVM 4.0 / Clang.
- Provides two fuzzer binaries and a shared static-analysis pass.
- Docker image available in the artifact repo for reproducibility on Ubuntu 16.04 LTS.
- Usage:
  ```
  memlock-stack-fuzz -i in -o out -d -- ./prog @@
  memlock-heap-fuzz  -i in -o out -d -- ./prog @@
  ```

## Evaluation

The paper reports evaluation on 14 widely-used C/C++ programs including `binutils` (objdump, readelf, nm, addr2line), `nasm`, `flex`, `mjs`, `yaml-cpp`, `bento4`, `tinyexr`, `elfutils`, and `binaryen`. Baselines are AFL, AFLfast, FairFuzz, PerfFuzz, Angora, and QSYM.

Reported results (subject to the caveat that only the abstract and secondary summaries are directly accessible here):
- MemLock detects memory-consumption bugs substantially faster than all baselines, and finds bugs the baselines never trigger within the fuzzing budget.
- The tool discovered **26 previously unknown memory-consumption bugs**, of which **15 received new CVE IDs**.
- Breakdown of notable bug classes:
  - `mjs 1.20.1`: 11 uncontrolled-recursion bugs (stack overflow via deeply nested inputs).
  - `binutils 2.31`: 6 bugs mixing recursion, unbounded allocation, and memory leaks.
  - `nasm`, `flex`, `yaml-cpp`: additional uncontrolled-recursion bugs.
  - `elfutils`, `binaryen`, `bento4`, `tinyexr`: uncontrolled heap allocation driven by attacker-controlled size fields.

## Relevance to APEX G-46

MemLock is the canonical reference for APEX's memory-side performance test generation. Several implementation choices map directly onto G-46 requirements:

1. **Separate feedback channels for stack vs heap.** A single "memory" metric is too coarse. G-46 should emit two CWEs (CWE-674 / CWE-789) and fuzz them with separate feedback signals.
2. **Static pre-pass to focus instrumentation.** Full-program memory tracing is far too slow; the "recursion + size-tainted alloc" filter is a good starting design.
3. **Max-per-edge feedback.** Inheriting PerfFuzz's max counter scheme (not total) avoids seed explosion from seeds that merely sum to a high count without a local hotspot.
4. **CVE grounding.** The concrete bug classes MemLock found (protocol parsers, binary analyzers, YAML/JSON parsers) map closely onto APEX's typical targets.

## Limitations

- Like SlowFuzz and PerfFuzz, MemLock does not produce a *parameterized generator*: it reports concrete inputs, not scaling rules. Combining MemLock-style memory feedback with Singularity-style pattern synthesis is an open direction.
- The static pre-pass relies on LLVM IR and assumes source availability; binary-only targets require an alternative front end.
- Uncontrolled leaks (long-lived heap retention across many small requests) are out of scope; MemLock focuses on single-execution consumption.

## Citation

Cheng Wen, Haijun Wang, Yuekang Li, Shengchao Qin, Yang Liu, Zhiwu Xu, Hongxu Chen, Xiaofei Xie, Geguang Pu, and Ting Liu. 2020. MemLock: memory usage guided fuzzing. In Proceedings of the ACM/IEEE 42nd International Conference on Software Engineering (ICSE '20). Association for Computing Machinery, New York, NY, USA, 765–777. https://doi.org/10.1145/3377811.3380396
