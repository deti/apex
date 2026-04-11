---
id: 01KNZ2ZDMC9YQR6MDJ9FZJGSEZ
title: "Criterion.rs: Statistical Microbenchmarking for Rust (README)"
type: literature
tags: [criterion, rust, benchmarking, statistics, bootstrap]
links:
  - target: 01KNWGA5GQ08MFV0XJXX3MTFC3
    type: extends
  - target: 01KNWE2QACWYZJXRJ8QE78T043
    type: references
created: 2026-04-12
modified: 2026-04-12
source: "https://github.com/criterion-rs/criterion.rs"
---

# Criterion.rs — Statistical Microbenchmarking for Rust

*Source: https://github.com/criterion-rs/criterion.rs (README) — fetched 2026-04-12.*
*Original author: Jorge Aparicio. Current maintainers: David Himmelstrup and Berkus Karchebnyi. Previously maintained by Brook Heisler (bheisler/criterion.rs), which is now redirecting to the criterion-rs org.*

## One-line summary

Criterion.rs is a drop-in replacement for Rust's built-in `#[bench]` that runs on **stable** (not just nightly) and produces **statistically rigorous** performance results with confidence intervals, outlier detection, and automatic regression checking against saved baselines.

## Core capabilities

- **Statistical analysis** — bootstrap resampling on the measured iteration times produces confidence intervals for the mean, median, and linear-regression slope. The slope is the primary estimator because it's robust to fixed per-iteration overhead.
- **Automatic regression detection** — each `cargo bench` saves results to `target/criterion/`. Subsequent runs compare against the stored baseline and report whether the change is statistically significant. Output includes a `Change: [-1.23% -0.45% +0.33%]` line with a confidence interval.
- **HTML reports with gnuplot** — the `html_reports` feature produces a `target/criterion/report/index.html` with PDF, violin, and iteration-count plots per benchmark, plus an aggregate summary index.
- **Stable Rust support** — unlike `#[bench]`, Criterion works on stable since day one. This is the dominant reason for its adoption.
- **Throughput measurement** — `throughput(Throughput::Bytes(n))` converts the time per iteration into MB/s or GB/s for bandwidth-oriented benchmarks.
- **Parameterised benchmarks** — `BenchmarkGroup` runs the same benchmark at multiple input sizes and produces a single plot showing the complexity curve.

## Basic usage

```rust
use criterion::{black_box, criterion_group, criterion_main, Criterion};

fn bench_sort(c: &mut Criterion) {
    c.bench_function("sort_10000", |b| {
        let mut data: Vec<i32> = (0..10000).rev().collect();
        b.iter(|| {
            let mut d = data.clone();
            d.sort();
            black_box(d);
        });
    });
}

criterion_group!(benches, bench_sort);
criterion_main!(benches);
```

And in `Cargo.toml`:

```toml
[dev-dependencies]
criterion = { version = "0.5", features = ["html_reports"] }

[[bench]]
name = "my_bench"
harness = false
```

`harness = false` disables Rust's built-in test harness so Criterion can take over.

## Parameterised benchmarks — the G-46-relevant feature

```rust
let mut group = c.benchmark_group("sort");
for size in [10, 100, 1_000, 10_000, 100_000].iter() {
    group.throughput(Throughput::Elements(*size as u64));
    group.bench_with_input(BenchmarkId::from_parameter(size), size, |b, &s| {
        let data: Vec<i32> = (0..s).rev().collect();
        b.iter(|| { let mut d = data.clone(); d.sort(); black_box(d); });
    });
}
group.finish();
```

This is exactly the data shape that APEX's empirical complexity estimator needs — `(size, time)` pairs across a geometric progression of input sizes. Criterion.rs **already** produces a regression plot for these in the HTML report, fitting a curve through the points. APEX can reuse the estimator output directly.

## Statistical details (brief)

See dedicated note `01KNZ2ZDN167V89N6CR6ZNRWYR` for the full breakdown. The headlines:

- **Bootstrap sampling** — 100,000 resamples by default; produces distributions for all estimators.
- **Outlier detection** — Tukey's method with `1.5 × IQR` (mild) and `3 × IQR` (severe) fences; outliers are reported but **not removed**.
- **T-test on bootstrapped samples** — the change-detection test.
- **Noise threshold** — configurable (`noise_threshold`), default 1%; differences smaller than this are reported as "no change".

## Integration gaps

- **No native CWE awareness.** Criterion reports performance changes, not security findings. APEX bridges this: a Criterion regression that crosses a threshold should become an APEX Finding with CWE-400 tagging when appropriate.
- **No worst-case input synthesis.** Criterion runs the inputs you give it. It doesn't search. This is the gap PerfFuzz / SlowFuzz fill.
- **One target at a time.** Criterion benchmarks are hand-written. APEX's generator can automate this — it already synthesises test harnesses for functional tests; extending to benchmark harnesses is a small delta.

## Relevance to APEX G-46

1. **Criterion.rs is APEX's output format for Rust projects.** When APEX generates a performance test for a Rust function, the natural shape is a `BenchmarkGroup` with parameterised sizes. The user can run `cargo bench` directly on APEX's output, and get the HTML report for free.
2. **Reuse the baseline-comparison infrastructure.** Criterion already solves baseline storage, statistical comparison, and regression flagging. APEX's regression mode can delegate to Criterion for Rust projects rather than re-implementing.
3. **Reuse the linear-regression slope estimator** for the empirical complexity estimator. Criterion's parameterised-benchmark output is the raw data for the O(1) / O(n) / O(n²) classifier.
4. **Treat `black_box` as a constraint.** APEX-generated benches must wrap inputs and outputs in `black_box` to prevent LLVM from constant-folding the benchmark to zero. This is a codegen rule APEX's harness writer needs to apply.
5. **Analogous targets for other languages.** Go `testing.B`, Python `pytest-benchmark`, JS `benchmark.js`, Julia `BenchmarkTools.jl`, JMH for Java. APEX should pick the statistically-honest benchmark runner per language; Criterion's design is the reference.

## References

- Criterion.rs — [github.com/criterion-rs/criterion.rs](https://github.com/criterion-rs/criterion.rs)
- Criterion.rs book analysis chapter — `01KNWGA5GQ08MFV0XJXX3MTFC3`
- Deep-dive on stats — `01KNZ2ZDN167V89N6CR6ZNRWYR`
- BenchmarkTools.jl analogue — `01KNZ2ZDMVRHZNDNG62H2HTAQY`
