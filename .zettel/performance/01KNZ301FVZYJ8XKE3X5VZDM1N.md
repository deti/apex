---
id: 01KNZ301FVZYJ8XKE3X5VZDM1N
title: "AFL++ In-Depth Fuzzing: Operational Guide and Performance Tuning"
type: reference
tags: [tool, afl, fuzzing, persistent-mode, timeout, memory-limit, performance, operational]
links:
  - target: 01KNWE2Q9YZBAR140ZX5P36TQ5
    type: references
  - target: 01KNWE2QA0Z52H8VVFAMSA7KGA
    type: extends
  - target: 01KNWGA5GD7A7WXW56682R280K
    type: extends
  - target: 01KNZ301FVGJ4SY6Q3W0XWQXEP
    type: related
  - target: 01KNZ301FVH6M9PHFVP9QETRB6
    type: related
created: 2026-04-12
modified: 2026-04-12
source: "https://aflplus.plus/docs/fuzzing_in_depth/"
license: "Apache-2.0"
---

# AFL++ In-Depth Fuzzing: Operational Guide and Performance Tuning

**Primary reference:** https://aflplus.plus/docs/fuzzing_in_depth/
**Project:** https://github.com/AFLplusplus/AFLplusplus
**License:** Apache-2.0

## What this document is

`fuzzing_in_depth.md` is the AFL++ project's operational guide: how to actually deploy AFL++ for production fuzzing campaigns. It is the single most-cited document on practical modern fuzzing technique and contains many recommendations that directly affect the success rate of any performance-oriented fuzzing effort on top of AFL++.

This note records the guide's recommendations that are directly relevant to APEX G-46: persistent mode, coverage assessment, memory and timeout tuning, multi-core deployment, and crash exploration.

## Persistent mode is mandatory for serious fuzzing

The guide makes an unusually strong claim: "if you do not fuzz a target in persistent mode, then you are just doing it for a hobby and not professionally." Persistent mode reuses a single fuzzer-harness process across many test cases instead of forking a fresh process per input. The reported speedup is 2–20×, and for small-input targets the ratio can be higher still.

Implementation: the target binary is compiled with `afl-clang-fast++ -DAFL_HARNESS`, and the harness function is wrapped in a `while (__AFL_LOOP(N)) { ... }` loop where `N` is the maximum iterations before the process is recycled. The fuzzer replaces stdin between iterations; the harness reads the input and calls the target API.

For APEX-style cost-aware fuzzing the speedup matters more, not less: cost feedback is inherently per-iteration, so a 20× throughput improvement directly translates to 20× more cost samples in the same time budget.

## Coverage assessment: do not trust corpus size

The guide warns that the number of entries in the corpus is not a meaningful measure of fuzzing progress. Instead, use `afl-showmap -C` to assess actual edge coverage. `afl-showmap -C` replays every corpus input under the same instrumentation that the fuzzer uses, produces a tuple count (distinct instrumentation IDs touched), and reports what percentage of the target's total edges has been reached. Two runs with identical corpus size can have radically different coverage — relying on corpus count alone hides this.

For cost-fuzzing, APEX should report both the coverage fraction *and* the maximum observed cost per edge. The combined metric is the right success indicator.

## Memory limits (`-m`)

`-m <MB>` bounds peak RSS per child process. The guide's recommended workflow:

1. Measure the peak RSS of every seed input in a calibration run.
2. Start with a limit 2–4× that baseline.
3. If the fuzzer kills too many inputs as OOM (visible in the UI), raise.
4. If the fuzzer is spending budget on runaway allocations, lower.

For G-46 / memory-consumption fuzzing the calculus inverts: you *want* to catch OOM-ing inputs, so set `-m` aggressively tight (e.g. 64 MB). Every "child killed by -m" becomes a reportable memory-exhaustion finding.

## Timeouts (`-t`)

`-t <ms>` bounds wall time per iteration. The guide recommends aggressive values (like `-t 5`, i.e. 5 ms) on fast and idle machines. Setting it higher wastes budget on pathological inputs; setting it lower than typical execution time generates false positives.

For G-46, aggressive timeouts are again the right default: any input that exceeds the threshold is a reportable slow input, and the fuzzer can then focus its energy on exploring the neighborhood of that slow input. AFL++ classifies timeouts as "hangs" and surfaces them in the UI distinctly from crashes — APEX can consume the `hangs/` directory as its primary output channel.

## Parallel / multi-core deployment

A production fuzzing campaign always runs many workers. The conventional layout:

- One **main** worker (`-M main-$HOSTNAME`) uses the default configuration and is responsible for deterministic mutations and cross-worker coordination.
- Many **secondary** workers (`-S variant-<N>`) run with different mutator mixes, different power schedules, different environment flags, and different compile-time passes (ASAN, CMPLOG, LAF-INTEL, MOpt, etc.).

The guide recommends 32–64 workers as the practical ceiling per machine. Beyond that, synchronization overhead dominates.

Recommended diversity within a worker pool:

- Two workers compiled with CMPLOG instrumentation (catches magic-constant comparisons).
- Two workers with LAF-INTEL instrumentation (breaks multi-byte comparisons into single-byte ones).
- One worker with MOpt mutation scheduling.
- One or more workers with explicit power schedules (`-p fast`, `-p explore`, `-p coe`).
- Workers compiled with different sanitizers (ASAN, MSAN, UBSAN) enabled.

For APEX the same diversity principle applies, but the pool should also include workers running with cost-aware instrumentation (PerfFuzz-style max-per-edge counters, MemLock-style recursion/allocation trackers) and at least one worker running without cost feedback as a control.

## Crash exploration mode (`-C`)

`-C` places AFL++ in crash exploration mode: given a crashing seed, the fuzzer enumerates *other* inputs that still crash the target while exploring distinct code paths post-crash. This is useful for triage: the exploration run produces a diverse set of crashes that may correspond to different root causes.

For APEX the analogous idea is "slow input exploration": given a known-slow seed, find as many other slow inputs as possible. This is not directly supported by `-C` (it retains inputs that crash, not inputs that time out), but is a useful target for future G-46-specific tooling.

## Environment variables for performance tuning

- `AFL_TMPDIR=/dev/shm/afl` — put heavy I/O in tmpfs. Big speedup on spinning disks, meaningful even on NVMe.
- `AFL_DISABLE_TRIM=1` — skip the per-input minimization step on secondary workers. Reduces serialized work and accelerates corpus growth.
- `AFL_FAST_CAL=1` — reduce the number of calibration runs at start-up. Useful when iterating on harness code.
- `AFL_CMPLOG_ONLY_NEW=1` — limit CMPLOG instrumentation to new inputs rather than re-running the full log on existing corpus. Significant speedup on CMPLOG-enabled workers.

## Relevance to APEX G-46

This document is the operational recipe APEX should follow for any AFL++-based performance fuzzer:

1. **Always use persistent mode** and measure cost per iteration inside the `__AFL_LOOP`.
2. **Set `-t` aggressively** to convert slow inputs into reportable hangs.
3. **Set `-m` aggressively** to convert memory-hungry inputs into reportable OOMs.
4. **Run many parallel workers with diverse instrumentation**, including at least one cost-aware (PerfFuzz/MemLock-style) worker.
5. **Use `afl-showmap -C` for real coverage tracking**, and augment with cost-coverage metrics.
6. **Put heavy I/O on tmpfs** and enable the three `AFL_*` flags above.

Following these is the difference between "AFL++ found the bug in minutes" and "AFL++ ran for a day and nothing happened."
