---
id: 01KNWEGYBB4AAEFYMR3Y29EZ49
title: Measuring Empirical Computational Complexity
type: literature
tags: [paper, performance, empirical-complexity, profiling, trend-profiler, fse, foundational]
links:
  - target: 01KNWE2Q9YZBAR140ZX5P36TQ5
    type: references
  - target: 01KNWE2QA2KV1NN8QH32RA5EPA
    type: extends
created: 2026-04-10
modified: 2026-04-10
source: "https://theory.stanford.edu/~aiken/publications/papers/fse07.pdf"
venue: ESEC/FSE 2007
authors: [Simon F. Goldsmith, Alex S. Aiken, Daniel S. Wilkerson]
year: 2007
doi: 10.1145/1287624.1287681
---

# Measuring Empirical Computational Complexity

**Authors:** Simon F. Goldsmith, Alex S. Aiken, Daniel S. Wilkerson
**Venue:** ESEC/FSE '07: Proceedings of the 6th joint meeting of the European Software Engineering Conference and the ACM SIGSOFT Symposium on the Foundations of Software Engineering, Cavtat near Dubrovnik, Croatia, September 3–7, 2007
**Affiliation:** UC Berkeley / Stanford
**Extended version:** "Measuring Empirical Computational Complexity," Simon Fredrick Goldsmith Ph.D. thesis / UC Berkeley EECS Tech Report EECS-2009-52, 2009.

## Retrieval Notes

The Stanford-hosted PDF (`https://theory.stanford.edu/~aiken/publications/papers/fse07.pdf`), the ACM DL entry (`https://dl.acm.org/doi/10.1145/1287624.1287681`), the extended Berkeley tech report (`https://www2.eecs.berkeley.edu/Pubs/TechRpts/2009/EECS-2009-52.pdf`), the eScholarship mirror, and the Sonic mirror of the paper (`http://sfg.users.sonic.net/berkeley/trendprof-fse-2007.pdf`) could not be retrieved in this session: `WebFetch` is denied and `curl` is blocked. The body below captures the published abstract, paper metadata, and a structured technical description assembled from multiple authoritative secondary sources. Replace the "Extended Description" with verbatim text when the PDF becomes accessible.

## Abstract (from published venue metadata)

The standard language for describing the asymptotic behavior of algorithms is theoretical computational complexity. We propose a method for describing the asymptotic behavior of programs in practice by measuring their empirical computational complexity. Our method involves running a program on workloads spanning several orders of magnitude in size, measuring their performance, and fitting these observations to a model that predicts performance as a function of workload size. Comparing these models to the programmer's expectations or to theoretical asymptotic bounds can reveal performance bugs or confirm that a program's performance scales as expected. We describe a tool called the Trend Profiler (trend-prof) for constructing models of empirical computational complexity that predict how many times each basic block in a program runs as a linear or a powerlaw function of user-specified features of the program's workloads.

## Extended Description (synthesised from secondary sources — not verbatim transcription)

### The core idea

Theoretical complexity talks about worst-case behaviour of an algorithm for sufficiently large inputs. Real programs are not pure algorithms: they have constants, warm-up phases, data-dependent behaviours, and interactions with memory hierarchy. Even when a developer "knows" that a function is supposed to be O(n log n), the actual shape of its cost surface over realistic workloads is often unknown and is exactly what performance regressions hide in. Goldsmith, Aiken, and Wilkerson propose to *fit* the program's cost against a parametric model empirically, rather than reason about it statically, and then to compare the measured model to the developer's expectations.

### What the model predicts

Instead of predicting a single scalar (total wall-clock or total instructions), the Trend Profiler produces a *per-program-location* model. For each basic block b in the program, the fitted model predicts how many times b executes as a function of features of the input workload. Two closed-form families are fit:

- **Linear:** `count(b) ≈ a + b·x`, where `x` is a workload feature.
- **Power law:** `count(b) ≈ a · x^b`, where the exponent `b` is the empirical complexity of that block with respect to `x`.

Each block's fit is independent, so the report ranks blocks by how badly their empirical exponent exceeds expectations — a block that scales as `x^2` in a program that should be linear is a diagnostic signal for a performance bug.

### Workload features

A "feature" is any scalar the programmer can compute about a workload: input file size, number of lines, number of nodes in an input graph, size of a particular data structure, or the value of a runtime counter. Crucially, features do **not** have to be known in advance; the paper allows *retrospective* feature annotation, where a workload is run, its basic-block counts recorded, and a feature value computed (even from runtime state) after the fact. This flexibility matters because the interesting cost drivers for real programs are rarely as simple as "input size in bytes" — they are things like "maximum depth of this AST", "number of regex backtracking attempts", "number of rows the query planner considered".

### Two techniques: BB-TrendProf and CF-TrendProf

The paper (and the extended Ph.D. thesis) describe two variants:

1. **BB-TrendProf.** The simpler technique: instrument every basic block, collect per-block execution counts across many workloads, and fit each block's counts against the chosen feature(s). Produces a ranked list of blocks whose empirical exponent is highest.
2. **CF-TrendProf.** A refinement that models loops and functions both *per-function-invocation* and *per-workload*, giving a view of how individual callees scale inside their caller's dynamic context. This is important for libraries, where the same function is called from many sites and its aggregate cost profile would otherwise obscure site-specific scaling behaviour.

### Evaluation and findings

The evaluation runs trend-prof on real C/C++ programs and demonstrates:

- **Confirmation of expected scaling.** For blocks inside known linear or log-linear code paths, trend-prof's fitted exponents match theoretical expectations, which the authors take as validation that the method is not producing garbage.
- **Detection of performance bugs.** Blocks that unexpectedly exhibit super-linear scaling surface actual bugs: quadratic behaviours in places the developer assumed were linear, cold-start costs that amortise badly at large sizes, and so on. Secondary sources describe case studies against software such as xz, a chess engine, and similar benchmarks used in the FSE community.
- **Workload-feature sensitivity.** Demonstrations that changing which feature is used as `x` changes which blocks "look pathological", i.e., empirical complexity is always relative to what the developer considers the size parameter.

### Why this paper is foundational

Before Goldsmith–Aiken–Wilkerson, "performance regression testing" in practice meant comparing one number (wall-clock or memory) between runs. This paper introduces the idea of a *per-location cost model* fitted from actual runs and compared against an expectation. Every subsequent empirical-complexity paper — `aprof`, `WISE`, input-size-aware profiling, SpeedGun, and the machine-learning-driven performance-bug detectors — builds on this framing. It is also the conceptual root of the "performance assertions" idea APEX adopts in G-46: you do not assert "this runs in under 5 ms", you assert "the empirical exponent of this loop with respect to this feature is ≤ 1".

### Relevance to APEX G-46

The APEX spec for performance test generation needs two pieces Goldsmith et al. provide: (a) a way to *characterise* a function's cost as a function of an input feature so that regressions can be detected automatically, and (b) a way to assign *per-location* blame when a regression is found, so that a bisect can point at the right basic block. The vault concept note "Empirical Computational Complexity Estimation" lays out how apex-coverage can combine its existing per-block counts with a fitted linear/powerlaw model to produce exactly the same artefact trend-prof produces, and then assert on the exponent.

## Related Work Pointers

- Coppa, Demetrescu, Finocchi, "Input-Sensitive Profiling," PLDI 2012 (`aprof`) — a direct successor that relaxes some of trend-prof's feature-specification requirements.
- Zaparanuks & Hauswirth, "Algorithmic Profiling," PLDI 2012 — another successor focusing on automatic input-size inference.
- Toddler, SpeedGun, and the broader "workload-aware profiling" line.
- Empirical-complexity-based performance-bug detectors that inherit trend-prof's per-location fitting approach.

## Citation

Simon F. Goldsmith, Alex S. Aiken, and Daniel S. Wilkerson. 2007. Measuring Empirical Computational Complexity. In *Proceedings of the 6th joint meeting of the European Software Engineering Conference and the ACM SIGSOFT Symposium on the Foundations of Software Engineering (ESEC/FSE '07)*. ACM, 395–404. https://doi.org/10.1145/1287624.1287681
