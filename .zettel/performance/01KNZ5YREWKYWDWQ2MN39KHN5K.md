---
id: 01KNZ5YREWKYWDWQ2MN39KHN5K
title: "pprof — Google's cross-language profile format and analysis tool"
type: literature
tags: [tool, pprof, profiler, profile-proto, go, google, cpu-profile, heap-profile, diff]
links:
  - target: 01KNZ666VRMB0N00T1E5GRPHT4
    type: related
  - target: 01KNZ666W240KABAHAYZP98C3T
    type: related
  - target: 01KNZ5YRJ0QSF5TEDG6FFGE6SS
    type: related
  - target: 01KNZ5YRJAFN3CMW4QBMEDJKA6
    type: related
  - target: 01KNZ301FVPT3WBK9D4AHAN5ZB
    type: related
  - target: 01KNWE2Q9YZBAR140ZX5P36TQ5
    type: related
created: 2026-04-11T21:07:53.436131+00:00
modified: 2026-04-11T21:07:53.436137+00:00
---

Source: https://github.com/google/pprof — Google pprof README, fetched 2026-04-12.

pprof is a visualization and analysis tool for profiling data, originally developed inside Google and open-sourced as part of the Go toolchain (and as a standalone Go command `github.com/google/pprof`). It is the reference implementation for the `profile.proto` protocol buffer format, which has become the de facto open-source interchange format for CPU, memory, and contention profiles across languages.

## What it consumes

pprof operates on profiles in the `profile.proto` format: a protocol buffer describing a set of callstack samples plus symbolization information. Profiles can come from:
- Go's runtime (`runtime/pprof` and `net/http/pprof`).
- C++ and Java via the perftools / gperftools / Prometheus bindings.
- Rust via `pprof-rs`.
- Any perf.data file via `perf_to_profile` conversion.
- Pyroscope, Parca, and Datadog Continuous Profiler all produce pprof-compatible output.

## Profile types

For a Go process via `net/http/pprof`, pprof can fetch and analyse:
- **CPU** (`/debug/pprof/profile?seconds=30`) — sampled stack traces at ~100 Hz.
- **Heap** (`/debug/pprof/heap`) — currently-live allocations (`inuse_space`, `inuse_objects`) or cumulative (`alloc_space`, `alloc_objects`).
- **Goroutine** (`/debug/pprof/goroutine`) — stack traces of all goroutines. Useful for stuck-goroutine diagnosis.
- **Block** (`/debug/pprof/block`) — contention on synchronisation primitives (channels, mutexes waiting).
- **Mutex** (`/debug/pprof/mutex`) — holders of contended mutexes.
- **Threadcreate** (`/debug/pprof/threadcreate`) — OS thread creation events.
- **Allocs** — equivalent to heap `alloc_space` semantics.

Each profile type has a different resource-consumption axis, and pprof's --base/--diff_base flags let you compute deltas between two profiles (e.g. heap growth between T0 and T1).

## Analysis commands

Interactive shell commands:
- `top` — functions sorted by flat or cumulative contribution.
- `list funcname` — annotated source listing with per-line sample counts.
- `disasm funcname` — annotated assembly.
- `peek funcname` — callers and callees of a function.
- `web` — interactive browser view (Graphviz call graph).
- `tree`, `traces`, `raw` — different structural views.

CLI shortcuts:
- `pprof -top binary profile.pb.gz` — quick top.
- `pprof -http=:8080 profile.pb.gz` — web UI with flame graphs, source views, and call graphs (the preferred modern workflow).
- `pprof -svg profile.pb.gz > profile.svg` — call graph as SVG.
- `pprof -flamegraph profile.pb.gz` — Brendan Gregg-style flame graph.

## Comparing profiles

```
pprof -base before.pb.gz after.pb.gz
pprof -diff_base before.pb.gz after.pb.gz
```

The difference between `-base` and `-diff_base` is subtle but important: `-base` subtracts the baseline's samples from the current profile (how much *extra* did we consume?); `-diff_base` shows the delta in both directions (where did samples move?). For regression diagnosis `-diff_base` is what you usually want.

## Go runtime integration

A typical Go web service enables pprof with:

```go
import _ "net/http/pprof"
// ...
go func() { http.ListenAndServe(":6060", nil) }()
```

This registers the `/debug/pprof/...` handlers on the default mux. Profiles are captured live from the running process; no restart, no recompilation. This "always-on profile endpoint" pattern is one of the most important operational ideas pprof mainstreamed, and it is the foundation for continuous profiling systems like Pyroscope, Parca, and Datadog Continuous Profiler.

## Strengths

- Cross-language via `profile.proto`.
- Flame graphs, source annotations, and call graphs in one tool.
- Profile arithmetic (diffs, base-subtracted comparisons) is first-class.
- net/http/pprof is the gold standard for "enable profiling on a live service".

## Failure modes

- **Sampling bias** on Go's CPU profiler — known limitations at function-inlining boundaries, and on very short runs where sampling noise dominates signal.
- **Allocation profiles are sampled** (every 512 KiB by default) — rare but large allocations may not appear.
- **Symbolization** requires the binary with debug info (or a matching `.dSYM`/split-debug) — stripped production binaries produce unreadable profiles.
- **Block/mutex profiling** must be explicitly enabled (`runtime.SetBlockProfileRate`, `SetMutexProfileFraction`) and has non-trivial overhead — do not leave on by default in production.
- **Go's CPU profiler has historical known-issue with SIGPROF on cgo-heavy programs** — accuracy degrades with cgo workloads.

## Relevance to APEX G-46

pprof is the reference for "language-neutral profile interchange" — APEX's resource-profiling phase (spec component 4 in G-46) should emit and consume `profile.proto` where possible, enabling users to pass APEX-captured profiles to any pprof-compatible visualisation. pprof's `-diff_base` semantics are a direct model for G-46's performance-regression-detection comparison.