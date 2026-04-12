---
id: 01KNZ5YRH40QCWBYW7FV6FRHJ2
title: async-profiler — JVM sampling profiler that avoids safepoint bias
type: literature
tags: [tool, async-profiler, profiler, jvm, java, safepoint-bias, flame-graph, perf-events]
links:
  - target: 01KNZ666VRMB0N00T1E5GRPHT4
    type: related
  - target: 01KNZ666W240KABAHAYZP98C3T
    type: related
  - target: 01KNZ5YRHEVRP4MTS5FS5RJ8RH
    type: related
  - target: 01KNWGA5H1MNJK8GWPFCZSSW7E
    type: related
  - target: 01KNZ301FVPT3WBK9D4AHAN5ZB
    type: related
  - target: 01KNZ4VB6JP254YSHY7N9PX4HQ
    type: related
  - target: 01KNWE2Q9YZBAR140ZX5P36TQ5
    type: related
created: 2026-04-11T21:07:53.508137+00:00
modified: 2026-04-11T21:07:53.508141+00:00
---

Source: https://github.com/async-profiler/async-profiler — async-profiler README, fetched 2026-04-12.

async-profiler is a low-overhead sampling profiler for the JVM. It was created by Andrei Pangin (then at Odnoklassniki) and is one of the two mainstream sampling profilers for Java (the other being Java Flight Recorder). Current stable version is 4.3. Apache-2.0.

## Why it exists: the safepoint bias problem

Traditional JVM sampling profilers (VisualVM's built-in sampler, many APM agents) use JVM TI's `GetCallTrace`-style APIs which only capture stack traces when the JVM is at a safepoint — a specific place in the generated code where the runtime can pause a thread. Because safepoint polls are deliberately placed at loop backedges and method entries, the sampled stack traces are systematically biased: the "hot" function measured is not the function actually consuming CPU, but the nearest safepoint-poll location to it. This is the **safepoint bias** and it is the single biggest reason traditional JVM sampling profilers lie.

async-profiler avoids safepoint bias by using `AsyncGetCallTrace`, an undocumented HotSpot internal API that can produce stack traces from any point during execution. Because `AsyncGetCallTrace` can be called from a signal handler, async-profiler triggers sampling via `perf_events` (or timer-based signals on macOS), producing per-sample stack traces from whatever code was actually running — compiled, interpreted, or even in native C/C++ via DWARF unwinding.

## Profile modes

- **CPU profiling** (`-e cpu`) — default; uses `perf_events` `PERF_COUNT_SW_CPU_CLOCK` or `PERF_COUNT_HW_CPU_CYCLES`. Measures on-CPU time only.
- **Wall-clock profiling** (`-e wall`) — samples both running and blocked threads. Measures total elapsed time across all threads. Critical for latency-bound workloads where "nobody is on CPU but the request is still waiting".
- **Allocation profiling** (`-e alloc`) — samples at allocation sites using TLAB (Thread Local Allocation Buffer) slow-path hooks. Tells you where heap allocation is happening and how much, with negligible overhead when there's not much allocation.
- **Lock profiling** (`-e lock`) — samples contended `Object.wait()` / `synchronized` / `ReentrantLock` calls.
- **Hardware events** — `cache-misses`, `page-faults`, `context-switches`, and other `perf_events` sources. Unlocks microarchitectural analysis from Java.

## Output formats

- **Flamegraph** (`-f output.html`) — interactive HTML flame graph (Brendan Gregg's d3-flamegraph). The default and most-used output.
- **Collapsed stacks** (`-o collapsed`) — pipe-able to `flamegraph.pl` or any downstream tool.
- **JFR** (`-f output.jfr`) — Java Flight Recorder file format. Opens directly in Java Mission Control (JMC), bridging async-profiler output into the JFR tooling ecosystem.
- **Raw traces** — per-sample records for custom analysis.
- **Tree** — aggregated call tree.

## Attachment

Two modes:
1. **Agent attach** via `jcmd <pid> JFR.start profile=settings` or `-agentpath:/path/to/libasyncProfiler.so`.
2. **AsyncProfilerLoader** on-demand — `AsyncProfiler.getInstance().start(Events.CPU, 1_000_000L)` from Java code.

Most CLI workflows use `asprof <pid> -e cpu -d 60 -f /tmp/profile.html` — 60 seconds of CPU profiling, HTML output.

## Native and kernel frames

One of async-profiler's distinguishing features: it shows native frames (JIT compiler, GC, JNI libraries) and kernel frames (syscalls, scheduler) alongside Java frames in a unified stack trace. A typical flame graph shows:
```
[kernel]
  [libc]
    [JIT-compiled Java method]
      [interpreted Java method]
        java.util.HashMap$TreeNode.putTreeVal
```
This is essential for diagnosing "Java is slow but no Java frame is hot" situations — usually GC, JIT compilation, or kernel-side I/O.

## Comparison to JFR

- **JFR** is the JDK's built-in event-based profiler. It produces higher-fidelity profiles with richer semantic events (GC phases, object allocations by class, thread parking, etc.) but historically has higher overhead and a steeper tooling curve.
- **async-profiler** is a lightweight external tool. It is simpler, faster to deploy, and its flame graphs are often more immediately useful. It also emits JFR-format output.

In practice, teams use both: async-profiler for "give me a flame graph from this one pod right now", JFR for "always-on continuous recording for post-incident analysis".

## Failure modes

- **AsyncGetCallTrace is undocumented** — HotSpot maintainers can change it in any JDK release. In practice it has been stable for 15+ years but is not a contract.
- **Stack walking on old JDKs** — JDKs older than 8u60 have known async-profiler bugs; modern JDKs (11+, 17+, 21+) are the supported targets.
- **Container support requires kernel perf_events access** — `/proc/sys/kernel/perf_event_paranoid` must be ≤ 1, and the container must run with appropriate capabilities.
- **Inlined frames** can be elided — the JIT aggressively inlines, and the flame graph shows the outer method. Use `-e cpu --inline` or inspect the JFR output to see inline-expanded frames.
- **Not officially supported by Oracle** — some enterprise environments forbid it. JFR is the Oracle-blessed alternative.

## Relevance to APEX G-46

For JVM-based targets, async-profiler is the resource-measurement tool G-46 should integrate with. Its per-sample per-stack output is directly consumable by a flame-graph viewer in an APEX finding report. The wall-clock mode (-e wall) is especially valuable for diagnosing latency-bound resource exhaustion where CPU-only profiling would miss the slow code path entirely.