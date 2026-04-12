---
id: 01KNZ55P40GPW11BF8T0JN3VR8
title: Dredd — Language-Agnostic API Documentation Testing (apiaryio/dredd)
type: literature
tags: [dredd, openapi, api-blueprint, contract-testing, documentation-testing, test-generation]
links:
  - target: 01KNZ4TTX5V1TESBMRM80J38XA
    type: related
  - target: 01KNZ55NWZ1EH9FSVP5ZA6E4E7
    type: related
  - target: 01KNWE2Q9YZBAR140ZX5P36TQ5
    type: related
created: 2026-04-11T20:54:11.840061+00:00
modified: 2026-04-11T20:54:11.840068+00:00
source: "https://dredd.org/"
---

# Dredd — Validate API Documentation Against Implementation

Dredd is a language-agnostic command-line tool from Apiary (acquired by Oracle in 2017) that validates an API description document against the backend implementation. It reads either an API Blueprint or an OpenAPI 2.0 document (OpenAPI 3.0 support was experimental as of 2024) and, for each example in the document, issues the corresponding HTTP request and verifies that the response matches what is documented.

## Primary use case

Dredd's goal is **documentation correctness**: it keeps the API docs in sync with the implementation by failing the build if the docs claim a response that the server does not actually produce. It is contract testing with the spec as the source of truth and the server as the implementation under test.

## Hooks

The most interesting engineering feature of Dredd is its hook system. Hooks are pieces of glue code that run before/after each test case or transaction and let the engineer set up and tear down test state, set dynamic headers, and conditionally skip tests. Hook languages supported: Go, Node.js, Perl, PHP, Python, Ruby, Rust. This is the cleanest "pluggable language" hooks system of any of the spec-driven tools, and it is why Dredd remained relevant for polyglot backends even as Schemathesis and RESTler pulled ahead in test-generation sophistication.

## What it generates

- One HTTP request per documented example.
- Assertions: HTTP status code matches, body conforms to the documented schema (via JSON Schema validation), headers match.

It does **not**: generate inputs beyond what's explicitly documented, explore state space, do property-based variation, or model sessions.

## Why it is interesting for performance test generation

On the surface Dredd is the weakest of the spec-driven tools for perf — it does exactly one request per example. But its hook model is worth noting as a design pattern:

- Per-language hooks let you compute *parameterised* setup (e.g., "fetch a fresh OAuth token before each test") without the generator having to know how. A perf-generation tool that needs to handle the auth-token-refresh problem across many languages could crib directly from Dredd's hook interface.
- Dredd explicitly decouples "what to test" (the spec + its examples) from "how to set up test state" (hooks). This is the right factoring for a generated load test too: the generator handles the call graph and the hooks handle the session/env state.

## Failure modes

1. **Only as good as the documented examples.** Dredd's coverage is exactly the cardinality of examples in the doc. No schema fuzzing, no property-based variation. Most OpenAPI specs have one example per endpoint (at most).
2. **OpenAPI 3.0 lag.** The maintainer community has repeatedly said 3.0 support is experimental. 3.1 has no support. Teams on modern specs sometimes drop Dredd for Schemathesis.
3. **Apiary dormant.** Since Oracle's acquisition, Apiary and Dredd have seen reduced maintenance. GitHub issues and PRs have long tail, and corporate sponsorship dried up.
4. **No perf model at all.** Dredd is strictly sequential, one transaction at a time.

## When it's the right tool

- Teams whose API docs use API Blueprint format (Dredd is essentially the reference tooling for API Blueprint).
- Polyglot backends where hooks-in-native-language are a hard requirement.
- CI docs-correctness gating, not load testing.

## Citations

- https://dredd.org/ (canonical docs)
- https://github.com/apiaryio/dredd
- https://github.com/apiaryio/dredd-example
- OpenAPI tooling registry: https://tools.openapis.org/categories/testing.html