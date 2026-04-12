---
id: 01KNZ55PD9MF8HE845RYWGYZ33
title: HAR → k6 and har-to-k6 — Browser Capture to Load Script
type: literature
tags: [har, k6, har-to-k6, recorder, test-generation, browser-capture]
links:
  - target: 01KNZ56MPVSZD05KM395ZKAM5J
    type: related
  - target: 01KNZ4TTX5V1TESBMRM80J38XA
    type: related
  - target: 01KNZ6GW3GYN9ZDT3A9JTVJFEW
    type: related
  - target: 01KNZ5SMF1GAFA93P6D8TQFM4Z
    type: related
  - target: 01KNWE2Q9YZBAR140ZX5P36TQ5
    type: related
created: 2026-04-11T20:54:12.137132+00:00
modified: 2026-04-11T20:54:12.137137+00:00
source: "https://grafana.com/docs/k6/latest/reference/integrations/"
---

# HAR → k6 — Browser Network Capture as Load Test Seed

HAR (HTTP ARchive) is a JSON-based capture format for HTTP transactions, supported by every major browser's devtools export ("Copy as HAR"), mitmproxy, Charles, Fiddler, and BrowserMob Proxy. Converters exist for every load tool: har-to-k6 (Grafana), JMeter's HAR import, Gatling's HAR import, Locust HAR plugins.

## The workflow

1. Open browser devtools, start network recording.
2. Drive the application through a representative user journey (login → search → add to cart → checkout).
3. Export the recording as HAR.
4. Run `har-to-k6 recording.har -o script.js` to emit a k6 script that replays every captured request in order.

The generated k6 script reproduces the captured sequence at a rate controlled by the k6 `options.scenarios` block the engineer adds.

## What makes HAR capture a useful starting point

- **Real browser sequencing.** Browsers make the actual set of requests (HTML, CSS, JS, images, XHR, fetch, websockets upgrade) that a real user experiences. A perf test built from HAR already looks like a browser session.
- **Headers are captured.** Cookies, Authorization, User-Agent, Accept, Accept-Encoding — all there. This is a huge time saver compared to reconstructing them from an OpenAPI spec.
- **Query strings and form bodies are recorded with the actual values the tester used.**
- **Request order is preserved.** Unlike OpenAPI, HAR has a temporal ordering that matches actual page navigation.

## Failure modes (the "captured traffic is a snapshot" problem)

1. **One user, one data set.** HAR is a capture of a single session by a single tester. Running N VUs through the same HAR fires identical requests with identical data. This collides on any uniqueness constraint (duplicate order IDs, repeated email signups) and completely misses load-relevant diversity.
2. **Session tokens are burned in.** The HAR includes specific JWTs/cookies that expire. A replay done an hour later fails auth. Fixing this requires either re-capturing every time or adding a pre-request step that fetches fresh tokens — both defeating the "just capture and replay" simplicity.
3. **Correlation is lost.** Browsers record request/response pairs independently; they do not annotate which response value ended up in which subsequent request. A replay tool that doesn't do correlation extraction misses the dependency (e.g., an order ID the server returned in response 5 is sent back in request 7). `har-to-k6` does not do correlation; the engineer has to hand-edit.
4. **Embedded resources dominate.** A captured HAR of a retail site might have 2 XHRs and 200 static-asset requests. If the goal is to load-test the API, the static assets are noise and need to be filtered — another hand-editing step.
5. **Browser-only — no mobile, no native.** To capture mobile API traffic you need a proxy (mitmproxy, Charles) installed on the device, which has increasing setup friction due to certificate pinning and iOS 17+ network extensions.
6. **No workload model.** Same gap as every other starting point: HAR says what the user did once, not what the population of users is doing per second.

## Where HAR shines

- Rapid bootstrap of a throwaway smoke load test for a web page. 10 minutes from "nothing" to "100 VUs hammering the API."
- Bridge artefact between a QA tester (who captures) and a perf engineer (who adds the workload profile). Teams with separated roles use this pattern.
- Authoring ground truth for correlation: a captured HAR can be diffed against the regenerated request stream from a synthetic generator to see what the generator missed.

## Related: BrowserMob Proxy

BrowserMob Proxy (Java) is the classic programmable browser proxy that generates HAR files on demand. Commonly used with Selenium to capture API traffic while functional tests run, then feed the HAR into a load tool. This is the closest thing the Java ecosystem has to "record a functional test, replay it as a load test."

## Citations

- k6 integrations reference: https://grafana.com/docs/k6/latest/reference/integrations/
- har-to-k6 (in the grafana/k6 org): referenced from integrations page
- HAR 1.2 spec: http://www.softwareishard.com/blog/har-12-spec/
- BrowserMob Proxy: https://github.com/lightbody/browsermob-proxy