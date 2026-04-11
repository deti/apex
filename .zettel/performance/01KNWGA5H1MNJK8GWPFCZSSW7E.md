---
id: 01KNWGA5H1MNJK8GWPFCZSSW7E
title: "Tool: JMH (Java Microbenchmark Harness)"
type: literature
tags: [tool, jmh, java, jvm, benchmarking, microbenchmark, jit-warmup]
links:
  - target: 01KNWE2QA5VP0K80TMSABACKWT
    type: related
  - target: 01KNWE2QACWYZJXRJ8QE78T043
    type: references
  - target: 01KNWE2QA6QKE8Z152WX8D9XYB
    type: related
created: 2026-04-10
modified: 2026-04-10
source: "https://openjdk.org/projects/code-tools/jmh/"
---

# JMH — Java Microbenchmark Harness

*Source: https://openjdk.org/projects/code-tools/jmh/ — fetched 2026-04-10. The project homepage returned 404 during this session; the content below draws on the G-46 spec reference plus widely-published JMH documentation.*

## What JMH Is

JMH is the official Java / JVM microbenchmark harness, developed by OpenJDK and maintained by Aleksey Shipilëv. It is the reference tool for measuring the performance of small code fragments on the JVM — a task that is notoriously subtle because the JVM's JIT compiler, escape analysis, dead-code elimination, and garbage collector can all silently invalidate naive timing measurements.

## Why the JVM Needs a Specialised Harness

A `System.currentTimeMillis()` bracket around a loop is a nearly useless microbenchmark on a JVM because:

- **Dead-code elimination** — if the loop body produces a result that is never used, HotSpot will eliminate it entirely. You are then timing an empty loop.
- **Constant folding** — if the inputs are compile-time constants, HotSpot may precompute the result.
- **Loop unrolling and peeling** — HotSpot may transform the loop structure in ways that change per-iteration cost.
- **On-stack replacement (OSR)** — a cold loop compiled partway through execution runs at a different speed than a loop entered already-compiled.
- **JIT warmup** — the first ~10K invocations of a method run interpreted, then C1, then C2. Steady-state performance is reached only after hundreds of thousands of iterations.
- **Inlining** — JMH methods may or may not be inlined depending on bytecode size and call depth.
- **GC pauses** — a single G1 or ZGC pause during measurement is a massive outlier.

JMH defends against all of these through annotation-driven control and a generated benchmark runner.

## Core Annotations

- `@Benchmark` — marks a method as a benchmark target. JMH generates a runner that invokes it.
- `@BenchmarkMode` — `Throughput` (ops/sec), `AverageTime` (time/op), `SampleTime` (percentiles), `SingleShotTime` (cold start).
- `@State(Scope.Benchmark | Thread | Group)` — holds mutable state outside the timed method to prevent constant folding.
- `@Warmup(iterations, time)` — default 5 iterations × 10 seconds each to reach steady state.
- `@Measurement(iterations, time)` — default 5 iterations × 10 seconds each for the actual measurement.
- `@Fork(value, warmups)` — number of forked JVMs (default 5) to defeat profile-guided re-optimisation across runs.
- `@OutputTimeUnit` — the unit reported.
- `Blackhole.consume(...)` — a JMH-provided sink that HotSpot cannot dead-code-eliminate. Essential for ensuring results are "used".

## How JMH Handles JIT Warmup

JMH runs benchmarks inside dedicated forked JVMs with multiple warmup iterations per fork. The warmup loops drive the JIT to steady state before measurement starts. Each fork is a fresh JVM, so profile-guided optimisations from one run don't leak into the next — reducing variance.

## Statistics

JMH reports mean, median, confidence intervals, and full percentile distributions for `SampleTime` mode. The default statistical mode uses bootstrapped confidence intervals similar to Criterion.rs. Outliers are reported but not dropped.

## Relevance to APEX G-46

JMH represents the best-in-class reference for managed-language microbenchmarking, but the G-46 spec explicitly marks **"JIT warmup and steady-state analysis for managed-language runtimes"** as *out of scope*. This is a pragmatic call — JMH's 5 × 10s warmup + 5 × 10s measurement means each benchmark takes at least 100 seconds, which is incompatible with the spec's 5-minute fuzzing budget.

What APEX should borrow from JMH:

1. **Forked-JVM isolation** — for Java targets, spawn a dedicated JVM per run to defeat cross-run profile pollution.
2. **Dead-code-elimination defences** — require a "sink" for fuzzer outputs (e.g. write to a file, print, checksum) to prevent the JIT eliminating the work being measured.
3. **Cold-start mode** — report cold-start timings as a separate metric from steady-state, so APEX findings on managed languages are at least honest about which regime they measured.

In the longer term, APEX + JMH integration (APEX drives input generation, JMH runs the measurement) is the right architecture for first-class JVM performance findings.
