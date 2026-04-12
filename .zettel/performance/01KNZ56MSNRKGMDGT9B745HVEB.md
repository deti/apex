---
id: 01KNZ56MSNRKGMDGT9B745HVEB
title: wrk and wrk2 — C HTTP benchmarks and the coordinated-omission fix
type: literature
tags: [tool, wrk, wrk2, load-testing, c, luajit, coordinated-omission, hdrhistogram, tail-latency]
links:
  - target: 01KNZ5F8VABE5TGW976NMQA1VP
    type: related
  - target: 01KNZ4VB6JB4Q5H3NPS72MZZ2A
    type: related
  - target: 01KNZ4VB6JPYBYW64S7NNYS1CM
    type: related
  - target: 01KNZ6FPTDYDB44VYN8Z4DQ7F4
    type: related
  - target: 01KNZ6WJ2XM5XSX3QEGH986VMD
    type: related
  - target: 01KNWE2Q9YZBAR140ZX5P36TQ5
    type: related
created: 2026-04-11T20:54:43.253938+00:00
modified: 2026-04-11T20:54:43.253940+00:00
---

Sources: https://github.com/wg/wrk and https://github.com/giltene/wrk2 — fetched 2026-04-12.

wrk and wrk2 are a pair of HTTP benchmarking tools that together illustrate the "coordinated omission" concept better than any other pair of tools. They are small C programs (a few thousand lines each) that saturate a local network from a single box. wrk was written by Will Glozer in 2012; wrk2 is Gil Tene's 2014 fork that corrects the coordinated-omission flaw.

## wrk architecture

wrk is a multithreaded, event-driven HTTP/1.1 benchmark. It uses one OS thread per `-t` flag and maintains `-c / -t` persistent connections per thread via epoll (Linux) or kqueue (BSD/macOS). Each connection issues a request, waits for the response, and immediately issues the next request. This is a **closed-loop** concurrency model: exactly `-c` requests are ever in-flight.

```
wrk -t 4 -c 400 -d 30s --latency http://localhost:8080/
```

wrk embeds a LuaJIT runtime. Scripts implement the lifecycle hooks:
- `init(args)` — once per thread
- `request()` — called to generate each request (returns headers + body)
- `response(status, headers, body)` — called on each response
- `done(summary, latency, requests)` — called at end

This is enough to randomise URLs, authenticate, and collect custom stats.

## wrk output

```
Running 30s test @ http://localhost:8080/
  4 threads and 400 connections
  Thread Stats   Avg      Stdev     Max   +/- Stdev
    Latency    12.34ms    5.67ms  120.0ms   78.50%
    Req/Sec     8.12k     1.23k   10.5k    83.20%
  Latency Distribution
     50%   11.00ms
     75%   15.00ms
     90%   20.00ms
     99%   50.00ms
  971234 requests in 30.00s, 120.5MB read
Requests/sec:  32374.47
Transfer/sec:     4.01MB
```

## The coordinated omission problem

wrk reports latency from "request sent" to "response received". Because wrk is closed-loop, if the server stalls for one second, the load generator sits and waits too — and therefore only one sample was taken in that one-second window. The latency buckets between "what the request would have seen if it had been sent on schedule" and "what the single request that was actually sent saw" are silently discarded. The 99th percentile in wrk's output is dramatically better than the 99th percentile the server would have exhibited under a true constant-rate workload.

Gil Tene's `--u_latency` flag in wrk2 demonstrates this: on a test deliberately stalling for 100ms mid-run, wrk and uncorrected wrk2 report p99 around 20 ms; the corrected wrk2 reports p99 around 100 ms — a 5x difference.

## What wrk2 fixes

wrk2 adds a required `-R / --rate` flag specifying total requests/sec across all connections. Internally:

1. **Constant throughput schedule** — requests are due at `start + i * (1/rate)`. The load generator tracks when each request *should* have been sent.
2. **Latency measurement anchored to the schedule** — `latency = response_time - scheduled_send_time`, not `response_time - actual_send_time`.
3. **HdrHistogram substitution** — wrk's latency sample buffer is replaced with an HDR histogram that records lossless quantiles up to 99.9999th percentile given enough run time.
4. **`--u_latency` flag** — outputs both corrected and "uncorrected" (wrk-style) distributions side-by-side so users can see the gap.

wrk2's README explains: *"high latency responses result in the load generator coordinating with the server to avoid measurement during high latency periods."*

## When to use each

- **wrk** — quick smoke test of a local service. What's the ceiling? Rough p50/p99? Good enough.
- **wrk2** — anything where the p99/p999 number is load-bearing (SLO validation, regression gates, tail-latency research).

## Failure modes

- **wrk closed-loop lies under a stalling server** — the entire reason wrk2 exists.
- **Single-host only** — neither tool has distributed mode; you run N copies and merge results manually.
- **HTTP/1.1 only** in both (there is no HTTP/2 support in upstream wrk; forks exist).
- **LuaJIT in the hot path** — complex scripts with per-request logic measurably lose throughput.
- **No body-level checks** in wrk; wrk2 inherits this. If you need "did the response contain X", you script it.
- **wrk2 fell behind warning** — if the machine cannot sustain the target rate, wrk2 prints a warning and the corrected latencies start reflecting the scheduling gap.

## Strengths

- Tiny, understandable C that teaches you HTTP I/O.
- Highest single-box HTTP/1.1 RPS of any OSS tool for comparable test shapes.
- wrk2 is the canonical reference implementation of a coordinated-omission-correct load generator.

## Relevance to APEX G-46

The wrk / wrk2 pair is the textbook argument for why G-46's performance measurement layer must distinguish scheduled-send-time from actual-send-time when computing latency SLOs. APEX's resource-measurement methodology should adopt wrk2's "schedule anchors the latency math" rule as a first principle.