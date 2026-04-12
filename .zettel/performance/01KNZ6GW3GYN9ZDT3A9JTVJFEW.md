---
id: 01KNZ6GW3GYN9ZDT3A9JTVJFEW
title: JMeter HTTP(S) Test Script Recorder and Gatling Recorder
type: literature
tags: [jmeter, gatling, recorder, har, browsermob, test-generation, record-replay]
links:
  - target: 01KNZ56MS27NFFMFBHZ0WPCV70
    type: related
  - target: 01KNZ56MRW2B1XSH2X5K5AEJ33
    type: related
  - target: 01KNZ55PD9MF8HE845RYWGYZ33
    type: related
  - target: 01KNZ5SMF1GAFA93P6D8TQFM4Z
    type: related
  - target: 01KNZ55NR8RQ3A2AM26Q7J92AG
    type: related
  - target: 01KNWE2Q9YZBAR140ZX5P36TQ5
    type: related
created: 2026-04-11T21:17:46.992071+00:00
modified: 2026-04-11T21:17:46.992077+00:00
source: "https://jmeter.apache.org/usermanual/jmeter_proxy_step_by_step.html"
---

# JMeter HTTP(S) Test Script Recorder and Gatling Recorder

The two most-used open-source recorders in the HTTP load-testing space. Both follow the same basic pattern — proxy the browser, capture every request, emit a script — but with different output languages and different trade-offs.

## JMeter HTTP(S) Test Script Recorder

Shipped with Apache JMeter (a Test Script Recorder, formerly HTTP Proxy Server). Usage:

1. In JMeter, add an "HTTP(S) Test Script Recorder" element.
2. Configure the browser to use JMeter as a proxy (e.g., localhost:8888).
3. For HTTPS, import JMeter's generated root certificate into the browser trust store.
4. Start recording; drive the app; stop recording.
5. Captured HTTP samplers appear in the JMeter tree, ready to be organised into thread groups.

Key JMeter-specific behaviour:

- **Includes/excludes.** You can filter by URL regex to drop static assets or auth redirects.
- **Transaction controller grouping.** Captured requests can be grouped into logical steps during recording with a "transaction controller" element.
- **Certificate generation.** JMeter generates its own root CA and signs per-host certificates on the fly using keytool (available in any JDK). This has been solid since JMeter 2.10 (2014).
- **BadBoy integration.** BadBoy (Windows-only, external tool) was long the recommended way to record on Windows because of UX issues with JMeter's built-in recorder. BadBoy exports .jmx files directly.

### Known failure modes

1. **Embedded resources explosion.** A typical page load captures 100+ requests (favicon, analytics pixels, CDN assets). The resulting .jmx is unreadable unless aggressively filtered.
2. **Certificate pinning defeat.** Many modern apps pin certificates and refuse to trust the JMeter root CA. Recording them requires disabling pinning, often not trivially.
3. **Correlation is manual.** JMeter has "regular expression extractor" and "JSON extractor" post-processors, but applying them to a captured recording is a hand edit. There's no automation.
4. **JMX XML is opaque.** The emitted file is deeply nested XML, hard to diff and hard to merge. The workflow assumes the .jmx is generated once and lovingly maintained thereafter.

## Gatling Recorder

Shipped as a separate tool in the Gatling distribution. Two modes: HTTP proxy (like JMeter) and **HAR file import**. The HAR mode is a distinctive and welcome feature — you capture in Chrome devtools (or any HAR-producing tool), then import the HAR into the Gatling Recorder, which emits a Scala/Kotlin/JavaScript Simulation class.

- **Proxy mode.** Gatling proxies, captures, generates Simulation classes. Similar to JMeter.
- **HAR mode.** No proxy setup — use any HAR source. This is much less fragile for modern HTTPS apps because you don't deal with certificate injection.
- **Output.** Scala DSL by default (Gatling 3.x also supports Java and Kotlin). The DSL is more readable than JMeter XML and much easier to version-control and review.

Gatling's recorder is generally considered the more modern, more maintainable choice. The downside is that Gatling is a more opinionated framework with a steeper Scala learning curve for teams who have always used JMeter.

### Known failure modes

1. **Same embedded-resource problem.** The HAR has everything; the recorder filters imperfectly.
2. **Same correlation problem.** Dynamic values need to be manually extracted. No auto-correlation.
3. **Scala is an adoption tax.** Engineers unfamiliar with Scala find the generated code less accessible than k6's JavaScript output.
4. **Versioning.** Gatling has had multiple incompatible versions (2.x → 3.x → 3.9+). Recorder output occasionally needs adjustment across upgrades.

## BrowserMob Proxy

A separate Java library that exposes a programmatic HAR-capture interface. Typically used in combination with Selenium: a test framework runs a functional test in a headless browser, BrowserMob captures the HAR, which is then fed to a load tool (k6, Gatling, JMeter). This is the Java ecosystem's answer to "record a functional test, replay it as a load test."

Key pattern: **decouple capture from load.** The capture is done in a real browser with real JavaScript execution; the load is driven at the HTTP level for scale. BrowserMob is the glue.

## Structural observation across the recorder family

All three tools (JMeter, Gatling, BrowserMob) share the same assumptions:

- The engineer has a browser and an app.
- The engineer can drive the app through a representative user flow.
- The captured flow is representative of the population of users.

The third assumption is almost always wrong. One engineer captures one session; that one session does not represent the distribution of real user behaviour. This is the *fundamental gap* of the record-and-replay workflow: it gets you *a* test, not a *realistic* test. The CBMG / session-mining approach is the principled alternative.

## Why these tools still dominate in practice

Despite the limitations:

1. **Zero setup.** Open the tool, record, save. No observability pipeline, no data science, no source code analysis.
2. **Immediate visual feedback.** You can run the recorded script and see if it reproduces what you did.
3. **Works for apps you don't own.** If you're load-testing a vendor's API, you don't have access to their logs or traces; recording is the only option.
4. **QA-team accessible.** A QA engineer who already runs manual exploratory tests can produce a starting load test in an hour with a recorder. The data-scientist workflow cannot reach them.

The recorder class of tools will remain alive for these reasons even after better options exist.

## LLM additions to this space

k6 Studio's autocorrelation feature (see dedicated note) is the first significant LLM addition to the recorder workflow. Natural next steps an LLM could automate:

- **Embedded-resource filtering.** Given a HAR, the LLM picks which requests are primary API calls and which are static-asset noise. This is a classification task LLMs are good at.
- **Transaction boundary detection.** Given a HAR, the LLM groups requests into logical "login," "search," "add to cart" steps. Another classification task.
- **Assertion authoring.** Given a captured response, the LLM proposes meaningful assertions beyond "status is 200." This is closer to the test-oracle problem and harder.

## Citations

- JMeter proxy step-by-step: https://jmeter.apache.org/usermanual/jmeter_proxy_step_by_step.html
- Gatling recorder docs: https://gatling.io/docs/current/http/recorder/
- Gatling recorder features: https://gatling.io/features/gatling-recorder/
- BadBoy JMeter integration: https://riptutorial.com/jmeter/example/27414/script-recording-with-badboy
- BrowserMob Proxy: https://github.com/lightbody/browsermob-proxy
- hrrs (Java HTTP recorder): https://github.com/vy/hrrs