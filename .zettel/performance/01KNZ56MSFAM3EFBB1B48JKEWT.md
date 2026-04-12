---
id: 01KNZ56MSFAM3EFBB1B48JKEWT
title: Vegeta — constant-rate HTTP load generator with UNIX-pipeline composition
type: literature
tags: [tool, vegeta, load-testing, go, http, constant-rate, open-model, unix-philosophy]
links:
  - target: 01KNZ5F8VABE5TGW976NMQA1VP
    type: related
  - target: 01KNZ4VB6JX0CQ5RFAZDJTQMCS
    type: related
  - target: 01KNZ4VB6JB4Q5H3NPS72MZZ2A
    type: related
  - target: 01KNZ56MPVSZD05KM395ZKAM5J
    type: related
  - target: 01KNWE2Q9YZBAR140ZX5P36TQ5
    type: related
created: 2026-04-11T20:54:43.247620+00:00
modified: 2026-04-11T20:54:43.247622+00:00
---

Source: https://github.com/tsenart/vegeta — Vegeta README, fetched 2026-04-12.

Vegeta is a command-line HTTP load testing tool written in Go by Tomás Senart, first released in 2014. Tagline: *"drill HTTP services with a constant request rate."* It is the canonical "unix-philosophy" load generator — one job, well-defined I/O contracts, pipeable.

## Attack model

Vegeta's load model is **constant arrival rate**, period. You specify `-rate=500/1s` and Vegeta issues exactly 500 HTTP requests per second, regardless of how slowly the target responds. This is the open model and it is coordinated-omission-correct by default. There is no thread-pool knob, no concurrency dial — under the hood Vegeta maintains enough in-flight requests to sustain the target rate and reports the latency between "time the request was scheduled" and "time the response arrived", not "time the request was actually sent".

Recent versions (v12+) add `-rate=0` with `-max-workers` for a "drain as fast as possible with N workers" closed-ish mode, but constant-rate is the first-class model.

## Subcommands

- `attack` — generate the load, output results as a binary `gob`/`protobuf` stream.
- `report` — aggregate a results stream into text, JSON, histogram, or Hdrhistogram.
- `plot` — render an HTML+JS time-series latency plot.
- `encode` — convert between result formats (`gob`, JSON, CSV).
- `dump` — raw dump of results for debugging.

The UNIX composition is in the pipeline:

```
echo "GET http://localhost/" | \
  vegeta attack -rate=100/1s -duration=30s | \
  tee results.bin | \
  vegeta report
vegeta report -type=hist[0,10ms,100ms,1s] < results.bin
vegeta plot < results.bin > plot.html
```

Targets can be inline (`echo "METHOD url"`) or a file with one target per line, with optional headers, bodies (via `@filename`), and HTTP/2 support.

## Library usage

Vegeta is also a Go library (`github.com/tsenart/vegeta/lib`). Tests can embed the attacker directly and post-process results in Go. This makes Vegeta a common building block for custom load tools.

## Output

Reports include: requests sent, success rate, latencies (mean, 50, 95, 99, max, min), throughput, and by-status-code breakdown. Hdrhistogram output is what you want for tail-latency analysis.

## Strengths

- Constant-rate is the correct default. No footgun.
- Stdin/stdout streaming means you can run a 24-hour attack and write results to disk in chunks without OOM.
- Single static Go binary, trivial distribution.
- Library is clean and small.
- HdrHistogram output.

## Failure modes

- **HTTP only** — no WebSocket, gRPC, JMS. If you need multi-protocol, look elsewhere.
- **No scenarios** — there is no "log in then browse then check out" flow. Targets are independent per-request. The workaround is pre-generating target files from a scenario description in another tool.
- **No concurrency ceiling** — at very high rates against a slow backend, Vegeta can pile up millions of in-flight goroutines. The `-max-workers` knob exists but must be tuned by hand.
- **Target-file format is quirky** — multi-line bodies are awkward; JSON targets are the practical escape hatch.
- **No dynamic data / checks** — response bodies are not inspected beyond status code unless you post-process the gob stream.

## Relevance to APEX G-46

Vegeta is the minimal case in the G-46 landscape: essentially "what if the load generator did just the one thing". For APEX, Vegeta's target-file format is the natural output format when emitting discovered worst-case HTTP inputs — a `.targets` file can be piped directly into a Vegeta attack for sustained-load reproduction.