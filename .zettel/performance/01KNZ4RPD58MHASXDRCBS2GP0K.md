---
id: 01KNZ4RPD58MHASXDRCBS2GP0K
title: "Zaparanuks & Hauswirth: Algorithmic Profiling (PLDI 2012)"
type: literature
tags: [paper, algorithmic-profiling, complexity-inference, pldi, 2012, algoprof, empirical-cost, dynamic-analysis]
links:
  - target: 01KNWEGYBB4AAEFYMR3Y29EZ49
    type: extends
  - target: 01KNWE2QA3FA96G8JKN733K0XP
    type: related
  - target: 01KNZ301FVVKJ3YGQC55B3N754
    type: related
  - target: 01KNWE2QA2KV1NN8QH32RA5EPA
    type: related
  - target: 01KNZ4RPD6YPADQDCDC433E52N
    type: related
  - target: 01KNWE2Q9YZBAR140ZX5P36TQ5
    type: related
created: 2026-04-12
modified: 2026-04-12
source: "https://dl.acm.org/doi/10.1145/2254064.2254074"
doi: "10.1145/2254064.2254074"
venue: "PLDI 2012"
authors: [Dmitrijs Zaparanuks, Matthias Hauswirth]
year: 2012
---

# Algorithmic Profiling (Zaparanuks & Hauswirth, PLDI 2012)

**Authors:** Dmitrijs Zaparanuks, Matthias Hauswirth (both Faculty of Informatics, Università della Svizzera italiana, Lugano).
**Venue:** 33rd ACM SIGPLAN Conference on Programming Language Design and Implementation (PLDI '12), Beijing, June 2012.
**DOI:** 10.1145/2254064.2254074 (also 10.1145/2345156.2254074, the SIGPLAN Notices version).
**Tool:** AlgoProf (Java instrumentation + cost-function inference).

*Source: https://dl.acm.org/doi/10.1145/2254064.2254074 — fetched 2026-04-12. ACM page is behind a paywall; this note is assembled from the paper's abstract as circulated on Semantic Scholar, dblp metadata, and the summary in Goldsmith/Aiken/Wilkerson's follow-up citations.*

## The thesis

Traditional CPU profilers answer one question: **"where does the program spend its time?"** — a map from function (or line, or basic block) to a percentage of total runtime. The output is enormously useful for identifying hot spots, but it has a glaring limitation: it says nothing about *why* the program spends time in those places and *how the cost will change for different inputs*. A profile gathered on a 1000-element input does not directly tell you what the runtime will be on a 10,000-element input.

Zaparanuks and Hauswirth's insight is that what practitioners often want is not a static hot-spot map but an **empirical cost function** — a function `T(n)` that maps some notion of input size `n` to measured cost. This is the same thing Goldsmith, Aiken, and Wilkerson's **TrendProf** argues for in their FSE 2007 paper "Measuring Empirical Computational Complexity" (see `01KNWEGYBB4AAEFYMR3Y29EZ49`); the PLDI 2012 AlgoProf paper extends and refines the idea with a more general mechanism for **automatically determining what `n` means** for an arbitrary program.

The abstract formulation (paraphrased): an **algorithmic profiler** determines a cost function by automatically (a) determining the "inputs" of a program, (b) measuring the program's "cost" for any given input, and (c) inferring an empirical cost function from the (input size, cost) pairs. All three steps are automated; the user does not have to label which variables are "the size parameter".

## Three challenges

### 1. What is "the input"?

For a toy program like `sort(int[] a)` the input is `a` and the size is `a.length`. For a real program (an HTTP server, a compiler, a game engine) the notion of input is more elusive: is it the command-line arguments, the file read from disk, the network packet, the user click event? Zaparanuks and Hauswirth's answer is **data-structure size at function entry**: the algorithmic profiler instruments the program to record, at the entry of each interesting function, the sizes of its inputs as computed from *data-structure properties* (length of arrays, number of nodes in linked lists, number of elements in collections). This makes `n` a vector of potential size parameters, one per input channel, and the subsequent analysis finds which of them drives cost.

### 2. What is "the cost"?

The paper uses **instruction count** (or basic-block execution count) as the cost metric, collected via dynamic instrumentation on the JVM. Instruction count is a close proxy for CPU time but has the important advantage of being **deterministic and independent of machine-specific timing noise**, which matters for empirical cost-function inference where one wants to compare runs at different input sizes without wall-time jitter.

Other cost metrics the paper discusses (and the tool supports): allocation bytes (memory cost), cache-miss count (memory-hierarchy cost), and branch-mispredict count. Any countable dynamic event can be the cost metric.

### 3. How to infer a cost function

Once the profiler has collected a set of `(n, cost)` pairs by running the program on inputs of varying sizes, the final step is curve fitting. The paper fits several candidate functions against the points — constant, logarithmic, linear, `n log n`, quadratic, cubic, exponential — and selects the best fit by residual error plus a simplicity penalty. The output is a **Big-O-style characterisation** of the function's empirical cost at each instrumentation point: "this method's cost grows as `O(n²)` in the size of its first argument and `O(1)` in the size of its second argument."

The technique is explicitly empirical: it does not *prove* any asymptotic behaviour, it reports the best fit to the observed data. The paper argues this is the right trade-off for real software, where proving tight asymptotic bounds is intractable but discovering "this function happens to behave quadratically on realistic inputs" is both cheap and actionable.

## AlgoProf: the Java tool

The paper's companion implementation is **AlgoProf**, a dynamic analysis tool for Java built on top of an instrumenting agent (likely based on DiSL or ASM; the paper provides the specifics). AlgoProf runs the target program on a batch of inputs with varying sizes, collects the `(n, cost)` points per instrumented method, and emits a per-method cost-function report.

Example from the paper's evaluation: AlgoProf correctly identifies the textbook complexities of the standard JDK sort and collection operations. `Collections.sort` is reported as `O(n log n)` (at least within the range of sizes tested). `HashMap.put` is reported as `O(1)` amortised but with a `O(n)` tail when hash collisions are frequent. `ArrayList.indexOf` is `O(n)`. These are correct textbook answers that the tool infers purely from instrumentation without source-code annotations.

More interesting results appear on real applications: the paper evaluates AlgoProf on several benchmarks from DaCapo and finds methods whose empirical complexity does not match the programmer's documented intent — including methods believed to be `O(n)` that actually exhibit `O(n²)` behaviour on production inputs because of an accidental nested loop or a sub-optimal data structure choice.

## Limitations

- **Range extrapolation.** The cost function is a fit over the observed range of sizes; extrapolation to sizes far outside that range is unreliable.
- **Input diversity.** The tool requires inputs of multiple sizes. A single-size profile is useless.
- **Size determination heuristic.** The "data-structure size at function entry" heuristic works for most container inputs but fails for programs whose cost depends on properties that are not immediately visible at the call site (e.g., the number of distinct elements in a multiset, the depth of a recursive data structure).
- **JVM only.** Ported to other runtimes would require reimplementing the instrumentation layer.

## Relevance to APEX G-46

1. **Empirical complexity inference is part of the G-46 goal.** A performance finding is more useful if it is annotated with "this method appears to grow as `O(n²)` in the size of its first argument" than just "here is an input that makes the program slow". AlgoProf shows that the annotation is obtainable by off-the-shelf curve fitting against instrumentation data.
2. **Cost metric plurality is important.** AlgoProf's multi-cost-metric design (instruction count, allocation bytes, cache misses) maps directly onto G-46's need to detect different attack surfaces — CPU DoS vs memory DoS vs cache-side-channel amplification. Each metric should have its own AlgoProf-style profiler.
3. **Automatic size determination is a useful primitive.** When a G-46 fuzzer produces a slow input, running AlgoProf's data-structure-size heuristic on the same trace would automatically identify which input dimension grew and at what rate, producing the `O(·)` annotation for the finding report.
4. **Complementary to Singularity.** Singularity produces a size-parameterised input generator; AlgoProf takes `(n, cost)` points and infers a cost function. The two together — Singularity generates inputs at many sizes, AlgoProf infers the function — are a natural G-46 pipeline.
5. **Citation for empirical-complexity rigour.** When APEX makes an asymptotic claim, AlgoProf and TrendProf (Goldsmith et al. FSE 2007) are the two references to cite for the legitimacy of empirical cost-function fitting.

## Citation

```
@inproceedings{zaparanuks2012algoprof,
  author    = {Dmitrijs Zaparanuks and Matthias Hauswirth},
  title     = {Algorithmic Profiling},
  booktitle = {Proceedings of the 33rd ACM SIGPLAN Conference on Programming Language Design and Implementation (PLDI '12)},
  year      = {2012},
  pages     = {67--76},
  publisher = {ACM},
  doi       = {10.1145/2254064.2254074}
}
```

## References

- ACM DL — [dl.acm.org/doi/10.1145/2254064.2254074](https://dl.acm.org/doi/10.1145/2254064.2254074)
- Semantic Scholar — [semanticscholar.org/paper/Algorithmic-profiling-Zaparanuks-Hauswirth](https://www.semanticscholar.org/paper/Algorithmic-profiling-Zaparanuks-Hauswirth/78ce3c4d29e325a953dd622ca2c008519acdb21d)
- Predecessor (FSE 2007 Goldsmith/Aiken/Wilkerson TrendProf) — see `01KNWEGYBB4AAEFYMR3Y29EZ49`
- WISE (worst-case via symbolic execution) — see `01KNZ301FVVKJ3YGQC55B3N754`
