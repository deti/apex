---
id: 01KNZ4VB6JF8CBPEK1YNFDTDAT
title: "Mytkowicz et al. 2009 — Measurement Bias from Uncontrolled Factors"
type: literature
tags: [measurement-bias, mytkowicz, asplos-2009, layout-effect, link-order, environment-variables, benchmarking]
links:
  - target: 01KNZ4VB6JZWDCTRVCP1R5V3GA
    type: extends
  - target: 01KNZ4VB6J5ZW3JERZNDNGP7GD
    type: related
  - target: 01KNZ6FPQ32R61BEN1K1WNZGPX
    type: related
  - target: 01KNZ6FPS6S89303Q1F11G6M1A
    type: related
  - target: 01KNZ6FPSNP9N8SFZMH3B8ZK13
    type: related
  - target: 01KNZ6FPT0VDMJ7J1R5PEBM0DX
    type: related
  - target: 01KNWE2QACWYZJXRJ8QE78T043
    type: related
  - target: 01KNWE2Q9YZBAR140ZX5P36TQ5
    type: related
created: 2026-04-12
modified: 2026-04-12
source: "Mytkowicz, T., Diwan, A., Hauswirth, M., Sweeney, P. — 'Producing Wrong Data Without Doing Anything Obviously Wrong!' — ASPLOS 2009"
---

# Mytkowicz et al. — "Producing Wrong Data Without Doing Anything Obviously Wrong"

*Source: Mytkowicz, T., Diwan, A., Hauswirth, M., Sweeney, P. — "Producing Wrong Data Without Doing Anything Obviously Wrong!" — ASPLOS 2009. DOI 10.1145/1508244.1508275.*

## The thesis

Standard "best-practice" benchmarking procedures — take the minimum of several runs, pin the CPU frequency, quiesce the system, use production-sized workloads — are **not sufficient** to produce reliable performance measurements. There exist *hidden, uncontrolled factors* that have large effects on measured runtime and that most benchmarkers are unaware of, let alone controlling for. Two specific factors they highlight:

1. **UNIX environment variable size.**
2. **Object-file link order.**

Both of these change the binary's memory layout. Neither is considered by researchers or benchmarkers. Both have measurable effects of tens of percent.

## The environment-variable finding

Mytkowicz et al. compile a benchmark program, run it in a shell, measure its runtime. Then they add a single environment variable (e.g. `FOO=aaaaaaaa`) with a specific length, and re-run. Measurement changes.

Scope of the effect: across the SPEC CPU2000 INT benchmarks on an Intel Pentium 4 (x86), varying environment-variable size produced measured speed changes of **up to 33 % in the SPEC benchmarks**, and the paper references other measurements of up to 300 % in related work and their own experiments.

Mechanism: UNIX passes environment variables on the process stack. Changing their size shifts the starting stack-pointer alignment. That shifts local-variable addresses. That shifts cache-set indices and shift DTLB entries. On architectures with direct-mapped or low-associativity caches, a different alignment can push working-set data into aliasing conflicts, dramatically changing miss rates.

The engineer's reaction to this is usually "that's absurd" followed by "but it's specific to x86/L1 and doesn't matter for modern code". Modern x86 caches are more associative, but the effect has been reproduced on every subsequent architecture with alignment-sensitive caches.

## The link-order finding

Same idea at the code side. They compile the same benchmarks several times with the object files linked in randomly-shuffled orders and measure each. Results:

- **Up to 57 % speed change** from link-order alone on some benchmarks.
- The distribution of results is roughly unimodal but with long tails — most link orders cluster, a few are extremely fast or extremely slow.

Mechanism: link order controls the layout of functions in the code segment, which controls instruction-cache behaviour and branch-predictor structure. A hot loop split across two cache lines is 10–30 % slower than the same loop on one line. A function whose address is aliased to another hot function in a direct-mapped i-cache thrashes.

## The statistical implication

If layout effects are this large, what does measuring a single binary mean?

- The standard "N runs, report mean and CI" approach assumes samples are independent draws from a noise distribution. But every run uses the *same* layout. The noise *between* runs captures OS jitter (small), while the *layout effect* is baked in and contributes a systematic shift that cannot be averaged out.
- A claim "optimisation X improved runtime by 4 %" from a single binary is measuring X + (layout noise between X-built and baseline-built binaries). The layout noise alone can be ±40 %. The 4 % claim is statistically empty.

This is the observation Stabilizer (`01KNZ4VB6JZWDCTRVCP1R5V3GA`) was later built to fix by randomising layout at runtime.

## What Mytkowicz et al. recommend

Their paper doesn't provide a complete solution (Stabilizer does later), but it gives practical mitigations:

1. **Measure many binaries, not many runs of one binary.** Re-compile with different link orders, different random seeds, different optimisation settings. Treat each as a sample from the distribution.

2. **Use non-parametric statistical tests** (Mann-Whitney, not t-test) because layout noise is non-Gaussian.

3. **Randomise what you can**: link order, compile order, random padding, ASLR-style address randomisation.

4. **Report distributions, not just means.** A mean hides the bimodality that layout can produce.

5. **Be suspicious of small reported effects.** A 5 % speedup is within layout noise; treat as tentative until proven otherwise.

6. **Disclose procedures fully.** Exact compiler version, exact link commands, exact environment. Reproducibility is the first step toward being able to audit for layout-bias contamination.

## The wider impact

Mytkowicz et al. 2009 is one of the most-cited papers in experimental computer science not because it invented a new technique, but because it undermined the confidence of a large body of prior work. Every performance paper that had reported a 5–10 % improvement from some compiler/runtime/library change was potentially measuring layout noise. The paper did not withdraw any specific prior result but argued that *none* of them could be fully trusted without layout-controlled re-measurement.

The follow-ups — Stabilizer (ASPLOS 2013), the Blackburn et al. JavaBench methodology, the Hoefler & Belli SC 2015 "Scientific Benchmarking" guidelines — all stand on Mytkowicz et al.'s foundation. Modern rigorous benchmarking assumes measurement bias exists by default.

## The name of the paper

"Producing Wrong Data Without Doing Anything Obviously Wrong" captures the essential point: the methodology looks correct at every step. Compile the benchmarks. Run them N times. Take the minimum. Report. Nothing in the procedure is visibly broken. The problem is *invisible* — a hidden dependency on layout that no standard procedure controls for. This is why the paper was necessary: the community needed a concrete demonstration that "looks right" isn't "is right".

## Anti-patterns the paper exposes

1. **Single-binary comparison.** The default. Invalid.
2. **"More runs fixes variance."** No. Layout variance doesn't average out across runs of the same binary.
3. **"We pinned frequency and quiesced the system."** Necessary but not sufficient. Eliminates *some* noise, not the dominant source.
4. **"Our improvement is 5 %, which is statistically significant."** Not significant relative to layout noise, if layout is uncontrolled.

## Adversarial reading

- The specific numbers (33 %, 57 %, 300 %) are worst cases on specific benchmarks on specific hardware. Median effects are smaller, single-digit-percent, but still large enough to contaminate typical "small optimisation" claims.
- Modern hardware (higher cache associativity, larger BTBs, hardware prefetch) has partially reduced *some* layout effects. Not all, and not reliably. The effect is still present on current hardware.
- The paper is not an argument against benchmarking; it is an argument for *rigorous* benchmarking. The conclusion is "measure more carefully", not "don't measure".

## Relevance to APEX

- Any APEX-generated performance finding that reports a < 10 % regression on CPU-bound code must be treated as tentative. APEX cannot randomise layout; it can only warn.
- APEX's complexity-estimation curves are relatively immune because they compare *within* a binary across input sizes, and layout is constant within a binary. The slope of the curve is unaffected by layout.
- APEX's regression detection (2x threshold) is above the layout-noise floor and therefore reliable for its stated purpose.

## References

- Mytkowicz, T., Diwan, A., Hauswirth, M., Sweeney, P. — "Producing Wrong Data Without Doing Anything Obviously Wrong!" — ASPLOS 2009.
- Stabilizer note — `01KNZ4VB6JZWDCTRVCP1R5V3GA`.
- Blackburn, S. M. et al. — "Wake Up and Smell the Coffee: Evaluation Methodology for the 21st Century" — CACM 2008 — earlier warning about benchmark methodology; influenced Mytkowicz.
