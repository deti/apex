---
id: 01KNZ55NR8RQ3A2AM26Q7J92AG
title: Postman → k6 converter (postman-to-k6)
type: literature
tags: [postman, k6, test-generation, load-testing, generator, correlation]
links:
  - target: 01KNZ56MPVSZD05KM395ZKAM5J
    type: related
  - target: 01KNZ4TTX5V1TESBMRM80J38XA
    type: related
  - target: 01KNZ6GW3GYN9ZDT3A9JTVJFEW
    type: related
  - target: 01KNWE2Q9YZBAR140ZX5P36TQ5
    type: related
created: 2026-04-11T20:54:11.464750+00:00
modified: 2026-04-11T20:54:11.464761+00:00
source: "https://github.com/grafana/postman-to-k6"
---

# postman-to-k6 — Postman Collection to k6 Script Converter

`grafana/postman-to-k6` is a CLI tool that converts a Postman collection (v2.0 or v2.1 JSON) into a k6 JavaScript load-test script. It is maintained by Grafana Labs as part of the k6 toolchain.

```sh
postman-to-k6 collection.json -o k6-script.js
# or
npx @apideck/postman-to-k6 collection.json -o k6-script.js
```

## Why this path matters more than OpenAPI → k6

A Postman collection is a *far richer* starting point than an OpenAPI schema for generating a performance test, for three concrete reasons:

1. **Ordered request sequences.** A collection is organised into folders of requests in a specific order. That order typically encodes a session flow (login → list → detail → action → logout), which is exactly the navigation structure a workload model needs. OpenAPI has no notion of order.
2. **Pre-request and test scripts with correlation.** Postman supports JavaScript pre-request and test scripts that extract values from responses (`pm.response.json().id`) and store them in environment variables (`pm.environment.set('orderId', ...)`). These are *hand-authored correlations*. `postman-to-k6` preserves them — the generated k6 script contains the equivalent JavaScript, so multi-step flows that pass an ID from `POST /orders` to `GET /orders/:id` actually work.
3. **Real auth flows.** Postman collections usually contain concrete auth steps (OAuth2 token exchange, JWT refresh) that the engineer already got working when developing the API. Regenerating a k6 script from a Postman collection inherits all of that.

## Translation scope

- Requests, headers, query parameters, body types (raw/form/urlencoded/binary) — all translated.
- Environment variables mapped to k6 `__ENV`.
- Pre-request / test scripts translated with a compatibility shim that re-implements a subset of the Postman JavaScript API (`pm.environment`, `pm.variables`, `pm.response`, `pm.test`, `pm.expect`).
- Collection-level auth configurations (Bearer, Basic, API Key) are translated; OAuth2 typically requires manual adjustment.

## Fidelity and failure modes

Even though Postman collections encode more structure than OpenAPI, the generated k6 script still has structural gaps as a *load test*:

1. **Single-user logic baked in.** A Postman collection is written for a single tester exercising one session end-to-end. Running N virtual users through the same collection in parallel may collide on shared state (order IDs, idempotency keys, unique constraints) unless the test script parameterises input with per-VU data. `postman-to-k6` does not inject data-driven parameterisation.
2. **No workload profile.** The collection says *what* to do, not *how many* per second or *how many concurrent users*. All the distributional questions (think time, arrival process, ramp shape) are left to the engineer's `options` block.
3. **Compatibility shim is partial.** Any pre-request script that uses Postman APIs outside the supported subset (newer `pm.visualizer`, `pm.sendRequest`, cookie jar manipulation, crypto helpers beyond built-ins) generates a script that fails at runtime. The converter prints warnings but does not refuse to emit, so the gap is found only when running.
4. **Collection drift vs. test drift.** If the collection gets edited by QA for exploratory testing, regenerating the script clobbers any k6-side customisation. The usual fix is to keep the generated output as an untouched baseline and layer workload configuration in a separate harness file that imports the generated module.
5. **Correlation fragility.** The generated extractors are textual copies of what the tester wrote. If the API response shape changes and the Postman collection was updated but the tester's script was not, the generated k6 script silently extracts `undefined` and subsequent requests fail in ways that look like application bugs.

## Use cases where it works well

- Smoke-level load (5–50 VUs) against a freshly changed API where the functional team already maintains a Postman collection for exploratory testing.
- CI bootstrap for a new service whose OpenAPI spec is thin but whose developers have been working iteratively in Postman.
- Bridging the gap between manual tester workflows and developer-owned perf tests — the converter is a *cheap way to show the tester their collection is now a load test*, which changes the shared-ownership conversation.

## Use cases where it fails

- Any test that needs realistic data diversity (the collection has one user's data).
- Stateful flows with shared resources (checkout for an e-commerce where two VUs buy the same SKU).
- GraphQL workloads — Postman's GraphQL support is generic HTTP POST; the collection does not encode query selection-set variation, which is often the axis on which GraphQL performance varies.

## Toolmaker gap

`postman-to-k6` is the best *structural* bridge but still delegates all distributional questions to the engineer. A tool that combined postman-to-k6 with a server-log Markov workload model (Menascé-style CBMG inferred from access logs) would automatically pick arrival rate and session-transition probabilities, emitting the missing `scenarios` block that the converter leaves blank.

## Citations

- https://github.com/grafana/postman-to-k6 (canonical)
- https://www.npmjs.com/package/@apideck/postman-to-k6 (community fork/npm mirror)
- k6 integrations overview: https://grafana.com/docs/k6/latest/reference/integrations/