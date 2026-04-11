---
id: 01KNZ301FVVKJ3YGQC55B3N754
title: "WISE: Automated Test Generation for Worst-Case Complexity"
type: literature
tags: [paper, performance, symbolic-execution, worst-case, complexity, path-explosion, generator, historical]
links:
  - target: 01KNWE2Q9YZBAR140ZX5P36TQ5
    type: references
  - target: 01KNWE2QA3FA96G8JKN733K0XP
    type: extends
  - target: 01KNWE2QA8H1GKHCVNHYS5QW1F
    type: extends
  - target: 01KNZ301FVSD3QCQM0KD2Y1M1K
    type: related
  - target: 01KNWEGYBB4AAEFYMR3Y29EZ49
    type: related
created: 2026-04-12
modified: 2026-04-12
source: "https://dl.acm.org/doi/10.1109/ICSE.2009.5070545"
source_mirror: "https://citeseerx.ist.psu.edu/document?repid=rep1&type=pdf&doi=6adb268aab366fc2bfa9166b62f37cb446412863"
venue: ICSE 2009
authors: [Jacob Burnim, Sudeep Juvekar, Koushik Sen]
year: 2009
---

# WISE: Automated Test Generation for Worst-Case Complexity

**Authors:** Jacob Burnim, Sudeep Juvekar, Koushik Sen
**Affiliation:** EECS, University of California, Berkeley
**Venue:** 31st International Conference on Software Engineering (ICSE 2009), Vancouver
**DOI:** 10.1109/ICSE.2009.5070545
**Tool:** WISE (Worst-case Inputs from Symbolic Execution), prototype for C and Java

## Retrieval Notes

The ACM DL, IEEE Xplore, and ResearchGate pages for this paper all returned 403/404 from the sandbox, and the CiteSeerX PDF mirror failed TLS verification. The body below is synthesized from the widely-cited abstract, the follow-on "Symbolic Execution and Recent Applications to Worst-Case Execution, Load Testing and Security Analysis" survey by Phan, Malacaria, and Pasareanu (which devotes a section to WISE), Semantic Scholar's metadata, and the standard textbook account in Cadar/Sen's surveys of symbolic execution. Quoted fragments <=125 chars; the rest is paraphrased.

## Historical Significance

WISE is the first published technique to automatically generate worst-case complexity inputs by symbolic execution. It predates SlowFuzz (2017), PerfFuzz (2018), Badger (2018), Singularity (2018), and HotFuzz (2020). Its core idea — use symbolic execution to discover a *generating rule* for worst-case inputs at small sizes, and then extrapolate the rule to larger sizes — is still reflected in every subsequent technique that claims to output parameterized or scaling-law worst-case witnesses.

## Problem Statement

Given a program `P` that accepts inputs of arbitrary size `n`, compute, for each `n`, an input `x(n)` of size `n` that exhibits the *worst-case* (highest cost) behavior of `P`. "Cost" can be any measurable resource: instruction count, branches executed, allocation bytes, or wall time.

Pure forward symbolic execution cannot answer this question because the number of feasible paths through a nontrivial program is astronomical even at small `n`, and grows super-linearly as `n` grows. A naive "explore all paths, pick the most expensive" strategy is defeated by path explosion.

WISE's insight is that worst-case paths in most real programs have a *regular structure*: the branch decisions taken on the worst-case path at size `n+1` are a predictable extension of the branch decisions taken at size `n`. If this regular structure can be learned from small sizes where exhaustive symbolic execution *is* tractable, it can be used as a "branch policy" to direct symbolic execution at larger sizes without re-exploring the entire path space.

## Approach

WISE has two phases:

### Phase 1: Exhaustive Symbolic Execution at Small Sizes

For small values of `n` (typically n = 1..k), WISE runs full bounded symbolic execution over `P`. It enumerates every feasible path, solves each path condition to produce a concrete input, and measures the cost. The path with maximum cost is the exhaustively-verified worst case at that size.

### Phase 2: Branch Policy Generalization

From the set of worst-case paths at sizes 1..k, WISE derives a *generator*: essentially a decision policy over the symbolic branches of `P`. For each symbolic branch point, the policy specifies whether the worst-case input prefers the true or the false branch. The paper shows that for a large class of common algorithms (sorting, searching, tree operations, text processing), this policy is compact and uniform — the same rule governs the decisions at all sizes.

### Phase 3: Guided Symbolic Execution at Larger Sizes

For `n > k`, WISE reuses the learned branch policy to steer symbolic execution away from unprofitable branches. Instead of exploring all `2^m` paths with `m` branches, it deterministically follows the policy at each branch, producing a single path condition which is then solved for a concrete worst-case input of size `n`. The net effect is that the expensive exhaustive search happens only for small `n`, and the generalization amortizes across all larger sizes.

## Example: Insertion Sort

Running WISE on a standard insertion sort implementation at small sizes discovers that the worst case is achieved by always taking the "shift-left" branch of the inner loop, i.e. by an input in which every newly inserted element is smaller than every already-sorted element. The learned policy is exactly "at every insertion, force the minimum so far." Applying this policy at larger sizes generates the reverse-sorted sequence `[n, n-1, ..., 1]`, which is the known quadratic worst case. WISE recovers this automatically without any human annotation.

## Evaluation

The paper evaluates WISE on a set of classical algorithmic benchmarks:
- sorting (insertion sort, quicksort variants);
- quick select;
- binary search tree insertion;
- splay tree;
- naive pattern matching.

For each benchmark, WISE extracts a branch policy from small sizes and uses it to generate worst-case inputs at sizes an order of magnitude larger than the exhaustive phase could directly handle. On each benchmark the generated inputs produce measured resource usage matching the known asymptotic worst case for the algorithm.

## Limitations

- **Policy must be uniform.** If the worst-case branch decisions change with `n` in a non-trivial way (e.g. depend on arithmetic relations), the learned policy at small `k` may fail to generalize. The authors explicitly note this and present their technique as a heuristic.
- **Small-size solver cost.** Phase 1 is still classical symbolic execution and inherits all of its costs on programs with complex symbolic state.
- **Single-input APIs.** WISE assumes the input is a single flat array/string; programs whose cost is driven by a *sequence* of API calls (e.g. a hash map used in a loop) are awkward to model.
- **Control-flow focus.** Data-dependent costs (e.g. allocation byte counts in a single branch) are not directly rewarded.

## Legacy

WISE is the historical starting point for the complexity-fuzzing literature. Every subsequent paper in this note family cites it:

- **SPF-WCA** (Luckow, Kersten, Păsăreanu) generalizes WISE's policy learning to structured inputs in SPF.
- **Badger** (ISSTA 2018) uses symbolic execution for worst-case analysis but cooperates with fuzzing to escape WISE's path-policy rigidity.
- **Singularity** (FSE 2018) abandons explicit symbolic execution in favor of genetic programming over an input DSL, producing a more expressive generator than WISE's branch policy.
- **PySE** (ICST 2019) replaces the handcrafted policy generalization with reinforcement learning.

## Relevance to APEX G-46

1. **Generator-first output.** WISE's central output is a *rule*, not a single input. APEX's G-46 generator should likewise produce reusable witnesses (scaling laws, DSL patterns, or branch policies) that downstream CI can re-instantiate at production sizes.
2. **Bounded exhaustive + extrapolation.** The two-phase design is highly practical for time-boxed test generation: spend a small budget doing expensive exhaustive exploration at small `n`, then amortize the result.
3. **Branch policy abstraction.** APEX's concolic and symbolic crates can emit WISE-style branch decision traces as a structured intermediate representation that the pattern synthesizer or the regression gate can consume.
4. **Baseline for evaluation.** WISE remains a useful low-end baseline in any G-46 evaluation: if the APEX generator cannot match WISE on insertion sort, something is wrong.

## Citation

Jacob Burnim, Sudeep Juvekar, and Koushik Sen. 2009. WISE: Automated test generation for worst-case complexity. In Proceedings of the 31st International Conference on Software Engineering (ICSE '09). IEEE Computer Society, USA, 463–473. https://doi.org/10.1109/ICSE.2009.5070545
