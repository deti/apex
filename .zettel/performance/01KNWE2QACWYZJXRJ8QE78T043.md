---
id: 01KNWE2QACWYZJXRJ8QE78T043
title: Resource Measurement Methodology and Noise Mitigation
type: concept
tags: [measurement, benchmarking, statistics, noise, criterion]
links:
  - target: 01KNWE2Q9YZBAR140ZX5P36TQ5
    type: extends
  - target: 01KNWE2QA2KV1NN8QH32RA5EPA
    type: related
  - target: 01KNWE2QA5VP0K80TMSABACKWT
    type: related
  - target: 01KNWGA5GQ08MFV0XJXX3MTFC3
    type: references
  - target: 01KNWGA5GYY1GE3G957BDNKX3D
    type: references
created: 2026-04-10
modified: 2026-04-10
---

# Resource Measurement Methodology and Noise Mitigation

Performance measurement is adversarial. Wall-clock time, CPU time, memory usage, and allocation counts are all affected by system load, garbage collection, JIT compilation, cache warmth, TLB pressure, scheduling, frequency scaling, interrupts, and DVFS. A naive `time.time()` bracket around a function call produces numbers that vary by 2–10× across runs. APEX's G-46 requirement — detect a 2× regression with <10% false positives — is only achievable with disciplined measurement. This note collects the techniques.

## Sources of noise and how to neutralise them

### 1. Process-level variation

- **Context switches** — running on a shared CPU, another process can evict your working set from L1/L2.
- **Fix**: `taskset -c 3 ./bench` to pin to a single core; `isolcpus=3` kernel parameter for dedicated cores in a CI runner.

### 2. Frequency scaling and turbo

- Modern CPUs change frequency based on thermal, workload, and DVFS state. A benchmark at the start of a run can be 10% faster than at the end.
- **Fix**: `cpupower frequency-set -g performance`, disable Turbo Boost (`/sys/devices/system/cpu/intel_pstate/no_turbo=1`), run a warm-up loop to stabilise thermal state.

### 3. Address Space Layout Randomisation

- ASLR changes absolute addresses of the stack, heap, and libraries between runs. Cache line placement changes; 5–15% variation possible.
- **Fix**: `setarch -R ./bench` (Linux) to disable ASLR for the benchmark process. Or average across many ASLR positions.

### 4. Garbage collection

- A major GC pause during measurement is a massive outlier.
- **Fix**: force a GC before the timed block (`gc.collect()` in Python, `System.gc()` hints in Java, `runtime.GC()` in Go); in managed runtimes, prefer `-XX:+ExplicitGCInvokesConcurrent` settings; or amortise by running many iterations and taking the median (outlier-robust).

### 5. JIT warmup

- JVM, V8, LuaJIT, PyPy, .NET — all compile bytecode to native code after a function is "hot". Cold runs are 10–100× slower than steady-state. Measuring cold and steady-state are both valid but different.
- **Fix**: run a warmup loop (JMH does this by default — 5 iterations × 10 seconds), then measure separately. Report warmup time and steady-state time as two separate metrics.
- **G-46 scope note**: the spec explicitly marks "JIT warmup and steady-state analysis for managed-language runtimes" as *out of scope*. APEX targets cold-start behaviour for managed languages unless the user opts in.

### 6. System background tasks

- cron, indexer, systemd services, apt updates, kernel memory compaction.
- **Fix**: run benchmarks on a dedicated runner with services disabled. On macOS, disable Spotlight indexing on the benchmark directory.

### 7. Thermal throttling

- Sustained load on a laptop or small cloud instance will throttle. A 10-minute fuzz run can end 30% slower than it started because the CPU downclocked.
- **Fix**: cooling, monitoring (`stress-ng -c 1` warmup to stabilise temperature), abort runs when temperature crosses a threshold.

### 8. Memory allocator variability

- jemalloc, glibc malloc, mimalloc, tcmalloc differ in fragmentation and lock contention characteristics. The same allocation pattern can be 2× apart on different allocators.
- **Fix**: pin the allocator (`LD_PRELOAD=libjemalloc.so.2`), report which one was used.

### 9. Cache warmth

- The first run of a benchmark touches cold pages; the second run is cached. Microbenchmarks often report the warm case; production often sees cold.
- **Fix**: explicitly flush caches (`echo 3 > /proc/sys/vm/drop_caches` on Linux) before each run, or measure both warm and cold explicitly.

## Statistical techniques

### Central tendency: median, not mean

The arithmetic mean is sensitive to outliers; a single GC pause or interrupt can double the reported average. **The median** is the standard statistic for benchmark time. Better still, report a five-number summary: `(min, p25, median, p75, max)`.

### Confidence intervals, not point estimates

Criterion.rs and BenchmarkTools.jl both report **95% confidence intervals** obtained by bootstrapping:

1. Collect N raw samples (default: 100).
2. Resample with replacement B times (default: 100,000).
3. For each resample, compute the statistic of interest.
4. The 2.5th and 97.5th percentile of the resampled distribution form the 95% CI.

Regression detection then compares the **CI of the candidate vs. the CI of the baseline**, not just point estimates.

### Outlier detection and filtering

Criterion.rs tags outliers as low-mild / high-mild / low-severe / high-severe using Tukey fences (1.5·IQR and 3·IQR). Severe outliers (>3·IQR) are discarded before fitting; mild outliers (1.5–3·IQR) are kept but flagged in the report.

### Linear regression for per-iteration cost

If you measure `k, 2k, 4k, 8k, ...` iterations of the same inner loop and plot (iterations, total time), the slope is the per-iteration cost and the intercept is the fixed overhead. **Linear regression** separates them robustly. Criterion.rs calls this the "linear regression method"; BenchmarkTools.jl uses it too.

### Change-point detection for regression identification

Given a time series of benchmark results across commits, **change-point detection** algorithms (PELT, binary segmentation) find commits where the distribution shifted, rather than comparing pairs of commits in isolation. Mongo's `signal-processing-algorithms` and Netflix's `kats` both implement this.

### Welch's t-test / Mann–Whitney U

When comparing two samples (baseline vs candidate), a **Welch's t-test** handles unequal variances. For non-normal distributions (common in benchmark data, which is heavy-tailed), the **Mann–Whitney U test** is more robust. Criterion.rs uses Welch's; JMH uses bootstrap-CIs.

## The "benchmark harness" checklist

A credible benchmark harness does all of these:

- [ ] Pins the process to a single core (`taskset -c N`)
- [ ] Disables ASLR (`setarch -R`)
- [ ] Runs warmup iterations before measurement
- [ ] Collects ≥30 samples per data point
- [ ] Reports median and 95% CI, not mean
- [ ] Filters outliers (1.5·IQR or 3·IQR)
- [ ] Uses hardware performance counters where available (deterministic)
- [ ] Reports instruction count alongside wall-clock (deterministic fallback)
- [ ] Reports the host: CPU model, frequency, memory, kernel, allocator
- [ ] Reports the run environment: thermal state, load average, other processes
- [ ] Uses paired measurement when comparing A vs B (run both in alternation, not all A then all B)

## What this means for APEX

APEX G-46 needs two measurement modes:

1. **Fuzzing inner loop** — prioritise *deterministic* signals (instruction count, basic-block execution count). Speed matters more than absolute accuracy; the goal is to *rank* candidate inputs, not to report absolute numbers. Throw away wall-clock; keep hardware counters.

2. **Final verification & reporting** — for the champion input and for SLO assertions, use the full harness: warmups, pinned CPU, 30+ iterations, median + CI. Wall-clock is the number users care about.

The CLI should expose the host environment in every report so that numbers are reproducible: "measured on AMD Ryzen 9 7950X, 4.5 GHz fixed, isolcpus=31, jemalloc 5.3, Linux 6.1, 32GB DDR5-5200".

## References

- Georges, Buytaert, Eeckhout — "Statistically Rigorous Java Performance Evaluation" — OOPSLA 2007 — the foundational paper on managed-language benchmark methodology
- Chen, Revels — "BenchmarkTools.jl" — [github.com/JuliaCI/BenchmarkTools.jl](https://github.com/JuliaCI/BenchmarkTools.jl)
- Criterion.rs — [github.com/bheisler/criterion.rs](https://github.com/bheisler/criterion.rs) — statistical benchmarking for Rust
- JMH — OpenJDK Java Microbenchmark Harness — [openjdk.org/projects/code-tools/jmh](https://openjdk.org/projects/code-tools/jmh/)
- Gregg — "Systems Performance" 2nd ed. — Pearson 2020
- Mytkowicz, Diwan, Hauswirth, Sweeney — "Producing Wrong Data Without Doing Anything Obviously Wrong!" — ASPLOS 2009 — demonstrates how environmental changes (link order, env vars) can swing benchmark results by more than a compiler optimisation
