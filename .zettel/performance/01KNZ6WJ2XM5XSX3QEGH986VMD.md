---
id: 01KNZ6WJ2XM5XSX3QEGH986VMD
title: "Coordinated Omission and Gil Tene's wrk2"
type: literature
tags: [coordinated-omission, gil-tene, wrk2, hdrhistogram, open-loop, load-testing, latency-measurement]
links:
  - target: 01KNZ4VB6JB4Q5H3NPS72MZZ2A
    type: related
  - target: 01KNZ4VB6JPYBYW64S7NNYS1CM
    type: related
  - target: 01KNZ56MSNRKGMDGT9B745HVEB
    type: related
  - target: 01KNZ6FPTDYDB44VYN8Z4DQ7F4
    type: related
  - target: 01KNZ56MPVSZD05KM395ZKAM5J
    type: related
  - target: 01KNWE2Q9YZBAR140ZX5P36TQ5
    type: related
created: 2026-04-11T21:24:09.949909+00:00
modified: 2026-04-11T21:24:09.949915+00:00
source: "https://github.com/giltene/wrk2"
---

# Coordinated Omission and wrk2

*Gil Tene (Azul Systems) coined "coordinated omission" as the name for a specific measurement pathology in load testing. His tool wrk2 (github.com/giltene/wrk2) is the canonical correct-by-construction open-loop HTTP benchmark; his HdrHistogram library is the de-facto high-precision latency-storage format. Both matter enormously for performance test generation because they are the only widely adopted pieces of tooling that explicitly fix the open-vs-closed measurement bug.*

## Coordinated omission — what it is

Load generators that run in closed-loop style (send a request, wait for response, send next request) fall into this pathology:

1. The target service becomes slow (whatever the reason — GC, lock contention, backend hiccup).
2. The load generator, waiting for a response, does *not* send the next scheduled request.
3. When the target recovers, the load generator sends the *backlog* of requests in rapid succession.
4. Each backlogged request's latency is measured from when it was *sent*, not from when it was *scheduled*.
5. The load generator reports all the catch-up requests as fast — because by the time it sends them, the server is responding quickly.

Result: the slow period is **invisible in the latency histogram**. The p99 reports the catch-up period, not the slow period. The measured p99 is drastically better than the true p99. Coordinated omission is a systematic measurement error that makes bad systems look good.

Gil Tene's talks (YouTube search: "Gil Tene coordinated omission") explain this with examples from real production systems where the same workload generated latency histograms that differ by 2-3 orders of magnitude in p99 depending on whether the measurement was CO-corrected.

## How wrk2 fixes it

wrk2 is a fork of the original wrk (Will Glozer's multi-threaded HTTP benchmarking tool). The "2" means: constant throughput, correlated histograms, coordinated-omission-corrected measurement.

### Key differences from wrk

1. **Fixed throughput, not open concurrency.** You specify `-R 10000` (10k RPS). wrk2 tries to maintain that rate regardless of server response. If the server slows down, wrk2 does **not** reduce its sending rate.
2. **Latency measured from scheduled time.** When wrk2 sends a request that was scheduled for time T, the latency it records is (response time) − T, not (response time) − (actual send time). Backlog is fully captured.
3. **HdrHistogram output.** Latencies are stored in an HdrHistogram, a high-dynamic-range histogram that losslessly captures values across many orders of magnitude with small memory footprint.
4. **Percentile reporting in the output.** Every wrk2 run prints p50/p75/p90/p99/p99.9/p99.99 with correct statistics.

### Usage

```sh
wrk2 -t4 -c400 -d60s -R10000 --latency https://target.example.com/
```

`-t4` four threads, `-c400` 400 connections, `-d60s` 60 second run, `-R10000` target 10k RPS, `--latency` print percentiles.

## HdrHistogram — the latency storage format

Gil Tene also maintains HdrHistogram (http://hdrhistogram.org/), a lossless histogram that handles wide dynamic range. It's been ported to Java, C, Go, Rust, Python, and more. Many modern load tools use it under the hood (k6 has it for certain metrics; wrk2 uses it for all latency).

The importance of HdrHistogram for perf test generation: it means you can *report* p99.9 and p99.99 accurately with only a few kilobytes of memory. Before HdrHistogram, accurately tracking these tail percentiles required sampling with all the attendant bias, or storing every observation.

## Why wrk2 matters for this research

The open-workload fix is conceptually simple but practically rare. Most load-test code in the wild uses closed-loop generators and reports CO-corrupted latencies. Teams trust the p99 numbers because they come from a "real" tool. Gil Tene's tools and talks are the single biggest cultural fix for this, and every serious perf engineer needs to know them.

For test generation specifically:

- Any tool that emits load-test scripts should default to open-loop executors.
- Any tool that measures response time should apply coordinated-omission correction.
- Any tool that reports percentiles should use HdrHistogram (or equivalent) under the hood.
- Any LLM that generates load tests should be prompted to emit `constant-arrival-rate` or `ramping-arrival-rate` executors in k6, not `constant-vus`.

## Adversarial reading

1. **Constant-throughput is not always realistic.** Real production arrivals are Poisson-like, not constant. wrk2's constant-throughput model is a step better than closed-loop but still a simplification. Proper Poisson generators (few exist in open source) are even better.
2. **wrk2 is unmaintained.** Gil Tene's last commit to the repo is from years ago. It works, but feature additions have moved elsewhere (k6, hyperfoil).
3. **Not a full load-test platform.** wrk2 is a stand-alone HTTP benchmark, not an orchestration platform. You can't run it across many machines or wire it into CI easily. For full-featured load testing you use k6 or Gatling with open-loop executors, and lose some of wrk2's elegance.
4. **Single-protocol.** HTTP only. No gRPC, WebSocket, etc.

## Related

- **hyperfoil** — a newer open-source distributed load generator that implements correct open-loop measurement and coordinated-omission correction at scale. Worth knowing as the "wrk2 ideas at cluster scale" evolution.
- **Fortio** — Istio's load-testing tool, also open-loop by default.
- **Vegeta** — Go-based HTTP load tester with native constant-rate support.

## Citations

- wrk2 repo: https://github.com/giltene/wrk2
- HdrHistogram: http://hdrhistogram.org/
- Gil Tene's coordinated omission talk: https://www.youtube.com/watch?v=lJ8ydIuPFeU
- Schroeder open-vs-closed paper (already in vault): https://www.usenix.org/legacy/events/nsdi06/tech/schroeder/schroeder.pdf
- hyperfoil: https://hyperfoil.io/
- Fortio: https://fortio.org/
- Vegeta: https://github.com/tsenart/vegeta