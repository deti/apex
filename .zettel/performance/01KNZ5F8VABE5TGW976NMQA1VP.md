---
id: 01KNZ5F8VABE5TGW976NMQA1VP
title: "Comparative matrix: open-source load generators"
type: literature
tags: [comparative, load-testing, matrix, load-generators, overview]
links:
  - target: 01KNZ56MPVSZD05KM395ZKAM5J
    type: references
  - target: 01KNZ56MRW2B1XSH2X5K5AEJ33
    type: references
  - target: 01KNZ56MS27NFFMFBHZ0WPCV70
    type: references
  - target: 01KNZ56MS9HQJ2HJ2ADJ7MBMAX
    type: references
  - target: 01KNZ706C8534Q8VXTRTE1F6TB
    type: related
  - target: 01KNZ56MSFAM3EFBB1B48JKEWT
    type: references
  - target: 01KNZ56MSNRKGMDGT9B745HVEB
    type: references
  - target: 01KNZ56MSWFGRCEDBEZ2XJ9PZF
    type: references
  - target: 01KNZ6QBAEKEYVV66VQ2W1PY3R
    type: related
  - target: 01KNZ5F8Q36WVG78WJ525WHDR0
    type: references
  - target: 01KNZ5F8S3YZFEGJX006A5WRA5
    type: references
  - target: 01KNZ5F8SB2W8HA76NC27J0P2S
    type: references
  - target: 01KNZ5F8SK9T91MS5SQGQ0ERXV
    type: references
  - target: 01KNZ5F8STAJX27B4ZXMWSSNFD
    type: references
  - target: 01KNZ5F8T45J3RZRCTBVN8W526
    type: references
  - target: 01KNZ5F8TB2GZGFQE4M9NVBM9E
    type: references
  - target: 01KNZ5F8TK2AR78RGESX54MZKQ
    type: references
  - target: 01KNZ5F8TVW837C1YJKKXFH504
    type: references
  - target: 01KNZ5F8V2F1AXVHP058DM3B4H
    type: references
  - target: 01KNZ4VB6JB4Q5H3NPS72MZZ2A
    type: related
  - target: 01KNZ4VB6JPYBYW64S7NNYS1CM
    type: related
  - target: 01KNZ4VB6JX0CQ5RFAZDJTQMCS
    type: related
  - target: 01KNWE2Q9YZBAR140ZX5P36TQ5
    type: related
  - target: 01KNZ5QA1KKW97555M789HA46V
    type: related
  - target: 01KNZ5QA24034HTXECJAA7EFY7
    type: related
  - target: 01KNZ5QA2C29D8A0NRJT4Z7YYE
    type: related
  - target: 01KNZ5QA2NZ2MVHYDQVT1SGGSJ
    type: related
  - target: 01KNZ5QA3843GNFYK8KMVXDSY5
    type: related
created: 2026-04-11T20:59:25.930440+00:00
modified: 2026-04-11T20:59:25.930442+00:00
---

A structured comparison of the open-source load generators documented in this vault (batch of 2026-04-12) plus the pre-existing k6 reference note. Cross-links are the individual per-tool notes.

## Core comparison

| Tool | Language/runtime | Workload model (default) | Scripting | Scenarios/flows | Distributed OSS | Percentile reporting |
|---|---|---|---|---|---|---|
| k6 | Go + JS (goja) | Closed + open (arrival-rate executors) | JavaScript | Yes | execution-segment / k8s-operator | p(95), p(99), configurable |
| Gatling | JVM (Scala/Java/Kotlin) | Explicit open or closed | Scala/Java/Kotlin DSL | Yes | Enterprise only | p50/p75/p95/p99 |
| JMeter | JVM | Closed (thread group) | JSR223 / Groovy | Yes | RMI master/slave (brittle) | Full, via report generator |
| Locust | Python + gevent | Closed | Python | Yes | ZeroMQ master/worker | p50..p99.99 |
| Goose | Rust + Tokio | Closed | Rust | Yes | Manager/worker (experimental) | Configurable |
| NBomber | .NET (C#/F#) | Explicit open or closed (Inject) | C#/F# | Yes (step model) | Cluster mode Enterprise | Standard set |
| Tsung | Erlang | Open (arrivalphase) | XML | Yes (session) | Erlang distribution (robust) | Quantiles |
| Artillery | Node.js | Open (phases) | YAML + JS hooks | Yes | `run-fargate` (Pro) | p50..p99 |
| Vegeta | Go | Open (constant-rate) | CLI/library; targets file | No (per-request only) | Shell composition | HDR histogram available |
| wrk | C + LuaJIT | Closed | Lua | Minimal | None | Mean+stdev (misleading for tail) |
| wrk2 | C + LuaJIT | Open (constant-rate) | Lua | Minimal | None | HDR histogram |
| Fortio | Go | Open (QPS target, Poisson optional) | CLI/library | No (single URL) | None | Log-bucket histogram |
| Bombardier | Go + fasthttp | Closed | CLI | No | None | p50/p75/p90/p99 |
| hey | Go | Closed | CLI | No | None | p10..p99 |
| autocannon | Node.js | Closed (+ `-R` rate) | JS library / CLI | Request-array sequences | None | p2.5/p50/p97.5/p99 |
| Drill | Rust | Closed | YAML | Yes (plans) | None | Basic stats |
| Siege | C | Closed | URL file | Sequential URL list | None | **Mean/min/max only** |

## The three axes that matter

### 1. Open vs closed workload model

The single most important property for tail-latency work. Open-model (rate-driven) tools measure what the server would see under constant arrival rate; closed-model (concurrency-driven) tools under-report the tail via coordinated omission when the server stalls. See wrk/wrk2 for the canonical demonstration.

**Open-model defaults:** Vegeta, Fortio, wrk2, Artillery, Tsung, k6 (`constant-arrival-rate`).
**Can do open-model via explicit executor/simulation/flag:** Gatling, NBomber, k6 (optional).
**Closed-model only:** wrk, Siege, hey, Bombardier, JMeter thread group, Locust default, Goose, Drill.

Tools that support both open and closed modes are usually the right choice for anything serious.

### 2. Distributed execution

All OSS load generators are essentially single-host unless you buy the commercial version or hand-roll multi-process coordination. The actually-distributed tools are:

- **Tsung** — Erlang distribution, works well over SSH.
- **Locust** — ZeroMQ master/worker, well-documented.
- **JMeter** — RMI, brittle but documented.
- **k6** — Kubernetes operator or execution-segment sharding, community-driven.
- **Goose** — manager/worker, experimental.

Everyone else assumes you run N copies and merge results manually, or you pay for the cloud variant.

### 3. Scenarios and session state

"Log in, fetch the cart, check out" requires captures, per-user variables, and ordered multi-step flows. Tools that have this as first-class: k6, Gatling, JMeter, Locust, Goose, NBomber, Artillery, Tsung, Drill. Tools that don't: Vegeta, Fortio, wrk, wrk2, hey, Bombardier, Siege, autocannon (sequence-only).

Without session state a tool is a throughput benchmark, not a load test.

## Recommendations by use case

- **SLO validation in CI for a web service (new project):** k6 with `constant-arrival-rate` and thresholds.
- **Enterprise legacy environment:** JMeter — the jmx files and BlazeMeter licenses are already there.
- **.NET shop:** NBomber.
- **Rust shop already writing Rust everywhere:** Goose.
- **Node.js microservice benchmark in a CI job:** autocannon.
- **Quick sanity check at a shell prompt:** hey or Vegeta.
- **Tail-latency research where p999 is load-bearing:** wrk2 or Fortio.
- **XMPP / AMQP load test at large concurrency:** Tsung.
- **Browser-level end-to-end load at synthetic-monitoring scale:** k6 browser or Artillery Playwright engine.
- **"Measure latency correctly by default":** Vegeta or Fortio. (Or wrk2 if you already know wrk.)

## The coordinated-omission corner

Of all the tools in this table, three were built from the ground up to avoid coordinated omission by default:

1. **wrk2** — Gil Tene's explicit fix of wrk.
2. **Vegeta** — constant-rate by first design.
3. **Fortio** — QPS target with optional Poisson distribution.

k6, Gatling, NBomber, Artillery, and Tsung can avoid it if you choose the right executor/simulation, but the default is either-or depending on version. Everyone else defaults to something that under-reports the tail.