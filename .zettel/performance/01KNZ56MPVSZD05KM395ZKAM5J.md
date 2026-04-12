---
id: 01KNZ56MPVSZD05KM395ZKAM5J
title: k6 — JS-scripted Go-runtime load generator (deep dive)
type: literature
tags: [tool, k6, load-testing, grafana, javascript, go, xk6, open-model, coordinated-omission]
links:
  - target: 01KNZ5F8VABE5TGW976NMQA1VP
    type: related
  - target: 01KNZ4VB6JX0CQ5RFAZDJTQMCS
    type: related
  - target: 01KNZ4VB6JB4Q5H3NPS72MZZ2A
    type: related
  - target: 01KNZ4VB6JPYBYW64S7NNYS1CM
    type: related
  - target: 01KNWGA5GS097K0SDS74JJ97X6
    type: related
  - target: 01KNZ5F8V2F1AXVHP058DM3B4H
    type: related
  - target: 01KNZ4TTX5V1TESBMRM80J38XA
    type: related
  - target: 01KNZ55NR8RQ3A2AM26Q7J92AG
    type: related
  - target: 01KNZ55PD9MF8HE845RYWGYZ33
    type: related
  - target: 01KNZ5SMF1GAFA93P6D8TQFM4Z
    type: related
  - target: 01KNZ5F5C3YS1EYDCVFQ7TQS9H
    type: related
  - target: 01KNWE2Q9YZBAR140ZX5P36TQ5
    type: related
created: 2026-04-11T20:54:43.163235+00:00
modified: 2026-04-11T20:54:43.163241+00:00
---

Source: https://grafana.com/docs/k6/latest/ — Grafana Labs k6 documentation, fetched 2026-04-12.

k6 is an open-source load testing tool written in Go, scripted in JavaScript (ES2015+, TypeScript via esbuild). It was created by Load Impact in 2017 and acquired by Grafana Labs in 2021. The Go runtime executes a pool of `goja` JavaScript VMs — one per virtual user (VU) — so scripts look dynamic but the actual I/O and scheduling are Go. This is the central architectural idea: JavaScript for ergonomics, Go for throughput.

## Scripting model

A k6 script exports a default function that is invoked per iteration, plus an `options` object and optional `setup()` / `teardown()` lifecycle hooks. Minimal example:

```javascript
import http from 'k6/http';
import { check, sleep } from 'k6';

export const options = {
  scenarios: {
    steady: {
      executor: 'constant-arrival-rate',
      rate: 100, timeUnit: '1s', duration: '5m',
      preAllocatedVUs: 50, maxVUs: 200,
    },
  },
  thresholds: {
    http_req_failed: ['rate<0.01'],
    http_req_duration: ['p(95)<500', 'p(99)<1500'],
  },
};

export default function () {
  const res = http.get('https://test.k6.io/');
  check(res, { 'status 200': (r) => r.status === 200 });
  sleep(1);
}
```

The scripting model is single-threaded per VU. Async control flow works via promises but there is no `Worker`, no Node APIs, and no access to the filesystem inside the VU function. Data must be loaded via the `init` stage (top-level module code) and passed by reference.

## Scenarios and executors

Scenarios are the unit of workload composition. Multiple scenarios can run concurrently with different executors, tags, envs, and exec functions. The executors break down into closed-model and open-model classes:

**Closed (VU-bound)** — a fixed pool of VUs executes iterations as fast as they can. The rate is implicit, shaped by the system under test's response time:
- `constant-vus` — fixed N VUs for duration.
- `ramping-vus` — stages list with `target` VU counts and `duration` per stage.
- `shared-iterations` — fixed total iteration count distributed across VUs.
- `per-vu-iterations` — each VU runs a fixed iteration count.

**Open (arrival-rate)** — k6 schedules new iterations at a target rate regardless of whether previous iterations have completed, allocating from a VU pool sized by `preAllocatedVUs` and `maxVUs`:
- `constant-arrival-rate` — fixed iterations per `timeUnit`.
- `ramping-arrival-rate` — stages of target rate.
- `externally-controlled` — VU count controlled via REST API while the test runs.

The open-model executors are the ones that actually model Poisson-ish traffic and are what you want for SLO testing. Closed-model tests suffer coordinated omission: if the server is slow, the load generator naturally slows down with it and hides the tail. `constant-arrival-rate` is k6's answer to wrk2's `-R` flag.

## Thresholds

Thresholds are assertions on metric aggregates evaluated at the end of the run (or continuously if `abortOnFail` is set). They operate on k6's four metric types: Counter, Gauge, Rate, and Trend. Syntax:

```javascript
thresholds: {
  'http_req_duration{endpoint:checkout}': [
    { threshold: 'p(95)<800', abortOnFail: true, delayAbortEval: '30s' },
  ],
  'checks{kind:critical}': ['rate>0.99'],
}
```

If a threshold fails, the process exits non-zero. This is the entire CI integration story — k6 fits into a shell step that gates a deploy.

## Output and reporting

Built-in output is stdout text plus a final summary. Structured outputs include JSON, CSV, InfluxDB, Prometheus remote-write, and the Grafana Cloud k6 sink. `handleSummary()` lets scripts post-process the end-of-test snapshot (common use: write HTML via `k6-reporter`, push to BigQuery). Real-time streaming uses the Prometheus remote-write or experimental Prometheus Pushgateway outputs.

## Distributed execution

OSS k6 is single-process: a single `k6 run` invocation on one machine. There is no built-in master/worker protocol. Distributed execution is achieved via (a) Grafana Cloud k6, (b) the Kubernetes operator `k6-operator` that shards by scenarios into N parallel runner pods and aggregates in Prometheus, or (c) DIY with `k6 run --execution-segment`. The segment flag deterministically partitions scenario load across N independent runners.

## xk6 extensions

xk6 is a Go toolchain that builds custom k6 binaries with additional modules. Extensions are Go modules registered via `modules.Register('k6/x/name', New)` and called from JS. Popular ones: `k6/x/browser` (in-tree since 2023, Chromium via CDP), `xk6-kafka`, `xk6-sql`, `xk6-grpc` (now core), `xk6-disruptor` (chaos injection), `xk6-output-prometheus-remote`. Extensions cannot be dynamically loaded — they must be compiled in. `xk6 build --with github.com/org/xk6-foo` produces the custom binary.

## Browser testing

The k6 browser module (formerly xk6-browser) drives Chromium via the Chrome DevTools Protocol. API is Playwright-like (`page.goto`, `page.locator(...).click()`). Each VU owns a browser context; each iteration opens a page. Core Web Vitals — LCP, FCP, CLS, TTFB, INP — are captured as Trend metrics. Browser VUs are 50–100x heavier than protocol VUs; practical limits are tens to low hundreds of browser VUs per core. Browser tests are explicitly for user-flow SLOs, not throughput.

## Failure modes in practice

- **"My CPU is the bottleneck"** — k6's own machine becomes the slowest link. Profiling the load generator itself (`k6 run --profiling-enabled`) is the fix; scaling to more runners via execution-segment is the workaround.
- **Coordinated omission** if you use closed-model executors under a slow service — use `constant-arrival-rate`.
- **Memory blowup** from retaining `http.Response` objects across iterations — the init stage is shared; any mutable state leaks.
- **`goja` is not V8** — some modern JS features (ESM dynamic import, async iterators in some edge cases) are missing or incomplete. TypeScript is compiled, not type-checked.
- **Thresholds only evaluate at end** unless you specify `abortOnFail`. Silent drift until the run completes.

## Strengths

- Developer-first DX: a k6 script reads like a unit test. Engineers who refuse to touch JMeter will write k6.
- Open-model executors built-in; coordinated-omission-aware by default if you pick the right executor.
- First-class Prometheus and Grafana integration.
- Binary is ~50MB static Go; trivial to ship in a container.

## Relevance to the APEX G-46 spec

k6 is the reference "load testing" tool in the G-46 competitive landscape. It validates SLOs against a known workload but does not generate worst-case inputs, does not estimate complexity, and does not surface security-relevant slowdowns as findings. APEX's G-46 resource-guided fuzzer and k6 are complementary: APEX finds the inputs that break things, k6 sustains the load once the worst cases are known.