---
id: 01KNWE2QA8H1GKHCVNHYS5QW1F
title: Worst-Case Input Generation Strategies
type: concept
tags: [fuzzing, worst-case, input-generation, grammar, mutation]
links:
  - target: 01KNWE2Q9YZBAR140ZX5P36TQ5
    type: extends
  - target: 01KNWE2QA0Z52H8VVFAMSA7KGA
    type: related
  - target: 01KNWE2QAAKZH8GGZ172HZ9RHS
    type: related
  - target: 01KNWEGYB1B15QGYTRC374Z7DQ
    type: extends
  - target: 01KNWEGYB3NXWFB6D4SV4DTD5X
    type: extends
  - target: 01KNWEGYB6AVG1FV1EQVYW3K9Q
    type: extends
  - target: 01KNWEGYB8807ET2427V3VCRJ3
    type: related
  - target: 01KNZ301FV5ET9FFP6QX0RPPH8
    type: extends
  - target: 01KNZ301FVK668YHRW0HKF82BZ
    type: extends
  - target: 01KNZ301FVVKJ3YGQC55B3N754
    type: extends
  - target: 01KNZ301FV7BZB2X9338XPDNK0
    type: extends
  - target: 01KNZ301FVSD3QCQM0KD2Y1M1K
    type: extends
  - target: 01KNZ301FVNJ1JA9TKGG46472T
    type: extends
  - target: 01KNZ3XK3QDTG1BB60XBMTNYFE
    type: related
created: 2026-04-10
modified: 2026-04-12
---

# Worst-Case Input Generation Strategies

"Worst-case input generation" is the task of producing an input that drives a program onto its most expensive execution path — whether that's longest running time, peak memory, most allocations, most file descriptors held, or deepest recursion. This is the core capability APEX G-46 needs.

The techniques cluster into four families, each with different strengths.

## 1. Resource-guided mutational fuzzing

**Idea**: start with a seed input and apply small random mutations. Keep mutants that improve a resource signal. Repeat.

- **Strengths**: no assumptions about input structure; works on opaque binaries; excellent at finding unexpected worst cases in parsers that are mostly string-driven.
- **Weaknesses**: slow to find structured pathologies (the mutation has to stumble onto valid syntax before it can explore structurally pathological forms); prone to getting stuck at local maxima.
- **Tools**: SlowFuzz (CCS 2017), PerfFuzz (ISSTA 2018). See the `Resource-Guided Fuzzing` and `LibAFL Feedback Architecture` notes.
- **When to pick**: the target takes unstructured or semi-structured bytes (images, audio, network protocols), or you have a seed corpus close to the worst case.

## 2. Grammar-aware / structure-aware generation

**Idea**: start from a formal grammar (BNF, protobuf schema, ASN.1, grammar of valid JSON) and generate syntactically valid inputs directly, with mutations that respect the grammar (swap nodes, inflate repetition, etc.).

- **Strengths**: every input is structurally valid, so the target spends all its budget exercising deep parse logic, not rejecting garbage at the entry gate. Essential for JSON / XML / SQL / protocol buffers, and for parsers whose worst case lives behind a "validate first" check.
- **Weaknesses**: requires a grammar; worst-case finding is constrained by the mutation operators' expressiveness.
- **Tools**: Gramatron (ISSTA 2021), Nautilus (NDSS 2019), domato, AFL-Grammar, LibAFL's `GramatronMutator`, Hypothesis's `strategies.from_regex` / `recursive`.
- **When to pick**: you're fuzzing a parser and want worst-case behaviour on structurally nested inputs (XML bomb, deeply nested JSON, quadratic YAML anchor resolution).

## 3. Symbolic / concolic path maximisation

**Idea**: collect path constraints from a concrete execution, then use an SMT solver to find an input that traverses a more expensive path (more loop iterations, different worst-case branch).

- **Strengths**: can reach deep states a random mutator would never hit; can directly solve for "input that makes loop at line 42 execute ≥1000 times" rather than searching.
- **Weaknesses**: path explosion; solvers struggle with complex arithmetic, hash functions, cryptographic primitives; instrumentation overhead is high.
- **Tools**: KLEE for worst-case analysis (Burnim et al.), SPF-WCA (Symbolic PathFinder — Worst-Case Analysis, Luckow et al., ICSE 2017), SymCC (APEX already uses this), PEX.
- **When to pick**: the worst case requires carefully constructed inputs that fuzzers cannot stumble onto (e.g. solving a CRC check, a magic number, a parity constraint).
- **APEX relevance**: APEX ships SymCC — the same concolic backend can be repurposed to maximise loop iteration counts instead of new coverage.

## 4. Adversarial / analytic constructors

**Idea**: for well-studied data structures and algorithms, construct the worst-case input by hand using knowledge of the algorithm's weak spots.

- **Hash collision for hash tables** — given the hash function, solve for keys that collide in the same bucket (Crosby–Wallach 2003; 28C3 2011 scaled this across web frameworks). For SipHash-keyed tables this is infeasible without the key; for `MurmurHash`/`FNV`/`Java.hashCode()` it's trivial.
- **McIlroy's Quicksort killer** — an input that deterministic-pivot quicksort partitions into its worst case on every call. Can be generated offline without running the target, just from the pivot rule.
- **ReDoS witness** — from a static ReDoS analyser's "exploitable ambiguity" witness, synthesise a string that maximises backtracking.
- **Billion-laughs XML** — construct entity definitions that recursively expand; no fuzzing needed, just a template.
- **Zip bomb** — 42.zip and its descendants are hand-crafted.
- **Expression-parser exponential blowup** — deep nesting of parenthesised sub-expressions targeting Pratt / recursive-descent parsers without depth limits.

These inputs should be in a **pre-seeded corpus** that every performance fuzzer starts from, since they're known-good seeds that no random mutator will reconstruct efficiently.

## 5. Hybrid strategies (best in practice)

Production-grade worst-case finders combine:

1. **Pre-seeded corpus** of hand-crafted adversarial inputs (hash collisions, ReDoS witnesses, billion laughs, McIlroy quicksort killer).
2. **Grammar-aware generation** for structured formats.
3. **Resource-guided mutation** on top of both, with per-edge feedback (PerfFuzz-style).
4. **Concolic bursts** when the fuzzer plateaus — SymCC solves for a constraint that bumps a plateaued edge over its current max.

APEX's G-46 spec implicitly requires all four — the existing `apex-fuzz` + `apex-concolic` (SymCC) + grammar mutator give three; the new resource feedback completes the fourth.

## Termination — when to stop generating

- Fixed budget (default 5 min in G-46).
- Stagnation detector — N iterations without improvement.
- Absolute SLO breach — any single input that exceeds the declared SLO terminates the search and becomes the witness.
- Diminishing returns — per-edge improvement rate drops below a threshold.

## Output: not just the input

A worst-case finding is not a single bytestring; it's a tuple of:

1. The input itself (concrete bytes).
2. The resource measurement (time, instructions, allocations, peak RSS).
3. The input size (for complexity scaling context).
4. The ratio of observed vs. median — "8.2× slower than median for this input size".
5. A minimised / shrunk version (Hypothesis-style shrinking makes witnesses intelligible).
6. A reproducer — a command or snippet that re-runs the target on the input deterministically.

G-46 explicitly requires the ReDoS finding to include a concrete worst-case string. The same principle should apply to all performance findings: a worst-case *claim* without a reproducible witness is not actionable.

## References

- Petsios et al. — SlowFuzz — CCS 2017
- Lemieux et al. — PerfFuzz — ISSTA 2018
- Luckow, Kersten, Păsăreanu — "Symbolic Complexity Analysis using Context-preserving Histories" — ICST 2017 (WCA-SPF)
- Burnim, Juvekar, Sen — "WISE: Automated Test Generation for Worst-Case Complexity" — ICSE 2009
- McIlroy — "A Killer Adversary for Quicksort" — SP&E 1999
- Crosby, Wallach — "Denial of Service via Algorithmic Complexity Attacks" — USENIX Security 2003
- MacIver et al. — "Test-case reduction for C compiler bugs" — and the Hypothesis shrinker — [hypothesis.works](https://hypothesis.works/)
