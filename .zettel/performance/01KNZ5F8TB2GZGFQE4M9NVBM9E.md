---
id: 01KNZ5F8TB2GZGFQE4M9NVBM9E
title: Drill — Rust YAML-first HTTP load tester (Ansible-inspired)
type: literature
tags: [tool, drill, load-testing, rust, yaml, http, closed-model]
links:
  - target: 01KNZ5F8VABE5TGW976NMQA1VP
    type: related
  - target: 01KNZ4VB6JB4Q5H3NPS72MZZ2A
    type: related
  - target: 01KNWE2Q9YZBAR140ZX5P36TQ5
    type: related
created: 2026-04-11T20:59:25.899985+00:00
modified: 2026-04-11T20:59:25.899987+00:00
---

Source: https://github.com/fcsonline/drill — Drill README, fetched 2026-04-12.

Drill is an HTTP load testing tool written in Rust by Ferran Basora, first released in 2018. The differentiator: test plans are YAML (Ansible-inspired) instead of scripts. The name is a reference to "drilling" services under load. Apache-2.0.

## Model

Drill is closed-loop with sequential plans per concurrent iteration:

```
drill --benchmark benchmark.yml --stats
```

The YAML defines "plans" (ordered steps) that get executed `iterations` times by `concurrency` workers:

```yaml
---
concurrency: 10
base: 'https://api.example.com'
iterations: 1000
rampup: 5

plan:
  - name: Login
    request:
      url: /login
      method: POST
      body: '{"u":"x","p":"y"}'
    assign: login
  - name: Fetch cart
    request:
      url: /cart
      headers:
        Authorization: 'Bearer {{ login.body.token }}'
    assert:
      - status: 200
```

## Features

- **Interpolation** — `{{ item }}`, `{{ foo.body.field }}` for chaining captured values across requests.
- **CSV data sources** — load rows and iterate per-user.
- **`assign`** — capture a response for later use, Ansible-register style.
- **Session cookies** — persisted across requests automatically.
- **Assertions** — on status and body values.
- **Ramp-up** — gradual concurrent-user increase over N seconds.

## Strengths

- YAML-first means non-programmers can write tests.
- Rust runtime — fast, memory-efficient, single static binary.
- Explicit multi-step flows with captures and assertions — more than wrk/Bombardier/Siege offer.
- Assertions gate CI exit code.

## Failure modes

- **Closed-loop model only** — no arrival-rate knob.
- **Limited reports** — text summary, no HTML, no streaming output.
- **Smaller community** than k6, Gatling, JMeter, Locust, Artillery. Few plugins.
- **YAML has limits** — complex logic ends up ugly. No scripting escape hatch.
- **No distributed mode.**
- **YAML-via-Handlebars interpolation is fragile** — errors at runtime, not compile time.

## Relevance to APEX G-46

Drill occupies the same niche as Artillery for the Rust ecosystem: YAML-first, declarative, closed-loop. Worth knowing as a comparative data point for G-46's workload-description schema design — when "YAML, but less than Artillery" is the right tradeoff.