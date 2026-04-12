---
id: 01KNZ6GW8J1VQ2XPF9E1V1BFHQ
title: gRPC Load Testing Beyond ghz — xk6-grpc and Gatling gRPC Plugin
type: literature
tags: [grpc, xk6-grpc, gatling-grpc, k6, gatling, load-testing, protobuf, test-generation]
links:
  - target: 01KNZ55P6BA1RE6Z5432ZYWG5W
    type: related
  - target: 01KNZ5F8TVW837C1YJKKXFH504
    type: related
  - target: 01KNZ56MPVSZD05KM395ZKAM5J
    type: related
  - target: 01KNZ56MRW2B1XSH2X5K5AEJ33
    type: related
  - target: 01KNWE2Q9YZBAR140ZX5P36TQ5
    type: related
created: 2026-04-11T21:17:47.154786+00:00
modified: 2026-04-11T21:17:47.154792+00:00
source: "https://k6.io/blog/performance-testing-grpc-services/"
---

# gRPC Load Testing Beyond ghz — xk6-grpc and Gatling gRPC Plugin

ghz (dedicated note) is the canonical single-purpose gRPC load tool. For anything more than one-off benchmarks, two mainstream options extend general-purpose load frameworks with gRPC support.

## xk6-grpc (k6 gRPC extension)

k6 is built on Go with an extension mechanism (xk6) that lets you compile custom builds with additional protocol support. Modern k6 ships gRPC support directly in core (moved from an xk6 extension into the main binary around k6 0.49), but the xk6-grpc archetype still matters for advanced cases.

### What it gives you on top of ghz

- **Full k6 scenario machinery.** Ramping arrival rate, shared state across VUs, data-driven parameterisation, stages, thresholds, checks. None of this is available in ghz.
- **JavaScript ergonomics.** Request data is JavaScript objects, not templated strings. Per-request logic (computing signatures, generating IDs) is natural.
- **Mixed-protocol scenarios.** A single k6 scenario can call HTTP, gRPC, and WebSockets in sequence. This matches real backend architectures where a session spans multiple protocols.
- **Stream support.** Client-streaming, server-streaming, and bidirectional streaming RPCs can be tested; k6's event loop makes streams natural to write.
- **Integration with k6's observability stack.** Prometheus remote-write, Grafana Cloud dashboards, InfluxDB — whatever the rest of the k6 setup uses.

### Limitations

- **Protobuf loading has to be explicit.** You provide a `.proto` file at script init time; k6 parses it. Unlike ghz, reflection-based loading is possible but less clean.
- **Throughput ceiling.** k6's per-VU Go execution adds overhead compared to ghz's minimal CLI. For maximum raw RPS in a single-node benchmark, ghz still wins.
- **Protoc plugin gap.** k6 does not generate typed stubs from the proto file automatically; you work with field names as strings.

## Gatling gRPC plugin

Gatling has a community-maintained gRPC plugin (gatling-grpc, from the phiSgr fork and its successors). Gatling's strengths transfer: the Scala DSL, the strong latency distribution reporting, the simulation-driven load model.

### When it's a fit

- Teams already running Gatling for HTTP load and wanting consistent tooling for gRPC services.
- Scala-heavy engineering orgs where the Gatling DSL is familiar.
- High-scale tests where Gatling's JVM-based async IO outperforms per-VU models.

### When it isn't

- Non-JVM teams. Gatling is a commitment to JVM infrastructure.
- Teams using k6 Cloud or Grafana-centric observability — Gatling has its own enterprise story (Gatling Enterprise) that doesn't integrate.

## What gRPC load testing tools consistently lack

1. **Stateful streaming workloads.** Long-lived bidi streams with realistic message-rate distributions. Every gRPC tool can *fire* a stream; none models the distribution of stream durations and message rates across a population.
2. **Deadline distribution modelling.** gRPC deadlines are per-request. Production deadlines follow a distribution (short for sync UI calls, long for batch jobs). No tool lets you specify a deadline distribution and check how the server handles the mix.
3. **Connection-pooling knobs that match reality.** Real clients multiplex many RPCs on few HTTP/2 connections, with per-connection stream limits. Test tools often use too few or too many connections relative to production.
4. **Protobuf field-level distribution control.** Same problem as graphql-faker: you can fire a "realistic" message but you can't say "20% have this field populated and 80% have this other field."
5. **Schema evolution testing.** Protobuf is designed for backward compatibility, but load tests rarely exercise old-client-new-server or new-client-old-server scenarios, which is where most compatibility perf bugs hide.

## Toolmaker gap

A declarative workload generator that reads a `.proto` file, a field-distribution spec (e.g., YAML), and a scenario spec (rate, duration, client concurrency) and produces a runnable gRPC load test would be immediately useful. None exists. It would be a ~2 week project on top of k6's gRPC extension.

## Citations

- k6 gRPC walkthrough: https://k6.io/blog/performance-testing-grpc-services/
- ghz home: https://ghz.sh/
- Gatling gRPC plugin (community): https://github.com/phiSgr/gatling-grpc
- gRPC benchmarking guide: https://grpc.io/docs/guides/benchmarking/
- Official gRPC performance benchmarks (C++): https://github.com/grpc/grpc/tree/master/tools/run_tests/performance