---
id: 01KNZ4VB6J5ZW3JERZNDNGP7GD
title: "Performance Regression Gating in CI"
type: concept
tags: [regression, ci, gating, continuous-performance-testing, workflow, baselines]
links:
  - target: 01KNZ4VB6JQZHJVB2EQK6HVXE0
    type: related
  - target: 01KNZ4VB6JY38THW04Z3MMGBZ3
    type: related
  - target: 01KNZ4VB6JEBFDN1QBC4680Y09
    type: related
  - target: 01KNZ67FBPEC378X6KZ79305T0
    type: related
  - target: 01KNZ67FCARB8N2V5KPN8TY1PG
    type: related
  - target: 01KNZ6WHSTDKHSQERTN6XZC90B
    type: related
  - target: 01KNZ6T759YNNAFPCMPAGSTCYV
    type: related
  - target: 01KNZ6T74XZWE3RYQ86DZ2WREJ
    type: related
  - target: 01KNWE2QA5VP0K80TMSABACKWT
    type: related
  - target: 01KNWE2Q9YZBAR140ZX5P36TQ5
    type: related
created: 2026-04-12
modified: 2026-04-12
source: "multiple; experience with LLVM benchmark-server, Firefox Arewefastyet, JMH regression suite in OpenJDK"
---

# Performance Regression Gating in CI

## The problem

Functional bugs fail tests. Performance regressions don't: the new code still produces correct output, just slower. Without an active mechanism to detect regressions, the codebase accumulates them silently. By the time somebody notices ("the service feels slow"), months of small regressions have compounded and bisecting to the cause is impossible.

Regression gating closes this loop: every PR runs a performance benchmark, and a significant slowdown *blocks the merge*. The goal is to detect regressions on the commit that caused them, when the fix is trivial.

## The core loop

```
for each PR:
    1. Check out base commit. Run benchmark N_base times.
    2. Check out PR commit. Run benchmark N_pr times.
    3. Apply statistical test to (baseline samples, PR samples).
    4. If PR is significantly slower → fail PR with report.
    5. If PR is significantly faster → celebrate (and update baseline).
    6. Otherwise → pass.
```

Most of the engineering is in steps 3 and 4.

## Why this is harder than functional CI

1. **Noise.** Every benchmark run has run-to-run variance from OS jitter, CPU frequency scaling, thermal effects, cache/TLB state, other tenants on shared runners. Variance is often 2–5 %; sometimes 10 %. A 1 % regression is undetectable in 5 % noise with a single sample; it requires statistical treatment.

2. **Cost.** Functional tests take seconds; performance tests take minutes to hours. Running the full suite on every PR is infeasible, so most orgs sample.

3. **Flake vs real regression.** A test that fails once in ten runs is a flake that must be removed or ignored. A performance test that fails once in ten runs might be a real 1 % regression drowning in 10 % noise — ignoring it accumulates.

4. **Hardware drift.** The CI runner today is not the CI runner from six months ago. Baselines must be re-established when hardware changes, or the comparison is apples-to-oranges.

5. **Build-order sensitivity.** Running benchmark A before B vs B before A can give different results (thermal, cache). Serialisation order matters.

## Statistical techniques for "is this a real regression?"

The problem is a two-sample hypothesis test: are the baseline and PR sample means drawn from distributions with the same (or close-to-same) means?

Methods in increasing sophistication:

1. **Threshold on mean change.** "Fail if mean(PR) > mean(base) × 1.02." Trivial; works for very stable benchmarks; noise-prone.

2. **Threshold with replication.** Run each side N times, take mean of means, compare to threshold. Lower variance but 2N total runs.

3. **t-test / Welch's t-test.** "Is there a statistically significant difference in means given sample variance?" Standard frequentist test. Works for approximately Gaussian data; performance data is often non-Gaussian (heavy-tailed, bi-modal) so this can over/under-trigger.

4. **Mann-Whitney U (non-parametric).** "Do the two samples come from distributions with different locations?" Doesn't assume Gaussian. More robust to outliers. More conservative (less powerful) when data is actually Gaussian.

5. **Effect-size (Cohen's d, Cliff's delta, A12 VDE).** "How large is the difference, normalised by variance?" Pair with a significance test so you don't flag "statistically significant but economically trivial" differences.

6. **Bootstrap confidence intervals.** Resample with replacement to build a CI on the difference. Works for any metric (including percentiles and other non-linear functionals). Recommended default.

7. **Chen & Revels minimum-based estimator** (see `01KNZ4VB6JEBFDN1QBC4680Y09`) — use the sample minimum instead of mean/median. Robust to transient noise spikes that dominate other estimators. The approach BenchmarkTools.jl uses for Julia's CI.

8. **Stabilizer / layout randomisation** (see `01KNZ4VB6JZWDCTRVCP1R5V3GA`) — at a layer below the statistical test, randomise the factors (code layout, stack placement, heap layout) that cause measurement bias, so results are actually Gaussian and parametric tests apply.

## The false-positive / false-negative trade-off

A strict threshold (e.g. "fail if mean regression > 1 % with p < 0.01") has high false positives — most PRs don't change performance, so most "significant" detections are noise. A lax threshold ("fail if mean regression > 5 %") misses real small regressions that accumulate.

Experience from Firefox's Arewefastyet and from OpenJDK's JMH regression suite:

- At 1 % threshold: every second PR flags a regression, mostly flakes. Engineers disable the gate.
- At 5 % threshold: catches most meaningful changes but misses small-but-persistent drift. Better than no gate; not good enough for a decade-long codebase.
- The right answer is usually *two* gates: a tight one that runs per-PR for fast feedback, and a loose one on the main branch that runs longer and investigates drift.

## The "baseline" problem

What do you compare the PR against?

- **Previous commit on main.** Simplest; measures marginal change. Drift accumulates silently over many commits.
- **Fixed golden baseline (e.g. last release).** Catches drift. Requires re-baselining each release. Good.
- **Rolling median of last N commits.** Smooths out noise. Delayed detection of sudden changes.
- **Historical floor.** Fail if PR is worse than the *best* observed in the last N days. Catches "this PR is the one that started the slow drift" — reintroduces noise sensitivity.

## The detection-vs-bisection split

Two separable jobs:

1. **Detect** — was there a regression? Binary output. Gates the merge.
2. **Attribute** — which commit caused it? Runs after detection, usually with git bisect.

Detection can be fast (per-PR). Attribution can be slow (run the full benchmark on each of N bisect points). Separating the two lets you afford good detection without paying the attribution cost until needed.

## Anti-patterns

1. **Single-run gating.** One run on base, one run on PR, compare. Variance swamps the signal. Fix: replicate and apply a statistical test.

2. **Pass-when-flaky.** Test flakes 30 % of the time; retries pass; considered green. The flake rate is a bug, not a workaround. Fix: remove the source of flake (noise isolation, warmup, randomisation) or the test is not worth running.

3. **"Benchmark failure = mark non-blocking"**. Once non-blocking, nobody looks at it. Regressions accumulate. Fix: failing must be blocking.

4. **No recorded baseline**. "Was yesterday's p99 80 ms or 85 ms?" Nobody knows. Fix: store every run's raw samples, not just aggregate.

5. **Regression detection without root-cause narrative**. Failing the PR with "perf -1.8 %" tells the author nothing. Fix: include flame-graph diff, profile-hot-path-diff, before-and-after histograms, suspected changed code paths.

6. **Running on shared CI infrastructure that adds 10 % variance**. Fix: dedicated perf runners, cores pinned, cpufreq fixed, no background tasks, thermal throttling disabled.

7. **Measuring only one benchmark**. A single test can only detect regressions in its own hot path. Fix: a portfolio — microbenchmarks for unit-level, integration benchmarks for end-to-end, application-level load tests for realistic load.

## Relationship to APEX G-46

The G-46 spec commits APEX to "performance regression detection in CI — compare against a saved baseline, flag regressions exceeding (default) 2x". The 2x threshold is extremely loose — it catches catastrophic regressions (a refactor that made an O(n) into O(n²)) but not the 5 % drip-feed that accumulates over a year. APEX's regression gating belongs at the "catch algorithmic regressions" end of the spectrum. Statistical regression detection at the 1–5 % level should be left to dedicated per-project benchmarking infrastructure (JMH, criterion.rs, BenchmarkTools.jl, Google benchmark), which APEX can integrate with but not replace.

## References

- Chen, J., Revels, J. — "Robust benchmarking in noisy environments" — HPEC 2016 — `01KNZ4VB6JEBFDN1QBC4680Y09`.
- Mytkowicz, T., Diwan, A., Hauswirth, M., Sweeney, P. — "Producing Wrong Data Without Doing Anything Obviously Wrong!" — ASPLOS 2009.
- Curtsinger, C., Berger, E. — "STABILIZER: Statistically Sound Performance Evaluation" — ASPLOS 2013 — `01KNZ4VB6JZWDCTRVCP1R5V3GA`.
- Stuart, L. (Rust) — criterion.rs book, "Analysis" — [bheisler.github.io/criterion.rs/book/analysis.html](https://bheisler.github.io/criterion.rs/book/analysis.html)
- OpenJDK JMH — [openjdk.org/projects/code-tools/jmh](https://openjdk.org/projects/code-tools/jmh/)
- LLVM lnt / test-suite — [llvm.org/docs/lnt](https://llvm.org/docs/lnt/)
- Firefox Arewefastyet and Perfherder.
