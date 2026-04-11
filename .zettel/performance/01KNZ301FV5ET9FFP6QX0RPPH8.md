---
id: 01KNZ301FV5ET9FFP6QX0RPPH8
title: "Singularity: Pattern Fuzzing for Worst Case Complexity"
type: literature
tags: [paper, performance, fuzzing, complexity-attack, worst-case, pattern-synthesis, genetic-programming, cwe-407]
links:
  - target: 01KNWE2Q9YZBAR140ZX5P36TQ5
    type: references
  - target: 01KNWE2QA0Z52H8VVFAMSA7KGA
    type: extends
  - target: 01KNWE2QA3FA96G8JKN733K0XP
    type: extends
  - target: 01KNWE2QA8H1GKHCVNHYS5QW1F
    type: extends
  - target: 01KNWEGYB1B15QGYTRC374Z7DQ
    type: related
  - target: 01KNWEGYB3NXWFB6D4SV4DTD5X
    type: related
  - target: 01KNWEGYB6AVG1FV1EQVYW3K9Q
    type: related
created: 2026-04-12
modified: 2026-04-12
source: "https://dl.acm.org/doi/10.1145/3236024.3236039"
source_mirror: "https://www.cs.utexas.edu/~isil/fse18.pdf"
venue: ESEC/FSE 2018
authors: [Jiayi Wei, Jia Chen, Yu Feng, Kostas Ferles, Isil Dillig]
year: 2018
---

# Singularity: Pattern Fuzzing for Worst Case Complexity

**Authors:** Jiayi Wei, Jia Chen, Yu Feng, Kostas Ferles, Isil Dillig
**Venue:** 26th ACM Joint European Software Engineering Conference and Symposium on the Foundations of Software Engineering (ESEC/FSE '18), November 2018
**DOI:** 10.1145/3236024.3236039
**Affiliations:** University of Texas at Austin; University of California Santa Barbara
**Artifact:** https://github.com/MrVPlusOne/Singularity

## Retrieval Notes

The ACM DL landing page was accessible via WebFetch, but full-text PDF mirrors (UT Austin, Ferles, Feng) are compressed binary streams the tool cannot decode in this sandbox. The body below is assembled from the author's project page (mrvplusone.github.io), the GitHub README of the reference implementation, and standard secondary summaries (Semantic Scholar, conference proceedings listings). Quoted fragments are <=125 characters; everything else is paraphrased.

## Problem Statement

Worst-case algorithmic complexity vulnerabilities occur when an implementation's behavior on adversarially-chosen inputs is asymptotically worse than its average case. Classical examples include quadratic quicksort on adversarially ordered arrays, O(n^2) hash table operations under collision-heavy keys, exponential regex backtracking, and O(2^n) recursive descent parsers. Attackers exploit these gaps to mount CPU- or memory-exhaustion denial-of-service attacks (CWE-400 / CWE-407 / CWE-1333) against production systems.

Prior work on complexity fuzzing had two limitations:
1. Domain-specific approaches (e.g. ReDoS-specific regex analyzers) cannot generalize to arbitrary programs.
2. Domain-independent coverage-guided fuzzers (AFL, libFuzzer) and resource-guided fuzzers (SlowFuzz, PerfFuzz) generate concrete worst-case inputs at a single fixed input size. They do not yield a *generating rule* that can be extrapolated to arbitrary sizes, and therefore cannot directly answer the question "what is the worst-case asymptotic complexity of this implementation?"

Singularity targets this extrapolation gap.

## Key Insight

Inputs that trigger asymptotic worst-case behavior are typically not random: they have a characteristic structural *pattern* that can be concisely described as a program over a small domain-specific language (DSL). Once a pattern is discovered, it can be instantiated at any desired input size, enabling both (a) scaling laws to be inferred empirically, and (b) the generating rule itself to serve as a machine-checkable witness.

Example patterns:
- **Insertion sort / quicksort (naive pivot):** the reverse-sorted sequence `[n, n-1, ..., 1]`.
- **Hash table under a known hash function:** a sequence of keys whose hashes collide.
- **Balanced tree insertion:** a monotonically increasing sequence.
- **Regex `(a+)+b`:** a run of `a`'s followed by a character that forces failure.

Each of these can be described by a short program with an internal state and an iteration rule.

## Approach

Singularity frames worst-case pattern discovery as a *program synthesis* problem.

### 1. Input Pattern DSL

Singularity introduces a compact DSL whose programs describe how to produce inputs of arbitrary size `n`. A pattern is represented as a **Recurrent Computation Graph (RCG)**: a small state machine with initial state and an iteration body. Running the RCG for `n` steps yields an input of size O(n). Core types in the DSL include `EInt`, `EVect`, vector append/concat, integer arithmetic, and a bounded set of combinators. The DSL is designed to be expressive enough to encode known worst-case patterns (reverse-sorted, alternating, collision-forming) while small enough for efficient search.

### 2. Genetic Programming Search

Because the DSL is finite but its program space is combinatorially large, Singularity uses **genetic programming (GP)** as the outer search loop. A population of candidate RCGs is maintained; each generation:
1. Instantiates every RCG at a set of size parameters `n_1, n_2, ..., n_k`.
2. Runs the target program under a resource profiler (instruction counts, time, memory).
3. Scores each candidate by a fitness function that combines (a) measured resource usage and (b) a regularity / simplicity penalty to avoid exploiting profiler noise.
4. Applies crossover (exchanging subtrees between parents) and mutation (point edits of DSL constants and operators) to produce offspring.
5. Selects survivors for the next generation.

A key refinement is **multi-size fitness**: rather than scoring at a single `n`, Singularity scores at several sizes simultaneously, favoring candidates whose resource usage grows rapidly with `n` (steep slope in the log-log plot of cost vs. size). This explicitly biases search toward *asymptotic* rather than *constant-factor* worst cases.

### 3. Complexity Extrapolation

Once a high-fitness RCG is found, Singularity runs it at a sweep of sizes and fits a power law `T(n) ≈ a * n^k` (or exponential) to the measured resource usage, producing an estimated Big-O class. This turns the fuzzing output into an empirically observed worst-case complexity bound — a direct input to performance regression gates and APEX G-46 SLO assertions.

### 4. Supernova: Parameter Auto-Tuning

The Singularity toolchain ships with **Supernova**, an automatic GP hyper-parameter tuner. Supernova picks population size, crossover/mutation rates, and size sweeps based on a short calibration run against the target, reducing the burden of per-target tuning.

## Implementation

- Written in Scala (~85%) + Java harness code.
- Target instrumentation is pluggable: Java (JVM bytecode counter), C/C++ (perf counters, instruction count), native subprocess (wall time).
- Repository: https://github.com/MrVPlusOne/Singularity
- Licensed under MIT.

## Experimental Results (summary from paper / artifact)

Singularity is evaluated on a diverse benchmark including:
- Classic sorting implementations (insertion sort, quicksort with various pivot strategies).
- The Java `LinkedHashMap` under adversarial keys (hash collision patterns).
- Regex engines on backtracking-prone patterns (reproducing known ReDoS).
- Graph algorithms (image compression, Apache Commons `StringUtils`).
- Real-world Java applications including those in the DARPA STAC corpus (Space/Time Analysis for Cybersecurity).

Highlights reported in the paper:
- Singularity discovers quadratic patterns for quicksort variants that coverage-guided fuzzers miss.
- It rediscovers ReDoS-triggering inputs for several libraries, and identifies new algorithmic vulnerabilities in the STAC corpus.
- The discovered RCGs generalize: running the synthesized pattern at larger `n` continues to trigger the worst case, confirming empirical asymptotic behavior.
- Singularity outperforms random search and SlowFuzz-style resource-guided mutation fuzzing on the benchmarks where a structural pattern exists.

## Comparison with Related Work

| Approach | Output | Extrapolates? | Domain-independent? |
|---|---|---|---|
| SlowFuzz | concrete input at one size | no | yes |
| PerfFuzz | concrete input at one size, per-edge hot count | no | yes |
| WISE (symbolic) | size-parameterized input via path exploration | yes (within solver limits) | yes |
| Badger (hybrid) | concrete inputs + symbolic paths | limited | yes |
| **Singularity** | **pattern program (RCG), instantiable at any n** | **yes, by construction** | **yes** |

The novel contribution of Singularity is that its output is a *generating rule*, not a single concrete input. This is crucial for APEX's goal of inferring asymptotic complexity classes rather than merely exhibiting a slow input.

## Relevance to APEX G-46

Singularity's RCG DSL and multi-size fitness function are directly applicable to APEX's performance test generation goals:
1. **Pattern-first search.** When an attacker has structural freedom (e.g. sort order, regex input shape), random byte-level mutation wastes budget; synthesizing a parameterized generator is much more sample-efficient.
2. **Asymptotic witnesses.** G-46's output should include not just a "slow input" but a *scaling law* that lets downstream users predict behavior at production sizes; Singularity's RCG + power-law fit is one concrete way to produce that.
3. **DSL reuse.** APEX could reuse Singularity's DSL as a first-pass representation for worst-case generators, then extend it with domain-specific primitives (e.g., JSON trees, SQL grammars).
4. **Benchmark overlap.** The STAC corpus Singularity uses is a natural evaluation target for any G-46 implementation.

## Open Questions

- The DSL as published is limited; richer grammars (context-free trees, typed AST generators) would unlock structured targets like compilers, SQL engines, and protocol parsers.
- GP is known to stall in flat fitness landscapes; combining Singularity-style pattern search with concolic or symbolic guidance (cf. WISE, Badger) is an obvious extension and is partially explored in later Noller et al. work.
- Multi-size fitness implicitly assumes the attacker controls size; for fixed-size APIs (e.g. fixed-width CAN frames) the technique has to be adapted.

## Citation

Jiayi Wei, Jia Chen, Yu Feng, Kostas Ferles, and Isil Dillig. 2018. Singularity: pattern fuzzing for worst case complexity. In Proceedings of the 2018 26th ACM Joint Meeting on European Software Engineering Conference and Symposium on the Foundations of Software Engineering (ESEC/FSE 2018). Association for Computing Machinery, New York, NY, USA, 213–223. https://doi.org/10.1145/3236024.3236039
