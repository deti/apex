---
id: 01KNWE2QA0Z52H8VVFAMSA7KGA
title: Resource-Guided Fuzzing
type: concept
tags: [fuzzing, performance, complexity-attack, libafl, feedback]
links:
  - target: 01KNWE2Q9YZBAR140ZX5P36TQ5
    type: extends
  - target: 01KNWE2QA2KV1NN8QH32RA5EPA
    type: related
  - target: 01KNWE2QA3FA96G8JKN733K0XP
    type: related
  - target: 01KNWE2QA700K0Z2W0TWV087JZ
    type: related
  - target: 01KNWE2QA8H1GKHCVNHYS5QW1F
    type: related
  - target: 01KNWEGYB1B15QGYTRC374Z7DQ
    type: extends
  - target: 01KNWEGYB3NXWFB6D4SV4DTD5X
    type: extends
  - target: 01KNWEGYB6AVG1FV1EQVYW3K9Q
    type: extends
  - target: 01KNWGA5GD7A7WXW56682R280K
    type: related
  - target: 01KNWGA5GFHMDSYKRHEE5BJXKJ
    type: related
created: 2026-04-10
modified: 2026-04-10
---

# Resource-Guided Fuzzing

**Resource-guided fuzzing** is a variant of evolutionary / feedback-directed fuzzing in which the fitness function rewards inputs that consume more of a *resource* (CPU time, instructions retired, memory allocation count, peak RSS) rather than inputs that exercise new *code coverage*. The goal is not to find correctness bugs but to find **performance pathologies** and **algorithmic complexity vulnerabilities**.

## Why it differs from coverage-guided fuzzing

Traditional coverage-guided fuzzers (AFL, libFuzzer, Honggfuzz) keep an input in the corpus if it hits a previously unseen edge in the control-flow graph. That's great for reaching deep states but useless for finding worst-case *quantities*. A resource-guided fuzzer keeps an input if it makes the target do more *work* on an edge it's already seen.

| Axis | Coverage-guided | Resource-guided |
|---|---|---|
| Fitness signal | new edges / new bits | max instruction count / edge hit count / allocation count / wall time |
| Goal | correctness bugs, new states | worst-case performance, DoS vectors |
| Oracle | crashes, sanitizers, asserts | time/memory thresholds, complexity scaling |
| Typical termination | no new coverage for N iters | regression observed, SLO breached |

## Two landmark designs

**SlowFuzz (CCS 2017)** — the first domain-independent framework. It uses a single, global fitness: total number of basic-block executions. Any input that increases the global count over the current champion is retained. Effective but coarse — finds one "most expensive" path and tends to plateau there.

**PerfFuzz (ISSTA 2018)** — refines SlowFuzz with **multi-dimensional** feedback. It tracks **per-edge** execution counts (a vector, not a scalar) and retains any input that *strictly increases* the count on *any* edge vs. all previous champions. This keeps a richer corpus, hits the hottest edge 5–69× more often than SlowFuzz, and produces 1.9–24.7× longer execution paths on the same budget.

Both are essentially AFL with a different feedback function — a testament to how reusable the coverage-guided mutation loop is.

## Implementing in LibAFL

LibAFL separates the `Feedback`, `Observer`, `Corpus`, and `Scheduler` abstractions, which makes resource-guided fuzzing a natural extension:

1. **Observer** — instrument the target to produce a per-edge counter array (AFL-style tuples) or an instruction-count scalar. PerfFuzz-style work needs the full vector.
2. **Feedback** — a `MaxMapFeedback` that computes `any(new[i] > best[i])` across the vector; keep the input if true. Also track a global "most work" counter.
3. **Corpus** — retain the champion per dimension (edge) so the scheduler can cross-pollinate mutations.
4. **Scheduler** — bias toward champions with high unique-hot-edge ownership.

APEX already has LibAFL integration under `crates/apex-fuzz/`; the spec (G-46) proposes swapping the coverage feedback for this resource-maximising feedback, not rebuilding the fuzzer.

## Measurement pitfalls

Resource signals are noisier than coverage signals (coverage is deterministic; time is not). Practitioners mitigate this by:

- Using **instruction count** (deterministic, hardware-counter or instrumentation-derived) as the primary fitness signal and wall-clock only as a final verifier.
- Running the "champion" candidates **multiple times** with variance estimation before accepting.
- **Pinning CPU** and disabling frequency scaling / turbo boost during the final verification pass.
- Measuring **allocation counts** rather than peak RSS when memory is the target (peak RSS is discretised by page allocators).

## Relationship to other techniques

- **Hybrid with concolic execution** — symbolic reasoning about loop counts can steer the fuzzer toward quadratic paths; HotFuzz uses a related "micro-fuzzing" idea for Java objects.
- **Grammar-aware mutation** — worst cases for a parser often require syntactically valid but structurally pathological inputs (deeply nested JSON, billion-laughs XML). Grammar mutators help reach those.
- **Complexity estimation** — after resource-guided fuzzing finds a worst-case input, feed it back into an empirical complexity estimator (Goldsmith et al. 2007) to classify the growth rate.

## References

- Petsios, Zhao, Keromytis, Jana — "SlowFuzz" — CCS 2017 — [arXiv:1708.08437](https://arxiv.org/abs/1708.08437)
- Lemieux, Padhye, Sen, Song — "PerfFuzz" — ISSTA 2018 — [PDF](https://www.carolemieux.com/perffuzz-issta2018.pdf)
- Blair et al. — "HotFuzz" — NDSS 2020
- Goldsmith, Aiken, Wilkerson — "Measuring Empirical Computational Complexity" — ESEC/FSE 2007
