---
id: 01KNZ4RPCXXK9RRRWN1X0Y12Q6
title: "HyDiff: Hybrid Differential Software Analysis (Noller et al., ICSE 2020)"
type: literature
tags: [paper, hybrid-fuzzing, differential-analysis, side-channel, regression, icse, 2020, shadow-symbolic-execution, spf]
links:
  - target: 01KNZ301FVSD3QCQM0KD2Y1M1K
    type: extends
  - target: 01KNZ301FV7BZB2X9338XPDNK0
    type: extends
  - target: 01KNZ301FVVKJ3YGQC55B3N754
    type: related
  - target: 01KNWEGYB1B15QGYTRC374Z7DQ
    type: related
  - target: 01KNWE2QA0Z52H8VVFAMSA7KGA
    type: related
  - target: 01KNWE2QA8H1GKHCVNHYS5QW1F
    type: related
  - target: 01KNZ4RPD28S804ZJB9QH9QBNY
    type: related
  - target: 01KNWE2Q9YZBAR140ZX5P36TQ5
    type: related
created: 2026-04-12
modified: 2026-04-12
source: "https://mboehme.github.io/paper/ICSE20.hydiff.pdf"
doi: "10.1145/3377811.3380413"
venue: "ICSE 2020"
authors: [Yannic Noller, Corina S. Păsăreanu, Marcel Böhme, Youcheng Sun, Hoang Lam Nguyen, Lars Grunske]
year: 2020
artifact: "https://github.com/yannicnoller/hydiff"
---

# HyDiff: Hybrid Differential Software Analysis

**Authors:** Yannic Noller (Humboldt-Universität zu Berlin), Corina S. Păsăreanu (CMU / NASA Ames), Marcel Böhme (Monash University), Youcheng Sun (Queen's University Belfast), Hoang Lam Nguyen (HU Berlin), Lars Grunske (HU Berlin).
**Venue:** 42nd International Conference on Software Engineering (ICSE '20), Seoul, July 2020.
**DOI:** 10.1145/3377811.3380413.
**Artifact:** https://github.com/yannicnoller/hydiff
**Mirror PDF:** https://mboehme.github.io/paper/ICSE20.hydiff.pdf — https://pureadmin.qub.ac.uk/ws/files/200988357/hydiff.pdf

*Source: https://mboehme.github.io/paper/ICSE20.hydiff.pdf — fetched 2026-04-12 but returned as compressed binary; the abstract below is verbatim from the ICSE 2020 program page and the Springer chapter summary, and the technical description is from the paper as summarised in subsequent citations and the `yannicnoller/hydiff` README.*

## Abstract (from ICSE 2020 proceedings)

> "Detecting regression bugs in software evolution, analyzing side-channels in programs and evaluating robustness in deep neural networks (DNNs) can all be seen as instances of **differential software analysis**, where the goal is to generate diverging executions of program paths. Two executions are said to be diverging if the observable program behavior differs, e.g., in terms of program output, execution time, or (DNN) classification. The key challenge of differential software analysis is to simultaneously reason about multiple program paths, often across program variants.
>
> HyDiff is the first hybrid approach for differential software analysis that integrates and extends two very successful testing techniques: feedback-directed greybox fuzzing for efficient program testing and shadow symbolic execution for systematic program exploration. The approach introduces differential metrics such as output, decision and cost difference, as well as patch distance, to assist the fuzzing and symbolic execution components in maximizing the execution divergence, and is implemented on top of the fuzzer AFL and the symbolic execution framework Symbolic PathFinder. HyDiff is illustrated on regression and side-channel analysis for Java bytecode programs, and further shows how to use HyDiff for robustness analysis of neural networks."

## Positioning: HyDiff vs Badger

HyDiff is the same research group's natural follow-up to **Badger** (Noller, Kersten, Păsăreanu — ISSTA 2018, see `01KNZ301FVSD3QCQM0KD2Y1M1K`). Badger solves the **single-program worst-case** problem: given a Java program, find an input that makes it slow. HyDiff generalises the goal to **two programs (or two branches of one program) that must be shown to diverge**. This generalisation covers three important analyses under a single framework:

1. **Regression analysis.** Two versions of a program; find inputs that behave differently before and after a patch. Directly useful for change-impact analysis, regression testing, and patch validation.
2. **Side-channel analysis.** Two executions of the same program under different secret inputs; find an input pair where an observable (timing, memory, cache, power proxy) differs. This is the Diffuzz / side-channel-resource-leak problem (see `01KNZ301FV7BZB2X9338XPDNK0` on SPF-WCA and `isstac/diffuzz`).
3. **DNN robustness.** Two neural network classifiers (or one classifier and its adversarial twin); find an input where the classification diverges. Robustness certification reframed as a divergence search.

The authors' insight is that these three applications share the same search objective — maximise divergence between two execution traces — and the same solution architecture: a hybrid of greybox fuzzing and shadow symbolic execution.

## Shadow symbolic execution

"Shadow" symbolic execution is a technique in which two versions of a program run in parallel over the same input, with **symbolic state representing both versions simultaneously**. Each branch point is annotated with which version (or both) takes the branch; the symbolic engine explores the product of both versions, so a single path in the shadow semantics corresponds to a specific pair of traces in the two versions. A divergence is a path where the shadow semantics shows the two versions taking different branches or producing different outputs.

HyDiff's symbolic worker uses **Symbolic PathFinder** (SPF) over shadow-instrumented Java bytecode. The symbolic worker's job is to exhaustively solve for inputs that reach known divergence points or maximise a divergence metric given the current constraints.

## Differential metrics

HyDiff's novelty over plain shadow execution is the set of metrics it uses to guide both the fuzzer and the symbolic engine toward diverging inputs:

1. **Output difference.** Hamming distance (or domain-specific distance) between the two versions' outputs on the same input. Directly captures "produced different results".
2. **Decision difference.** Number of branch points at which the two versions take different sides. A coarse but cheap proxy for divergence.
3. **Cost difference.** Difference in measured execution cost (instruction count, allocation count, wall time) between the two versions. This is the side-channel-specific metric: a non-zero cost difference on different secret inputs is a timing channel.
4. **Patch distance.** How close, in terms of control-flow distance, the current execution reaches to the patched region of the code. A smaller patch distance means the fuzzer has at least reached the changed code; zero patch distance means the input exercised the patch.

All four metrics are wired into the fuzzer (AFL's feedback is extended to reward inputs that improve any of them) and the symbolic engine (SPF's worklist prioritises paths that maximise them). The hybrid handshake is the same as Badger's: the fuzzer produces concrete inputs which seed SPF's exploration, and SPF produces path constraints which the fuzzer uses as mutation guidance.

## Evaluation

HyDiff is evaluated on three axes:

- **Regression:** standard Java regression benchmarks including the Defects4J programs patched for specific bugs. HyDiff reaches the patched region and exhibits divergence faster than AFL alone or SPF alone.
- **Side-channel:** the DARPA STAC benchmarks (airport, Smart Ticketing, etc.) with side-channel challenge suites. HyDiff finds leaks that neither Badger's single-program analysis nor Diffuzz's pure fuzzing alone could exploit within the same budget.
- **DNN robustness:** MNIST and CIFAR classifiers, framed as shadow classifiers trained on different samples. HyDiff produces adversarial examples by searching for divergence rather than by gradient ascent.

On all three axes HyDiff outperforms both the pure-fuzzing and pure-symbolic baselines, with the largest margins on problems where either component alone stalls (deep constraints for fuzzing, path explosion for symbolic).

## Relevance to APEX G-46

1. **APEX's concolic crate already supports a symbolic-executor role similar to SPF.** Implementing HyDiff's shadow shim would let APEX attack regression-performance bugs — a version-over-version increase in cost — as a first-class finding class. This is the natural next step after single-program G-46.
2. **Cost difference is the G-46 side-channel signal.** Time- and resource-based side channels are an adjacent finding class to performance DoS, and the cost-difference metric is identical whether the attacker's goal is DoS (amplify absolute cost) or leakage (amplify relative cost between two secrets).
3. **Patch distance as a CI signal.** When APEX runs on a PR, the patch distance metric gives a cheap way to bias fuzzing budget toward the changed code — a natural feature for "performance regression gate" use cases.
4. **The four metrics are a minimal feedback vocabulary for any G-46 differential mode.** Implement them once in apex-fuzz and reuse across regression, side-channel, and robustness analyses.

## Citation

```
@inproceedings{noller2020hydiff,
  author    = {Yannic Noller and Corina S. P{\u a}s{\u a}reanu and Marcel B\"ohme and Youcheng Sun and Hoang Lam Nguyen and Lars Grunske},
  title     = {{HyDiff}: Hybrid Differential Software Analysis},
  booktitle = {Proceedings of the ACM/IEEE 42nd International Conference on Software Engineering (ICSE '20)},
  year      = {2020},
  pages     = {1273--1285},
  doi       = {10.1145/3377811.3380413},
  publisher = {ACM}
}
```

## References

- PDF mirror (Böhme homepage) — [mboehme.github.io/paper/ICSE20.hydiff.pdf](https://mboehme.github.io/paper/ICSE20.hydiff.pdf)
- QUB mirror — [pureadmin.qub.ac.uk/ws/files/200988357/hydiff.pdf](https://pureadmin.qub.ac.uk/ws/files/200988357/hydiff.pdf)
- Artifact — [github.com/yannicnoller/hydiff](https://github.com/yannicnoller/hydiff)
- ICSE page — [conf.researchr.org/details/icse-2020/icse-2020-papers/77/HyDiff-Hybrid-Differential-Software-Analysis](https://conf.researchr.org/details/icse-2020/icse-2020-papers/77/HyDiff-Hybrid-Differential-Software-Analysis)
- Badger (ISSTA 2018) — see `01KNZ301FVSD3QCQM0KD2Y1M1K`
- SPF-WCA (ISSTA 2017) — see `01KNZ301FV7BZB2X9338XPDNK0`
- WISE (ICSE 2009) — see `01KNZ301FVVKJ3YGQC55B3N754`
