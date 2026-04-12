---
id: 01KNZ72G2VNNP6JHWQAK0HJTXM
title: "Barr et al. 2015 — The Oracle Problem in Software Testing: A Survey (TSE)"
type: literature
tags: [oracle-problem, barr, harman, mcminn, shahbaz, yoo, tse-2015, metamorphic-testing, implicit-oracle, survey, test-oracle]
links:
  - target: 01KNZ68KJMZSSAZVFAB3ZNXNTJ
    type: related
  - target: 01KNZ72G5955YGB9B2W61QD2Z4
    type: related
  - target: 01KNZ72G5SVY6JH66N7BP825C6
    type: related
  - target: 01KNZ6QBKG64F2WEP3PNJK61JH
    type: related
  - target: 01KNWE2Q9YZBAR140ZX5P36TQ5
    type: related
created: 2026-04-11T21:27:24.507982+00:00
modified: 2026-04-11T21:27:24.507990+00:00
---

*Source: Earl T. Barr, Mark Harman, Phil McMinn, Muzammil Shahbaz, Shin Yoo — "The Oracle Problem in Software Testing: A Survey" — IEEE Transactions on Software Engineering, Vol. 41, No. 5, May 2015, pp. 507-525. DOI 10.1109/TSE.2014.2372785.*

The canonical survey of the **test oracle problem** — the question of how, given a test input, you decide whether the observed output is correct. The paper gives the field a taxonomy of oracle types and motivates why the oracle is the bottleneck preventing full test automation. Although it does not primarily discuss performance oracles, the taxonomy it introduces is the lens through which all subsequent performance-oracle work is organised (Segura et al.'s performance metamorphic testing, SLOs as oracles, differential perf testing, etc.).

## The core problem

From the abstract:

> "Given an input for a system, the challenge of distinguishing the corresponding desired, correct behaviour from potentially incorrect behaviour is called the 'test oracle problem'. Test oracle automation is important to remove a current bottleneck that inhibits greater overall test automation."

In words: you can generate a million test inputs with a fuzzer in minutes. You cannot afford a human to check each output. An **oracle** is something that can automatically say "this output is correct" or "this is buggy". Without oracles, automated testing only catches crashes — anything subtler than a segfault requires human judgement.

## Barr et al.'s taxonomy of oracle types

The survey organises the literature into four main categories:

### 1. Specified oracles

A formal specification describes what the program should do, and the oracle checks outputs against the specification. Examples:
- Pre/postconditions (Design by Contract, JML, Eiffel).
- Temporal logic specifications (LTL, CTL) checked against traces.
- Behavioural models (UML state machines, Alloy specifications) against which implementations are compared.

**Strength:** unambiguous; when a spec exists, checking is mechanical.
**Weakness:** writing a formal spec is often as hard as writing the program. For performance, "formal performance specs" exist (SLOs, worst-case execution time bounds, Markov chain models) but rarely capture the full intended behaviour.

### 2. Derived oracles

The oracle is derived from some other source of information, not a full specification. Sub-types in the survey:
- **Metamorphic oracles** — rather than checking "is this output correct?", check "does this pair of related outputs satisfy a known invariant?" e.g., if `sort(L1) = sort(L2)`, then L1 and L2 must be permutations of each other. This is the **metamorphic testing** approach (T.Y. Chen, Segura, et al.) and is especially important for performance — see the Segura et al. 2018 note for performance metamorphic relations.
- **Regression oracles** — outputs of the previous version of the program are the oracle for the new version. You're asking "did this change break anything?" rather than "is this output correct?". This is most of what CI regression testing does, including perf regression testing.
- **N-version / differential oracles** — run N independent implementations of the same spec; disagreements are bugs. Expensive but powerful. Variant: golden-model comparison (compare the program against a reference implementation).
- **Machine-learned oracles** — a model trained on past behaviour predicts expected outputs; deviations are flagged. A 2020s direction: LLM-based oracles.

### 3. Implicit oracles

The oracle is any property that is trivially checkable without knowledge of what the program should do:
- **Does the program crash?** (segfault, assertion failure, unhandled exception).
- **Does it violate language-level safety?** (use-after-free under ASan, data race under TSan, integer overflow under UBSan).
- **Does it exceed resource bounds?** (timeout, memory limit, file descriptor limit).

**Strength:** universal, requires zero specification.
**Weakness:** only catches the most obvious bugs. A program that produces the wrong output but does so within resource limits and without crashing is not caught.

For performance testing, **"timeout" is the de facto implicit oracle** — most CI pipelines kill a test after N minutes and count that as a failure. This is a blunt instrument but catches catastrophic regressions (e.g., an O(n²) introduced into what should be O(n)).

### 4. No oracle — partial verdicts

When no oracle is available, the best you can do is record the output and have a human look at it later, or flag changes in output across versions. The paper notes this is still useful as "at least it's reproducible" evidence.

## Why this taxonomy matters for perf

The original Barr et al. paper does **not** have a dedicated chapter on performance oracles, but its taxonomy maps cleanly onto the performance testing landscape:

- **Specified performance oracles** = SLOs, WCET bounds, analytic models ("this must be O(n log n)"). See dedicated SLO note.
- **Derived performance oracles**:
  - **Metamorphic performance oracles** = Segura et al. 2018 "PMRs" (Performance Metamorphic Relations). Examples: "2× input size → ≤ 4× runtime for O(n²) algorithm", "adding a constant to all inputs should not change sort cost".
  - **Regression performance oracles** = pairwise / time-series comparison against a historical baseline. This is what Perfherder, Kayenta, CodSpeed, Bencher.dev all do.
  - **Differential performance oracles** = A/B testing between versions, between implementations, between platforms. Canary analysis is a form of this.
- **Implicit performance oracles** = timeouts, OOM kills, FD/socket exhaustion. Catches catastrophic regressions.

This is the most important point for a CI/CD practitioner: when you set up a perf regression gate, you are making a choice about which oracle type you are using, and each has different strengths, weaknesses, and failure modes.

## Specific quotes worth remembering

On the bottleneck framing:

> "Without test oracle automation, the human has to determine whether observed behavior is correct."

On where oracles come from:

> "The literature on test oracles has introduced techniques for oracle automation, including modelling, specifications, contract-driven development and metamorphic testing. When none of these is completely adequate, the final source of test oracle information remains the human, who may be aware of informal specifications, expectations, norms and domain specific information that provide informal oracle guidance."

The "human as last resort" framing is apt for perf: the Mozilla/MongoDB/Chrome sheriff rotations are all literal implementations of "human as last-resort oracle."

## Adversarial commentary

- **The survey is 2015; the ML-oracle landscape has changed dramatically since.** LLMs as oracles (judge LLMs, DebugBench evaluations) are a new category that the 2015 taxonomy anticipated only vaguely.
- **Performance oracles get short shrift.** The paper focuses on functional correctness; performance is acknowledged as a non-functional concern but not examined in depth. Segura et al.'s 2018 work is a direct response to this gap.
- **Metamorphic oracles are harder to construct than the survey implies.** Finding a valid metamorphic relation is itself a creative act that requires understanding the domain. The paper cites successes (sorting, compilers) but glosses over the fact that most domains lack obvious invariants.
- **Oracle automation is not a free lunch.** Every "derived" oracle is either (a) requiring a reference implementation you have to keep in sync, (b) relying on a spec you have to maintain, or (c) detecting regressions rather than bugs. Each has a cost.

## Connections

- Segura et al. 2018 (dedicated note) — performance metamorphic relations, direct application of Barr's taxonomy.
- SLOs as perf oracles (dedicated note) — specified oracle category.
- Differential perf testing / A-B as oracle (dedicated note) — derived oracle category.
- Intramorphic testing (Rigger 2022, arxiv.org/pdf/2210.11228) — recent refinement of metamorphic testing.
- Daly et al. 2020 / Perfherder / Kayenta — all implement regression oracles at the infrastructure level.

## Reference

Barr, E. T., Harman, M., McMinn, P., Shahbaz, M., Yoo, S. (2015). *The Oracle Problem in Software Testing: A Survey*. IEEE TSE, 41(5): 507-525. DOI 10.1109/TSE.2014.2372785. UCL Discovery: discovery.ucl.ac.uk/1471263/
