---
id: 01KNZ5F5C3YS1EYDCVFQ7TQS9H
title: Trace-Driven Load Generation from Distributed Traces
type: permanent
tags: [distributed-tracing, jaeger, zipkin, tempo, opentelemetry, trace-replay, k6, xk6-client-tracing, test-generation]
links:
  - target: 01KNZ5F4WQ8VSNJBFYJFDSX7NT
    type: related
  - target: 01KNZ5F59R6M5ATD9X2YW87XAC
    type: related
  - target: 01KNZ56MPVSZD05KM395ZKAM5J
    type: related
  - target: 01KNZ666TDP7H8GRG9RF62384D
    type: related
  - target: 01KNZ4VB6J56B59YB7SZDKTAKD
    type: related
  - target: 01KNZ6QBKG64F2WEP3PNJK61JH
    type: related
  - target: 01KNWE2Q9YZBAR140ZX5P36TQ5
    type: related
created: 2026-04-11T20:59:22.371932+00:00
modified: 2026-04-11T20:59:22.371938+00:00
---

# Trace-Driven Load Generation — Using Jaeger/Zipkin/OpenTelemetry as Workload Source

Distributed tracing systems (Jaeger, Zipkin, Tempo, DataDog APM, New Relic, Honeycomb) capture the structure of every request as a tree of spans annotated with service, operation, start/end time, and optional tags. A "trace" is the per-request execution record that crosses service boundaries. In 2023+, most production backends produce traces via OpenTelemetry SDKs.

A trace is almost but not quite a workload specification. It tells you:

- Which service-to-service calls happened, in what order.
- Which operation names and HTTP methods and URLs.
- Which latencies at each hop.
- Often, which tags (user ID, tenant ID, feature flag) were attached.

What a trace does *not* tell you:

- The actual request body or response body. Traces are metadata; bodies are deliberately not captured for privacy and size reasons.
- The arrival process: traces record individual requests, not the inter-arrival distribution, unless you aggregate.
- Auth tokens and cookies.
- Any non-HTTP detail that the instrumentation wasn't configured to emit.

## The trace → load test pipeline

A reasonable pipeline looks like:

1. **Query the trace backend for a time window.** Jaeger and Tempo expose HTTP APIs to list and fetch traces.
2. **Aggregate by operation.** Group spans by root operation (e.g., `POST /orders`) and count per-minute rates. This gives the arrival-rate histogram per endpoint.
3. **Extract sequences.** For each root, flatten the span tree into a list of child calls. This gives the call graph that each root request triggers — useful for a load test that wants to know "when I hit /orders, the backend fans out to /inventory, /pricing, /user."
4. **Emit load-generator scenarios.** Generate k6 scenarios (or Gatling simulations) that issue requests at the observed rates against the top-level endpoints. The internal fan-out will occur naturally because the real services are still in the middle.

This pipeline sidesteps the replay divergence problem because you're not replaying bodies — you're regenerating a workload profile and letting the real services handle the payloads. Bodies are synthesised from schemas or captured separately.

## xk6-client-tracing

Grafana maintains `xk6-client-tracing`, an extension to k6 that generates OpenTelemetry-compatible trace data and sends it to an agent or collector. This is useful in the *opposite* direction — it lets you load-test the tracing infrastructure itself (Tempo, Jaeger, collectors) by emitting synthetic traces at scale. It is **not** a trace-to-load-test bridge, which is what we'd really want. That bridge does not exist in open source as of 2024.

## Existing academic and industry work

- **Microsoft's SOSP 2019 paper** on "Causal profiling" (Coz, Curtsinger & Berger) was not specifically about trace-driven load but is directly relevant: tracing data can identify which calls on the critical path matter for p99.
- **Uber's "Jaeger as workload source"** internal tooling has been mentioned in conference talks but is not publicly released.
- **OpenTelemetry Demo (astronomy shop)** is a reference deployment but does not include a trace-to-load-test utility.

## Value proposition and gap

The value: traces are available for free in most production environments post-2022 because everyone is now emitting OpenTelemetry. A tool that takes a Jaeger/Tempo query and emits a matching load test would give you:

- Real arrival-rate-per-endpoint distributions.
- Real concurrency (because traces overlap in time, you can count how many were in flight).
- Real fan-out (span tree structure).
- A per-user-ID session model (if the trace has user IDs in tags, you can cluster).

This is strictly more informative than any schema-driven generator because it's measured, not declared.

## Failure modes

1. **Sampling bias.** Most tracing in production is sampled (1% or head-based). The captured traces are a biased sample — tail-latency traces are often over-represented because of tail-based sampling, inflating the apparent latency distribution. A load test built from a tail-based sample will *over*-estimate average latency.
2. **Body-less.** Request bodies are missing, so the generated load test has to synthesise them. Back to the schema-driven problem.
3. **Non-deterministic span structure.** The same endpoint can produce different fan-out depending on state (cache hit vs. miss, A/B test bucket). A single trace is a sample from a distribution, not a definition.
4. **Schema changes.** Operation names drift as code changes. Joining a trace from last month with the current code is fragile.
5. **Scale of the trace corpus.** Production trace backends store PB of data. Aggregating over a full day is expensive. Most teams would aggregate over a much smaller slice and hope it's representative.
6. **No session model extraction.** Traces are per-request. To get a session model you need to join by user ID across traces, which most tracing backends don't make easy and which is often blocked by PII policies.

## Where this intersects with LLMs

A natural LLM-assisted workflow: feed a trace (maybe in OTLP JSON) to an LLM with the prompt "generate a k6 scenario that reproduces this workload at the observed rate." GPT-4-class models can write a plausible-looking k6 script for a given trace. The hard part the LLM cannot do: inferring the right arrival rate, think time distribution, or session structure from a sampled trace corpus. That requires statistical work the LLM will hallucinate through.

## Toolmaker gap

This is arguably the single highest-leverage gap in trace-driven performance generation: nobody open-sources a "`tempo query → k6 scenarios`" tool. Grafana is the obvious home for one; they have the pieces (Tempo + k6 + xk6-client-tracing) but no orchestration.

## Citations

- xk6-client-tracing: https://github.com/grafana/xk6-client-tracing
- Jaeger docs: https://www.jaegertracing.io/docs/
- Tempo query API: https://grafana.com/docs/tempo/latest/api_docs/
- OpenTelemetry Demo: https://opentelemetry.io/docs/demo/
- Coz causal profiling: https://arxiv.org/abs/1608.03676