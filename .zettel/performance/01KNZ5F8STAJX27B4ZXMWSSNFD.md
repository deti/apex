---
id: 01KNZ5F8STAJX27B4ZXMWSSNFD
title: Bombardier — fasthttp-based Go HTTP benchmark
type: literature
tags: [tool, bombardier, load-testing, go, fasthttp, http, closed-model]
links:
  - target: 01KNZ5F8VABE5TGW976NMQA1VP
    type: related
  - target: 01KNZ4VB6JB4Q5H3NPS72MZZ2A
    type: related
  - target: 01KNWE2Q9YZBAR140ZX5P36TQ5
    type: related
created: 2026-04-11T20:59:25.882538+00:00
modified: 2026-04-11T20:59:25.882540+00:00
---

Source: https://github.com/codesenberg/bombardier — Bombardier README, fetched 2026-04-12.

Bombardier is a cross-platform HTTP benchmarking tool written in Go by Alexey Ivanov. It is built on top of the `fasthttp` library, which makes it notably faster than tools built on Go's stdlib `net/http` — fasthttp bypasses the stdlib HTTP allocator and reuses connection objects aggressively. MIT license.

## Model

Bombardier is closed-loop like hey/wrk/Siege: concurrent connections (`-c`) and either a fixed request count (`-n`) or duration (`-d`).

```
bombardier -c 250 -d 30s -l https://api.example.com/items
bombardier -c 100 -n 1000000 --http2 https://api.example.com/
```

- `-c` — concurrent connections (default 125).
- `-d` — duration (e.g. 30s, 5m).
- `-n` — fixed total requests.
- `-l` — enable latency distribution reporting.
- `-H` — headers; `-b` — body; `-m` — method.
- `--http1` / `--http2` — switch from fasthttp (HTTP/1.1) to stdlib `net/http` client for HTTP/2 testing and strict RFC compliance.

## Output

End-of-run summary with requests/sec, latency (mean, stdev, max), latency percentiles (50, 75, 90, 99), throughput (MB/s), and HTTP status-code distribution (1xx/2xx/3xx/4xx/5xx bucketed). Latency histogram available via `-p` (plain), `-r` (plain + response histogram), or `-o json`.

## Strengths

- Highest out-of-the-box single-host throughput among Go HTTP benchmarks, thanks to fasthttp.
- Stdlib `--http2` fallback for protocol correctness when fasthttp's limitations bite.
- Sane CLI, minimal config.
- Percentiles in the default summary.

## Failure modes

- **fasthttp is not a standards-compliant HTTP client** — it cuts corners for speed (Host header handling has known issues per the README, case-sensitive header comparison, non-standard chunked behaviour). Use `--http1`/`--http2` when the target cares about correctness.
- **Closed-loop** — same coordinated-omission problem as wrk.
- **No scenarios**, single URL per run, no session state.
- **Thread-model overhead still matters** — very high `-c` values on slow backends pile up goroutines.
- **No pre-generated target files** — every request is the same URL plus whatever is in `-b`.

## When to reach for Bombardier

- "How fast can a single machine flood this endpoint?" benchmarks.
- CI jobs that need a single-binary tool with percentile output and no dependencies.

## Relevance to APEX G-46

Bombardier's fasthttp vs stdlib toggle is a useful example of a performance/correctness tradeoff that APEX's own resource-measurement layer will have to make: "measure as fast as possible" vs "measure what production actually sees" are not the same number, and the difference can be order-of-magnitude.