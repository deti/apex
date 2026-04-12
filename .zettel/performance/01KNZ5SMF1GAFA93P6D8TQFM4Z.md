---
id: 01KNZ5SMF1GAFA93P6D8TQFM4Z
title: k6 Studio — Desktop Recorder with LLM-Driven Autocorrelation
type: literature
tags: [k6-studio, k6, recorder, autocorrelation, llm, har, grafana, test-generation]
links:
  - target: 01KNZ5SM642DR52PJ1CDNEZ101
    type: related
  - target: 01KNZ56MPVSZD05KM395ZKAM5J
    type: related
  - target: 01KNZ55PD9MF8HE845RYWGYZ33
    type: related
  - target: 01KNZ6GW3GYN9ZDT3A9JTVJFEW
    type: related
  - target: 01KNWE2Q9YZBAR140ZX5P36TQ5
    type: related
created: 2026-04-11T21:05:05.505426+00:00
modified: 2026-04-11T21:05:05.505431+00:00
source: "https://grafana.com/docs/k6-studio/"
---

# k6 Studio — Desktop Recorder for k6 Scripts

Grafana k6 Studio is an open-source desktop application (Electron app, released publicly in 2024, repo grafana/k6-studio) that helps engineers create k6 scripts without hand-writing JavaScript. Available for macOS, Windows, and Linux.

## The workflow

1. **Record.** Click "New Recording," k6 Studio launches a Chrome instance through an internal mitm-style proxy, and captures every HTTP request the browser makes as the user drives the application.
2. **Inspect.** The captured HAR is displayed in a tree view with timing, headers, bodies.
3. **Rule-based transformation.** k6 Studio applies "test rules" to the recording to transform it into a usable script. Rules include: correlation (extract a value from a response, substitute it into a later request), parameterisation (replace a hardcoded value with a data-file lookup), and custom JS (inject code after a request).
4. **Generate.** Emit a k6 script (JavaScript) that the engineer can run or commit.
5. **Validate.** Run the generated script against the target to confirm it still works.

This is the lineal descendant of Gatling Recorder and JMeter's proxy recorder — a GUI tool that turns "drive the app in a browser" into "runnable load test."

## The LLM twist: AI autocorrelation

The feature that makes k6 Studio worth a separate note from every other browser-proxy recorder is **AI autocorrelation**. Clicking "Autocorrelate" sends the recording to an LLM-backed service that (per the docs):

- Analyses every request/response pair to identify dynamic values — things that change between runs (session tokens, CSRF tokens, order IDs, paging cursors).
- Identifies which *subsequent requests consume* each dynamic value.
- Emits extractor/replacer rules that put the right dynamic value in the right place.
- Validates the rules against the recording to confirm they produce a passable replay.

Correlation is by far the most tedious step in turning a recording into a runnable load test — it's what kills most script-recording workflows in practice. If AI autocorrelation works reliably, it closes the single biggest gap in the record-and-replay workflow.

## How well does it work?

As of the first released version in 2024, the autocorrelation feature:

- Handles straightforward cases well: JWT extracted from a login response and used in `Authorization` headers on all later requests.
- Handles order/resource IDs well when the name of the field in the extractor source matches the name in the consumer.
- Struggles on unusual casing or synonyms (e.g., `orderId` in the producer vs. `OrderID` or `order` in the consumer path).
- Struggles on values constructed as a concatenation of multiple response fields.
- Struggles on values embedded in non-JSON responses (HTML forms, XML).

This is the same brittleness RESTler's rule-based dependency inference has, but applied at recording time. The LLM gets more cases right than static rules because it can reason about semantics, but the long tail is still real.

## Failure modes beyond correlation

1. **One recording = one user.** Same problem as HAR → k6 in general. Need to layer data-file parameterisation on top.
2. **Only records browser traffic.** No mobile, no native client, no server-to-server.
3. **No workload profile.** k6 Studio generates a script; the engineer still has to write the `options.scenarios` block.
4. **LLM cost leak.** The autocorrelation feature calls an LLM service (Grafana-hosted in the free tier); there's a quota and latency for large recordings.

## Strategic read

k6 Studio is the clearest sign Grafana has looked at the test-generation landscape and decided the leverage point is **recording → correlation → script**, not **spec → script**. That's probably correct for product-focused teams — most engineers have a browser and an app, fewer have a clean OpenAPI spec with great examples.

Combined with the existing `postman-to-k6`, `openapi-to-k6` (the newer TypeScript client generator), and `xk6-client-tracing`, Grafana's picture of "how you get to a k6 script" now spans every common starting point. The LLM-autocorrelation step is the only place they have put AI to work so far, and it is a narrow, well-chosen application.

## Toolmaker gap around k6 Studio

The piece still missing: **LLM-driven workload-profile generation**. Given a recording and access-log histograms (or a trace), an LLM that emits the correct `scenarios` block with arrival-rate-based executors would finish the story. k6 Studio doesn't do this — it stops at the script.

## Citations

- https://github.com/grafana/k6-studio
- https://grafana.com/docs/k6-studio/
- https://grafana.com/docs/k6/latest/k6-studio/
- Releases: https://github.com/grafana/k6-studio/releases