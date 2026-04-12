---
id: 01KNZ6WHSTDKHSQERTN6XZC90B
title: Continuous Benchmarking as a Path to Performance Test Generation
type: permanent
tags: [continuous-benchmarking, bencher, codspeed, github-actions, ci, regression-detection, performance-testing, concept]
links:
  - target: 01KNZ6T759YNNAFPCMPAGSTCYV
    type: related
  - target: 01KNZ6T74XZWE3RYQ86DZ2WREJ
    type: related
  - target: 01KNZ6T75SFGWWGZPNF1AXR09K
    type: related
  - target: 01KNZ6T765RV5SX44WKJKCEPVJ
    type: related
  - target: 01KNZ67FBPEC378X6KZ79305T0
    type: related
  - target: 01KNZ67FCARB8N2V5KPN8TY1PG
    type: related
  - target: 01KNZ4VB6J5ZW3JERZNDNGP7GD
    type: related
  - target: 01KNWE2QA5VP0K80TMSABACKWT
    type: related
  - target: 01KNWGA5GQ08MFV0XJXX3MTFC3
    type: related
  - target: 01KNWGA5H1MNJK8GWPFCZSSW7E
    type: related
  - target: 01KNWE2Q9YZBAR140ZX5P36TQ5
    type: related
created: 2026-04-11T21:24:09.658901+00:00
modified: 2026-04-11T21:24:09.658907+00:00
---

# Continuous Benchmarking — The Other Path to Perf Testing

## What continuous benchmarking is

Continuous benchmarking (CB) is the practice of running performance microbenchmarks on every commit or pull request, tracking the results over time, and flagging statistically significant regressions. Unlike load testing, which exercises a running service end-to-end, CB targets individual functions or code paths and relies on microbenchmark harnesses (Criterion.rs, JMH, Google Benchmark, BenchmarkDotNet, hyperfine).

The relevance to this research lane: CB is the *one* performance-testing sub-area where **test generation has largely been solved** — you annotate a function with `#[bench]` or `@Benchmark` and the harness handles the rest. Engineers don't hand-write test schedules, warm-up loops, or statistical oracles. The tool does it.

**This is the template.** Whatever load-test-generation tooling looks like in 10 years should feel like continuous benchmarking feels today: you describe what to measure and the system handles everything else.

## The modern CB tool landscape

### Open-source hubs

- **Bencher (bencher.dev).** Language-agnostic CI tool that ingests benchmark output from any harness and stores results in a time-series database. Emits PR comments and integrates with GitHub status checks. The most mature open-source "where do my results live" layer.
- **Codspeed (codspeed.io).** Commercial but with a generous free tier. Uses simulation (not wall-clock timing) to reduce noise and make CI-based benchmarks reliable. Specifically addresses the "CI machine is noisy" problem that torpedoes most naive CB setups.
- **NYRKiö.** Self-hosted CB service with change-point detection. Less commonly seen but technically sophisticated.
- **ASV (airspeed velocity).** Python-specific; mature within the scientific-Python community.

### Harness-level tools (already in this vault)

- Criterion.rs — statistical Rust benchmarks with bootstrap analysis, outlier detection, Tukey fences.
- JMH — the gold standard for Java microbenchmarks, immune to most JIT traps.
- Google Benchmark — C++ with Big-O inference.
- hyperfine — command-line hyperfine benchmarking.
- BenchmarkTools.jl — Julia.

## What CB has that load testing lacks

1. **Statistical rigour built in.** Criterion.rs and JMH ship with proper hypothesis testing. They know about warm-up, outlier removal, confidence intervals. Macrobenchmark tools don't.
2. **Zero test-generation burden.** Annotate, run, done. The test content *is* the harness call.
3. **Regression oracles that work.** Compare current run to historical distribution of the same benchmark and flag outliers. Real regression detection, not threshold gates.
4. **Minimal per-benchmark overhead.** A benchmark is a few lines of code. Nobody writes load-test DSL just to measure one function.
5. **CI-native.** Every major CB tool is designed for GitHub Actions / GitLab CI from day one.

## What CB can't do that load testing is needed for

1. **Integration path exercise.** A microbenchmark measures one function in isolation. It doesn't catch perf bugs that emerge from the interaction of many components.
2. **Load-dependent behaviour.** Concurrency issues, lock contention, GC pressure under realistic allocation patterns — these only show up at scale.
3. **Database and network effects.** Microbenchmarks don't hit a real database. Real perf bugs often live at the DB boundary.
4. **User-facing SLOs.** p99 user-facing latency is a system-level property, not a function-level one.

## Why continuous load testing hasn't followed the same pattern

Load testing conceptually *should* work the same way:

1. Declare a workload spec.
2. CI runs it on every commit.
3. Tool stores the results, compares to history, flags regressions.

The three reasons this hasn't happened:

1. **Infrastructure cost.** Running a 100-VU load test on every commit is expensive. Microbenchmarks run for seconds on the free GitHub Actions tier. Load tests don't fit.
2. **Workload spec authoring is manual.** Declaring a workload today requires engineering effort that Criterion.rs-style microbenchmarks don't need. Until workload specs are generated automatically (see the top-5 gaps note), load-test-in-CI is too expensive in engineering time.
3. **Noise floor is higher.** Macrobenchmark results on a shared CI runner are dominated by network jitter, shared-tenant noise, and test-harness overhead. The signal-to-noise ratio is much worse than a tight Criterion.rs loop.

## The intersection

An interesting emerging pattern: run *some* load tests in CI — not full production-scale, but enough to catch most regressions. Tools like bencher support this by letting you track macro metrics (request latency p95) as if they were microbenchmark results. The value is catching obvious regressions without waiting for production canaries.

Codspeed's simulation-based approach is an attempt to fix the noise problem by *not* timing wall-clock at all — they count CPU instructions via a simulator, which is deterministic. This eliminates noise but at the cost of missing real-world effects (branch prediction, cache, memory latency). It's a trade-off that works for some projects (especially Rust/Go libraries) but not for macro systems.

## Why this matters for test generation

CB is the proof that perf testing with good tooling can be *as frictionless as unit testing*. Load testing is not there yet because the test-generation step is manual. Every gap in the top-5 list points at fixing that: generate the workload from logs, generate the oracle from the SLO, generate the script from the spec. Once those gaps are closed, continuous load testing becomes possible, and the CB experience becomes the template.

## A concrete recommendation

For teams considering adopting CB today:

- Start with Bencher or Codspeed for microbenchmarks.
- Track a small set of macro metrics (endpoint latency p95, total throughput) in the same system by piping k6 JSON output into the CB store.
- Use the same regression-detection logic for both. This gives a unified view of perf health.

This is the most pragmatic way to bridge the CB / load-testing gap while waiting for proper workload-generation tooling to exist.

## Citations

- Bencher: https://bencher.dev/
- Codspeed: https://codspeed.io/
- NYRKiö: https://nyrkio.com/
- airspeed velocity: https://asv.readthedocs.io/
- Criterion.rs analysis (already in vault): https://bheisler.github.io/criterion.rs/book/analysis.html
- JMH (already in vault): https://openjdk.org/projects/code-tools/jmh/
- Google Benchmark (already in vault): https://github.com/google/benchmark
- hyperfine (already in vault): https://github.com/sharkdp/hyperfine