---
id: 01KNZ301FVAKXYGT00V1AP7N97
title: "Tool: Google Benchmark (C++ microbenchmarks with Big-O inference)"
type: reference
tags: [tool, benchmarking, microbenchmark, bigo, cpp, complexity-inference, statistics]
links:
  - target: 01KNWE2Q9YZBAR140ZX5P36TQ5
    type: references
  - target: 01KNWE2QA2KV1NN8QH32RA5EPA
    type: extends
  - target: 01KNWE2QACWYZJXRJ8QE78T043
    type: extends
  - target: 01KNWEGYBB4AAEFYMR3Y29EZ49
    type: related
  - target: 01KNWGA5GQ08MFV0XJXX3MTFC3
    type: related
  - target: 01KNWGA5H1MNJK8GWPFCZSSW7E
    type: related
created: 2026-04-12
modified: 2026-04-12
source: "https://github.com/google/benchmark"
license: "Apache-2.0"
---

# Tool: Google Benchmark (C++ microbenchmarks with Big-O inference)

**Repository:** https://github.com/google/benchmark
**Language:** C++ (C++11 use, C++17 to build)
**License:** Apache-2.0

## What it is

Google Benchmark is a C++ library for writing microbenchmarks for code snippets. It fills the same niche in the C++ ecosystem that JMH fills for the JVM and Criterion.rs fills for Rust: a framework that takes care of run-count selection, warm-up, statistical reporting, and — crucially for APEX G-46 — *empirical Big-O estimation*.

## Basic shape of a benchmark

A benchmark is a function taking a `benchmark::State&` argument. The body loops over `state` and the library times the loop body:

```cpp
#include <benchmark/benchmark.h>

static void BM_StringCopy(benchmark::State& state) {
  std::string x = "hello";
  for (auto _ : state) {
    std::string copy(x);
  }
}
BENCHMARK(BM_StringCopy);

BENCHMARK_MAIN();
```

## Features relevant to performance test generation

### Asymptotic complexity (Big-O) inference

This is Google Benchmark's headline G-46-relevant feature. By attaching `Complexity()` to a benchmark family, the library runs the family across a sweep of input sizes, measures the per-iteration cost, and fits the measurements to a complexity class.

Workflow:

1. Designate an input size parameter via `state.SetComplexityN(state.range(0))` inside the benchmark body. This tells the library "this benchmark's cost should scale in `range(0)`."
2. Generate a family of benchmarks across sizes using `.Range(1, 1 << 18)` or `.RangeMultiplier(2).Range(1, 1024)`.
3. Chain `.Complexity(benchmark::oN)` (or `oN2`, `oNLogN`, `oNCubed`, `o1`, etc.) to *declare* the expected complexity. The library will report the fitted coefficient and the root-mean-square error for *that* declared class.
4. Alternatively, chain `.Complexity()` without an argument. The library will pick the complexity class that best fits the empirical data and report its name, coefficient, and RMS error.

This is the most accessible tool in the C++ ecosystem for turning "I think this function is O(n log n)" into "Google Benchmark reports oNLogN with RMS 2.3%, coefficient ~0.8" as part of CI output.

### `BENCHMARK_TEMPLATE`

Template benchmarks instantiate a benchmark for each type, useful for comparing data structure implementations (`std::map` vs `std::unordered_map` vs `absl::flat_hash_map`) under the same access pattern.

### Statistical summary

Run multiple repetitions with `--benchmark_repetitions=10`. Google Benchmark automatically computes mean, median, standard deviation, and coefficient of variation. It flags runs where the CV exceeds a threshold, mirroring hyperfine's outlier warning.

### `DoNotOptimize` and `ClobberMemory`

Two intrinsics prevent the optimizer from elim-dead-coding the benchmark body. `benchmark::DoNotOptimize(x)` treats `x` as an observed value, preventing the compiler from propagating its known value. `benchmark::ClobberMemory()` emits a compiler fence. Without these, LLVM/GCC can collapse a tight benchmark loop into nothing and measure zero.

### Manual timing

`state.PauseTiming()` / `state.ResumeTiming()` exclude per-iteration setup and teardown from the measured cost. Useful when the setup is expensive but not what you are trying to measure.

### Range and DenseRange

`.Range(start, end)` generates benchmarks at exponentially spaced sizes (default multiplier 8). `.RangeMultiplier(2).Range(1, 1024)` gives powers of 2. `.DenseRange(start, end, step)` gives uniformly spaced sizes. The exponential default is the right shape for fitting a power law (good log-space coverage).

### CPU scaling & noise reduction

The README and companion `docs/user_guide.md` recommend disabling CPU frequency scaling (`cpupower frequency-set --governor performance`), fixing the process to a single core (`taskset`), disabling hyperthreading's sibling, and running on a quiescent system. Google Benchmark itself emits a `CPU scaling enabled` warning when it detects frequency scaling at runtime — a hint the measurement is unreliable.

## Build system support

- CMake (`find_package(benchmark)`), with CMake targets `benchmark::benchmark` and `benchmark::benchmark_main`.
- Bazel (`@com_google_benchmark//:benchmark`).
- Direct `-lbenchmark`.

## Relevance to APEX G-46

Google Benchmark is the native answer for two G-46 subproblems in C++ targets:

1. **Empirical complexity estimation.** The `Complexity()` attachment is a direct in-library implementation of what Goldsmith et al. (PLDI 2007) call "measuring empirical computational complexity." APEX can use it off the shelf — either by emitting Google Benchmark harness code as part of its performance test output, or by running against an existing benchmark suite and parsing the JSON output for power-law fits.

2. **Regression reporting.** The JSON output (`--benchmark_format=json`) includes per-benchmark mean, median, stddev, coefficient, and RMS error. A downstream regression gate can assert both on absolute cost *and* on fitted complexity class, catching cases where cost stays below the time budget but the scaling exponent silently degraded from O(n log n) to O(n^2).

Two limitations to note:

- Google Benchmark targets C++ only. For Rust, use Criterion.rs (already in the vault); for Java, use JMH (already in the vault); for cross-language workflows, hyperfine is the fallback.
- The `Complexity()` fit is only as meaningful as the range swept. If the range is too narrow, any two adjacent classes will fit equally well. APEX should generate sweeps that span at least three orders of magnitude when feasible.
