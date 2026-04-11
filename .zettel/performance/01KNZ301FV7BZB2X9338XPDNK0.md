---
id: 01KNZ301FV7BZB2X9338XPDNK0
title: "SPF-WCA: Symbolic Complexity Analysis using Context-Preserving Histories"
type: literature
tags: [paper, tool, symbolic-execution, worst-case, complexity-analysis, java, pathfinder, isstac]
links:
  - target: 01KNWE2Q9YZBAR140ZX5P36TQ5
    type: references
  - target: 01KNWE2QA3FA96G8JKN733K0XP
    type: extends
  - target: 01KNWE2QA8H1GKHCVNHYS5QW1F
    type: extends
  - target: 01KNZ301FVVKJ3YGQC55B3N754
    type: extends
  - target: 01KNZ301FVSD3QCQM0KD2Y1M1K
    type: related
  - target: 01KNWE2QA2KV1NN8QH32RA5EPA
    type: related
created: 2026-04-12
modified: 2026-04-12
source: "https://github.com/isstac/spf-wca"
paper: "Symbolic Complexity Analysis using Context-preserving Histories"
venue: ICST 2017 (Best Paper Award)
authors: [Kasper Luckow, Rody Kersten, Corina S. Păsăreanu]
year: 2017
---

# SPF-WCA: Symbolic Complexity Analysis using Context-Preserving Histories

**Paper:** "Symbolic Complexity Analysis using Context-preserving Histories" — Best Paper Award, 10th IEEE International Conference on Software Testing, Verification and Validation (ICST 2017)
**Authors:** Kasper Luckow (Carnegie Mellon University), Rody Kersten (Synopsys), Corina S. Păsăreanu (NASA Ames / CMU)
**Artifact:** https://github.com/isstac/spf-wca
**License:** MIT

## Positioning

SPF-WCA is the direct successor to Burnim et al.'s WISE (ICSE 2009) and a sibling to Noller et al.'s Badger (ISSTA 2018). All three use symbolic execution to discover worst-case complexity witnesses for a program. WISE learns a *flat* branch policy at small sizes and replays it at larger sizes. Badger uses symbolic execution in cooperation with coverage-guided fuzzing. SPF-WCA's contribution is between the two: it introduces **context-preserving histories** as the policy representation, which makes the learned policy strictly more expressive than WISE's while keeping symbolic execution as the only exploration mechanism.

The authors' motivation is that a flat, context-free branch policy (as used in WISE) fails for programs whose worst-case path depends on earlier decisions. A standard example is a balanced binary search tree insertion: whether the *next* insertion triggers a rotation depends on the history of prior rotations. A context-free policy cannot capture this dependence, so WISE under-predicts the worst case. SPF-WCA's history-aware policies restore the necessary expressiveness.

## Approach

SPF-WCA operates in two phases, mirroring WISE but with a richer policy representation.

### Phase 1: Policy generation at small input sizes

For a small user-chosen input size `N_gen`, SPF-WCA runs bounded symbolic execution over the target program, exploring all feasible paths. Along each path it records not just the branch decisions but also a *context* — a bounded sliding window of recent symbolic state. The worst-case path at `N_gen` is then projected onto a function from contexts to branch decisions: "given this context, take the worst-case branch."

The size of the context window is a tunable parameter (`history size`). Size 0 recovers WISE's flat policy; size 1 lets the policy depend on the most recent prior decision; larger sizes capture longer correlations at a cost of exponentially more policy states to track.

### Phase 2: Guided symbolic execution at larger sizes

For target sizes `N > N_gen`, SPF-WCA re-runs symbolic execution but uses the learned policy to resolve branch points. At each symbolic choice, it computes the current context, looks up the recommended branch in the policy, and prefers that branch's successor. Other branches are not pruned entirely (they would make the search incomplete), but they are deprioritized so the expected cost of reaching the worst case is low.

The key technical trick is making context comparisons *robust*: context representations must abstract over irrelevant program state (loop counters, iteration indices) so that two points with the same "logical" context map to the same policy entry even though their literal symbolic states differ. The paper introduces a family of abstraction functions for this purpose and discusses which abstractions preserve the worst-case path for which benchmark families.

### Output: fitted scaling law + test inputs

At the end of a run, SPF-WCA produces:

- A sequence of concrete test inputs of increasing size, each triggering the worst-case behavior discovered at that size.
- A fitted scaling function (polynomial or exponential) relating input size to observed cost.
- Regression statistics (coefficient, RMS error, R^2) letting the user assess confidence in the fit.
- A CSV of the raw measurements for plotting.
- Visualization of complexity growth patterns.

The scaling function is what makes SPF-WCA directly useful for APEX-style G-46 pipelines: the end artifact is not just a pathological input but an empirically-justified Big-O claim the developer can act on.

## Implementation

- Built on top of Java PathFinder (JPF) and Symbolic PathFinder (jpf-symbc).
- Distributed as a JPF module that can be installed alongside an existing JPF setup.
- Shipped with a Docker image for reproducible experiments.
- Requires a JPF configuration file specifying:
  - target class and entry method;
  - policy generation input size `N_gen`;
  - maximum input size `N_max` for the guided phase;
  - history size `h`.
- Best-paper award at ICST 2017.

## Benchmarks

The ICST 2017 paper evaluates SPF-WCA on a standard worst-case-analysis benchmark set that includes:

- Classic sorts: insertion sort, merge sort, quicksort.
- Balanced and unbalanced tree operations (binary search trees, red-black trees).
- Java HashMap and LinkedHashMap under collision patterns.
- Smaller items from the DARPA STAC (Space/Time Analysis for Cybersecurity) corpus.

The paper shows that a flat policy (history size 0, equivalent to WISE) fails to find the asymptotic worst case on several benchmarks where a size-1 or size-2 history succeeds. Conversely, increasing history size beyond a benchmark-specific threshold offers no further benefit but increases the cost of the policy-generation phase.

## Relation to later work

- **Badger (ISSTA 2018)** moves in a different direction, coupling symbolic execution with fuzzing instead of increasing the expressiveness of the symbolic policy. The two techniques are complementary: Badger handles programs where symbolic execution alone stalls on path explosion; SPF-WCA handles programs where the worst-case policy needs context.
- **Singularity (FSE 2018)** replaces symbolic execution entirely with genetic programming over a DSL of input patterns. The DSL is a different expressive dimension: SPF-WCA expresses worst-case *decisions* with context; Singularity expresses worst-case *inputs* as structured generators. They are orthogonal improvements over WISE.
- **PathFuzzing (arXiv 2507.09892, 2025)** encodes symbolic execution paths as binary strings representing branch choices and applies evolutionary fuzzing over that encoding — conceptually a union of SPF-WCA's symbolic search and Badger's fuzzing cooperation, reached from a different direction.

## Relevance to APEX G-46

1. **History-aware branch policies.** APEX's concolic/symbolic crate can adopt SPF-WCA's context-window representation as a design pattern for worst-case guidance, independent of the specific backend (JPF, KLEE, angr, or APEX-native).
2. **Fitted scaling laws as first-class output.** SPF-WCA's combined output (test input + polynomial fit + RMS error) is the right shape for a G-46 report. APEX should emit the same triple so that downstream CI gates can assert on the exponent, not just the absolute cost.
3. **Benchmark overlap.** The ICST 2017 evaluation corpus overlaps with the STAC corpus, providing a ready-made evaluation baseline. An APEX G-46 implementation should be able to match SPF-WCA on its published benchmarks as a sanity check.
4. **The ISSTAC project.** SPF-WCA and Badger both come out of the DARPA ISSTAC (Information Security through Symbolic Transformation of Algorithmic Constructs) project, along with several related tools. The ISSTAC GitHub organization (`isstac`) is a good single source for artifact access when APEX's benchmark harness needs these tools.
