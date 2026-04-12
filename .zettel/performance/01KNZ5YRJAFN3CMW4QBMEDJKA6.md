---
id: 01KNZ5YRJAFN3CMW4QBMEDJKA6
title: "Parca — Polar Signals' eBPF-based continuous profiling with in-BPF unwinding"
type: literature
tags: [tool, parca, continuous-profiling, ebpf, polar-signals, profiler, dwarf, frostdb]
links:
  - target: 01KNZ666VRMB0N00T1E5GRPHT4
    type: related
  - target: 01KNZ666W240KABAHAYZP98C3T
    type: related
  - target: 01KNZ5YRJ0QSF5TEDG6FFGE6SS
    type: related
  - target: 01KNZ5YRHQG4PJ8BPWFHJXY0W5
    type: related
  - target: 01KNZ5YREWKYWDWQ2MN39KHN5K
    type: related
  - target: 01KNWE2Q9YZBAR140ZX5P36TQ5
    type: related
created: 2026-04-11T21:07:53.546206+00:00
modified: 2026-04-11T21:07:53.546208+00:00
---

Source: https://github.com/parca-dev/parca — Parca README, fetched 2026-04-12.

Parca is an open-source continuous profiling system developed by Polar Signals. Founded 2021, Parca is one of the two major OSS continuous-profiling projects (alongside Grafana Pyroscope) and pioneered the "eBPF + frame-pointer/DWARF unwinding" approach to language-neutral profiling. Apache-2.0.

## Core thesis

Parca's design premise: continuous profiling should be **language-neutral** and **zero-instrumentation**. Rather than requiring an SDK or agent per language, Parca uses an eBPF profiler that unwinds stack traces of any running process on the host — C, C++, Rust, Go, and any language with native frame pointers or DWARF debug info. A single daemon profiles the whole fleet.

## Components

- **Parca Agent** — a per-host daemon that runs the eBPF profiler. It attaches to `perf_events` sampling, captures stack traces every sample via in-kernel unwinding, symbolizes user and kernel frames (via the in-kernel DWARF unwinding infrastructure it contributed to Linux), and ships pprof-format profiles to the Parca Server at ~10-second intervals.
- **Parca Server** — ingest, storage, query. Uses **FrostDB**, a columnar in-memory + object-storage database Polar Signals built specifically for profiling workloads. Columns are labels (service, pod, namespace) and timestamps; values are profile samples.
- **Web UI** — interactive flame graph explorer with diff support, label filtering, and time-range selection.

## What the eBPF unwinder does

Unlike pure-perf profilers that give you only kernel + native addresses, Parca Agent implements stack walking in BPF. It:
1. Samples `perf_events` at ~19 Hz per CPU (adaptive).
2. In the eBPF probe, walks the user-space stack via either frame pointers (Go, Rust compiled with `-force-frame-pointers`, C/C++ compiled with `-fno-omit-frame-pointer`) or DWARF CFI (`.eh_frame`) decoded from the process binary, uploaded to the kernel via a BPF map.
3. Uploads the stack ID to user space, where the agent symbolizes it against the binary's debug info.

This is genuinely novel — DWARF unwinding in BPF was not practical before Parca and the Linux kernel patches that landed alongside it. It is the reason Parca Agent can profile stripped production binaries from completely different runtimes without per-language SDKs.

## Storage: FrostDB

FrostDB is Parca's purpose-built columnar database. Key design points:
- Parquet as the on-disk format.
- Arrow as the in-memory format.
- Object storage (S3, GCS) for cold data.
- Label-based indexing for fast "filter by service, aggregate across pods" queries.

FrostDB is open-source and used independently of Parca; the schema is pprof-compatible.

## Query model

Parca queries are time-range + label-selector + profile-type. "CPU profile for service=api, pod=~api-.*, from T0 to T1, merged". Diff mode subtracts one query from another — the foundation for regression analysis.

## Grafana integration

Parca has a Grafana data source plugin. It can be queried alongside Prometheus metrics and Loki logs in the same dashboard. This parallels Pyroscope's integration but via a plugin path.

## Strengths

- Zero per-process instrumentation — one Agent, whole host.
- Language-neutral via in-BPF DWARF unwinding.
- Contributed real kernel improvements upstream (DWARF CFI in BPF).
- FrostDB is a legitimate piece of infrastructure, usable standalone.
- Pprof compatibility end-to-end — profiles are interoperable with any pprof tool.

## Failure modes

- **eBPF profiler is Linux-only**, kernel >= 5.8 recommended for CO-RE.
- **Stripped binaries without debug info** cannot be symbolized — the addresses show, the function names don't. Requires `debuginfod` or a similar symbol server.
- **Frame-pointer requirements** — Go 1.x has frame pointers; C/C++ and Rust need `-fno-omit-frame-pointer`. Many distro packages do not. DWARF unwinding covers this but is slower.
- **Resource overhead** of the Agent is measurable (~0.5-1% CPU per host) — budget for it.
- **Smaller ecosystem** than Pyroscope post-Grafana merger.

## Relevance to APEX G-46

Parca represents the "language-neutral observability substrate" vision. For APEX, Parca's eBPF profiler is the most faithful operational counterpart to what G-46's resource-measurement phase wants to achieve on production hosts: language-neutral, low-overhead, language-agnostic profiling that catches regressions across the entire fleet without per-service instrumentation work. Parca's DWARF-in-BPF technique also informs APEX's symbolization strategy for stripped release binaries.