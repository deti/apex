---
id: 01KNZ55NZFXHMGS5NN0TN020MR
title: RESTler — Stateful REST API Fuzzing (Microsoft Research)
type: literature
tags: [restler, stateful-fuzzing, rest-api, openapi, microsoft-research, producer-consumer, test-generation]
links:
  - target: 01KNZ55NWZ1EH9FSVP5ZA6E4E7
    type: related
  - target: 01KNZ55P1P0TZTWKT0K9YCACJN
    type: related
  - target: 01KNZ5SMAD6NJG3EYE06C67A6S
    type: related
  - target: 01KNZ4TTX5V1TESBMRM80J38XA
    type: related
  - target: 01KNZ6QBKG64F2WEP3PNJK61JH
    type: related
  - target: 01KNWE2Q9YZBAR140ZX5P36TQ5
    type: related
created: 2026-04-11T20:54:11.695187+00:00
modified: 2026-04-11T20:54:11.695194+00:00
source: "https://github.com/microsoft/restler-fuzzer"
---

# RESTler — The First Stateful REST API Fuzzer

RESTler is a stateful REST API fuzzing tool from Microsoft Research. It takes an OpenAPI (formerly Swagger) specification, statically infers *producer-consumer dependencies* between endpoints, and then generates and executes sequences of requests that exercise the service in valid states. It is distributed as open source under microsoft/restler-fuzzer (Python + .NET compiler) and has a companion academic paper in ICSE 2019 (Atlidakis, Godefroid, Polishchuk).

## Why "stateful" matters

Earlier REST fuzzers treated each endpoint as an independent input space. That approach misses the vast majority of real code paths because most REST endpoints require a valid state (a logged-in user, an existing resource, a valid parent record) before they can be exercised at all. RESTler's key insight is that the **OpenAPI spec implicitly encodes dependencies**: if one endpoint returns a field named `orderId` in its response schema and another endpoint takes a path parameter named `{orderId}`, those two endpoints are in a producer-consumer relationship.

RESTler's pipeline:

1. **Compile step.** Parse the OpenAPI spec, build a dependency graph between endpoints by matching response fields to request parameters (type- and name-based matching with configurable heuristics).
2. **Generate step.** Emit request sequences in topological order. A sequence always starts with producer endpoints (those that create resources with no prerequisites) and builds up to consumers.
3. **Fuzz step.** Execute the sequences against a live service, capture responses, and use dynamic feedback (HTTP status, response bodies) to learn which sequences are actually valid.
4. **Check step.** A set of pluggable "checkers" look for specific classes of bug: additional 500s, resource leaks (create without delete), unauthorised access (consumer used with foreign producer's ID), hierarchy violations.

## Results claimed

- 28 bugs found in GitLab.
- Multiple bugs in four Azure and Office365 cloud services during internal deployments.
- Higher code coverage than stateless fuzzers in controlled comparisons.

## RESTler vs. performance generation

RESTler is a **security** tool, not a load-test generator. It does not model arrival rate, think time, or user sessions; it runs as fast as it can extract valid sequences. But its producer-consumer inference is the single most interesting thing in the spec-driven space for performance purposes:

- The producer-consumer graph *is* a call-sequence model. If you could feed it into a load generator, you would have a realistic multi-step workload without a human writing it.
- Sequence sampling could be driven by probabilities learned from production logs (some sequences are common; others are rare) — RESTler itself is uniform over sequences that reach deep states.

No one has published a "RESTler → k6" bridge as of early 2024. The closest path is to run RESTler in logging mode, harvest the successful sequences, and replay them through a load tool. This would be a worthwhile APEX-adjacent experiment.

## RESTler failure modes (adversarial reading)

1. **Name-based matching is brittle.** If your API uses `id` as a path parameter and the producer response calls the field `identifier`, the dependency is missed. RESTler has heuristics but they break on any unusual naming convention.
2. **No body-field dependencies.** RESTler's dependency inference is primarily on response-body fields → request-path/query parameters. If a consumer needs a producer's response field in its request *body*, inference is weaker.
3. **Bug-finding goals ≠ load goals.** RESTler wants to maximise *coverage of bug classes*, not *load on critical paths*. The sequences it generates exercise corner states that are unusual in production. For performance, you want the opposite — you want the *common* paths, because that's where performance regressions hurt most.
4. **Ignores payload size/cardinality.** RESTler generates minimal valid payloads. It will not discover a performance bug that only appears when the payload contains 10,000 list elements (the classic N+1 / unbounded-loop issue).
5. **State pollution and retries.** Because RESTler creates real resources, long runs pollute the test database. This is fine for security fuzzing in isolation but a concern for any integration with a performance test that wants to *preserve* database state across runs.
6. **OpenAPI 3.1 lag.** As of 2023, RESTler's OpenAPI support was strongest for 2.0 and 3.0, weaker for 3.1. Community users report needing to downconvert specs.

## RAFT — the self-hosted wrapper

Microsoft also publishes `microsoft/rest-api-fuzz-testing` (RAFT), a self-hosted Azure service that runs RESTler + OWASP ZAP in CI/CD. Conceptually similar to OSS-Fuzz for web APIs. This is the realistic deployment path if you want RESTler running continuously.

## Citations

- https://github.com/microsoft/restler-fuzzer
- https://www.microsoft.com/en-us/research/publication/restler-stateful-rest-api-fuzzing/
- ICSE 2019 paper PDF: https://patricegodefroid.github.io/public_psfiles/icse2019.pdf
- RAFT: https://github.com/microsoft/rest-api-fuzz-testing
- FuzzCon 2021 slides (Polishchuk): https://f.hubspotusercontent20.net/hubfs/7466322/FuzzCon%20-%20WebSec%20Edition%20Slides/Slides%20-%20Marina%20Polishchuk%20-%20Stateful%20REST%20API%20Fuzzing%20with%20RESTler.pdf