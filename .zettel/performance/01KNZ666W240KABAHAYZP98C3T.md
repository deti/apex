---
id: 01KNZ666W240KABAHAYZP98C3T
title: "Comparative matrix: profilers, tracers, and observability tools"
type: permanent
tags: [comparative, profiler, observability, matrix, continuous-profiling, pprof, ebpf, opentelemetry]
links:
  - target: 01KNZ5YREWKYWDWQ2MN39KHN5K
    type: references
  - target: 01KNZ5YRH40QCWBYW7FV6FRHJ2
    type: references
  - target: 01KNZ5YRHEVRP4MTS5FS5RJ8RH
    type: references
  - target: 01KNZ5YRHQG4PJ8BPWFHJXY0W5
    type: references
  - target: 01KNZ5YRJ0QSF5TEDG6FFGE6SS
    type: references
  - target: 01KNZ5YRJAFN3CMW4QBMEDJKA6
    type: references
  - target: 01KNZ666V3J0MQ0HS4F6DMAJZR
    type: references
  - target: 01KNZ666TDP7H8GRG9RF62384D
    type: references
  - target: 01KNZ666VE9ZGQ8DKVV36PZ7MZ
    type: references
  - target: 01KNZ666VRMB0N00T1E5GRPHT4
    type: related
  - target: 01KNZ301FVPT3WBK9D4AHAN5ZB
    type: related
  - target: 01KNZ301FV6BZ60F02QWNWR4JB
    type: related
  - target: 01KNZ301FV2VB7BHH13YAAG7SA
    type: related
  - target: 01KNZ4VB6JHP7W47HM7QREWW53
    type: related
  - target: 01KNZ4VB6J6ED6F3YHN1SMDNQ5
    type: related
  - target: 01KNWE2Q9YZBAR140ZX5P36TQ5
    type: related
created: 2026-04-11T21:11:57.570672+00:00
modified: 2026-04-11T21:11:57.570674+00:00
---

A structured comparison of profilers, tracers, and observability tools documented in this vault (batch of 2026-04-12). Cross-links are to individual per-tool notes: pprof, async-profiler, JFR, Pyroscope, Parca, eBPF/BCC/bpftrace, OpenTelemetry, Linux perf (pre-existing), Valgrind Callgrind (pre-existing), Brendan Gregg flame graphs (pre-existing).

## Classification by scope

| Layer | Tools |
|---|---|
| **CPU sampling profilers** (per-process, on-demand) | pprof, async-profiler, JFR method sampling, py-spy, perf record |
| **Continuous profiling** (fleet-wide, always-on) | Pyroscope, Parca, Datadog Continuous Profiler, Google Cloud Profiler |
| **Instrumented event-based profiling** (deterministic counting) | Valgrind Callgrind, Java Flight Recorder event streams |
| **Kernel-level tracing** (system-wide, event-driven) | Linux perf, eBPF / BCC / bpftrace, DTrace (historical), SystemTap (legacy) |
| **Distributed tracing** (per-request causal chains) | OpenTelemetry (vendor-neutral), Jaeger, Zipkin, Datadog APM, Honeycomb |
| **Metrics** (aggregated numeric time-series) | Prometheus, OpenTelemetry Metrics, StatsD, InfluxDB |
| **Flame graph rendering** | pprof -flamegraph, FlameGraph.pl (Gregg), d3-flamegraph, Grafana flame graph panel |

## Core comparison of profilers

| Tool | Languages | Overhead | Always-on | Output | Notable feature |
|---|---|---|---|---|---|
| pprof | Go (first-class), C/C++ via gperftools, Rust via pprof-rs | Low (~1%) | Via `net/http/pprof` | profile.proto | Cross-language interchange format |
| async-profiler | JVM (any) | Low (<1% CPU mode) | No (attach/detach) | HTML flame, JFR, collapsed | Safepoint-bias-free |
| JFR | JVM only | Very low (<1% default) | Yes | `.jfr` binary | Event-based framework, not just sampling |
| Linux perf | Any native binary | Low (sampling) / Higher (tracing) | Yes | `perf.data` | Most flexible, full kernel access |
| Valgrind Callgrind | Any native (under Valgrind) | **Very high (10-100x slowdown)** | No | Callgrind format | Deterministic, cache simulation |
| bpftrace | Any (via probes) | Nanoseconds per event | Yes | text/histogram | One-liners, ad-hoc |
| BCC | Any | Nanoseconds per event | Yes | Python scripts | Large tool library (bcc-tools) |

## Continuous profiling platform comparison

| System | Agent mode | Profiler | Storage | Query model | Grafana integration |
|---|---|---|---|---|---|
| Grafana Pyroscope | Push (SDK) or Pull (Alloy) | Language SDKs + eBPF | Phlare blocks on object store | Prometheus-like labels | First-class |
| Parca | Push (Parca Agent) | eBPF + DWARF unwinding | FrostDB on object store | Label selectors + time range | Plugin |
| Datadog Continuous Profiler | Push (Datadog Agent) | Language SDKs | Proprietary | Datadog APM dashboards | External |
| Google Cloud Profiler | Push (SDK) | Language-specific | Proprietary | Cloud Console | External |

## The three axes that matter

### 1. Sampling vs instrumentation

**Sampling** (pprof, async-profiler, Linux perf sampling, JFR execution sampling, all continuous profilers) statistically captures a small fraction of events. Overhead is proportional to sampling frequency, not program activity. Scales to production. Loses precise causal information.

**Instrumentation** (Valgrind Callgrind, JFR event framework, per-function tracing, distributed tracing via OTel spans) deterministically records every event. Captures complete causal information. Overhead is proportional to program activity. Does not scale to production — typically reserved for targeted analysis or low-rate events.

Modern observability combines the two: continuous sampling for hot-path profiling, instrumented tracing for per-request causal chains, exemplars to link the two.

### 2. In-process vs kernel-level

**In-process profilers** (pprof, async-profiler, JFR, language SDKs) know the language, understand managed runtimes, can surface managed-runtime frames (JIT methods, interpreted bytecode, GC state). Language-specific.

**Kernel-level profilers** (perf, eBPF, Parca Agent) operate on any process regardless of language. Must reconstruct stack traces externally; managed runtimes require per-language helpers for symbolization. Language-neutral but requires more infrastructure.

### 3. Data archive vs ad-hoc

**Ad-hoc** tools (pprof CLI, async-profiler attach, perf record, Valgrind) capture a single snapshot. The user brings the analysis session to the data.

**Archive-based** tools (continuous profilers, OTel with long-retention stores) keep historical data in a queryable form. The user brings queries to the archive. This is what enables "what happened at 03:42 last Tuesday" workflows.

## OpenTelemetry's role

OTel is not a profiler — it is an instrumentation and interchange specification. But the recently-begun OTel profiles signal (fourth signal after traces, metrics, logs) intends to standardise profile-data ingestion, and exemplars on OTel histograms already bridge metrics and traces. In the 2026-2028 timeframe, OTel will almost certainly become the vendor-neutral wire format for profile data, replacing language-specific APIs as the way profilers feed backends.

## Recommendations by use case

- **Diagnose a slow Go service on your laptop:** `pprof -http :8080 http://localhost:6060/debug/pprof/profile?seconds=30`
- **Diagnose a slow JVM service in production:** async-profiler attach, CPU or wall mode, 60 seconds.
- **Continuous production profile archive for regressions:** Pyroscope or Parca.
- **System-level problem (disk I/O, scheduling, off-CPU time):** BCC tools (`biolatency`, `offcputime`, `runqlat`) or bpftrace one-liners.
- **Cross-service latency debugging:** OpenTelemetry distributed traces with exemplar-linked metrics.
- **Exact instruction counts and cache behaviour of a hot function:** Valgrind Callgrind (but expect 10-100x slowdown).
- **"I don't know what's wrong":** start with `perf top`, move to `perf record`, then specialize.

## Relevance to APEX G-46

For each layer, G-46 has a natural integration point:
- **Sampling profilers** — APEX runs its generated worst-case tests under a sampling profiler and emits profile.proto in the finding report.
- **Continuous profiling** — APEX's regression detector consumes Pyroscope or Parca baselines instead of (or in addition to) APEX's own per-run snapshots.
- **Kernel-level tracing** — APEX uses bpftrace / BCC tools for off-CPU analysis when a worst-case input produces a blocked (not CPU-bound) slowdown.
- **OpenTelemetry** — APEX emits findings as OTLP events (metrics + traces + exemplars) so they can flow through existing observability pipelines.