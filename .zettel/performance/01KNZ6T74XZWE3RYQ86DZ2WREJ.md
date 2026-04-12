---
id: 01KNZ6T74XZWE3RYQ86DZ2WREJ
title: CodSpeed — Valgrind-Based Instruction Counting for Stable CI Benchmarks
type: literature
tags: [codspeed, valgrind, cachegrind, callgrind, instruction-count, ci-cd, benchmarking, noise-free, gating]
links:
  - target: 01KNZ6T759YNNAFPCMPAGSTCYV
    type: related
  - target: 01KNZ6WHSTDKHSQERTN6XZC90B
    type: related
  - target: 01KNZ301FV2VB7BHH13YAAG7SA
    type: related
  - target: 01KNWGA5GQ08MFV0XJXX3MTFC3
    type: related
  - target: 01KNZ4VB6J5ZW3JERZNDNGP7GD
    type: related
  - target: 01KNWE2Q9YZBAR140ZX5P36TQ5
    type: related
created: 2026-04-11T21:22:53.213238+00:00
modified: 2026-04-11T21:22:53.213240+00:00
---

*Sources: codspeed.io, docs.codspeed.io, codspeed.io/blog/unrelated-benchmark-regression, github.com/CodSpeedHQ/codspeed, pythonspeed.com/articles/consistent-benchmarking-in-ci/.*

CodSpeed is a commercial CI benchmarking service that sidesteps the entire wall-clock-noise problem by **not measuring wall-clock**. Instead, it runs benchmarks under **Valgrind's Cachegrind/Callgrind** simulator and reports **simulated instruction counts and cache events** as the performance metric. Because Valgrind is a deterministic whole-program simulator, the output is identical across CI runners regardless of hardware variation, background noise, thermal state, or co-tenants. This is a radically different answer to the CI perf noise problem than change-point detection or Stabilizer.

## The insight

Wall-clock measurement noise on shared CI runners is intrinsic — you can't make GitHub Actions bare-metal. The alternatives:
1. **Accept the noise, use change-point detection / robust statistics** (MongoDB, Hunter).
2. **Invest in bare-metal perf runners** (Google Perflab, Mozilla Talos).
3. **Stop measuring wall-clock; measure a deterministic proxy** (CodSpeed, iai-callgrind).

Option 3 is the CodSpeed move. The deterministic proxy is the **simulated instruction count** from Valgrind, augmented with **simulated L1/LL cache access counts** from Cachegrind. These are functions of the binary and its inputs only — they are hardware-independent and noise-free.

## How it works

From CodSpeed's own blog and source code:

> "Internally, CodSpeed invokes callgrind with appropriate cache setup arguments, and disables callgrind at the start with specific instrumentation inside the benchmark library to ensure only benchmark code is measured."

Roughly:

```
valgrind --tool=callgrind --instr-atstart=no --cache-sim=yes -- ./bench
```

- `--tool=callgrind`: use Callgrind, which extends Cachegrind with call-graph information.
- `--instr-atstart=no`: don't instrument setup code; instrumentation is toggled on inside the benchmark via a client request (`CALLGRIND_START_INSTRUMENTATION`).
- `--cache-sim=yes`: enable cache simulation. Reports L1 and LL hit/miss counts.

The benchmark library (Rust, Python, etc.) inserts calls to the Valgrind client request API around the benchmark hot loop, so only the measured code's instructions and cache events are counted. Setup code, allocation, and teardown are excluded.

## What you get in the report

Per benchmark, per run:
- **Instruction count** — the primary metric. How many simulated x86/arm instructions the hot loop executed.
- **L1 access counts** (data reads, writes, instruction fetches).
- **L1 miss counts** (data, instruction).
- **LL (last-level) miss counts**.
- **Optional: branch counts and mispredict counts**.

For regression detection, CodSpeed compares PR runs against the baseline (main branch) and reports instruction-count deltas. Because the metric is deterministic, any delta at all is real — there is no noise floor. A 0.01% regression is detectable and meaningful.

## Why this is interesting

Three properties that are impossible with wall-clock:

1. **Determinism.** The same binary with the same input produces identical counts. No run-to-run variance, no warm-up, no flakiness. CI gate can be a strict inequality (`instructions < baseline * 1.00`) without alert fatigue.
2. **Hardware independence.** A benchmark run on GitHub Actions (shared Xeon of unknown generation), on an M1 Mac, and on a bare-metal workstation all produce the same counts. CI pool heterogeneity disappears as a confounder.
3. **Fine-grained sensitivity.** Instruction count can detect regressions of a single instruction. A wall-clock gate needs a 1–5% threshold to be non-flaky; CodSpeed detects a 0.001% delta reliably.

## Why this is also dangerous

Instruction count is a **proxy**, not the real performance quantity. The proxy is imperfect:

1. **Not all instructions cost the same.** A `div` is 20× slower than an `add`; a cache-missing `mov` is 100× slower than a hitting `mov`. Instruction count misses instruction-level microarchitectural variation. CodSpeed's cache-event counters partially address this by weighing L1 misses and LL misses, but the model is still simplified compared to real CPU behaviour.
2. **Branch prediction is not simulated.** A branch mispredict is ~15–20 cycles on real hardware; Valgrind's Callgrind does not model branch prediction (Cachegrind has an optional branch predictor simulator but it's coarse).
3. **Instruction scheduling, out-of-order, superscalar effects** are entirely absent. Two code sequences with identical instruction counts may run at very different IPC on a real CPU.
4. **SIMD width sensitivity.** Moving from scalar to AVX-512 reduces instruction count dramatically, matching real speedup, but AVX-512 on some CPUs causes frequency downclocking that reduces the benefit. CodSpeed sees the instruction count improvement; the real-world user sees something different.
5. **Memory bandwidth and latency are not modelled.** Workloads dominated by DRAM stalls show tiny cache-miss-count differences in Cachegrind but enormous wall-clock differences on real hardware.
6. **Syscall and kernel time are not measured.** Valgrind intercepts syscalls but the cost of kernel-side work is not counted. An I/O-heavy benchmark's instruction count misses most of what the wall-clock would catch.
7. **Valgrind is slow.** A 1-second benchmark runs for 30–60 seconds under Valgrind. This affects how many iterations you can run in a CI budget.

The upshot: **CodSpeed is excellent for CPU-bound, cache-sensitive, single-threaded code.** It is less reliable for I/O-bound, allocation-heavy, SIMD-dominated, or massively parallel workloads. Teams using it successfully do so with full awareness of the proxy's limits.

## The "unrelated benchmark regression" post

A 2024 CodSpeed blog post ("Why glibc is faster on some GitHub Actions Runners") is a good example of the proxy's subtlety: they observed instruction-count differences between runners that they traced to different glibc versions. Even with Valgrind's determinism, the *inputs* — including dynamically-linked library code — change between runners, and instruction count reflects that. The mitigation is to pin the environment (container image, glibc version) alongside the benchmark itself.

## Related tools

- **iai-callgrind** (github.com/iai-callgrind/iai-callgrind) — the open-source Rust cousin. Does the same thing (Cachegrind/Callgrind-based benchmarking) but as a Cargo benchmark harness without a hosted service. Provides the same determinism without the UI/PR-bot integration.
- **iai** (github.com/bheisler/iai) — predecessor of iai-callgrind, less actively maintained.
- **Gungraun** (github.com/gungraun/gungraun) — another Rust Valgrind-based benchmark harness.

The underlying insight is the same: use Valgrind's deterministic simulation instead of wall-clock.

## Connections

- Mytkowicz et al. 2009 — the noise problem CodSpeed sidesteps.
- Stabilizer (Curtsinger & Berger) — the in-process randomization answer, orthogonal to CodSpeed's "don't measure wall-clock" answer.
- iai-callgrind — open-source counterpart.
- Criterion.rs (01KNWGA5GQ08MFV0XJXX3MTFC3) — wall-clock alternative using bootstrap statistics.

## Reference

codspeed.io, docs.codspeed.io, github.com/CodSpeedHQ. Commercial service, free tier for open-source projects.
