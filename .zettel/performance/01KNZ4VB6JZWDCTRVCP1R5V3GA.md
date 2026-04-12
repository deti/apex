---
id: 01KNZ4VB6JZWDCTRVCP1R5V3GA
title: "Stabilizer — Statistically Sound Performance Evaluation (Curtsinger & Berger ASPLOS 2013)"
type: literature
tags: [stabilizer, curtsinger, berger, measurement-bias, layout-randomisation, asplos-2013, llvm, statistical-tests]
links:
  - target: 01KNZ4VB6JF8CBPEK1YNFDTDAT
    type: extends
  - target: 01KNZ4VB6JEBFDN1QBC4680Y09
    type: related
  - target: 01KNZ4VB6J5ZW3JERZNDNGP7GD
    type: related
  - target: 01KNZ6FPS6S89303Q1F11G6M1A
    type: related
  - target: 01KNZ6FPQ32R61BEN1K1WNZGPX
    type: related
  - target: 01KNWE2QACWYZJXRJ8QE78T043
    type: related
  - target: 01KNWE2Q9YZBAR140ZX5P36TQ5
    type: related
created: 2026-04-12
modified: 2026-04-12
source: "Curtsinger, C., Berger, E.D. — 'STABILIZER: Statistically Sound Performance Evaluation' — ASPLOS 2013"
---

# Stabilizer — Statistically Sound Performance Evaluation

*Source: Curtsinger, C., Berger, E.D. — "STABILIZER: Statistically Sound Performance Evaluation" — ASPLOS 2013 (Houston, TX, March 2013). PDF available at people.cs.umass.edu/~emery/pubs/stabilizer-asplos13.pdf — fetched and extracted 2026-04-12.*

## The problem in one sentence

**A single binary represents just one sample from the vast space of possible memory layouts, and because modern CPU caches and branch predictors are sensitive to addresses, that single layout biases *every* measurement of that binary — making statistical hypothesis tests invalid regardless of how many runs you do.**

## The broader statement

From the abstract (verbatim):

> *Researchers and software developers require effective performance evaluation. ... Unfortunately, modern architectural features make this approach unsound. Statistically sound evaluation requires multiple samples to test whether one can or cannot (with high confidence) reject the null hypothesis that results are the same before and after. However, caches and branch predictors make performance dependent on machine-specific parameters and the exact layout of code, stack frames, and heap objects. A single binary constitutes just one sample from the space of program layouts, regardless of the number of runs. Since compiler optimizations and code changes also alter layout, it is currently impossible to distinguish the impact of an optimization from that of its layout effects.*

Key phrases:

- **"a single binary constitutes just one sample"** — no matter how many times you run `./prog` and record the time, you're drawing from the same layout.
- **"impossible to distinguish the impact of an optimization from that of its layout effects"** — the failure mode Stabilizer fixes.

## The measurement-bias phenomenon

The prior work Stabilizer builds on is Mytkowicz, Diwan, Hauswirth, Sweeney — "Producing Wrong Data Without Doing Anything Obviously Wrong!" ASPLOS 2009. They showed:

1. Changing the size of an unrelated environment variable can change program execution time by **up to 300 %** — because environment variables sit on the stack, and changing their size shifts stack alignment, which shifts cache associativity conflicts.
2. Changing the order of object files at link time can change program execution time by **up to 57 %** — same mechanism, at the code side.

Their conclusion: performance measurements carried out without randomising layout are fundamentally unreliable. A paper claiming "optimisation X yields 5 % speedup" may be reporting 5 % of layout noise, indistinguishable from true compiler work.

Stabilizer is the follow-up that *solves* the problem, not just exposes it.

## How Stabilizer works

Stabilizer dynamically re-randomises three axes of program layout *at runtime*:

1. **Code**: function placement. Each function is dynamically relocated, with its call sites rewritten. New layout every randomisation interval.
2. **Stack**: stack frame alignment. Each function's stack frame gets a random padding on entry, so successive calls use different stack addresses.
3. **Heap**: object placement. The heap allocator randomises placement at the granularity of individual allocations.

Re-randomisation happens *during* execution (default every 500 ms). Any single run thus samples many layouts rather than a single fixed one. The Central Limit Theorem then applies: as the program accumulates time across many random layouts, the total execution time converges to a Normal distribution whose mean is the layout-independent "true" runtime of the code (plus a fixed randomisation overhead).

## The Central Limit Theorem trick

Because the per-layout contribution to total execution time is a random variable drawn iid from the distribution of layouts, and total time is the sum of many such contributions, CLT gives:

> *total execution time is approximately Normally distributed, with mean = expected per-layout × number of layouts, independent of which specific layouts were sampled.*

This enables **parametric statistical tests** (Welch's t-test, ANOVA) to apply to Stabilizer runs. Without Stabilizer, layout effects are an uncontrolled systematic factor that violates the iid assumption of parametric tests; with it, they become a controlled random factor and the tests are sound.

## Overhead

Curtsinger & Berger measured Stabilizer's overhead on SPEC CPU2006: **< 7 % median overhead**. The main costs are the code relocation and the re-randomisation itself. 7 % is small enough that Stabilizer can be used routinely for benchmarking without distorting the question ("does change X speed up the program by > 7 %?" is a well-defined question under Stabilizer).

## The killer finding on LLVM optimisations

Curtsinger & Berger evaluated LLVM compiler optimisations on SPEC CPU2006 using Stabilizer:

- `-O1 → -O2` shows a **statistically significant** improvement (large effect size, p << 0.05).
- `-O2 → -O3`: **the performance impact of -O3 over -O2 is indistinguishable from random noise** on the tested benchmark suite.

This is a stunning result. Compiler teams had been iterating on -O3 pipelines for years, with various optimisations at the -O3 tier (argument promotion, dead global elimination, global common subexpression elimination, scalar replacement of aggregates). The measured "wins" reported in prior research were indistinguishable from layout noise. In other words: a substantial body of published compiler research had been measuring *layout effects* and mis-attributing them to compiler optimisations.

The finding is not that -O3 has no value — specific programs can benefit — but that the *aggregate* benefit across SPEC CPU2006 is within the noise floor. Any measurement that claimed a 3 % improvement from -O3 was likely measuring something else.

## Implications for performance evaluation in general

Stabilizer's lesson is not "use Stabilizer specifically". It's the more general claim:

1. Without layout randomisation, your benchmark's sample size is effectively **one**, regardless of how many times you ran the program.
2. The standard "mean ± CI" and "t-test" reporting depends on having many independent samples.
3. Therefore reports of small (< 5–10 %) performance differences from a single binary — regardless of how many iterations or runs — are **scientifically invalid**.
4. The fix is *either* Stabilizer (randomise layout at runtime), *or* multiple independently-compiled binaries (link-order shuffling, different compile seeds), *or* minimum-based estimation (Chen & Revels — bias is still present but the minimum is less noise-sensitive).

Most orgs do none of the above. The benchmarks reported in most CI systems, most research papers, and most vendor whitepapers are measuring layout noise mixed with real performance differences and reporting the mixture.

## Adversarial reading

- Stabilizer is a research artifact, not production infrastructure. It is a modified LLVM toolchain, specific to x86-64 Linux. Most projects can't realistically adopt it.
- The "only within 7 %" guarantee is for SPEC CPU2006-class codes — C/C++ benchmarks with hot loops. Complex runtimes (JVM, V8, Python) have their own layout-dependent state (inline caches, class hierarchies) that Stabilizer doesn't randomise.
- A weaker but more accessible alternative: *link-order randomisation*. Randomly shuffle object-file order at link time; build and measure N binaries; compute CI across binaries. Captures some of the layout-noise effect without runtime randomisation.
- Stabilizer does not address I/O-bound or concurrency-bound noise. It targets CPU-bound benchmarks where layout is the dominant noise source.

## Relevance to APEX

- APEX's performance regression detection (2x threshold) is coarse enough to survive layout noise. Smaller-grained regression detection would need Stabilizer-class layout control or minimum-based estimation.
- When APEX reports "function f is 5 % slower than baseline", the report should include a note that sub-10 % differences on CPU-bound code without layout randomisation are scientifically weak. This protects users from over-reacting to noise-level reports.

## References

- Curtsinger, C., Berger, E.D. — "STABILIZER: Statistically Sound Performance Evaluation" — ASPLOS 2013.
- Mytkowicz, T., Diwan, A., Hauswirth, M., Sweeney, P. — "Producing Wrong Data Without Doing Anything Obviously Wrong!" — ASPLOS 2009 — `01KNZ4VB6JF8CBPEK1YNFDTDAT`.
- Berger, E. — Stabilizer project page — originally at emeryberger.com/research/stabilizer (now redirects).
- Chen & Revels minimum estimator note — `01KNZ4VB6JEBFDN1QBC4680Y09`.
