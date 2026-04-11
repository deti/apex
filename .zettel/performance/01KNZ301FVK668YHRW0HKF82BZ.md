---
id: 01KNZ301FVK668YHRW0HKF82BZ
title: "PathFuzzing: Worst-Case Analysis by Fuzzing Symbolic-Execution Paths"
type: literature
tags: [paper, arxiv, fuzzing, symbolic-execution, worst-case, hybrid, 2025, evolutionary]
links:
  - target: 01KNWE2Q9YZBAR140ZX5P36TQ5
    type: references
  - target: 01KNWE2QA3FA96G8JKN733K0XP
    type: extends
  - target: 01KNWE2QA8H1GKHCVNHYS5QW1F
    type: extends
  - target: 01KNZ301FVSD3QCQM0KD2Y1M1K
    type: related
  - target: 01KNZ301FV7BZB2X9338XPDNK0
    type: related
  - target: 01KNZ301FVVKJ3YGQC55B3N754
    type: related
  - target: 01KNWEGYB1B15QGYTRC374Z7DQ
    type: related
created: 2026-04-12
modified: 2026-04-12
source: "https://arxiv.org/abs/2507.09892"
venue: arXiv preprint
year: 2025
---

# PathFuzzing: Worst-Case Analysis by Fuzzing Symbolic-Execution Paths

**arXiv:** 2507.09892
**Venue:** arXiv preprint (2025)
**Category:** cs.SE (Software Engineering)

## Retrieval notes

Only the arXiv abstract page was accessible; the PDF mirror was not directly usable in the sandbox. The body below is based on the abstract as returned by the arXiv landing page plus general context from the surrounding worst-case-analysis literature.

## Abstract (paraphrased from arXiv)

Estimating the worst-case resource consumption of a program is a critical software development task that can be framed as an optimization-based worst-case analysis (WCA) problem. The paper proposes **PathFuzzing**, a hybrid technique that merges fuzzing and symbolic execution. PathFuzzing transforms the target program into a *symbolic form* in which execution paths are interpreted as binary strings representing branch decisions, then applies evolutionary fuzzing to search for binary strings that:

1. satisfy the resulting path conditions (i.e., correspond to feasible program paths), and
2. maximize resource usage along those paths.

Experimental evaluation shows that PathFuzzing "generally outperforms a fuzzing and a symbolic-execution baseline" on benchmark suites from prior work and on newly constructed benchmarks.

## Core idea: binary-string encoding of symbolic paths

This is the key conceptual move. Classical symbolic execution represents a path as a conjunction of branch conditions — an expression in a first-order theory. Classical coverage-guided fuzzing represents an input as a byte string. These two representations live in different worlds and are hard to mix.

PathFuzzing proposes a bridge: fix a canonical ordering of the branches in a program, and encode each path as a bit string where bit `i` records whether branch `i` was taken or not. The space of feasible paths is then a subset of `{0,1}^k` for some `k`, defined implicitly by the path conditions the target's SMT solver can check.

Three immediate consequences:

1. **Fuzzing operates natively in the path space.** The mutator can flip bits, cross over bit strings, and apply genetic-programming-style operators without needing to understand the underlying symbolic formulas. Every candidate is a bit string.
2. **Path-condition checking is delegated to a constraint solver.** After each mutation, the candidate bit string is handed to the symbolic engine, which checks whether the corresponding sequence of branch decisions is satisfiable. If yes, it solves for a concrete input and runs the target; if no, the candidate is dropped.
3. **Resource maximization is a standard fuzzer fitness function.** Once the candidate reaches a concrete execution, its runtime (or instruction count, or allocated bytes) is the fitness score. Standard evolutionary operators (selection, crossover, mutation) then drive the population toward paths with higher resource usage.

## Comparison to Badger

- **Badger (ISSTA 2018)** uses symbolic execution and fuzzing as two cooperating workers that trade concrete inputs and path prefixes. Each worker has its own search strategy.
- **PathFuzzing** collapses the two workers into one: the search is entirely evolutionary, but the "inputs" it mutates are path decision vectors rather than raw byte strings. The symbolic engine is pushed into a subsidiary role (path-feasibility oracle + path-to-input solver), and the evolutionary loop does the rest.

The trade-off: Badger keeps symbolic execution in control for deep constraint satisfaction but has to manage the handshake between its two workers; PathFuzzing eliminates the handshake but relies more heavily on the constraint solver as a per-candidate cost.

## Comparison to Singularity

- **Singularity (FSE 2018)** evolves programs in an input-pattern DSL. The candidate space is a grammar of input generators.
- **PathFuzzing** evolves bit vectors over a fixed path space. The candidate space is defined by the symbolic structure of the target.

Singularity's representation is more expressive in input space but does not see path conditions; PathFuzzing's representation encodes path conditions directly but is limited to the paths the symbolic engine can represent. They are complementary and could potentially be combined (evolve DSL programs whose instantiations produce feasible symbolic paths).

## Evaluation claims

The paper reports experimental results on:

- Benchmark suites used by earlier worst-case-analysis work (likely overlapping with STAC, SPF-WCA, and Singularity benchmarks).
- A new benchmark set constructed by the authors.

On both, PathFuzzing "generally outperforms" both a pure fuzzing baseline and a pure symbolic-execution baseline. The specific numbers and target coverage are not available from the abstract alone.

## Relevance to APEX G-46

PathFuzzing is the most recent (2025) entry in the worst-case analysis literature that is relevant to APEX. Two design takeaways:

1. **Path encoding as a first-class representation.** If APEX's concolic and fuzzing crates already share a constraint solver, the PathFuzzing approach is natural to implement: expose a "next feasible bit" primitive from the concolic engine, and drive it with an evolutionary loop in the fuzz crate.
2. **Single-loop hybrid over two-worker hybrid.** PathFuzzing suggests that the Badger-style two-worker design may be unnecessary if the symbolic engine can be accessed synchronously as an oracle. For APEX, whose engine is in-process (not a separate JPF process), the PathFuzzing simplification is probably the right starting point.

## Caveats

- As an arXiv preprint, it has not yet undergone peer review at the time of this note (2026-04).
- Evaluation depth and baselines cannot be fully assessed from the abstract alone; a detailed comparison with Badger, SPF-WCA, and Singularity on a common benchmark set would be necessary to judge the claimed improvement quantitatively.
- The binary-string path encoding requires the target program's branch count to be manageable and known in advance, which is non-trivial for large or dynamically-generated code.
