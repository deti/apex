---
id: 01KNZ6FPQ32R61BEN1K1WNZGPX
title: Mytkowicz et al. 2009 — Producing Wrong Data Without Doing Anything Obviously Wrong (ASPLOS)
type: literature
tags: [measurement-bias, mytkowicz, asplos-2009, benchmarking, reproducibility, randomization, must-have, link-order]
links:
  - target: 01KNZ4VB6JF8CBPEK1YNFDTDAT
    type: related
  - target: 01KNZ6FPS6S89303Q1F11G6M1A
    type: related
  - target: 01KNZ4VB6JZWDCTRVCP1R5V3GA
    type: related
  - target: 01KNZ6FPSNP9N8SFZMH3B8ZK13
    type: related
  - target: 01KNZ6FPT0VDMJ7J1R5PEBM0DX
    type: related
  - target: 01KNWE2QACWYZJXRJ8QE78T043
    type: related
  - target: 01KNWE2Q9YZBAR140ZX5P36TQ5
    type: related
  - target: 01KNZ7829AP3CN4KCMV1RE7WYC
    type: related
created: 2026-04-11T21:17:08.707203+00:00
modified: 2026-04-11T21:17:08.707210+00:00
---

*Source: Todd Mytkowicz, Amer Diwan, Matthias Hauswirth, Peter F. Sweeney — "Producing Wrong Data Without Doing Anything Obviously Wrong!" — Proceedings of ASPLOS '09, March 2009. ACM DOI 10.1145/1508244.1508275. Pages 265-276.*

Foundational paper for anyone running performance experiments. It demonstrates empirically that what look like completely innocuous aspects of your experimental setup — the order of object files on the linker command line, the length of the `UNIX_SHELL` environment variable, the working directory path — can change measured performance by amounts **larger than the compiler optimization you are trying to measure**. The authors call this **measurement bias**, borrowing the term from the social sciences.

## The core finding

From the abstract:

> "A systems researcher may end up drawing wrong conclusions from an experiment. What appears to be an innocuous aspect in the experimental setup may in fact introduce a significant bias in an evaluation."

The experiments are a catalog of horror stories:

### Example 1 — Link order determines the winner

The authors take two versions of a C program compiled with `gcc -O2` vs `gcc -O3`. The textbook expectation is that `-O3` is faster. They show that by changing **only the order of `.o` files on the linker command line**, they can make either version appear faster. The effect is large enough to completely flip the sign of the apparent speedup of `-O3` over `-O2`. The mechanism is that link order affects the starting addresses of functions and thus their alignment relative to instruction cache lines and branch-predictor structures — a 16- to 32-byte accident of linker ordering swings hit rates in L1-I by several percent.

### Example 2 — Environment variable length

Changing the value of an unrelated environment variable (they picked `UNIX_SHELL`) shifts the initial stack pointer at program startup, which in turn shifts the addresses of all stack-allocated variables. This changes L1-data-cache conflict misses and produces measurable runtime differences — several percent on SPEC CPU2006 benchmarks — with no code change whatsoever. The authors exploit this to show that a single environment variable can be tuned to make an experimental "treatment" look beneficial when it isn't.

### Example 3 — Universality

The biases appear **on every architecture tested** (Pentium 4, Core 2, and the simulated m5 O3CPU), with **both compilers tested** (gcc and Intel's icc), and on **most of SPEC CPU2006 C programs**. It is not a quirk of one microarchitecture or one toolchain — it is the natural consequence of the fact that modern machines' performance depends on memory layout and that memory layout depends on seemingly unrelated environmental inputs.

## Definition of measurement bias

Borrowing from the natural and social sciences:

> "Measurement bias occurs when a measurement is systematically different from the quantity one wishes to measure. In an experiment to determine if idea I is beneficial for system S, if the measurement setup is biased towards S+I, a researcher may conclude that I is beneficial even when it is not."

The distinctive feature of measurement bias in computer systems is that **the bias depends on factors that researchers do not usually control or even report**: link order, environment variables, filesystem layout, memory layout. Two researchers running "the same benchmark on the same hardware" can easily get different results because they inadvertently run from different working directories.

## The two methods the paper proposes

### 1. Causal analysis (detection)

Use profiling and microarchitectural counters to detect when an observed performance difference is explained by a change in something like L1-I miss rate rather than by the quantity you thought you were varying. If toggling the treatment also toggles the cache behaviour in a way that's too coincidental to be the treatment's effect, you've got measurement bias.

### 2. Setup randomization (avoidance)

Instead of trying to hold every confounder constant (impossible), **randomize them**. Run the experiment many times with different link orders, different environment variables, different stack offsets — and report the distribution of outcomes. This transforms the confounders from systematic biases into random noise that averages out across replicates. This is the direct conceptual ancestor of Curtsinger & Berger's **Stabilizer** (ASPLOS 2013), which implements this idea by runtime-randomizing code, stack, and heap layouts repeatedly during a single execution.

## Why this paper matters for CI perf gating

Everything a CI perf gate measures is vulnerable to measurement bias:
- Between commits, **link order can change** when a file is added, a function is added, or a dependency version bumps. The "regression" you detect may be a link-order shift, not a logical change.
- **Filesystem paths** differ between CI runners. A move from `/home/runner/work/foo/foo` to `/var/lib/jenkins/ws/foo` can shift stack addresses and change cache behaviour.
- **Background services** on a CI runner (auto-updaters, agents, shared hosting workloads) introduce timing jitter and cache pollution.
- **Hardware heterogeneity** across a CI pool means today's run might land on a different SKU than yesterday's baseline.

Without randomization or tight hardware control, a CI perf gate is effectively running an experiment where every comparison has confounds that Mytkowicz et al. demonstrated can dwarf the effects you're trying to measure. **This is the single strongest argument for Stabilizer-style randomization, bare-metal perf runners, or instruction-count benchmarking (CodSpeed) rather than wall-clock benchmarks on shared CI runners.**

## The paper's legacy

- **Stabilizer** (Curtsinger & Berger ASPLOS 2013) directly implements the randomization remedy.
- **LLVM** added function section ordering randomization as a build flag partly in response.
- **criterion.rs**, **Google Benchmark**, **JMH** all cite the need for statistical rigor in the presence of layout noise.
- Almost every modern paper on systems benchmarking cites this one when justifying randomized or statistical methodology.
- **Reviewer checklists** in top systems conferences now commonly ask "did you randomize the factors that Mytkowicz et al. identified?"

## Adversarial commentary

- The remedy of **setup randomization** adds experimental cost: you need many runs, not one. For a CI system running thousands of benchmarks, this is expensive. The trade-off is between (cheap + biased) and (expensive + unbiased). Most CI teams compromise with "run n=10 iterations and hope the environment is consistent," which partially addresses run-to-run noise but does nothing for link-order bias because the linking happens once per build.
- **Microarchitectural evolution hasn't solved the problem.** Modern Intel/AMD/Arm machines have larger caches and better branch predictors, but they're also more complex (more layers of prefetchers, more speculation, more state), and the layout sensitivity per Mytkowicz et al. is arguably *worse* now than in 2009.
- **Not every layout effect is noise to be averaged away.** Sometimes link order genuinely matters (e.g., hot/cold function ordering for instruction cache density). The paper's point is not that you should ignore layout effects but that you should not confuse them with algorithmic or compiler differences.
- **The paper does not solve the problem, it diagnoses it.** Solutions require either (a) systematic randomization (Stabilizer, expensive), (b) extreme environmental control (bare-metal perf labs, also expensive), or (c) accepting that wall-clock benchmarks on consumer hardware are inherently noisy and moving to deterministic proxies like instruction counts (CodSpeed, iai-callgrind).

## Connections

- Curtsinger & Berger 2013 "Stabilizer" — implements the randomization remedy.
- Chen & Revels 2016 "Robust benchmarking in noisy environments" — robust-statistics view of the same problem.
- Daly et al. 2020 — uses statistical methods partly to handle the irreducible noise Mytkowicz identified.
- CodSpeed's Valgrind-based instruction counting — a different answer: sidestep wall-clock noise entirely.

## Reference

Mytkowicz, T., Diwan, A., Hauswirth, M., Sweeney, P. F. (2009). *Producing wrong data without doing anything obviously wrong!* ASPLOS '09, pp. 265-276. ACM DOI 10.1145/1508244.1508275.
