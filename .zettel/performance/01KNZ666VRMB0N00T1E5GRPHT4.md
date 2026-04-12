---
id: 01KNZ666VRMB0N00T1E5GRPHT4
title: "Continuous profiling — pattern, enabling technologies, and tool landscape"
type: permanent
tags: [concept, continuous-profiling, observability, pyroscope, parca, gwp, ebpf, landscape]
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
  - target: 01KNZ666W240KABAHAYZP98C3T
    type: related
  - target: 01KNZ4VB6JHP7W47HM7QREWW53
    type: related
  - target: 01KNZ4VB6J6ED6F3YHN1SMDNQ5
    type: related
  - target: 01KNZ4VB6JMSSE4E40PBE23S3M
    type: related
  - target: 01KNZ301FVPT3WBK9D4AHAN5ZB
    type: related
  - target: 01KNZ301FV6BZ60F02QWNWR4JB
    type: related
  - target: 01KNWE2Q9YZBAR140ZX5P36TQ5
    type: related
  - target: 01KNZ4RPD1HFN3PDXPR09VY5Q8
    type: references
created: 2026-04-11T21:11:57.560617+00:00
modified: 2026-04-11T21:11:57.560619+00:00
---

A synthesis note covering the continuous-profiling subfield — how Google-Wide Profiling (Ren et al., 2010) established the pattern, and how the 2018-2026 generation of open-source and commercial tools implements it. See individual tool notes for Pyroscope, Parca, async-profiler, JFR, and GWP for details.

## The idea

Continuous profiling is the operational practice of running low-overhead sampling profilers on every production process, continuously, and storing the resulting profile stream in a central, queryable archive. The three critical properties:

1. **Always-on.** The profiler is always running, not attached-on-demand. You can go back and ask "what was running at 03:42 last Tuesday" and get an answer.
2. **Low overhead.** Per-process cost is under 1-2% CPU, tuned so it can run in production without measurable SLO impact.
3. **Aggregatable.** Profiles are tagged with labels (service, pod, cluster, build_id, region) so they can be sliced, filtered, and compared across the fleet.

The practice is built on top of sampling profilers — in-language (pprof in Go, async-profiler/JFR in JVM, py-spy in Python) or language-neutral (perf_events + eBPF unwinding in Parca, Pyroscope eBPF profiler). The outputs are aggregated in a columnar store (FrostDB for Parca, Phlare's block store for Pyroscope, proprietary stores for Datadog/Dynatrace) and queried over time ranges with label selectors.

## Why it matters for performance investigation

Traditional profiling assumes you can reproduce the performance problem: "start a profiler, run the workload, stop the profiler, analyse". But in production, many performance problems are:

- **Transient.** A 15-minute spike 3 days ago. You cannot reproduce it.
- **Traffic-dependent.** The slow code path only triggers under a specific user interaction pattern.
- **Environment-dependent.** The problem only occurs on one cluster, one region, one machine with a specific CPU model.
- **Scale-emergent.** The code is fast at 100 RPS and slow at 10,000 RPS, and you cannot reliably reproduce 10,000 RPS in staging.

Continuous profiling addresses all four: because the profiler was always running, the data exists. You query it after the fact.

## The techniques that enable low-overhead sampling

All modern continuous profilers depend on a set of enabling technologies:

- **perf_events** (Linux) — kernel-level sampling at a specified frequency, with negligible overhead and per-PID filtering.
- **AsyncGetCallTrace** (HotSpot) — off-safepoint stack sampling in the JVM. Enables async-profiler's safepoint-bias-free mode.
- **Frame pointers / DWARF CFI unwinding** — stack walking without pausing the thread.
- **In-kernel stack aggregation** (eBPF maps) — aggregate on the kernel side so the user-space consumer reads summarized data.
- **Protocol buffer interchange format** (profile.proto) — pprof-compatible wire format letting profilers from one language be analysed by tools from another.
- **Columnar storage** (Parquet via FrostDB, Phlare's blocks) — time-series-aware storage for the peculiar workload of "many small profiles, high cardinality labels".

Without all of these, the overhead and storage cost would make continuous profiling impractical. Their arrival in the late 2010s is what turned GWP-in-paper into Pyroscope-in-production.

## The landscape (as of 2026)

**Open source:**
- **Grafana Pyroscope** — formerly Pyroscope Inc., merged with Grafana Phlare, now Grafana's official continuous profiling project. Pull or push. Multiple language SDKs plus eBPF profiler. Tight Grafana integration.
- **Parca** (Polar Signals) — eBPF-first, language-neutral, FrostDB storage. Contributed significant upstream Linux kernel work for in-BPF DWARF unwinding.
- **OpenTelemetry profiles** — in-progress specification to add profiles as a fourth OTel signal alongside traces, metrics, and logs. When stable, will provide a vendor-neutral wire protocol for profile data.

**Commercial:**
- **Datadog Continuous Profiler** — first mainstream commercial offering (2019). Multi-language SDKs, flamegraphs in the Datadog dashboard, correlated with APM traces.
- **Google Cloud Profiler** — hosted continuous profiling for GCP workloads. The commercial sibling of GWP.
- **Polar Signals** — the commercial Parca offering.
- **Grafana Cloud Profiles** — Pyroscope-as-a-service as part of Grafana Cloud.
- **Dynatrace, New Relic, Splunk APM** — each has added continuous profiling to its APM suite over 2021-2024.

## Implementation costs and tradeoffs

- **Storage cost** — a 10-second profile per process per 10 seconds across 1,000 processes is 3.6 million profiles per day. Compression helps; retention policies are necessary.
- **Symbolization** — stripped production binaries need either debug info shipped alongside, a `debuginfod` symbol server, or build-id keyed symbol archives. This is operationally non-trivial.
- **Cardinality management** — labels like `user_id` or `request_id` blow up the index. Labels should be at most service-level (service, pod, container, version).
- **Sampling rate trade-off** — 100 Hz is the canonical CPU sampling frequency, but higher rates catch short-lived hotspots at the cost of storage and overhead.
- **Privacy concerns** — profiles can leak information about workload (function names, timing). In sensitive environments, profiles may need access controls.

## Relevance to APEX G-46

Continuous profiling is the production-side counterpart to G-46's test-generation-side resource measurement. Two natural integration points:

1. **Baseline from production data.** G-46's regression-detection phase compares current test runs against a baseline. A production continuous-profiling archive is the most accurate baseline available — it represents real workload rather than a reproduction of it.
2. **Validation of APEX-discovered worst cases.** When APEX finds an input that makes a function slow in a test, you can query the continuous-profile archive to see whether that function is actually a hotspot in production. If yes, the finding is load-bearing; if no, it may be a micro-benchmark artefact.

The two notes this most-strongly depends on are GWP (the paper that started it all) and Pyroscope + Parca (the operational implementations one would actually deploy).