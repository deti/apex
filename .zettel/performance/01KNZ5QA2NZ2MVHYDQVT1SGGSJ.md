---
id: 01KNZ5QA2NZ2MVHYDQVT1SGGSJ
title: "LoadView — Dotcom-Monitor's real-browser SaaS load testing"
type: literature
tags: [tool, loadview, saas, browser-testing, load-testing, synthetic-monitoring, dotcom-monitor, commercial]
links:
  - target: 01KNZ5F8V2F1AXVHP058DM3B4H
    type: related
  - target: 01KNWE2Q9YZBAR140ZX5P36TQ5
    type: related
created: 2026-04-11T21:03:49.333162+00:00
modified: 2026-04-11T21:03:49.333163+00:00
---

Source: https://www.loadview-testing.com/ — LoadView product page by Dotcom-Monitor, fetched 2026-04-12.

LoadView is a cloud-based load testing platform from Dotcom-Monitor, a synthetic monitoring company. It distinguishes itself in the SaaS landscape by leading with **real browser load testing** — every virtual user is a full browser instance, not a protocol-level HTTP client.

## What it runs

LoadView supports three test types:
1. **Web application / EveryStep** — real browser testing with scripts recorded via the **EveryStep Web Recorder**, Dotcom-Monitor's proprietary browser recorder. Scripts capture click, type, navigate, assert operations on actual DOM elements.
2. **HTTP/cURL** — protocol-level tests against API endpoints. Simple request/response, lightweight VUs.
3. **Mobile web** — browser tests with a mobile user-agent and viewport.

The distinctive path is real-browser testing at scale. LoadView claims to spin up thousands of real browser instances simultaneously from cloud load generators — an operationally impressive feat given the per-browser resource cost (~50–100x a protocol VU).

## Global load injectors

LoadView runs load from 40+ global cloud locations (AWS, Azure, Rackspace, Dotcom-Monitor's own infrastructure). Geographic distribution is a first-class knob, supporting "run the test from Tokyo, London, and São Paulo simultaneously" scenarios.

## Synthetic monitoring integration

LoadView is tightly integrated with Dotcom-Monitor's broader synthetic monitoring suite: the same EveryStep scripts used in load tests can run as continuous monitoring checks from the same global locations. This is an operational synergy SaaS load-only tools lack.

## Test types

- **Load Step Curve** — concurrent user count rises/plateaus/drops over the test.
- **Load Test Goal** — target a specific user count and sustain it.
- **Dynamic Adjustable** — manually adjust user count during the test via the dashboard.

## Strengths

- Real browsers at scale with minimal configuration — the platform hides the infrastructure complexity.
- EveryStep recorder reduces script authoring friction for non-developers.
- Geographic distribution across 40+ cloud regions.
- Synthetic monitoring + load testing in one pane of glass.

## Failure modes

- **Expensive** — real browser VUs are the most costly per-VU model. Large tests add up fast.
- **EveryStep is proprietary** — scripts don't port to other tools. Vendor lock-in at the script level.
- **Smaller ecosystem** than BlazeMeter or Grafana Cloud k6. Fewer integrations with CI/CD and APM tools.
- **Browser overhead distorts measurements** at extreme scale — you're testing the browser stack as much as the service.
- **No OSS script compatibility** — LoadView does not run JMeter, Gatling, or k6 scripts; you must use EveryStep.

## Relevance to APEX G-46

LoadView is the commercial reference for real-browser load testing, the SaaS counterpart to k6 browser and Artillery Playwright. For G-46's browser-level workload class, LoadView illustrates the operational tradeoff: real browsers give the most accurate user-perceived metrics but scale poorly per dollar compared to protocol-level load. APEX's browser-workload output path should emit EveryStep-compatible scripts (or the in-tree alternatives) where the user-perceived-performance question is the primary driver.