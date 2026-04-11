---
id: 01KNZ2ZDMVRHZNDNG62H2HTAQY
title: "BenchmarkTools.jl: Microbenchmarking for Julia (README)"
type: literature
tags: [julia, benchmarking, benchmarktools, statistics, microbenchmark]
links:
  - target: 01KNWE2QACWYZJXRJ8QE78T043
    type: references
  - target: 01KNZ2ZDMC9YQR6MDJ9FZJGSEZ
    type: related
created: 2026-04-12
modified: 2026-04-12
source: "https://github.com/JuliaCI/BenchmarkTools.jl"
---

# BenchmarkTools.jl — Microbenchmarking for Julia

*Source: https://github.com/JuliaCI/BenchmarkTools.jl (README) — fetched 2026-04-12.*
*Maintained under JuliaCI. Original author: Jarrett Revels.*

## One-line summary

BenchmarkTools.jl is the canonical Julia performance-measurement library. It provides `@benchmark` and `@btime` macros that produce **statistically honest** timing data with configurable noise filtering, and treats **minimum execution time** as the primary estimator — a deliberate departure from the mean-based estimators used by Criterion.rs and JMH.

## The `@benchmark` macro

```julia
using BenchmarkTools
@benchmark sort(xs) setup=(xs = rand(1000))
```

Output:

```
BenchmarkTools.Trial: 10000 samples with 1 evaluation.
 Range (min … max):  14.050 μs …  1.091 ms  ┊ GC (min … max): 0.00% … 97.80%
 Time  (median):     16.730 μs              ┊ GC (median):    0.00%
 Time  (mean ± σ):   18.221 μs ± 12.454 μs  ┊ GC (mean ± σ):  1.58% ±  2.22%

   ▃▅██▇▅▄▃▂▁                                                 ▂
  ████████████▇▆▅▄▅▆▆▆▆▆▆▆▆▆▇▆▇▇▇▇▇▇▇▇▆▇▇▇▇▆▇▇▆▇▇▆▆▆▆▆▆▆▆▅▅▅▄ █
  14 μs        Histogram: log(frequency) by time       46.3 μs <

 Memory estimate: 7.94 KiB, allocs estimate: 1.
```

Key output features:
- Explicit **min/median/mean/max**.
- **GC time as a fraction** — you immediately see whether a benchmark was contaminated by a garbage collection pause.
- **Memory estimate and allocation count** — Julia makes this free via the runtime.
- **ASCII histogram** on a log scale — visual outlier check without leaving the REPL.

## Minimum as primary estimator — why?

BenchmarkTools.jl's philosophy, per Revels, is that for a pure function the **minimum observed time** is the cleanest estimator of the "true" time. The argument:

> The minimum is the closest thing to an ideal timing measurement we can get, because noise from the operating system, garbage collection, cache effects, and context switches can only slow a computation down, never speed it up. So if you observe n runs, the minimum is the least-contaminated sample.

This is in contrast to mean-based estimators (Criterion.rs uses the bootstrapped slope of iteration-time vs iteration-count; JMH emphasises mean + confidence interval). The dispute is real and there are tradeoffs:

- **Minimum is robust to one-sided noise.** Operating-system interrupts, cache evictions, thermal throttling — all slow a computation. They don't speed it up.
- **Mean is robust to measurement granularity.** On a CPU with 1 ns timer resolution, a 0.3 ns operation can only be measured by running it N times and dividing — the minimum of one iteration is meaningless.
- **Minimum is optimistic for impure functions.** An allocation that hits the GC once per 1000 iterations won't show in the min but will show in the mean; for benchmarking allocation-sensitive code, the mean is more honest.

BenchmarkTools handles the mean-estimator case by automatically increasing iterations-per-sample until the sample is large enough for the mean to be meaningful. It reports both min and mean, and invites the user to use whichever is appropriate.

## `@btime` — the concise variant

```julia
@btime sort($xs)
```

Prints one line: `  14.050 μs (1 allocation: 7.94 KiB)`. Useful inside REPL sessions where you don't want the histogram.

## Variable interpolation with `$`

A Julia-specific gotcha: without `$`, the macro interpolates the *name* of the variable, which forces a global-variable lookup on every iteration. With `$`, the variable's *value* is baked in at macro expansion time:

```julia
xs = rand(1000)
@btime sort(xs)     # slow — accesses global `xs` per iteration
@btime sort($xs)    # fast — `xs` is a local
```

This is a common source of misleading benchmarks in Julia because the global lookup overhead can be larger than the function being measured.

## Setup and teardown

```julia
@benchmark foo(xs) setup=(xs = copy(base)) teardown=(finalize(xs))
```

The `setup` expression runs **once per sample** (not once per iteration), so it can be arbitrarily expensive without contaminating the measurement. `teardown` similarly runs per sample.

## `BenchmarkGroup` and `judge()`

For tracking performance over time:

```julia
bg = BenchmarkGroup()
bg["sort"]["small"]  = @benchmarkable sort($small)
bg["sort"]["large"]  = @benchmarkable sort($large)
bg["sum"]            = @benchmarkable sum($vec)

results = run(bg)
save("baseline.json", results)
```

And to compare a new run to the saved baseline:

```julia
baseline = BenchmarkTools.load("baseline.json")[1]
new_results = run(bg)
diff = judge(minimum(new_results), minimum(baseline))
```

`judge()` returns `improvement`, `regression`, or `invariant` for each benchmark, with a configurable tolerance (default 5%). This is the Julia ecosystem's standard regression-checking pattern; PkgBenchmark.jl wraps this for CI integration.

## Philosophy differences from Criterion.rs and JMH

| Aspect | BenchmarkTools.jl | Criterion.rs | JMH |
|---|---|---|---|
| Primary estimator | Minimum | Bootstrap slope (mean-like) | Mean with CI |
| GC reporting | Always | No (Rust has no GC) | Explicit forks / warmup to warm GC |
| Warmup | Automatic, JIT-aware | Fewer iterations | Mandatory, explicit warmup param |
| Allocation tracking | Always on | No | With `@BenchmarkMode.Throughput` etc. |
| Report format | ASCII + JSON | HTML + gnuplot + JSON | Text + JSON |

All three converge on the same underlying practices: isolate the code under test from global lookups, run enough iterations to average out noise, warm up the execution engine, and report more than one statistic.

## Relevance to APEX G-46

1. **Julia target support.** When APEX targets a Julia codebase, the natural benchmark harness shape is a `@benchmarkable` expression inside a `BenchmarkGroup`. APEX's harness generator should emit Julia code that slots directly into a PkgBenchmark workflow.
2. **Minimum vs mean is a deliberate choice APEX needs to take.** The G-46 spec should document whether APEX's "empirical complexity" estimator uses min, median, or mean. For complexity estimation (where we want the slope across sizes), the **median** is typically the robust choice, rejecting both outlier min (measurement floor) and outlier max (noise).
3. **GC reporting is load-bearing.** A quadratic in garbage collection (a common bug in Java and Python) is invisible in wall-clock benchmarks unless GC time is separately reported. APEX's Julia and JVM targets should lift this convention from BenchmarkTools.jl.
4. **Setup separation.** APEX's empirical complexity estimator generates inputs of different sizes. The input generation is the `setup`; the measurement is the function call. The BenchmarkTools API is the model for how to separate these in APEX's harness template.
5. **`judge()` is APEX's regression mode in 15 lines.** The BenchmarkGroup + judge pattern is the minimum viable regression-checker. APEX should ship the equivalent for every language it targets.

## References

- BenchmarkTools.jl — [github.com/JuliaCI/BenchmarkTools.jl](https://github.com/JuliaCI/BenchmarkTools.jl)
- Revels — "BenchmarkTools.jl: A Benchmarking Framework for the Julia Language" — 2016 technical report
- PkgBenchmark.jl — [github.com/JuliaCI/PkgBenchmark.jl](https://github.com/JuliaCI/PkgBenchmark.jl)
- Criterion.rs analogue — `01KNZ2ZDMC9YQR6MDJ9FZJGSEZ`
