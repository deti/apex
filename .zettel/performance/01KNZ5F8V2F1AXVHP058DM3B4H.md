---
id: 01KNZ5F8V2F1AXVHP058DM3B4H
title: k6 browser module — Chromium-based browser load testing with Web Vitals
type: literature
tags: [tool, k6, k6-browser, browser-testing, web-vitals, load-testing, chromium, playwright-like]
links:
  - target: 01KNZ56MPVSZD05KM395ZKAM5J
    type: related
  - target: 01KNZ5F8VABE5TGW976NMQA1VP
    type: related
  - target: 01KNWE2Q9YZBAR140ZX5P36TQ5
    type: related
created: 2026-04-11T20:59:25.922666+00:00
modified: 2026-04-11T20:59:25.922668+00:00
---

Source: https://grafana.com/docs/k6/latest/using-k6-browser/ — Grafana k6 browser module documentation, fetched 2026-04-12.

The k6 browser module (formerly `xk6-browser`, merged into k6 core in 2023) drives real Chromium browsers via the Chrome DevTools Protocol (CDP) for browser-level load testing. It fills the same role as Playwright / Puppeteer for functional UI testing, but with the explicit goal of measuring user-perceived performance under synthetic user load.

## API

The API is intentionally Playwright-shaped. Engineers coming from Playwright can read and write k6 browser scripts with minimal new vocabulary.

```javascript
import { browser } from 'k6/browser';
import { check } from 'https://jslib.k6.io/k6-utils/1.5.0/index.js';

export const options = {
  scenarios: {
    ui: {
      executor: 'shared-iterations',
      options: { browser: { type: 'chromium' } },
    },
  },
  thresholds: {
    'browser_web_vital_lcp': ['p(95)<2500'],
    'browser_web_vital_cls': ['p(95)<0.1'],
  },
};

export default async function () {
  const page = await browser.newPage();
  try {
    await page.goto('https://test.k6.io/');
    await check(page.locator('h1'), {
      header_ok: async (h) => (await h.textContent()) === 'Welcome',
    });
    await page.locator('input[name=q]').type('test');
    await page.locator('button[type=submit]').click();
  } finally {
    await page.close();
  }
}
```

## Browser metrics

k6 browser automatically captures Core Web Vitals and surfaces them as k6 `Trend` metrics:

- `browser_web_vital_lcp` — Largest Contentful Paint
- `browser_web_vital_fcp` — First Contentful Paint
- `browser_web_vital_cls` — Cumulative Layout Shift
- `browser_web_vital_fid` — First Input Delay (deprecated in favour of INP)
- `browser_web_vital_inp` — Interaction to Next Paint
- `browser_web_vital_ttfb` — Time to First Byte

Plus lower-level metrics: `browser_http_req_duration`, `browser_http_req_failed`, `browser_data_sent`, `browser_data_received`, `browser_dom_content_loaded`.

Because these are ordinary k6 metrics they participate in `thresholds` and CI gating. "LCP p95 under 2.5 s" becomes an executable SLO.

## Architecture

Each virtual user owns a Chromium browser context (`browser.newContext()`). Each iteration typically opens and closes a page. This is critical for scaling: a browser VU is 50–100x heavier than a protocol VU. Realistic per-core budget is 5–20 browser VUs, not 500. A 50-VU browser test needs a substantial test host (8–16 cores).

Chromium is launched headless by default; `K6_BROWSER_HEADLESS=false` reveals the browser for debugging. The browser binary is provided by the system (`google-chrome`, `chromium`) rather than bundled — install is an external prerequisite.

## Difference from Playwright

Playwright is a general browser automation library: test one user at a time, assert the page is correct. k6 browser is a load-testing tool: run 100 synthetic users in parallel, aggregate latencies, gate releases on LCP p95. The two share the surface API but diverge on:

- **Parallelism**: Playwright Test parallel workers vs k6's VUs.
- **Measurement**: k6 computes aggregated percentile Trends; Playwright reports per-test timings.
- **Integration**: k6 browser metrics join k6's existing `http_req_duration`, thresholds, and output sinks. Playwright would need separate export tooling.
- **Sampling**: k6 browser tests are intended for "10% of traffic is browser-level" patterns — the bulk of load is still protocol, and a small fraction is browser-level for end-to-end coverage.

## Failure modes

- **Resource cost at scale** — a 100-VU browser test needs real infrastructure. People routinely design tests that don't fit on the test host and wonder why latencies are all terrible.
- **Headless Chromium != real browser** — rendering paths differ subtly; Web Vital numbers are indicative, not identical to real-user monitoring (RUM) data.
- **Test flakiness**: browser tests are always flaky. `page.waitForNavigation()`, `locator().waitFor()` are mandatory, not optional.
- **CDP connection thrashing** at high VU counts — processes crash, results are missing.
- **Single OS per k6 process** — no cross-OS distribution without multiple runners.

## When to reach for it

- End-to-end SLO tests: "checkout flow LCP under 2.5 s at 50 RPS".
- Detecting frontend regressions that only show up under load (JS main-thread contention, hydration bottlenecks).
- Synthetic monitoring from a handful of cloud regions.

## Relevance to APEX G-46

G-46's competitive landscape mentions k6 but not the browser module specifically. Browser-level load is the one area where load generation and functional correctness naturally merge, and it is the closest any mainstream tool comes to "worst-case user flow discovery". APEX's G-46 pipeline should treat browser-level workloads as a distinct target class with its own resource-measurement methodology (per-frame timings, CPU main-thread utilisation) rather than shoehorning them into the protocol-level SLO framework.