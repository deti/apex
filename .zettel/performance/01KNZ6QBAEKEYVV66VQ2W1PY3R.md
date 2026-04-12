---
id: 01KNZ6QBAEKEYVV66VQ2W1PY3R
title: Artillery — JavaScript Load Testing with Fake Data Plugins
type: literature
tags: [artillery, load-testing, nodejs, fake-data, falso, faker, plugin, playwright]
links:
  - target: 01KNZ56MSWFGRCEDBEZ2XJ9PZF
    type: related
  - target: 01KNZ5F8VABE5TGW976NMQA1VP
    type: related
  - target: 01KNZ4VB6JX0CQ5RFAZDJTQMCS
    type: related
  - target: 01KNWE2Q9YZBAR140ZX5P36TQ5
    type: related
created: 2026-04-11T21:21:19.182769+00:00
modified: 2026-04-11T21:21:19.182776+00:00
source: "https://www.artillery.io/"
---

# Artillery — Node.js Load Testing with First-Class Data Generation

Artillery is an open-source Node.js-based load testing tool maintained by the Artillery team. It has been around since 2016, shipped as `artillery.io`, and has evolved into a full platform with serverless distributed execution (Artillery Pro / Artillery Cloud) and a browser-level load engine on top of Playwright.

## What differentiates Artillery from k6 and Gatling

- **YAML-first scenarios.** Simple tests can be expressed entirely in YAML, with minimal scripting. More complex tests use JavaScript Node.js modules.
- **Any Node.js module.** Because it runs on Node, any npm package is importable. This is a major ergonomic win for teams already using Node on the backend.
- **Playwright engine.** `artillery-engine-playwright` lets you write load tests that drive full Chromium/Firefox/WebKit browsers through real user journeys. This is closer to production-accurate for heavy-JS apps than header-level HTTP generation.
- **Multi-protocol.** HTTP, WebSocket, Socket.io, gRPC, Kinesis. One tool for most web workloads.

## First-class fake data

The capability most relevant to this research: Artillery ships a **fake-data plugin** (powered by `falso`, a faker.js-style library) that is trivially enabled in any test configuration.

```yaml
config:
  plugins:
    fake-data: {}
scenarios:
  - flow:
      - post:
          url: /signup
          json:
            email: '{{ $randEmail() }}'
            password: '{{ $randPassword() }}'
            username: '{{ $randUserName() }}'
```

In-place `$randEmail()`, `$randUserName()`, `$randInt(1, 100)` and dozens more functions substitute realistic-looking values per request with zero setup. A separate community plugin `artillery-plugin-faker` (using faker.js) is available when you need faker.js's more extensive API or localisation.

This solves the single biggest real-world friction point in load-test data setup: getting each virtual user to have different values. Artillery has it out of the box; k6 and Gatling make you load CSV files.

## Adversarial reading

1. **Fake data is not realistic data.** `$randEmail()` generates `abc@def.com` style random strings, not real-distribution emails. If your service has email-based uniqueness or domain-based rate limiting, fake emails don't reflect real behaviour.
2. **`falso` is shallower than faker.js.** It's faster and smaller but less comprehensive. Deep localisation and obscure generators aren't there. Hence the separate `faker` plugin.
3. **Fake data plugin only works in the `http` engine.** Not in the Playwright engine, which is ironic because browser-level tests arguably need realistic data even more.
4. **Known edge cases.** GitHub issue #2756 notes that the fake-data substitution doesn't work in `before` sections — a common gotcha where engineers try to pre-generate data for a scenario and get literal strings instead.
5. **No production-distribution fit.** Same limitation as every other load tool: you get *random* data, not data sampled from observed production distributions. Matching real-world email domain frequency, realistic username length distribution, etc. requires custom code.
6. **YAML limits.** Complex scenarios outgrow YAML and need JavaScript payloads. At that point the "YAML-first" simplicity is mostly lost and you're writing Node code.

## Why Artillery's design choices matter for test generation

Artillery's fake-data approach is an object lesson in what's missing from other tools. The fact that a load tool from 2016 already ships with a fake-data plugin, while the k6/Gatling ecosystems require CSV files, shows that ergonomic data generation is an under-prioritised feature everywhere else. For an LLM-driven test generator, inheriting Artillery's plugin model (declarative `{{ $func() }}` substitutions) is cleaner than emitting custom JavaScript.

## Playwright engine for realistic browser-level load

Artillery's Playwright engine deserves a separate mention. Running 100 real Chromium instances on a load generator is a different trade-off than running 10,000 headless HTTP clients:

- **Pro:** captures all the real browser behaviour (JavaScript execution, CSS rendering, XHR timing, caching) that HTTP-level load tests miss.
- **Con:** resource-intensive. You need large load generators to reach even modest VU counts.
- **Con:** much slower ramp-up and warm-up.

Use case: front-end heavy single-page apps where HTTP-level load tests systematically underestimate latency (because the client-side JS work that real users do is invisible to the test).

## Toolmaker gap

Artillery has the fake-data slot; nobody has filled it with *production-distribution-aware* generators. A plugin that reads a sample of production data, fits a distribution, and emits per-request values from the fit would be an immediate improvement. Requires tooling bridge from observability data to the plugin registry.

## Citations

- https://www.artillery.io/
- https://github.com/artilleryio/artillery
- Fake data docs: https://www.artillery.io/docs/reference/extensions/fake-data
- Faker plugin (community): https://github.com/fabsrc/artillery-plugin-faker
- https://www.npmjs.com/package/artillery-plugin-fake-data
- Playwright engine: https://github.com/artilleryio/artillery-engine-playwright
- Falso library: https://ngneat.github.io/falso/