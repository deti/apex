---
id: 01KNZ6GW6232G68HHVGR9ANYZM
title: WireMock and Hoverfly — Service Virtualization for Isolated Performance Tests
type: literature
tags: [wiremock, hoverfly, service-virtualization, api-mocking, stubs, test-isolation, performance-testing]
links:
  - target: 01KNZ56MS27NFFMFBHZ0WPCV70
    type: related
  - target: 01KNZ56MPVSZD05KM395ZKAM5J
    type: related
  - target: 01KNZ5F57EZAMVV3P5991NVHJ9
    type: related
  - target: 01KNZ6QBKG64F2WEP3PNJK61JH
    type: related
  - target: 01KNWE2Q9YZBAR140ZX5P36TQ5
    type: related
created: 2026-04-11T21:17:47.074055+00:00
modified: 2026-04-11T21:17:47.074061+00:00
source: "https://wiremock.org/"
---

# WireMock and Hoverfly — Service Virtualization for Perf Test Isolation

Service virtualization tools replace real downstream dependencies with programmable stand-ins during testing. They are essential infrastructure for performance testing because most meaningful perf tests of one service need to *not* be constrained by the latency or availability of every downstream service.

## The two canonical tools

### WireMock

WireMock is an open-source HTTP mocking library in Java, widely used. It presents itself as either:

- A **library** embedded in a JUnit test (for unit/integration tests).
- A **standalone server** (for cross-language or out-of-process scenarios). The standalone mode is what matters for performance testing.

WireMock acts as a reverse proxy: it looks like the real dependency to the service under test, matches incoming requests against configured "stub mappings," and returns canned or dynamically constructed responses. Unmatched requests can be optionally forwarded to the real upstream (pass-through mode) so you can record traffic by running against production once and then replay against the recording.

Key features for perf testing:

- **Request matching.** URL, method, headers, query params, cookies, body (JSON, XML, regex, XPath).
- **Response templating.** Handlebars templates let you generate dynamic responses from request data.
- **Simulated delays.** Fixed and log-normal distributions for response latency. This is the killer feature — you can make the stub respond with a realistic latency distribution, so your service's perf test sees the same delays it would see against the real downstream.
- **Fault injection.** Return 500s at a rate, drop connections, slow down to test timeout handling.
- **Record-and-replay.** Run WireMock as a pass-through proxy against the real dependency to capture interactions, then switch to replay mode.

### Hoverfly

Hoverfly is an open-source Go tool from SpectoLabs (now iO-Sphere). Positioning: "out-of-process" service virtualization specifically. Its authors have a specific language position — they argue that WireMock is a "mock" (in-process faking) and Hoverfly is a "service virtualization" (out-of-process). Operationally they do very similar things.

Hoverfly-specific strengths:

- **Works as a proxy without reconfiguring the SUT.** You point the SUT at Hoverfly via HTTP_PROXY; no code changes.
- **Modes.** Capture, simulate, synthesise, modify, spy, diff. The capture mode records interactions for later replay; the diff mode compares real and simulated responses for regression detection.
- **Middleware scripts.** Python scripts can transform simulations on the fly.
- **Java DSL.** Similar ergonomics to WireMock for engineers embedded in JUnit tests.
- **Commercial Hoverfly Cloud.** SpectoLabs markets Hoverfly Cloud specifically for performance testing, with scaled-out simulation for high-throughput workloads.

## Why service virtualization matters for performance test generation

A performance test of service A that transitively calls services B, C, and D measures the combined latency of all four. That's sometimes what you want (end-to-end capacity planning), but often what you want is to measure A's scaling behaviour *given* a specified latency profile for its downstreams. You can't do that with the real downstreams because they vary.

WireMock and Hoverfly let you:

1. **Fix downstream latency.** Make B's stub always respond in 50 ms. Now any variation in test results is A's fault.
2. **Inject realistic latency distributions.** Use the observed p50/p95/p99 of real B and make the stub match. Now A's test matches production timing without requiring real B.
3. **Isolate A under extreme conditions.** Simulate B returning 500s at 5% rate or responding 10× slower than normal. See how A handles it.
4. **Make tests deterministic.** Real downstreams are noisy; stubs are reproducible. Regression detection is much easier against stubs.
5. **Avoid shared-staging-environment contention.** Many teams share a single staging environment where competing test runs interfere. Local stubs eliminate this.

## Failure modes

1. **Stub drift.** Stubs are a snapshot of what B used to return. When B's API changes, the stubs are stale and A's test passes against an obsolete contract. Record-and-replay workflows need periodic refresh. Contract-testing tools (Pact, Spring Cloud Contract) are a partial answer but add their own overhead.
2. **Stub under-specification.** You stub the 80% of calls that matter and miss the 20% that cause real problems. A production incident happens on an edge-case response your stub doesn't emit.
3. **Latency profile is wrong.** A stub with fixed 50 ms latency when production actually has a bimodal 10 ms/500 ms distribution gives A a misleading picture of its own scaling behaviour.
4. **No state.** Most stubs are stateless — the same request always returns the same response. Real downstreams have state; tests that depend on state progression (create order → read order) fail against simple stubs. WireMock's "scenarios" feature adds stateful sequences but it's a workaround.
5. **Rate limiting is missing by default.** Downstreams have rate limits; stubs don't. Tests that would have been rate-limited in production run free against stubs and give optimistic numbers.
6. **Authorization is stubbed away.** Real auth tokens, JWT validation, mTLS are all absent in stubbed mode. Tests miss auth-related bottlenecks.

## Fit with the other tools in this note family

- **Spec-driven generators (OpenAPI → k6, etc.) + WireMock** — generate the test *and* the stubs from the same OpenAPI spec. This works for the consumer side.
- **Production traffic replay + Hoverfly as downstream** — replay real requests against a SUT whose downstreams are virtualised, giving you realistic traffic with fully controlled downstream latency.
- **LLM-authored stubs** — give an LLM an API spec and a prose description, ask it to produce WireMock mappings. Cheap sub-task that works well in practice.

## Citations

- https://wiremock.org/ (canonical)
- Hoverfly home: https://hoverfly.io/
- HoverFly vs WireMock discussion: https://github.com/SpectoLabs/hoverfly/issues/417
- Test automation notes comparing approaches: https://github.com/Osipion/NotesOnTestAutomation/blob/master/service-virtualization/article.md
- Java DSL for Hoverfly stubbing: https://www.ontestautomation.com/creating-stubs-using-the-hoverfly-java-dsl/
- WireMock buyer's guide on API mocking: https://www.wiremock.io/post/api-mocking-tools-buyers-guide-market-landscape