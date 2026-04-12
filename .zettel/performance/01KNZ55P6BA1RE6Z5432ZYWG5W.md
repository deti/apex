---
id: 01KNZ55P6BA1RE6Z5432ZYWG5W
title: ghz — gRPC Load Testing and Benchmarking Tool
type: literature
tags: [ghz, grpc, protobuf, load-testing, benchmarking, test-generation]
links:
  - target: 01KNZ6GW8J1VQ2XPF9E1V1BFHQ
    type: related
  - target: 01KNZ5F8TVW837C1YJKKXFH504
    type: related
  - target: 01KNZ56MPVSZD05KM395ZKAM5J
    type: related
  - target: 01KNWE2Q9YZBAR140ZX5P36TQ5
    type: related
created: 2026-04-11T20:54:11.915225+00:00
modified: 2026-04-11T20:54:11.915232+00:00
source: "https://ghz.sh/"
---

# ghz — gRPC Benchmarking and Load Testing

ghz (pronounced "gigahertz") is the de-facto open-source gRPC benchmarking tool, maintained by Bojan Djurkovic (bojand/ghz). It is a CLI and a Go package; source is at github.com/bojand/ghz.

## Core capabilities

- **Proto / Protoset / Reflection.** ghz can load a gRPC service definition from a `.proto` file, a pre-compiled protoset bundle, or at runtime via server reflection. Reflection is the easiest path; it needs no build artefacts and works against any service that exposes the gRPC reflection API.
- **Data specification.** Request payloads are specified via JSON that maps to the proto message. ghz supports templating with Go text/template so payloads can be parameterised with counters, random values, or per-request data from a file.
- **Concurrency and rate control.** `--concurrency` (number of parallel workers), `--rps` (requests per second cap), `--total` (total request count), `--duration` (walltime), `--connections` (number of underlying HTTP/2 connections — important because gRPC multiplexes many streams over one connection).
- **Output.** Human-readable summary, CSV, JSON, HTML with latency histogram. The HTML report includes a histogram similar to wrk2's and is often the most used artefact.

## Fidelity and failure modes

### What ghz does well

- Handles protobuf serialisation correctly, which is surprisingly non-trivial: wire format errors are silent in toy tools.
- Understands HTTP/2 multiplexing and surfaces the `connections` axis, which is load-generation-critical for gRPC — a single client with many virtual users but one TCP connection hits a different bottleneck (HEAD-of-line blocking in HTTP/2) than the same load spread over many connections.
- Streams: unary, client-streaming, server-streaming, and bidirectional streaming are all supported with different templating semantics.

### What ghz does badly for realistic perf testing

1. **Uniform payload problem.** ghz fires the same templated payload shape for every request. Real gRPC workloads have variance: different users send different field values, different sizes, different subsets of optional fields. ghz has no native payload-distribution support; you can either provide one payload or a flat file of payloads and round-robin.
2. **No stateful flows.** Unlike k6 or Gatling, ghz does not natively support sessions where request N depends on response N-1. For a typical bidi-streaming chat workload you model the stream duration and message rate but not the conversational content. Stateful ghz workloads must be scripted externally.
3. **No deadline distribution modelling.** gRPC deadlines are a first-class performance-relevant input. Most real services see a wide range of deadlines from different clients; ghz takes a single deadline value per run.
4. **Scaling ceiling.** Single-process ghz typically tops out in the 10–50 k RPS range depending on payload size. For higher scale you need multiple ghz instances behind an orchestrator (Flagger, k6 distributed, Kubernetes Job farm), none of which is built in.
5. **Reflection dependency reveals test-vs-prod mismatch.** Reflection is off in most production services for security reasons, so ghz-via-reflection works on staging but not prod. This is a subtle and recurring source of test environments that do not match production.

## Alternatives in the gRPC load space

- **k6 with xk6-grpc extension** — k6 ecosystem, JavaScript scripting, much better for stateful/session-oriented workloads because it inherits all of k6's scenario logic.
- **Gatling gRPC plugin** — Scala DSL, strong for engineers already using Gatling for HTTP.
- **grpc_cli / grpcurl** — not load tools, but useful for manual one-off calls when the workload is tiny.
- **Google's own `grpc_benchmarks` suite** — C++/Go benchmark suite used for gRPC core development, not engineering-team-friendly, but the most accurate for protocol-level numbers.

## Toolmaker gap

ghz fires fast but blind. A tool that ingested a protobuf service and generated a *diverse* request stream (sampling each field's domain independently or from observed production distributions) would be an immediate improvement. The RESTler-for-gRPC design pattern — infer producer-consumer dependencies from message field types and run multi-call sequences — has been discussed in academic work but there is no open-source implementation targeting perf.

## Citations

- https://ghz.sh/ (canonical)
- https://github.com/bojand/ghz
- https://ghz.sh/docs/intro
- gRPC benchmarking guide: https://grpc.io/docs/guides/benchmarking/
- k6 gRPC walkthrough: https://k6.io/blog/performance-testing-grpc-services/