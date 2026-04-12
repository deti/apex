---
id: 01KNZ4VB6JP254YSHY7N9PX4HQ
title: "Warm-Up and Steady-State Detection (JMH-style)"
type: concept
tags: [warmup, steady-state, jmh, jit, benchmarking, microbenchmark]
links:
  - target: 01KNZ4VB6JEBFDN1QBC4680Y09
    type: related
  - target: 01KNZ4VB6JR9DSJA90V0WAW1TF
    type: related
  - target: 01KNWGA5H1MNJK8GWPFCZSSW7E
    type: related
  - target: 01KNWE2QACWYZJXRJ8QE78T043
    type: related
  - target: 01KNWE2Q9YZBAR140ZX5P36TQ5
    type: related
created: 2026-04-12
modified: 2026-04-12
source: "OpenJDK JMH source & docs; Georges, Buytaert, Eeckhout OOPSLA 2007"
---

# Warm-Up and Steady-State Detection

## Why it's a thing

A benchmark that starts measuring immediately measures the wrong thing. Running systems pass through several distinct phases:

1. **Cold start**: caches empty, JIT not compiled (for managed runtimes), branch predictor cold, page tables not resident.
2. **Warm-up / compilation**: JIT (HotSpot, V8, LuaJIT, PyPy, .NET ReadyToRun) observes the hot methods and compiles them. Caches fill. TLB entries materialise.
3. **Transient tuning**: JIT may recompile at a higher tier (C1 → C2 in HotSpot; Maglev → TurboFan in V8). GC generations settle.
4. **Steady state**: hot code is at its optimised tier, caches at the working-set, GC in rhythm with allocation rate. Measurements stabilise.
5. **Drift** (in long runs): fragmentation, leaks, GC pressure, thermal throttling. Measurements degrade over time. This is what soak tests look for.

The *only* window where the measurement corresponds to "what the code does when it's doing its job" is phase 4. Phases 1–3 are *in transit*; phase 5 is *breaking*. A benchmark tool must identify phase 4 and take samples only there.

## JMH — the canonical approach

**OpenJDK JMH** (Java Microbenchmark Harness) is the reference design for warm-up handling. Its approach, which many other frameworks copied:

### Forks

JMH launches a *new JVM process* per "fork" (default 5). This is crucial because JIT decisions depend on profile data that's process-local — a second run in the same JVM has warmer caches and more compiled code than a fresh JVM. By running multiple forks, JMH samples across fresh-JVM variance, capturing the full span of JIT states. Georges et al. OOPSLA 2007 showed that without forks, between-run variance is systematically underestimated.

### Warm-up iterations

Per fork, JMH runs a configurable number of **warm-up iterations** before measurement. Defaults: 5 warm-up iterations × 10 seconds each = 50 seconds of warm-up. During warm-up:

- Methods are invoked enough times for HotSpot to profile and compile them (C1 → C2 tier-up happens after ~10 000 invocations on default HotSpot).
- Caches fill.
- Any one-time initialisation (lazy class loading, static initialisers) completes.

Warm-up measurements are *recorded but discarded*. The tool doesn't just stop-and-restart; it records and displays so you can visually confirm the benchmark reached steady state.

### Measurement iterations

After warm-up, JMH runs N measurement iterations (default 5) × 10 seconds each. Each iteration is a self-contained measurement. JMH computes per-iteration mean, then aggregates across iterations to get a per-fork mean, then aggregates across forks to get the final estimate with CI.

### JMH's steady-state detection — "convergence"

JMH's core trust is that *5 × 10-second warm-up* is enough for most Java benchmarks to reach steady state. It doesn't use an automatic change-point detector; the user is expected to verify convergence from the per-iteration output. A diverging trend across warm-up iterations (e.g. iteration 1: 100 ms, iteration 2: 80 ms, iteration 3: 60 ms, iteration 4: 55 ms, iteration 5: 54 ms) means "extend warm-up" or "your benchmark hasn't converged".

## Warm-up in other ecosystems

- **Criterion.rs (Rust)**: runs warm-up for a target duration (default 3 s), then measurement, then statistical analysis. No JIT to warm, but it still benefits from cache and branch-predictor warm-up. Uses bootstrap CIs on steady-state samples.
- **Google Benchmark (C++)**: auto-scales iteration count to fit a target time. No explicit warm-up phase; assumes compiled code has no warm-up.
- **Go testing.B**: similar — auto-scales iterations until the benchmark's total time is ≥ benchtime. Minimal warm-up logic.
- **BenchmarkTools.jl (Julia)**: includes warm-up runs. Julia has a JIT (precompiling methods lazily), so warm-up matters.
- **pytest-benchmark (Python)**: "calibration" runs that estimate iteration count. CPython has no JIT, so warm-up is weaker; PyPy benefits from explicit warm-up.
- **k6 load test**: no automatic steady-state detection. User configures "stages" manually — typically a ramp-up phase and a steady phase.

## What "warm" actually means, per runtime

### HotSpot JVM

- JIT compilation tiers:
  - Interpreter (initial).
  - C1 (tier 3) — fast compiler, simple optimisations.
  - C2 (tier 4) — slow compiler, aggressive optimisations (inlining, escape analysis, loop unrolling).
- Transitions happen at invocation count thresholds (`CompileThreshold=10000` default).
- A method may also *deoptimise* back to the interpreter if a speculative optimisation's assumption is invalidated, then recompile.
- Steady state typically reached after 10^4 to 10^6 iterations of the hot method. For tight loops, < 1 s. For cold paths, much longer.

### V8

- Ignition interpreter → Maglev (fast compiler) → TurboFan (full compiler).
- Feedback-driven; "optimized call sites" trigger reoptimisation when assumptions break.
- Warm-up typically 100 ms to 5 s depending on code shape.

### .NET / CoreCLR

- ReadyToRun (precompiled) + TieredJIT (Tier 0 fast → Tier 1 optimised).
- Warm-up shorter than HotSpot because ReadyToRun reduces first-call cost.

### Native (C/C++/Rust/Go)

- No JIT. "Warm-up" means cache warming, branch-predictor training, page-table population.
- Typically 10–100 ms of warm-up suffices. Less for small benchmarks.

### Python (CPython)

- No JIT. "Warm-up" is minimal — the module is loaded, caches fill.
- 100 ms is usually plenty.

### PyPy / LuaJIT

- Trace-based JIT. Warm-up matters (traces form and compile).
- Seconds of warm-up often needed.

## Anti-patterns

1. **No warm-up.** Measured numbers include cold-start. Reports slower-than-real performance. Classic beginner mistake with JMH in the pre-JMH era.

2. **Warm-up in the same JVM as measurement but mixing benchmark invocations.** Method A warms up, method B is measured, but method A's profile affects method B's JIT decisions. Fix: separate forks.

3. **Too-short warm-up.** Benchmark hasn't reached steady state; the numbers are on the transient slope. Fix: run long enough to visibly converge.

4. **Too-long warm-up.** Benchmark crosses into phase 5 (drift) during "warm-up", making the "steady state" window actually be the declining phase. Rare but possible on memory-limited machines. Fix: stop warm-up when the metric plateaus.

5. **Assuming "fresh run" has the same characteristics as "production".** Production has been running for days; local benchmark is at 30 s. Different JIT state, different cache contents. Add an extremely long warm-up, or (better) gather production profile data and use it.

6. **Warm-up for microbenchmarks, none for macrobenchmarks.** Load tests that measure from t=0 with no steady-state window. The first 30 s of a load test is cold and should not contribute to the SLO check. Fix: explicit ramp-up + measurement phase separation.

## Steady-state detection heuristics

If you want to *automatically* detect when the steady state begins (rather than trust a fixed warm-up duration):

- **Running mean stability**: sliding window mean over the last N iterations; declare steady state when window-over-window change < ε.
- **CUSUM change-point**: classical control-chart technique for detecting level shifts.
- **Variance stability**: running variance also needs to stabilise, not just mean.
- **Visual inspection**: the one most practitioners actually use — look at the per-iteration plot, identify the knee, draw a vertical line. Crude but effective.

## Relevance to APEX

- APEX's performance-test generation (spec item 4: "resource profiling during ordinary test execution") collects data on functional test runs. These runs almost certainly *do not* reach steady state (they're short and synthetic). APEX should either:
  - Accept the cold-start bias and report numbers as "cold path" explicitly.
  - Do multiple warm-up iterations before taking the measured sample, per-function.
  - Offer both modes and label outputs.
- APEX's complexity estimation walks a function through input sizes 10, 100, ..., 10⁵. Each size gets its own cold start if not handled. Warm-up per size is essential for clean fits; otherwise the curve is polluted by warm-up noise that drops over time.

## References

- OpenJDK JMH — [openjdk.org/projects/code-tools/jmh](https://openjdk.org/projects/code-tools/jmh/)
- Georges, A., Buytaert, D., Eeckhout, L. — "Statistically Rigorous Java Performance Evaluation" — OOPSLA 2007 — the foundational argument for forks.
- Criterion.rs book, "Analysis" — [bheisler.github.io/criterion.rs/book/analysis.html](https://bheisler.github.io/criterion.rs/book/analysis.html)
- BenchmarkTools.jl — `01KNZ4VB6JEBFDN1QBC4680Y09`.
