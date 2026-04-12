---
id: 01KNZ5F4WQ8VSNJBFYJFDSX7NT
title: GoReplay (gor) — HTTP Traffic Capture and Replay for Shadow Testing
type: literature
tags: [goreplay, traffic-replay, shadow-testing, production-traffic, test-generation, load-testing]
links:
  - target: 01KNZ5F59R6M5ATD9X2YW87XAC
    type: related
  - target: 01KNZ5F52X5746A9ASY0W6DKDS
    type: related
  - target: 01KNZ5F557YM1X8Q2ZBXZEBXRM
    type: related
  - target: 01KNZ5F50KTSH9742RX4CFT45K
    type: related
  - target: 01KNZ5ZPYD6VWX13H5G57D0TCH
    type: related
  - target: 01KNZ4VB6J56B59YB7SZDKTAKD
    type: related
  - target: 01KNZ6QBKG64F2WEP3PNJK61JH
    type: related
  - target: 01KNWE2Q9YZBAR140ZX5P36TQ5
    type: related
created: 2026-04-11T20:59:21.879443+00:00
modified: 2026-04-11T20:59:21.879450+00:00
source: "https://goreplay.org/"
---

# GoReplay — Open-Source HTTP Traffic Replay

GoReplay (also called `gor`) is an open-source tool for capturing live HTTP traffic from a production server interface and replaying it — either in real time or from a saved file — against another environment. Originally created by Leonid Bugaev around 2013, now at goreplay.org with commercial extensions.

## Core operations

- **Capture.** Binds to a network interface (libpcap) on a production node and captures raw HTTP traffic without any application-side agent. Zero-copy path minimises impact on production.
- **Serialise.** Writes captured requests to stdout, a file, S3, or a pipe in a parseable format.
- **Replay.** Takes the captured stream and issues equivalent requests against a target URL.
- **Modify middleware.** Between capture and replay, user-provided middleware scripts (in any language that can read/write stdin/stdout) can rewrite requests — strip PII, rewrite hostnames, rewrite auth tokens.
- **Percentage replay.** Replay only N% of captured traffic, useful when the test environment has less capacity than production.
- **Multi-destination / amplification.** Fan out one captured request to multiple destinations at once, or replay at >100% to load-test beyond current traffic.

## Usage shape

```sh
# Capture on prod, write to file
gor --input-raw :8080 --output-file requests.gor

# Replay to staging
gor --input-file requests.gor --output-http https://staging.example.com

# Shadow 10% of prod into staging live
gor --input-raw :80 --output-http "https://staging.example.com|10%"
```

## Fidelity arguments in favour

- **Real user data shapes.** The replayed requests have actual payload distributions. No generator-imposed uniformity. This is the single biggest win compared to schema-driven generation — the tail cases that trigger perf bugs *are in the capture*.
- **Real correlations.** Because what you are replaying is what real users did, multi-step flows are correct by construction.
- **Real arrival process.** If you replay at original wall-clock pacing, you get the real Poisson/burst arrival characteristics that are almost impossible to synthesise.
- **Minimal application coupling.** Capture is a network-level operation; no SDK to install, no code to change.

## Failure modes (the "replay divergence problem")

These are what make replay *not* a solved problem:

1. **State divergence.** Replayed traffic assumes the target environment has the same state as production. An authenticated request replayed 10 minutes later fails auth. A `DELETE /orders/42` replayed against a staging environment where order 42 never existed returns 404 and teaches you nothing. This is the single biggest practical issue; the GoReplay middleware hooks help but do not solve it.
2. **Side effects.** Replayed `POST`/`PUT`/`DELETE` requests cause real state changes in the target. If the target is connected to real downstream services (payment processors, email sender), replay sends real emails and charges real cards. Teams have been burned badly here. Mitigations: route side-effecting calls to stubs (Hoverfly / WireMock), or only replay GETs, or run against a fully isolated copy of the database and downstreams.
3. **PII and compliance.** Captured production traffic contains PII. GDPR, HIPAA, PCI compliance requires scrubbing before the capture hits any non-prod storage. Scrubbing is usually pattern-based (regex on header/body fields) and fragile.
4. **Encrypted traffic.** Most production HTTP is TLS. GoReplay captures at the TCP layer and cannot see inside TLS without access to keys. Practical deployments put GoReplay *behind* the TLS terminator (nginx, envoy, ALB). That's a topology requirement and constrains where you can run it.
5. **Clock and schema drift.** Captured traffic embeds timestamps, idempotency keys, and other time-sensitive fields. Replayed days/weeks later, those fields collide or look stale. Plus the API schema may have drifted since capture, so the replay now hits deprecated endpoints that 404 or behave differently.
6. **Load amplification correlation.** If you replay at 200% of captured traffic, the resulting load is *not* 2× the production workload — it is the same 1× workload with every request duplicated. Duplicates often hit the same cache lines and database rows, giving an artificially optimistic cache hit rate. Real 2× production traffic has twice as many *different* users. This is a subtle but important distinction for capacity planning.

## Where it is strictly dominant

- Regression testing for non-destructive GET-heavy APIs (search, content, read-only dashboards).
- Any scenario where you want a workload closer to production than any synthetic generator can reach.
- When combined with Diffy-style response diffing, it becomes a tool for catching behavioural regressions in addition to perf regressions.

## Commercial extension

The GoReplay Pro fork adds TCP-level TLS decryption support (given a key), better large-scale orchestration, and an S3 backend. The open-source version is still the baseline.

## Citations

- https://goreplay.org/
- https://goreplay.org/docs/
- https://goreplay.org/shadow-testing/
- https://goreplay.org/load-testing/
- https://goreplay.org/blog/how-traffic-replay-improves-load-testing-accuracy/