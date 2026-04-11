---
id: 01KNZ301FVSD3QCQM0KD2Y1M1K
title: "Badger: Complexity Analysis with Fuzzing and Symbolic Execution"
type: literature
tags: [paper, performance, fuzzing, symbolic-execution, hybrid, worst-case, complexity-attack, java, kelinci, spf]
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
source: "https://arxiv.org/abs/1806.03283"
doi: "10.1145/3213846.3213868"
artifact: "https://github.com/isstac/badger"
venue: ISSTA 2018
authors: [Yannic Noller, Rody Kersten, Corina S. Păsăreanu]
year: 2018
---

# Badger: Complexity Analysis with Fuzzing and Symbolic Execution

**Authors:** Yannic Noller (Humboldt-Universität zu Berlin), Rody Kersten (Synopsys), Corina S. Păsăreanu (CMU / NASA Ames)
**Venue:** 27th ACM SIGSOFT International Symposium on Software Testing and Analysis (ISSTA 2018), Amsterdam
**DOI:** 10.1145/3213846.3213868
**arXiv:** 1806.03283
**Artifact:** https://github.com/isstac/badger (MIT)

## Retrieval Notes

The arXiv abstract page and the GitHub artifact README were both reachable via WebFetch. The PDF mirrors at wcventure.github.io and the ACM DL full text were not directly decodable in the sandbox. The body below is assembled from those two accessible sources plus secondary summaries.

## Problem Statement

Two dominant techniques exist for discovering worst-case complexity vulnerabilities, each with complementary strengths and weaknesses:

- **Greybox fuzzing** (AFL, SlowFuzz, PerfFuzz): fast, easy to deploy, good at exploring wide local neighborhoods via mutation, but poor at satisfying deep input constraints (checksums, magic numbers, grammatical structure) and prone to plateauing once the easy worst-case regions are covered.
- **Symbolic execution** (WISE, SPF-WCA): precise, constraint-aware, can construct inputs that satisfy deep path conditions, but scales poorly to large programs because of path explosion and expensive constraint solving.

Badger's thesis is that these two techniques can be used *in tandem*, feeding each other, to push past the plateaus of either one in isolation for the specific task of *worst-case complexity analysis*.

## Approach

Badger is a hybrid complexity-analysis engine composed of two cooperating workers:

### 1. Fuzzing worker: KelinciWCA

`KelinciWCA` is an extension of Kelinci — an adapter layer that lets the AFL fuzzer drive arbitrary Java programs. WCA ("worst-case analysis") adds a resource cost channel: for every executed input, the Java target reports a resource measure (instruction count or bytecode instruction count) along with edge coverage. Inputs that expand edge coverage *or* increase the observed maximum cost for a given edge set are retained (the PerfFuzz "max per edge" idea generalized to bytecode).

### 2. Symbolic worker: SymExe (Symbolic PathFinder)

`SymExe` is a worst-case-cost-guided symbolic execution engine built on top of Java PathFinder (JPF) and Symbolic PathFinder (SPF), with Z3 as the backend solver. SymExe performs a cost-prioritized worklist traversal of the symbolic execution tree: branches that have historically led to high cost are expanded first. When a terminal path is reached, SymExe solves the accumulated path condition and emits a concrete input.

### 3. Cross-pollination

The two workers run in parallel and share discoveries through a pair of queues:
- When fuzzing finds a new high-cost input, it is handed to SymExe as a seed: SymExe executes the program concretely on that input while recording the symbolic path, establishing a starting prefix for further symbolic exploration. This lets symbolic execution "ride the coat-tails" of fuzzing's cheap exploration rather than starting from scratch.
- When SymExe solves for a new high-cost path and produces a concrete witness, that input is fed back into KelinciWCA's corpus. Fuzzing then mutates it locally, potentially finding cost-amplifying neighbors that were not visible in the symbolic tree (e.g. because the symbolic model was too coarse for some library calls).

This producer/consumer handshake is analogous to the hybrid design in Driller (but targeting cost rather than coverage), and is the practical contribution of the paper.

## Architecture and Execution Model

Badger's execution layout requires a specific directory structure:

```
project/
├── config.txt
├── kelinciwca_analysis/
│   ├── src/ bin/ instr/ in/
├── spf_analysis/
│   ├── src/ bin/
└── fuzzer-out/
    ├── AFL-queue/
    └── SymExe-queue/
```

Configuration parameters include:
- analysis mode: `wca` (worst-case analysis) or `cov` (pure coverage);
- input/output directory layout and sync intervals;
- JPF classpath and target class;
- symbolic decision procedure and value ranges;
- worklist heuristic (highest observed cost vs. highest delta cost).

Invocation:
```
java -cp <badger + jpf classpath> edu.cmu.sv.badger.app.BadgerRunner config.txt
```

## Evaluation (summary from paper / GitHub)

Badger is evaluated against KelinciWCA-only and SymExe-only baselines on several Java benchmarks drawn from the DARPA Space/Time Analysis for Cybersecurity (STAC) corpus and standard data structures:

- Insertion sort (the paper's walking example; Badger quickly converges on the reverse-sorted pattern).
- Red-black tree insertion / deletion (targets worst-case rotation count).
- Textbook implementations of Dijkstra, matrix multiplication, and string manipulation.
- STAC engagement challenges ("Smart Ticketing System," "AirPlan," and similar Java applications).

Headline claims:
- Badger reaches higher worst-case costs in less wall time than either component alone, especially on benchmarks with non-trivial input constraints where pure fuzzing plateaus quickly.
- On insertion sort, Badger reproduces the reverse-sorted quadratic input and extrapolates to larger sizes.
- On the STAC benchmarks, Badger finds inputs exposing polynomial blow-up in places where SPF alone either ran out of memory or coverage-only fuzzing saturated.

## Implementation Notes

- Written against SPF and JPF, so it is Java-specific. Porting to other managed languages (e.g. .NET via Pex) would require a different symbolic back end.
- The symbolic worker's cost model is instruction count over JVM bytecode; real wall-time worst cases are only approximated.
- The artifact includes a working insertion-sort example; extending to new targets requires writing a small SPF driver and picking a symbolic input model.

## Relationship to Other G-46 Citations

- **SlowFuzz (CCS 2017)** and **PerfFuzz (ISSTA 2018)** supply the "cost-guided greybox" component; Badger essentially generalizes their feedback loop to the JVM and adds symbolic cooperation.
- **Singularity (FSE 2018)** is an alternative take on the same problem: rather than cross-pollinating with symbolic execution, it synthesizes a *pattern program*.
- **HotFuzz (NDSS 2020)** addresses a complementary concern: micro-fuzzing individual methods rather than whole programs.
- **WISE (ICSE 2009)** is the direct symbolic-only ancestor Badger extends.

A principled G-46 implementation is likely to blend all three ideas: PerfFuzz/MemLock-style feedback channels, Badger-style symbolic handshake for constraint-heavy programs, and Singularity-style pattern extrapolation for reporting Big-O.

## Relevance to APEX G-46

1. **Hybrid backend design.** APEX's concolic crate (apex-concolic) can play the role of SymExe in a Badger-like architecture, handing off high-cost paths to apex-fuzz and ingesting high-cost fuzzer inputs as concolic seeds.
2. **Cost metric plurality.** Badger uses bytecode instruction count; APEX can reuse the same hookable-metric design (instruction count, cycles, allocation bytes, wall time) and swap per target.
3. **Worklist heuristics.** Badger's "highest observed cost first" and "delta cost first" worklist heuristics are cheap additions to a symbolic worker and materially improve throughput.
4. **Benchmark corpus.** The STAC engagement corpus used by Badger is a shared benchmark across G-46 literature and should become a regression target for the APEX performance test generator.

## Follow-up Work

- **HyDiff** (Noller et al., ICSE 2020) extends Badger-style cooperation to *differential* fuzzing (finding side-channel leaks).
- **PySE** and later work pivot from symbolic execution to reinforcement learning as the "deep" component.

## Citation

Yannic Noller, Rody Kersten, and Corina S. Păsăreanu. 2018. Badger: complexity analysis with fuzzing and symbolic execution. In Proceedings of the 27th ACM SIGSOFT International Symposium on Software Testing and Analysis (ISSTA 2018). Association for Computing Machinery, New York, NY, USA, 322–332. https://doi.org/10.1145/3213846.3213868
