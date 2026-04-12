---
id: 01KNZ5F8T45J3RZRCTBVN8W526
title: autocannon — Node.js HTTP benchmark with pipelining and worker threads
type: literature
tags: [tool, autocannon, load-testing, nodejs, http, pipelining, closed-model]
links:
  - target: 01KNZ5F8VABE5TGW976NMQA1VP
    type: related
  - target: 01KNZ4VB6JB4Q5H3NPS72MZZ2A
    type: related
  - target: 01KNWE2Q9YZBAR140ZX5P36TQ5
    type: related
created: 2026-04-11T20:59:25.892371+00:00
modified: 2026-04-11T20:59:25.892373+00:00
---

Source: https://github.com/mcollina/autocannon — autocannon README, fetched 2026-04-12.

autocannon is an HTTP/1.1 benchmarking tool written in Node.js by Matteo Collina, first released in 2016. Explicitly "inspired by wrk and wrk2 but written in Node so you can use it as a library." MIT license.

## Model

autocannon is closed-loop with pipelining support. Default model: maintain `-c` concurrent connections, each with `-p` pipelined requests in-flight, for `-d` seconds or `-a` amount of requests.

```
autocannon -c 100 -d 40 -p 10 https://api.example.com/items
```

- `-c 100` — 100 connections.
- `-d 40` — run for 40 seconds.
- `-p 10` — pipeline 10 requests per connection.
- `-w` — worker threads for SMP scaling.
- `-R` — overall rate limit (constant throughput mode).

Pipelining is important: autocannon can saturate a Node.js HTTP server at rates Node-based load generators typically cannot reach, because the target can process multiple requests per round trip.

## Request factory

Scripts can drive autocannon as a Node library:

```javascript
const autocannon = require('autocannon');

const instance = autocannon({
  url: 'https://api.example.com',
  connections: 100,
  duration: 30,
  requests: [
    { method: 'POST', path: '/login', body: '{"u":"x"}' },
    { method: 'GET', path: '/cart' },
  ],
});

autocannon.track(instance);
```

The `requests` array defines a sequence of requests each connection issues. Dynamic bodies and captures are supported via the `setupClient` hook.

## Output

Latency percentiles at 2.5, 50, 97.5, and 99; RPS percentiles at the same points; throughput (bytes/sec percentiles). The latency distribution is computed over a streaming histogram — not HdrHistogram, and the percentile set is thinner than wrk2 or k6.

## Worker threads mode

`-w N` spawns N Node.js worker threads, each running an independent autocannon. Results are aggregated in the main thread. This is what lets a single invocation saturate multi-core machines without separate processes.

## Strengths

- Best-in-class for benchmarking Node.js services (same runtime, same HTTP client behaviour as clients).
- Library-first — embeds cleanly in CI scripts.
- HTTP pipelining support (most others do not).
- Worker threads for SMP scaling in a single command.

## Failure modes

- **Percentile set is limited** — no p99.9 / p999 out of the box.
- **Closed-loop by default** — the `-R` rate knob exists but is not the default.
- **Node.js has its own perf foibles** — event-loop blocking, GC pauses. At very high load the load generator itself exhibits the behaviours it is testing for.
- **HTTP/1.1 only**, no HTTP/2 / HTTP/3.
- **Scenarios are request arrays**, not real flows with captures and branching.
- **No distributed mode** — multiple machines means running autocannon N times and aggregating by hand.

## Relevance to APEX G-46

For the Node.js ecosystem autocannon plays the role k6 plays for the JS ecosystem at large — a familiar runtime lowering the cost of writing performance tests. APEX's output recipes for Node.js targets should prefer autocannon syntax where possible.