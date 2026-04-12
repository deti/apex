---
id: 01KNZ55NWZ1EH9FSVP5ZA6E4E7
title: Schemathesis — Property-Based API Testing from OpenAPI/GraphQL
type: literature
tags: [schemathesis, property-based-testing, openapi, graphql, fuzzing, test-generation, hypothesis]
links:
  - target: 01KNZ4TTX5V1TESBMRM80J38XA
    type: related
  - target: 01KNZ55NZFXHMGS5NN0TN020MR
    type: related
  - target: 01KNZ55P1P0TZTWKT0K9YCACJN
    type: related
  - target: 01KNZ5SMAD6NJG3EYE06C67A6S
    type: related
  - target: 01KNZ55P40GPW11BF8T0JN3VR8
    type: related
  - target: 01KNZ6QBKG64F2WEP3PNJK61JH
    type: related
  - target: 01KNWE2Q9YZBAR140ZX5P36TQ5
    type: related
created: 2026-04-11T20:54:11.615952+00:00
modified: 2026-04-11T20:54:11.615958+00:00
source: "https://schemathesis.io/"
---

# Schemathesis — Property-Based API Testing from API Schemas

Schemathesis is an open-source property-based testing tool that generates test cases from OpenAPI (2.0, 3.0, 3.1) and GraphQL schemas. It is built on top of Hypothesis (the Python property-based testing library) and uses the schema as a contract: every generated test case is by construction a request that conforms to the declared types, and the oracle is "the response conforms to the declared response schema."

Canonical usage:

```sh
schemathesis run https://api.example.com/openapi.json
```

## What it generates

- **Positive cases.** Hypothesis samples values from the declared schema (strings, numbers, enums, nested objects) using strategies derived from JSON Schema. This gives broad coverage of the declared input space with minimal manual work.
- **Negative cases.** Schemathesis can mutate generated inputs to violate the schema (wrong types, out-of-range numbers, missing required fields) and checks that the server returns a well-formed 4xx rather than crashing or leaking a stack trace.
- **Stateful tests (experimental/stable depending on version).** Via OpenAPI `links`, Schemathesis can chain calls: the `POST /orders` response's `id` can feed the `GET /orders/{id}` request, giving a limited producer-consumer dependency inference comparable to RESTler's.

## Built-in checks (its test oracles)

- Response status in the declared set.
- Response body validates against declared schema.
- Response headers present.
- No 5xx (default "the server didn't crash").
- Contract checks: use of the declared content-type, CORS headers present if declared, etc.

This is a stronger oracle than any of the pure-load tools (k6, Gatling) because it actually validates *content*, not just status codes. For regression testing of API shape it is currently the best free tool.

## Why it matters for *performance* test generation

Schemathesis is not a load test generator — it runs sequentially and emphasises correctness, not throughput. However, it is the **closest free equivalent to RESTler** for driving realistic payload diversity, and the payload diversity it achieves is exactly the input that most spec-driven load generators lack. Two concrete paths to use it for performance:

1. **As a workload producer.** Run Schemathesis in `--workers N` with no assertions against a *staging* environment and measure latency distributions. This gives you a per-endpoint latency histogram under synthetic-but-conformant traffic. It is not a load test (no arrival-rate control, no think-time), but it is enough to catch asymptotic-complexity regressions when payload sizes change.
2. **As a payload-generation oracle.** Run Schemathesis offline, record the generated-request stream to HAR, then feed the HAR to `har-to-k6` to get a k6 script with realistic input diversity rather than the single-example diet that OpenAPI → k6 produces.

## Performance-test failure modes

- **No arrival-rate control.** Schemathesis runs as fast as it can with a worker pool; it does not simulate Poisson arrivals.
- **No session/user model.** Stateful mode chains calls within a single test case but does not model user sessions or think time.
- **Hypothesis shrinking on latency.** When a test case fails due to latency assertion, Hypothesis tries to shrink the input, which is the wrong strategy for perf (shrinking a slow input usually makes it fast, hiding the regression). This is a specific failure mode of naive property-based perf testing — see Hypothesis `@given` with deadlines.
- **JSON Schema strategies are biased toward small values.** Hypothesis defaults to shrinking-friendly small integers and short strings; to find size-sensitive performance bugs (the ReDoS/Billion Laughs family) you have to override the strategies for specific parameters. This is a well-known hypothesis limitation that is especially acute for perf.
- **Recursive schemas → shallow generation.** Hypothesis has to bound recursion to avoid non-termination, so complex nested schemas are sampled shallowly, missing the depth-dependent bugs (quadratic tree traversal, unbounded recursion that's fine at depth 3 and explodes at depth 50).

## Production adoption

Used by Spotify, WordPress, JetBrains, Red Hat, Capital One. Typical first-run yields 5–15 issues on a medium-sized API, most of them schema/implementation divergences (missing required fields in responses, undeclared 500 responses) rather than perf issues.

## Relation to RESTler and EvoMaster

- **RESTler** (Microsoft) is closed-to-Windows heavy, stateful-first, security-flavoured — it emphasises producer-consumer chain discovery and specific security-checkers. It is the reference tool for stateful REST fuzzing.
- **EvoMaster** uses an *evolutionary* algorithm with white-box feedback (code coverage via instrumentation). Strictly more powerful than schema-only generation when you can run the server under instrumentation, but much higher setup cost.
- **Schemathesis** sits between: black-box, schema-only, cheap to run in CI, weaker guidance than EvoMaster but far less setup.

For *performance* generation specifically, none of the three have native support. All of them could in principle drive a load test by firing many workers, but none models a workload (think time, arrival process, user sessions as probability distributions).

## Toolmaker gap

A `schemathesis → k6` bridge (one exists in nascent form) that captures the Hypothesis-generated requests and replays them at a controlled arrival rate would immediately be strictly better than `openapi-generator -g k6` on the payload-diversity axis. Nobody appears to have built this in a polished form as of 2024.

## Citations

- https://schemathesis.io/
- https://github.com/schemathesis/schemathesis
- https://schemathesis.readthedocs.io/
- Capital One usage: https://www.capitalone.com/tech/software-engineering/api-testing-schemathesis/
- https://pypi.org/project/schemathesis/