---
id: 01KNZ5F8SK9T91MS5SQGQ0ERXV
title: hey — minimal Go HTTP load generator (Apache Bench successor)
type: literature
tags: [tool, hey, load-testing, go, http, closed-model, apache-bench]
links:
  - target: 01KNZ5F8VABE5TGW976NMQA1VP
    type: related
  - target: 01KNZ4VB6JB4Q5H3NPS72MZZ2A
    type: related
  - target: 01KNWE2Q9YZBAR140ZX5P36TQ5
    type: related
created: 2026-04-11T20:59:25.875119+00:00
modified: 2026-04-11T20:59:25.875121+00:00
---

Source: https://github.com/rakyll/hey — hey README, fetched 2026-04-12.

hey is a tiny HTTP load generator written in Go by Jaana Dogan (rakyll), first released in 2016 as a modern replacement for Apache Bench (`ab`). It is single-purpose, single-file (roughly 1k LoC), and has no configuration beyond CLI flags. Apache-2.0.

## Model

hey is closed-loop concurrency:

```
hey -n 10000 -c 50 -q 100 https://api.example.com/items
```

- `-n 10000` — total requests.
- `-c 50` — concurrent workers.
- `-q 100` — rate limit in queries-per-second per worker (so 50 workers × 100 qps = 5000 qps ceiling).
- `-z 30s` — run for duration instead of fixed request count.

Supports custom HTTP method (`-m`), headers (`-H`), body (`-d` inline or `-D file`), HTTP/2 (`-h2`), proxy (`-x`), disabling keepalive (`-disable-keepalive`).

## Output

hey prints a summary with latency histogram bucket counts, percentiles (10, 25, 50, 75, 90, 95, 99), and a per-status-code breakdown. Optional CSV output via `-o csv`.

Example output form: "Summary" block with total, slowest, fastest, average, RPS, histogram over response-time bins, latency distribution percentiles, details (DNS+dialup, DNS-lookup, req write, resp wait, resp read), status code distribution.

## Strengths

- Single Go binary, trivial install.
- Sane defaults, zero config.
- Percentile reporting out of the box (unlike Siege).
- HTTP/2 support.
- Replacing `ab` removes `ab`'s well-known ancient-HTTP-client bugs and keepalive weirdness.

## Failure modes

- **Closed-loop** — same coordinated-omission trap as wrk, Siege, `ab`.
- **The rate limit is per-worker, not global** — easy to misconfigure. Total QPS is `c * q`, not `q`.
- **Single-host only**, no distributed mode.
- **No scenarios** — one URL at a time.
- **No scripting** — no per-request dynamic bodies without a wrapper.
- **Has been quasi-abandoned** — very few commits since 2020, the `rakyll/hey` repo is more "archive" than "maintained". Still works fine for its scope.

## Relevance to APEX G-46

hey is the "Apache Bench successor" reference in the landscape. Its percentile reporting and simple CLI make it a natural fit for APEX when emitting a one-off "reproduce this worst-case input" shell command for a discovered finding. `hey -n 100 -c 10 -H "Content-Type: application/json" -d @payload.json $URL` is the shortest possible reproduction recipe.