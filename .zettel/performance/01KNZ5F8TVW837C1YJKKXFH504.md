---
id: 01KNZ5F8TVW837C1YJKKXFH504
title: "Fortio — Istio's QPS-targeted Go load tester with gRPC and echo server"
type: literature
tags: [tool, fortio, load-testing, go, grpc, istio, open-model, poisson, qps]
links:
  - target: 01KNZ5F8VABE5TGW976NMQA1VP
    type: related
  - target: 01KNZ4VB6JNFK2XFWP9N1HEJ5M
    type: related
  - target: 01KNZ4VB6JX0CQ5RFAZDJTQMCS
    type: related
  - target: 01KNZ6GW8J1VQ2XPF9E1V1BFHQ
    type: related
  - target: 01KNWE2Q9YZBAR140ZX5P36TQ5
    type: related
created: 2026-04-11T20:59:25.915038+00:00
modified: 2026-04-11T20:59:25.915040+00:00
---

Source: https://github.com/fortio/fortio — Fortio README, fetched 2026-04-12.

Fortio is Istio's built-in load testing library and CLI, originally developed by Laurent Demailly at Google in 2017 to load-test the Istio service mesh, later spun out as an independent project. It is both a daemon (with a web UI and REST API) and a command-line tool. Apache-2.0.

## Model

Fortio's load model is **QPS-targeted** — you specify `-qps N` and Fortio issues exactly N requests per second. Internally it computes the inter-request interval (uniform or exponential distribution via `-uniform=false`) and maintains goroutines to fire requests at the scheduled times. This is open-model and coordinated-omission-correct: the `latency` metric is measured relative to when the request was due, not when it was sent.

```
fortio load -qps 100 -t 30s -c 10 https://api.example.com/items
fortio load -qps 100 -t 30s -uniform=false http://localhost/
```

- `-qps N` — target queries per second (0 means unlimited).
- `-t 30s` — duration; or `-n N` for fixed request count.
- `-c 10` — max concurrent in-flight.
- `-uniform=false` — use exponential inter-arrival distribution (Poisson process) instead of uniform.
- `-jitter` — jitter applied to scheduled times.

## Protocols

HTTP/HTTPS (1.1 and 2), gRPC (with ping/echo health checks), TCP, UDP. The gRPC support is the main feature that kept Fortio installed in every Istio operator's toolbox. `fortio server` starts a daemon that hosts the web UI, provides a REST API for triggering tests, and runs an echo server useful as a test target.

## Histogram output

Fortio reports a full latency histogram with configurable bucket resolution plus percentiles (p50, p75, p90, p99, p99.9). The histogram bins are log-spaced, not HdrHistogram's power-of-two, but precision is adequate for SLO work. Output formats: text, JSON, and a web UI that overlays results from multiple runs for before/after comparisons.

## Advanced echo server

`fortio server` doubles as a configurable echo server for testing load generators themselves: you can make it respond with configurable delays, status codes, payload sizes, and drop connections. This is the underrated feature — it lets you validate that *your load generator is measuring correctly* before trusting it against production.

## Strengths

- Open-model QPS target with uniform or exponential inter-arrival.
- First-class gRPC support (unique among OSS load testers of this size).
- Web UI for ad-hoc tests against any endpoint.
- Echo server is a perfect controlled test target.
- Small static Go binary; Docker image under 6 MB.
- Designed by people who knew coordinated-omission existed.

## Failure modes

- **Single-URL / single-target** — like Vegeta, no scenario/flow support.
- **No session state, no captures** — can't log in then do something else.
- **Reports are serviceable, not pretty** — no HTML dashboard, just JSON and the web UI.
- **No CSV data feeding** — all requests are structurally identical within one run.
- **Community is small outside of Istio users.**

## Relevance to APEX G-46

Fortio's `uniform=false` flag is the concrete realisation of "Poisson arrival process for load testing" — the correct statistical model for most real traffic. APEX's workload schema should support Poisson arrivals as a first-class option, and Fortio is the reference implementation to point at. The echo server is also useful as an APEX test target for validating that the resource-measurement layer sees what it thinks it sees.