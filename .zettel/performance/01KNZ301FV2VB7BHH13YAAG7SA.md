---
id: 01KNZ301FV2VB7BHH13YAAG7SA
title: "Tool: Valgrind Callgrind (call-graph profiler with cache simulation)"
type: reference
tags: [tool, profiler, callgraph, valgrind, instruction-count, kcachegrind, methodology]
links:
  - target: 01KNWE2Q9YZBAR140ZX5P36TQ5
    type: references
  - target: 01KNWE2QACWYZJXRJ8QE78T043
    type: extends
  - target: 01KNWE2QA2KV1NN8QH32RA5EPA
    type: related
  - target: 01KNWEGYBB4AAEFYMR3Y29EZ49
    type: related
created: 2026-04-12
modified: 2026-04-12
source: "https://valgrind.org/docs/manual/cl-manual.html"
license: "GPL-2.0-or-later"
---

# Tool: Valgrind Callgrind (call-graph profiler with cache simulation)

**Manual:** https://valgrind.org/docs/manual/cl-manual.html
**Project:** https://valgrind.org/
**License:** GPL-2.0-or-later

## What it is

Callgrind is a profiling tool built on top of the Valgrind dynamic binary instrumentation framework. It records the call history among functions in a program's run as a **call-graph**, counting instructions executed within each function and propagating those counts back through the call chain to produce both exclusive and inclusive costs.

Unlike sampling profilers (`perf record`, `gprof`, Instruments) which estimate cost by interrupting the process at fixed intervals, Callgrind is a **deterministic simulation**: it counts every executed instruction. The cost of this fidelity is a ~20–100× slowdown, which rules it out for production profiling but makes it exceptionally well-suited for APEX-style worst-case analysis where determinism matters more than speed.

## How it works

Callgrind runs your binary on Valgrind's synthetic CPU. As each basic block executes, Valgrind hands it to Callgrind, which:

1. Attributes the block's instructions to the currently active function.
2. Tracks call and return events to maintain a per-thread call stack.
3. Increments call-graph edges whenever one function invokes another.
4. Optionally simulates the L1/L2/LL cache hierarchy and the branch predictor to produce miss counts.

At program exit (or on demand), Callgrind writes a machine-readable profile file `callgrind.out.<pid>` containing per-function, per-source-line, per-instruction, and per-edge cost counters.

## Event counters

By default Callgrind collects:

- **Ir** — instructions read (total instructions executed).

With `--cache-sim=yes`, it adds:

- **I1mr / ILmr** — L1 and last-level instruction cache misses.
- **D1mr / DLmr** — L1 and last-level data read misses.
- **D1mw / DLmw** — L1 and last-level data write misses.

With `--branch-sim=yes`:

- **Bc / Bcm** — conditional branches and mispredictions.
- **Bi / Bim** — indirect branches and mispredictions.

These counters are simulated, not read from hardware performance counters. The simulation is deterministic, so repeated runs of the same input produce bit-identical counts. This determinism is enormously valuable for regression testing: unlike `perf stat` (whose cycle counts vary with system load, thermal state, and frequency scaling), Callgrind's Ir count is an exactly reproducible measure of "how much work did this program do on this input."

## Call graph and inclusive cost

The distinguishing feature versus Cachegrind is the **call graph**. When you call `annotate` on a Callgrind output file, you see both the exclusive cost (instructions executed in the function itself) and the inclusive cost (instructions executed in the function plus anything it called transitively). This lets you identify a hot function whose cost is mostly *in* its callees — something a pure exclusive-cost profiler will miss.

KCachegrind (the companion KDE/Qt GUI) visualizes the same data as an interactive call-graph with cycle detection. You can zoom in on a function, see its callers and callees, pick a source line, and see per-line counters. For worst-case analysis the usual workflow is:

1. `valgrind --tool=callgrind ./target pathological_input`
2. `kcachegrind callgrind.out.<pid>`
3. Drill into the hot call chain; compare to a baseline profile captured on a benign input.

## Command-line basics

```
valgrind --tool=callgrind [options] ./your-program [args]
```

Useful options:

- `--dump-line=yes` (default) — attribute costs to source lines.
- `--dump-instr=yes` — attribute costs to individual machine instructions (for fine-grained analysis).
- `--cache-sim=yes` — enable cache miss simulation (roughly doubles slowdown).
- `--branch-sim=yes` — enable branch predictor simulation.
- `--collect-jumps=yes` — record per-jump counts inside a function (useful for finding the hot edge inside a loop).
- `--separate-threads=yes` — one profile per thread.
- `--toggle-collect=<fn>` — only collect costs while `fn` is on the stack. Useful for skipping program startup.

Runtime control:

```
callgrind_control -b   # dump a snapshot without stopping the process
callgrind_control -s   # print short status
callgrind_control -e   # list events
```

`callgrind_annotate callgrind.out.<pid>` produces a text summary on stdout.

## Relevance to APEX G-46

Callgrind is the right tool whenever APEX needs a **deterministic, reproducible cost measurement**. Three concrete use cases:

1. **Oracle for regression tests.** Once a performance test has been generated (by PerfFuzz, SlowFuzz, Singularity, or an APEX analogue), CI can re-run the test under Callgrind on a known input and assert that the Ir count falls within an expected envelope. Unlike wall-clock regressions, Ir-count regressions are not flaky: the number is the same on every machine and every run.

2. **Scaling law extraction.** By running the test at a sweep of input sizes and recording Ir counts, APEX can fit a clean power law without noise interference. This is the backbone of the "Measuring Empirical Computational Complexity" paper (Goldsmith et al.) and is significantly easier to implement with Callgrind than with `perf`.

3. **Hotspot localization.** Given a slow input, Callgrind + KCachegrind identifies which function and which source line is responsible for the extra cost. This is essential diagnostic output for any G-46 report that wants to point developers at a specific fix.

## Limitations

- ~20–100× slowdown rules out long-running workloads. For workloads where this matters, couple Callgrind on a single representative input with `perf stat` on the bulk run.
- Single-threaded by default; for multi-threaded targets use `--separate-threads=yes` and analyze per-thread.
- The cache model is a simplified generic model, not bit-identical to any particular CPU's caches. It is useful for comparisons, not for absolute predictions.
- Valgrind dispatch does not support some recent ISA extensions on day zero; check `valgrind --help` against the target binary.
