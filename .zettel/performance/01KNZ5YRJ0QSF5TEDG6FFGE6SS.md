---
id: 01KNZ5YRJ0QSF5TEDG6FFGE6SS
title: "Pyroscope — Grafana's open-source continuous profiling platform"
type: literature
tags: [tool, pyroscope, continuous-profiling, grafana, profiler, ebpf, flame-graph, observability]
links:
  - target: 01KNZ666VRMB0N00T1E5GRPHT4
    type: related
  - target: 01KNZ666W240KABAHAYZP98C3T
    type: related
  - target: 01KNZ5YRJAFN3CMW4QBMEDJKA6
    type: related
  - target: 01KNZ5YRHQG4PJ8BPWFHJXY0W5
    type: related
  - target: 01KNZ5YREWKYWDWQ2MN39KHN5K
    type: related
  - target: 01KNZ301FVPT3WBK9D4AHAN5ZB
    type: related
  - target: 01KNWE2Q9YZBAR140ZX5P36TQ5
    type: related
created: 2026-04-11T21:07:53.536836+00:00
modified: 2026-04-11T21:07:53.536838+00:00
---

Source: https://github.com/grafana/pyroscope — Pyroscope README, fetched 2026-04-12.

Pyroscope is an open-source continuous profiling platform now developed by Grafana Labs after the 2023 merger with Grafana Phlare. It stores profiles as time-series data, much like Prometheus stores metrics, and presents them via flame graphs in Grafana. Apache-2.0.

## What continuous profiling is

Continuous profiling means running a low-overhead profiler *all the time* on *every* production process and shipping the profiles to a central store for later query. Instead of "the system was slow between 2:15 and 2:20, let me attach a profiler", you ask "what did the p99-latency path look like between 2:15 and 2:20?" after the fact, because the data is already there. The architectural ancestor is Google-Wide Profiling (Ren et al., 2010).

## Architecture

Three components:
1. **Pyroscope Server** — ingests, stores, and queries profiling data. Backed by a columnar storage engine (post-merger with Phlare, leveraging Parquet and object storage).
2. **Clients** — either Pyroscope SDK integrated into the application (push model), or Grafana Alloy / Grafana Agent scraping pprof endpoints (pull model, Prometheus-style).
3. **UI** — the Explore Profiles experience inside Grafana, or Pyroscope's standalone web UI. Flame graphs are the primary visualisation.

## Profile types

Pyroscope stores multiple profile streams per process:
- **CPU** — sampling profiler, 100 Hz.
- **inuse_space** — currently-live heap.
- **alloc_space** — cumulative allocations.
- **goroutines** — for Go.
- **mutex** — contention hot spots.
- **block** — off-CPU wait.

## Language integrations

- **Go** — `pyroscope-go` SDK wraps `runtime/pprof`, ships profiles on an interval.
- **Java** — uses async-profiler under the hood, respecting all of async-profiler's modes (CPU, alloc, lock, wall).
- **Python** — `py-spy`-based sampling profiler.
- **Ruby** — `rbspy`-based sampling profiler.
- **.NET** — native .NET profiler integration.
- **Node.js** — `v8-profiler-next`-based.
- **eBPF** — a Pyroscope eBPF profiler that uses perf_events + stack unwinding to profile any process on a Linux host without per-application agent install. This is language-neutral and the operationally preferred mode for Kubernetes deployments.
- **Rust** — via `pprof-rs`.

## Query interface

Pyroscope's query model is Prometheus-shaped — profiles have labels (`service_name`, `pod`, `environment`, ...) and can be filtered/aggregated across time ranges. The equivalent of `rate(http_requests[5m])` for profiling is "give me the flame graph for service=foo between T0 and T1 across all pods, merged". Diff queries compute "the difference between this 10-minute window and the previous 10-minute window" — the foundation for automated regression detection.

## Grafana integration

Post-merger, Pyroscope is a first-class data source in Grafana. The Explore Profiles app offers a "queryless" interface that walks users through finding interesting profiles via automatic anomaly detection. Exemplars from OpenTelemetry traces can link directly to the profile for the same trace_id — bridging traces, metrics, and profiles in one UI.

## Strengths

- Language-neutral via the eBPF profiler.
- Prometheus-like storage and query model — ops-familiar.
- Flame graphs in Grafana dashboards alongside metrics and logs.
- Diff queries for regression detection are first-class.
- OSS, no licensing hurdle for adoption.

## Failure modes

- **Storage costs** — continuous profiling generates a lot of data. Sampling rate and retention need tuning, or storage budgets balloon.
- **eBPF profiler is Linux-only** and requires kernel >= 4.9 (ideally 5.8+ for CO-RE).
- **Symbolization of production binaries** — stripped binaries produce unreadable flame graphs unless debug info is shipped.
- **Language integration maturity varies** — Go and Java are best; Ruby, Node, .NET trail.
- **Overhead is not zero** — continuous profiling adds 0.5-2% CPU depending on mode; production-critical services need to verify.

## Relevance to APEX G-46

Pyroscope is the operational deployment model for "APEX-discovered regressions should be investigated in the continuous profile archive". A G-46 finding that says "function X became 3x slower" is most actionable when the user can click through to Pyroscope and see the flame graph difference between "last week" and "now" for the same function on production traffic. Pyroscope's diff-query semantics are the direct operational analogue of G-46's baseline-regression detection.