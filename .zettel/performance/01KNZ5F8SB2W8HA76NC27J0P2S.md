---
id: 01KNZ5F8SB2W8HA76NC27J0P2S
title: "Siege — 1999-era C HTTP load tester (no percentiles, still ubiquitous)"
type: literature
tags: [tool, siege, load-testing, c, http, closed-model, historical]
links:
  - target: 01KNZ5F8VABE5TGW976NMQA1VP
    type: related
  - target: 01KNZ4VB6JB4Q5H3NPS72MZZ2A
    type: related
  - target: 01KNWE2Q9YZBAR140ZX5P36TQ5
    type: related
created: 2026-04-11T20:59:25.867726+00:00
modified: 2026-04-11T20:59:25.867728+00:00
---

Source: https://github.com/JoeDog/siege — Siege README on GitHub, fetched 2026-04-12.

Siege is an HTTP/HTTPS load testing and benchmarking utility written in C by Jeffrey Fulmer, first released in 1999. It is one of the oldest tools in the space (contemporary with Apache Bench), still maintained, and included by default in Debian/Ubuntu/Fedora package repositories. GPL-3.0.

## Model

Siege is closed-loop: you specify the number of concurrent simulated users (`-c`) and they hit URLs in a loop for a duration (`-t 30S`) or fixed repetitions (`-r 100`). There is no arrival-rate knob. Each "user" is one worker thread issuing blocking HTTPS requests against a target.

Modes:
- **Benchmark mode** (`-b`) — removes think-time delays; hit as hard as possible.
- **Internet mode** (`-i`) — simulates more realistic browsing with random URL selection from the URL list and random think times drawn from `delay` config.
- **Regression mode** — walks the URL list sequentially.

Minimal example:

```
siege -c 50 -t 1M http://localhost:8080/
siege -c 25 -t 30S -f urls.txt
```

The URL file format is one URL per line, optionally with a method, POST body, and headers.

## Output

Plain-text end-of-run summary: transactions, availability (2xx rate), elapsed time, data transferred, response time, transaction rate, throughput, concurrency, successful vs failed transactions. No percentiles — min/mean/max only. This is one of the single biggest differences between Siege and anything modern.

## Strengths

- Ubiquitous — already installed on most Linux systems.
- Minimal dependencies (OpenSSL, zlib optional).
- Quick "is the server alive under N users?" sanity check.
- `urls.txt` format is trivially scriptable.

## Failure modes

- **No percentile reporting** — mean-latency reporting hides the entire tail. Unusable for SLO work.
- **Closed-loop only** — all the coordinated-omission problems.
- **No assertions beyond HTTP status** — cannot validate response content.
- **Thread-per-user model** limits single-host concurrency to a few thousand before the kernel scheduler pushes back.
- **Reports flush to stdout as the run ends** — no streaming output to a time-series backend.
- **Limited scripting** — the URL file is all you get. No dynamic data, no captures, no session state.

## When Siege is still appropriate

- "Is it up under 20 users" smoke tests.
- Shell scripts where you want a one-liner health check.
- Legacy ops runbooks that already call `siege`.

## Not appropriate for

- Anything where you need p99 or p999.
- SLO validation.
- Workloads that require session state or multi-step flows.

## Relevance to APEX G-46

Siege represents the "historical minimum" reference point: what load testing looked like before percentile reporting was standard. Its continued existence on every Linux host is a useful reminder that "load testing" has many loose definitions and that a large fraction of production teams still think in terms of mean-latency-and-RPS rather than tail-latency-and-SLO.