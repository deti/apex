---
id: 01KNZ56MSWFGRCEDBEZ2XJ9PZF
title: Artillery — Node.js YAML load generator with Playwright and synthetic monitoring
type: literature
tags: [tool, artillery, load-testing, nodejs, yaml, playwright, open-model, synthetic-monitoring]
links:
  - target: 01KNZ5F8VABE5TGW976NMQA1VP
    type: related
  - target: 01KNZ6QBAEKEYVV66VQ2W1PY3R
    type: related
  - target: 01KNZ4VB6JX0CQ5RFAZDJTQMCS
    type: related
  - target: 01KNWE2Q9YZBAR140ZX5P36TQ5
    type: related
created: 2026-04-11T20:54:43.260387+00:00
modified: 2026-04-11T20:54:43.260389+00:00
---

Source: https://www.artillery.io/docs — Artillery documentation and test script reference, fetched 2026-04-12.

Artillery is a Node.js-based load testing and synthetic monitoring platform first released in 2015 by Hassy Veldstra. The open-source core (`artillery` npm package) runs tests described in YAML with optional JavaScript processor hooks; Artillery Cloud/Pro is the commercial layer.

## Test script structure

Tests are YAML with two top-level keys: `config` and `scenarios`.

```yaml
config:
  target: https://api.example.com
  phases:
    - duration: 120
      arrivalRate: 10
      rampTo: 50
      name: ramp-up
    - duration: 600
      arrivalRate: 50
      name: sustained
  payload:
    path: users.csv
    fields: [username, password]
    order: sequence
    skipHeader: true
  plugins:
    ensure: {}
    expect: {}
  processor: ./hooks.js

scenarios:
  - name: checkout-flow
    weight: 6
    flow:
      - post:
          url: /login
          json: { user: "{{ username }}", pw: "{{ password }}" }
          capture: { json: "$.token", as: "token" }
      - get:
          url: /cart
          headers: { Authorization: "Bearer {{ token }}" }
      - think: 2
```

## Phases and arrival model

Artillery is **open-model** at the YAML level. Phase types:
- `arrivalRate` — constant N virtual users arrive per second.
- `arrivalCount` — exactly N arrivals over the duration.
- `rampTo` — linear ramp between two arrival rates.
- `pause` — no arrivals for N seconds (soak between phases).

Optional `maxVusers` caps concurrent VUs (turns the phase into a bounded open model when the system can't keep up).

## Scenarios and engines

Scenarios define VU behaviour as flows. The flow is a list of steps executed sequentially per virtual user. Engines pluggable via `engine: ...`:

- `http` (default) — REST APIs, GET/POST/PUT/DELETE, `capture` for extracting response data into session variables, `match` for assertions.
- `ws` — WebSocket connections.
- `socketio` — Socket.IO messaging.
- `playwright` — browser automation, full page-level user flows.
- `kafka` — Kafka producer/consumer load.
- `grpc` — gRPC service calls (community engine).

The Playwright engine is what makes Artillery distinctive in the load-generator space: it runs full real-browser user journeys at scale, and is one of two competitors (alongside k6 browser) for "load test at the DOM level".

## Data feeding and variables

`payload.path` loads a CSV; `order: sequence` / `random` controls how rows are drawn. Inline `variables` define lists of choices. Templating uses `{{ }}` with `$env`, `$testId`, `$uuid` built-ins plus any `capture`'d value.

## Plugins

`ensure` — SLO thresholds (pass/fail gates on response times, error rates). `expect` — per-request assertions (status, body content, response time). `metrics-by-endpoint` — disaggregates the summary metrics per URL. `publish-metrics` — Prometheus/CloudWatch/Datadog output. `apdex` — Apdex scoring.

## Artillery Cloud / Pro

Commercial offering. Features over OSS: hosted test execution on AWS Fargate with multi-region load generation, historical test dashboards, test-run comparison, scheduled synthetic monitoring (run the same test every 5 minutes in production and alert on SLO drift), team collaboration.

## Strengths

- YAML for 90% of cases means non-programmers can author tests.
- Open-model by default.
- Playwright engine for real browser testing.
- `artillery run-fargate` — first-class distributed mode via AWS Fargate.
- Integrated synthetic monitoring story is unique among load generators.

## Failure modes

- **Node.js per-VU overhead** — a Node event loop per VU scales worse than k6's `goja` or Gatling's actors. Practical per-host ceiling is lower.
- **YAML hides logic** — complex flows end up 50% in `processor.js`, defeating the YAML ergonomic argument.
- **Plugin quality varies** — core plugins are solid, community plugins are mixed.
- **OSS distributed mode is limited** — without Artillery Cloud, you run N `artillery run` processes and aggregate manually.
- **Playwright engine overhead** is high — expect 10s of browser VUs per host, not 1000s.

## Relevance to APEX G-46

Artillery's YAML-first model is the closest analogue to what G-46's schema-driven perf-test description would look like. The `expect` plugin's assertion model maps to G-46's SLO assertion concept. Artillery's Playwright engine is the closest OSS equivalent to the k6-browser hooks G-46 would want for browser-side resource-guided exploration.