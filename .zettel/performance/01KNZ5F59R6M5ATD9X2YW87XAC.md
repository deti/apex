---
id: 01KNZ5F59R6M5ATD9X2YW87XAC
title: The Replay Divergence Problem — Why Traffic Replay Lies
type: permanent
tags: [replay, traffic-replay, divergence, state, stateful-endpoints, concept, failure-mode]
links:
  - target: 01KNZ5F4WQ8VSNJBFYJFDSX7NT
    type: related
  - target: 01KNZ5F52X5746A9ASY0W6DKDS
    type: related
  - target: 01KNZ5F557YM1X8Q2ZBXZEBXRM
    type: related
  - target: 01KNZ5F57EZAMVV3P5991NVHJ9
    type: related
  - target: 01KNZ5F50KTSH9742RX4CFT45K
    type: related
  - target: 01KNZ5ZPYD6VWX13H5G57D0TCH
    type: related
  - target: 01KNZ6QBKG64F2WEP3PNJK61JH
    type: related
  - target: 01KNWE2Q9YZBAR140ZX5P36TQ5
    type: related
created: 2026-04-11T20:59:22.296028+00:00
modified: 2026-04-11T20:59:22.296034+00:00
---

# The Replay Divergence Problem

*A concept note extracted from reading across the traffic-replay tool family (GoReplay, tcpreplay, Envoy tap, VPC Traffic Mirroring, Diffy). This is the canonical failure mode that every replay-based perf test hits and most teams underestimate.*

## The claim

Every production traffic replay tool captures a request stream that was valid in one environment at one time, and attempts to re-issue it against a (possibly) different environment at a different time. The transformation "same bytes, different time/context" is lossy. The set of things that can break form the **replay divergence problem**.

A tool that captures 100% of production traffic does not give you 100% accuracy in the replay target. The accuracy degrades along several independent axes:

## Axis 1: Temporal state drift

Tokens expire. Nonces are used. Rate-limit buckets reset. A request that was valid at 14:00 UTC with a JWT issued at 13:55 is invalid at 15:00. Any replay run more than a few minutes after capture will see massive auth failures unless it rewrites tokens.

Typical failure signatures: 401/403 storms, empty session caches, cold-cache latency that does not match the captured response latency.

## Axis 2: Resource-ID divergence

A captured `GET /orders/42` assumes order 42 exists. Replaying against a staging environment where order 42 doesn't exist returns 404 and tells you nothing about the real latency of the not-found path vs. the found path.

More subtly: a replay at 50% of captured traffic hits 50% of the resource IDs, missing the long-tail cache-miss population that causes real incidents.

## Axis 3: Side-effect divergence

Replayed `POST /checkout` charges a credit card. Replayed `POST /emails` sends a real email. Replayed `DELETE /orders/42` deletes a real record.

Mitigations (all partial):

- Filter POST/PUT/DELETE and replay only GETs — loses the workload that matters most for perf.
- Route downstream calls to stubs (Hoverfly/WireMock) — requires intercepting every side-effect call and maintaining stub configs.
- Mark shadow traffic with a header (Envoy's `-shadow` suffix) and make every downstream service honour it — requires coordinated code changes across the whole service surface.
- Replay against a fully isolated replica with its own databases and stubbed downstreams — expensive, doesn't scale, and the isolated replica's data rapidly diverges from prod.

## Axis 4: Schema drift

The captured request was valid against the API schema that existed at capture time. Between capture and replay, engineers ship API changes: fields renamed, endpoints deprecated, response shapes altered. Replays of old requests hit either 404 Not Found or, worse, 200 OK with behaviour that is legitimately different from what the captured response showed.

This is the replay-correctness analog of the "workload drift" problem — your recording's schema drifts in exactly the same way that synthetic test-suite schemas drift, but it's hidden because the recording *looks* like real prod.

## Axis 5: Cache and working-set divergence

Production runs with warm caches full of the real working set. Staging usually runs cold. The same 1× replay has a fundamentally different memory-pressure and cache-hit profile. Observed latency is often 3–10× worse on the first few minutes of replay and then comes down as the cache warms.

Replaying 2× the captured traffic is not 2× the load — it's the same 1× working set with every request duplicated. Cache-hit rates are artificially inflated because repeated requests hit the same keys. For a capacity-planning load test this is a severe distortion.

## Axis 6: Timing divergence

Replaying at "original wall-clock pace" assumes the replay target processes requests at the same speed as production. If staging is slower, queues build up and the offered load diverges from the captured arrival process. Replay tools can either (a) pace by original timestamps regardless of target response times ("open workload," mimics real Poisson) or (b) gate new requests on prior completions ("closed workload," mimics capacity). These two modes give *different* answers, and there is no universally correct choice.

## Axis 7: Network topology divergence

IPs and hostnames in captured traffic have to be rewritten. NATed sources look different. TLS sessions cannot be replayed at all. Wire-level tools (tcpreplay) hit this hardest; application-level tools (GoReplay) hit it for Host headers and embedded URLs in bodies.

## Axis 8: Consumer/downstream divergence

For event-driven systems, replayed produce traffic only tests the producer path. The consumer side sees different downstream services — the same consumer code consuming the same events behaves differently depending on how the downstream Redis/DB/API is configured in staging. This is why Kafka replay is almost useless for capacity planning unless the whole downstream topology is mirrored, which practically no one does.

## Consequences for tool design

1. **Replay is never a closed-form operation.** Every replay system needs a per-service extension point to rewrite fields, refresh tokens, skip endpoints, and handle state.
2. **Replay accuracy is best reported as a divergence metric, not a binary.** Tools that tell you "we replayed 100% of traffic" without reporting what fraction of replays returned the expected status code are misleading. Diffy's baseline-subtraction idea is the right conceptual move: measure the unavoidable noise floor and report deltas relative to it.
3. **Replay is always a complement, not a substitute.** The industry pattern that works is: synthetic tests in CI for fast feedback + continuous shadow replay in staging for realism. Neither alone is sufficient.
4. **The "good replay" tooling gap.** No open-source tool currently gives first-class support for all of (a) token refresh, (b) PII scrubbing, (c) state management, (d) side-effect routing, (e) working-set amplification (replay at N× with distinct resource IDs, not duplicated keys). Any one of these is a hard problem; together they are the bulk of the engineering cost of running replay in production.

## Research directions

- **Data-aware replay** — sampling requests in a way that maintains the captured working-set distribution while allowing amplification. No open-source tool does this.
- **Causal replay** — using distributed traces (Jaeger, OpenTelemetry) to preserve causality across related requests when replaying.
- **Model-guided replay** — fit a Markov model to the captured sequence, then generate new synthetic-but-distributionally-faithful traffic at arbitrary rates. This is the conceptual bridge between replay and synthetic generation.

## Citations

- Drawn from the GoReplay, tcpreplay, Envoy tap, VPC Mirror, and Diffy notes in this vault.
- Axis 6 (open vs. closed workload) is a classic result; see Schroeder, Wierman, Harchol-Balter, "Open versus closed: a cautionary tale" (NSDI 2006): https://www.usenix.org/legacy/events/nsdi06/tech/schroeder/schroeder.pdf
- Axis 5 (working-set effects) is discussed in capacity-planning texts; Menascé "Scaling for E-Business" is the canonical reference.