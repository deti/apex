---
id: 01KNZ301FV6BZ60F02QWNWR4JB
title: "Tool: Linux perf (Kernel Performance Analysis Subsystem)"
type: reference
tags: [tool, profiler, perf, linux, hardware-counters, sampling, tracing]
links:
  - target: 01KNWE2Q9YZBAR140ZX5P36TQ5
    type: references
  - target: 01KNWE2QACWYZJXRJ8QE78T043
    type: extends
  - target: 01KNZ301FVPT3WBK9D4AHAN5ZB
    type: related
  - target: 01KNZ301FV2VB7BHH13YAAG7SA
    type: related
created: 2026-04-12
modified: 2026-04-12
source: "https://perfwiki.github.io/"
source_mirror: "https://man7.org/linux/man-pages/man1/perf.1.html"
license: "GPL-2.0"
---

# Tool: Linux perf (Kernel Performance Analysis Subsystem)

**Wiki:** https://perfwiki.github.io/
**Man page:** https://man7.org/linux/man-pages/man1/perf.1.html
**Source:** in-tree under `tools/perf/` of the Linux kernel source tree

## What it is

`perf` is the user-space front end to the Linux kernel's `perf_events` subsystem. It provides a unified interface to hardware performance monitoring units (PMUs), software counters, tracepoints, kprobes, uprobes, and eBPF-based instrumentation. It is the primary performance-analysis tool on Linux and is the entry point for most of the techniques APEX's G-46 pipeline will use.

From the upstream man page: perf "provide[s] a framework for all things performance analysis. It covers hardware level (CPU/PMU, Performance Monitoring Unit) features and software features (software counters, tracepoints) as well."

## Subcommand inventory

The `perf` executable is really a family of subcommands, each of which provides a distinct analysis mode. The man page groups them roughly as:

**Profiling and counting**
- `perf stat` — run a command and print aggregate counter values (cycles, instructions, cache-misses, branch-mispredicts, ...). The go-to tool for "is this faster or slower" with hardware-counter precision.
- `perf record` — sample hardware events (typically cycles) and record per-sample stack traces to `perf.data`.
- `perf report` — browse `perf.data`, produce call-graph tree, or dump top functions.
- `perf top` — live `top`-style view of the hottest functions system-wide.
- `perf annotate` — display source/assembly with per-instruction sample counts.
- `perf list` — list every event, tracepoint, and PMU counter available on the current system.

**Tracing and debugging**
- `perf trace` — strace-like process tracer that can also attach to kernel/user probes.
- `perf ftrace` — wrap the in-kernel ftrace framework.
- `perf script` — scripted dump of the samples in `perf.data` (used as input to `FlameGraph/stackcollapse-perf.pl`).
- `perf probe` — dynamically add uprobes and kprobes at arbitrary functions or lines without recompiling the target.

**System-level analysis**
- `perf sched` — scheduler latency and wait analysis.
- `perf lock` — lock contention analysis.
- `perf kmem` — kernel allocator stats.
- `perf mem` — memory access sampling (requires PEBS-style hardware).
- `perf c2c` — cache-to-cache contention analysis; identifies false sharing.
- `perf iostat` — per-device IO counters.

**Architecture-specific**
- `perf intel-pt` — Intel Processor Trace decoding (full branch trace).
- `perf amd-ibs` — AMD Instruction-Based Sampling.
- `perf arm-spe` — ARM Statistical Profiling Extension.

**Data management**
- `perf data` / `perf diff` — compare two `perf.data` files (regression analysis).
- `perf inject` / `perf archive` — transform and package recorded data for transport to another machine.
- `perf buildid-cache` — manage the mapping from build IDs to binaries and DWARF.

## The events model

Every measurement is expressed as an "event" to count or sample on. Events come from several sources:

- **Hardware events** — mapped by the kernel to PMU counters: `cycles`, `instructions`, `cache-references`, `cache-misses`, `branches`, `branch-misses`, `bus-cycles`, and many microarchitecture-specific raw events exposed via `cpu/event=0xNN,umask=0xMM/`.
- **Software events** — synthesized by the kernel without hardware support: `cpu-clock`, `task-clock`, `page-faults`, `context-switches`, `cpu-migrations`, `minor-faults`, `major-faults`.
- **Tracepoint events** — static tracepoints compiled into the kernel (and in some cases into user-space libraries via SDT). Examples: `sched:sched_switch`, `block:block_rq_issue`, `syscalls:sys_enter_open`.
- **Dynamic probes** — kprobes (kernel functions) and uprobes (user functions) attached via `perf probe`. Used for zero-instrumentation profiling of arbitrary code points.
- **eBPF events** — `perf stat --bpf-counters` and `perf record --bpf-filter` plug eBPF programs into the sampling pipeline for arbitrary in-kernel aggregation.

## Example invocations relevant to G-46

**Count instructions executed by a command (noise-free microbenchmark):**

```
perf stat -e instructions,cycles,branch-misses ./target input
```

Instructions retired is a deterministic measure that, on a given build, reproduces bit-identically across runs (modulo library version skew). It is often a better regression metric than wall time.

**Record and produce a flame graph:**

```
perf record -F 99 -a -g -- ./target pathological_input
perf script report flamegraph
```

**Compare baseline vs pathological:**

```
perf record -o baseline.data -F 99 -g ./target benign_input
perf record -o patho.data   -F 99 -g ./target pathological_input
perf diff baseline.data patho.data
```

**Capture full branch trace with Intel PT:**

```
perf record -e intel_pt//u --filter 'filter * @/path/to/target' ./target input
perf script --itrace=i1us
```

Full branch traces enable exact per-iteration counting of hot loops — the highest-fidelity measurement available on commodity hardware.

## Relevance to APEX G-46

`perf` fills three roles in the G-46 stack:

1. **Hardware-counter oracle.** `perf stat -e instructions` is the deterministic regression metric. It sidesteps frequency scaling, scheduler noise, and thermal throttling in the same way Callgrind does, but at native speed.
2. **Sampling profiler for flame graphs.** The `perf record`/`perf script` pipeline is what Brendan Gregg's `stackcollapse-perf.pl` consumes.
3. **Diff-based regression reporting.** `perf diff` between two runs of the same binary localizes which symbol got more expensive; that symbol plus the flame graph is the complete diagnostic output a developer needs.

## Caveats

- Hardware counters require kernel version 2.6.31+ and appropriate `kernel.perf_event_paranoid` settings. Modern distributions default to a restrictive value; bench machines should set `sysctl kernel.perf_event_paranoid=1`.
- Sampling profilers need frame pointers or DWARF unwinding; build with `-fno-omit-frame-pointer` or use `--call-graph dwarf`.
- Cross-machine comparisons of hardware events are not meaningful — different microarchitectures expose different events. Stick to portable counters (`cycles`, `instructions`) for any metric that must travel across machines.
- In VMs and containers, some counters are unavailable or emulated; `perf list` will tell you which.
