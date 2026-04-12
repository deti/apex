---
id: 01KNZ5YRHQG4PJ8BPWFHJXY0W5
title: eBPF ecosystem — BCC and bpftrace for Linux kernel and user-space tracing
type: literature
tags: [tool, ebpf, bcc, bpftrace, linux, kernel, tracing, profiler, observability]
links:
  - target: 01KNZ666VRMB0N00T1E5GRPHT4
    type: related
  - target: 01KNZ666W240KABAHAYZP98C3T
    type: related
  - target: 01KNZ5YRJAFN3CMW4QBMEDJKA6
    type: related
  - target: 01KNZ5YRJ0QSF5TEDG6FFGE6SS
    type: related
  - target: 01KNZ301FV6BZ60F02QWNWR4JB
    type: related
  - target: 01KNWE2Q9YZBAR140ZX5P36TQ5
    type: related
created: 2026-04-11T21:07:53.527379+00:00
modified: 2026-04-11T21:07:53.527381+00:00
---

Sources: https://github.com/iovisor/bcc and https://github.com/iovisor/bpftrace — fetched 2026-04-12.

BCC and bpftrace are the two canonical user-space front-ends for the Linux eBPF subsystem, used together as the modern replacement for DTrace / SystemTap / Kprobes / Ftrace on Linux. Both are products of the IO Visor project and were championed by Brendan Gregg and the Netflix performance team.

## eBPF in one paragraph

Extended BPF (eBPF) is a safe in-kernel virtual machine for running small programs attached to kernel events (syscalls, kprobes, tracepoints, scheduler events, network events, perf-counter overflows). Programs are verified by the kernel's BPF verifier at load time to guarantee they terminate and cannot corrupt kernel state; they run with very low overhead (nanoseconds per event) because there is no user-kernel context switch. eBPF is the substrate for modern Linux observability, security, and networking tooling — Cilium, Falco, Parca, Pyroscope, and many APM agents all build on it.

## BCC (BPF Compiler Collection)

BCC is a Python/Lua/C++ framework for authoring eBPF programs. A BCC tool looks like:

```python
from bcc import BPF
b = BPF(text='''
int kprobe__sys_clone(void *ctx) {
  bpf_trace_printk("Hello, clone!\n");
  return 0;
}
''')
b.trace_print()
```

BCC compiles the embedded C source with Clang/LLVM at runtime, loads the bytecode via the bpf() syscall, and presents events back to Python via perf rings and maps. The advantage of BCC is richness: full kernel headers, full C, arbitrary complexity. The disadvantage is the 100+ MB Clang/LLVM dependency baked into the runtime — bcc tools are heavyweight.

### The canonical BCC toolkit

BCC ships a kitchen-sink of ready-made tools in `/usr/share/bcc/tools/`. The most-used:
- `execsnoop` — trace process executions (`execve` calls) across the system.
- `opensnoop` — trace `openat()` file opens.
- `biolatency`, `biosnoop`, `bitesize` — block I/O latency, per-request tracing, and size histograms.
- `tcplife`, `tcpconnect`, `tcpaccept`, `tcpretrans` — TCP session lifecycle and problems.
- `offcputime` — aggregate time processes spent *off* CPU (blocked) with stack traces. Essential for latency-bound workloads.
- `profile` — sampled CPU profiling across the whole system, 99 Hz, with kernel and user stacks.
- `memleak` — track allocations that were never freed over time.
- `filetop`, `filelife`, `dcstat` — filesystem activity and lifetime.
- `runqlat` — run-queue scheduling latency histogram.
- `softirqs`, `hardirqs` — interrupt handler time.

Each tool is 30-200 lines of Python-wrapping-C and is individually readable. Brendan Gregg's *BPF Performance Tools* book documents them extensively.

## bpftrace

bpftrace is a higher-level DSL for eBPF, modelled directly on DTrace's `D` language and awk. It is the first tool to reach for when you want to write a new probe from scratch.

A bpftrace script:

```bpftrace
#!/usr/bin/env bpftrace

tracepoint:syscalls:sys_enter_openat
{
  @opens[comm] = count();
}

interval:s:5
{
  print(@opens);
  clear(@opens);
}
```

Probe types:
- `kprobe:` / `kretprobe:` — kernel function entry/exit.
- `uprobe:` / `uretprobe:` — user-space function entry/exit (including in libraries).
- `tracepoint:category:name` — static kernel tracepoints.
- `usdt:binary:probe` — user-space statically defined tracepoints (e.g. in PostgreSQL, Python, JVM).
- `profile:hz:N` — sampled profile at N Hz.
- `interval:s:N` — fire every N seconds.
- `software:cpu-clock:N` / `hardware:cache-misses:N` — perf-event probes.

Built-in aggregations (`@map = count()`, `hist()`, `lhist()`, `avg()`, `sum()`, `min()`, `max()`, `stats()`) compile to in-kernel BPF maps. The `hist()` builtin is particularly valuable: it produces log-2 histograms for latency distributions with zero user-space cost.

### bpftrace vs BCC

- **bpftrace** — best for one-liners and ad-hoc exploration. Minimal dependency (just the `bpftrace` binary). Limited expressiveness vs BCC.
- **BCC** — best for shipping production tools with complex logic. Heavy LLVM dependency at runtime (though `libbpf-tools` is a lighter rewrite that pre-compiles).

Many BCC tools have been rewritten as bpftrace scripts because the dependency cost is lower.

## libbpf-tools and CO-RE

Recent eBPF ecosystem: **CO-RE (Compile Once - Run Everywhere)** uses BTF (BPF Type Format) debug info to make compiled BPF programs portable across kernel versions. `libbpf-tools` is a rewrite of the BCC tools using `libbpf` and CO-RE — no runtime LLVM dependency, runs from a tiny static binary. This is the future of production eBPF tooling.

## Strengths

- Nanoseconds of overhead per event — eBPF can profile production systems under load.
- Kernel-side aggregation via maps means the user-space consumer reads compact summaries, not per-event streams.
- Covers kernel, user-space, and network events uniformly.
- Stack traces span kernel, native code, and instrumented runtimes (Python, JVM, Go).

## Failure modes

- **Kernel version lottery** — BPF features are added incrementally. Older kernels (< 4.9, then < 5.8) lack capabilities that tools assume. CO-RE helps but doesn't eliminate this.
- **Verifier rejections** — "too complex", "unbounded loop", "out-of-bounds access" errors at load time. Debugging requires understanding the verifier's model.
- **uprobes have real overhead** — each firing takes ~microseconds. A high-volume function with uprobes attached becomes the bottleneck.
- **Symbol resolution on stripped binaries** — stack traces show addresses, not function names. Requires the binary with debug info.
- **Container and cgroup scoping** is non-trivial — filtering eBPF output to a single container requires passing cgroup IDs or PID namespaces through the program.
- **JIT frame unwinding** (Python, JVM, Go) requires per-runtime helpers and special handling — this is where async-profiler-style tools still excel.

## Relevance to APEX G-46

For Linux targets, eBPF is the lowest-overhead, most-flexible observability substrate available, and it is language-neutral. APEX's resource-measurement phase should integrate with `libbpf-tools` or bpftrace for system-level measurements (off-CPU time, run-queue latency, block I/O) that complement language-specific profilers. `offcputime` is directly relevant to detecting blocking-I/O-based resource exhaustion — the workload is waiting, not burning CPU, and a traditional CPU profiler will not see it.