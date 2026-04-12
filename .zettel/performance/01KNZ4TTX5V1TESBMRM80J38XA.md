---
id: 01KNZ4TTX5V1TESBMRM80J38XA
title: OpenAPI → k6 code generator (openapi-generator-cli k6)
type: literature
tags: [openapi, k6, test-generation, load-testing, spec-driven, generator]
links:
  - target: 01KNWGA5GS097K0SDS74JJ97X6
    type: extends
  - target: 01KNWE2Q9YZBAR140ZX5P36TQ5
    type: related
  - target: 01KNWE2QA5VP0K80TMSABACKWT
    type: related
  - target: 01KNZ4VB6JX0CQ5RFAZDJTQMCS
    type: related
  - target: 01KNZ55NR8RQ3A2AM26Q7J92AG
    type: related
  - target: 01KNZ55NWZ1EH9FSVP5ZA6E4E7
    type: related
  - target: 01KNZ55NZFXHMGS5NN0TN020MR
    type: related
  - target: 01KNZ55P1P0TZTWKT0K9YCACJN
    type: related
  - target: 01KNZ55P40GPW11BF8T0JN3VR8
    type: related
  - target: 01KNZ55P6BA1RE6Z5432ZYWG5W
    type: related
  - target: 01KNZ55P8NSTHYJ535FRVZZYAB
    type: related
  - target: 01KNZ55PB02X6BJN52R032R18K
    type: related
  - target: 01KNZ55PD9MF8HE845RYWGYZ33
    type: related
  - target: 01KNZ56MPVSZD05KM395ZKAM5J
    type: related
  - target: 01KNZ5SMF1GAFA93P6D8TQFM4Z
    type: related
  - target: 01KNZ5SM642DR52PJ1CDNEZ101
    type: related
  - target: 01KNZ6QBKG64F2WEP3PNJK61JH
    type: related
created: 2026-04-11T20:48:16.293692+00:00
modified: 2026-04-11T20:48:16.293699+00:00
source: "https://github.com/OpenAPITools/openapi-generator/blob/master/docs/generators/k6.md"
---

# OpenAPI → k6 Code Generator

The OpenAPI Generator project maintains a `k6` target that emits a runnable k6 JavaScript load-test script from an OpenAPI 2/3 specification. Invocation:

```sh
openapi-generator-cli generate -i openapi.yaml -g k6 -o ./k6-tests
```

The generator was contributed by Mostafa Moradian (then at k6/Load Impact / now Grafana) in OpenAPITools/openapi-generator PR #5300 (merged 2020). It targets ES5.1+ compatible k6 and is the de-facto baseline for spec-driven performance-test bootstrap in the k6 ecosystem.

## What it emits

- For each OpenAPI `path`, a k6 `group()` block is emitted.
- For each HTTP method under that path, a request function that constructs the URL using path parameters and query parameters extracted from the specification.
- Base URL is taken from `servers[0].url`.
- Request body is generated from the `requestBody.content` schema; when `example` fields exist in the OpenAPI document they are used directly.
- Basic smoke assertions are emitted: `check(res, { 'status is expected': (r) => r.status === <declared>... })`.

## Input requirements

- OpenAPI 2.0 or 3.0 spec (3.1 support lagged as of 2024).
- For realistic bodies, the spec must carry either `example`, `examples`, or sufficiently constrained schemas. Empty or minimal schemas produce nulls and empty strings.
- Servers must be declared; otherwise the generated script uses a placeholder.

## Fidelity and failure modes (adversarial reading)

This generator produces a **structural skeleton**, not a realistic workload. Concretely it fails on the dimensions that matter for performance testing:

1. **No realistic payload distributions.** `example:` values in OpenAPI are typically one or two hand-picked values — they are not statistical samples of real traffic. A generated test exercises exactly one shape of input per endpoint, which misses the cardinality-dependent code paths (N+1, pagination blow-up, cache misses) that cause most perf incidents.
2. **No call sequence model.** Each endpoint is tested in isolation. Real users have sessions (login → list → detail → action); the generator cannot express `POST /orders` must follow `POST /carts` must follow `POST /auth/login`. RESTler's producer-consumer inference is a strictly superior model here, but RESTler targets security fuzzing not load.
3. **No think-time distribution.** The default emitted script uses `sleep(1)` (or nothing). Real user think time follows roughly log-normal or Pareto distributions; constant think time massively over-estimates concurrency for a given arrival rate, flattening the tail-latency curve that matters for SLOs.
4. **No arrival-process choice.** k6 defaults to closed-workload (VUs with think time) but open-workload (`arrival-rate`) is what production actually looks like for user-facing HTTP APIs; the generator does not emit `scenarios: { ..., executor: 'ramping-arrival-rate' }` configurations tied to observed RPS.
5. **No correlation/extractor logic.** If endpoint B needs the `id` returned by endpoint A (the record-and-replay "correlation" problem), the generator cannot infer this from the spec; it emits two unrelated requests.
6. **Assertions are status-code only.** No p95/p99 thresholds, no response-body content validity checks — so regressions that return 200 with degraded payload or shape are invisible to the generated test.
7. **Maintenance drift.** Anytime the spec is regenerated, the k6 script is clobbered; any custom correlation logic the engineer added gets lost unless kept in a separate imported module.

## Maintenance cost

Low to bootstrap, high to keep: engineers end up hand-editing the generated output extensively, at which point regeneration becomes destructive. This is the classic "generated test code is write-only" anti-pattern.

## Toolmaker gap

A plausible APEX-adjacent tool would: (a) parse OpenAPI **and** a CBMG-style session model, (b) synthesize correlation extractors by tracking which response field types are structurally compatible with subsequent request schemas, (c) emit `ramping-arrival-rate` scenarios parameterized by observed RPS from production RUM or access logs, and (d) include p95/p99 threshold assertions pulled from the SLO document. None of the existing generators close all four gaps.

## Related Grafana offerings

- `grafana/openapi-to-k6` (separate repo, 2024) generates a TypeScript client for k6 rather than a full test script, pushing the write-the-actual-scenario work back to the engineer but providing ergonomic call-site typing. This is the modern direction Grafana appears to favour over the OpenAPI Generator path.
- `grafana/postman-to-k6` converts Postman collections — which *do* carry ordered request sequences and often contain extractor scripts — into k6 scripts. Because a Postman collection already encodes the call sequence (and correlations, via Postman's `pm.environment.set(...)`), the postman→k6 path preserves more of what you actually need than openapi→k6 does. For teams that already do manual API exploration in Postman, this is the more useful bridge.

## Citations

- OpenAPITools/openapi-generator docs: https://github.com/OpenAPITools/openapi-generator/blob/master/docs/generators/k6.md
- PR #5300 (generator contribution): https://github.com/OpenAPITools/openapi-generator/pull/5300
- Medium walkthrough by Mostafa Moradian: https://medium.com/k6-io/load-testing-your-api-with-swagger-openapi-and-k6-f15f969d97c1
- grafana/openapi-to-k6: https://github.com/grafana/openapi-to-k6
- Known-limitation issue (OpenAPI examples as query params): https://github.com/OpenAPITools/openapi-generator/issues/8378