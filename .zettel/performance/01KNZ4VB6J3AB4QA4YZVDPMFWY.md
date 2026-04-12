---
id: 01KNZ4VB6J3AB4QA4YZVDPMFWY
title: "Think Time and Session Modeling"
type: concept
tags: [think-time, session, workload-model, closed-system, schroeder, meier-2007]
links:
  - target: 01KNZ4VB6JX0CQ5RFAZDJTQMCS
    type: related
  - target: 01KNZ4VB6JNFK2XFWP9N1HEJ5M
    type: related
  - target: 01KNZ4VB6J56B59YB7SZDKTAKD
    type: related
  - target: 01KNZ4VB6JDWZF3NFVSD5ATJV8
    type: related
  - target: 01KNZ5ZPPXW26VRNZ9BHKB0AYV
    type: related
  - target: 01KNZ6QBH0YZYKPZNYDCZD5P2B
    type: related
  - target: 01KNWE2Q9YZBAR140ZX5P36TQ5
    type: related
created: 2026-04-12
modified: 2026-04-12
source: "Schroeder et al. NSDI 2006; Meier et al. 2007 Ch. 12; Arlitt 'Characterizing web user sessions' SIGMETRICS 2000"
---

# Think Time and Session Modeling

## What think time is supposed to model

Real users of a system pause between actions. They read the page, type a search, consider the options. These pauses — "think time" — separate successive requests from the same user. In a workload model that tries to represent real usage:

- A user submits a request.
- Waits for the response.
- **Thinks** for some time drawn from a distribution (typical mean: 5–30 seconds for web browsing, longer for read-heavy apps, near-zero for API clients or SPA clients).
- Submits the next request.
- Occasionally ends the session and leaves.

A **session** is the sequence of requests from one user between arrival and departure. The session has:

- **Length** (number of requests — Geometric, or heavy-tailed in practice).
- **Shape** (which pages/endpoints in which order — often a small state machine derived from a traces).
- **Inter-request think times** (one distribution per transition, or one global distribution).
- **Arrival time** (when the session starts — Poisson-of-sessions is a common assumption).

## How think time is used in load generators

Most closed-model generators (JMeter, Gatling closed mode, Locust, k6 constant-VUs executor) have a per-virtual-user loop:

```
for each virtual user:
    loop:
        request = pick next from session state machine
        send(request); wait for response
        sleep(think_time_sample())
```

The think-time sample is drawn either from a constant, a uniform, a Gaussian, or (rarely) a heavy-tailed distribution matching traces. The mean think time, multiplied by the number of users, relates to the effective offered request rate via Little's Law: throughput = users / (mean think time + mean response time).

## The counter-intuitive result: think time barely affects partly-open system throughput

Schroeder et al. NSDI 2006 (`01KNZ4VB6JX0CQ5RFAZDJTQMCS`) prove as their Principle (viii):

> *In a partly-open system, think time has little effect on mean response time.*

Intuition: changing think time only changes *when* a user's next request goes out, not how many requests they issue. The total offered load ρ = λ × E[requests per session] × E[service time] is independent of think time. The only effect of think time is to add small correlations in the arrival stream; for PS and FCFS scheduling under product-form workloads, the correlations cancel out in the response-time distribution.

This is contrary to common practitioner intuition ("if we cut think time in half, the server gets twice the load"). Halving think time in a *closed* system (fixed MPL) does increase load — but only because you're forcing the same N users to issue more requests, which is a reparametrisation of the workload, not an effect of think time per se. In a *partly-open* system with independent session arrivals, think time is a measurement-frequency knob, not a load knob.

## Why this matters

If you believe the folklore ("think time controls load"), you will spend effort calibrating think time to match production traces, thinking it's critical. It's not. What is critical:

1. **Session arrival rate** (λ — requests per second of new sessions).
2. **Session length distribution** (E[R] — requests per session, and its tail).
3. **Session shape** (the endpoint mix).

These three determine the offered load. Think time is (for the partly-open case) decoration.

For a *closed* model, think time is essential because it's the only free parameter that can change load given fixed MPL. But see Schroeder et al. again: closed models are appropriate for only a minority of real web workloads, so think time's importance is usually over-weighted.

## Heavy-tailed think time

Real think-time distributions are heavy-tailed. Arlitt 2000 (web traces) found that some users pause for hours between clicks (open tab, forgot about it). Modelling think time as exponential or uniform truncates the tail and underestimates the variance of session activity.

However, per Principle (viii), the tail of the think-time distribution barely affects the load, so you can use a simple distribution without losing accuracy. The tail matters for *session detection* in the trace (see below) but not for response-time prediction.

## Session identification in traces

To derive session parameters from a real access log:

1. **Group requests by user identifier** (IP, cookie, session ID, authenticated user).
2. **Split into sessions by timeout**: a gap larger than τ = 1800 s (30 min is the de facto standard, per Menasce & Almeida 2000 and Arlitt 2000) separates sessions.
3. **Count requests per session** (→ E[R] and its distribution).
4. **Measure inter-request time within sessions** (→ think-time distribution).
5. **Measure session arrival rate** (→ λ).

Schroeder et al. also suggest picking τ from the knee in the "number-of-sessions vs timeout" curve rather than using 1800 s blindly.

## The partly-open model in practice

Partly-open model parameters:

- Session arrival rate: λ sessions/s (often Poisson or self-similar).
- Requests per session: Geometric with mean 1/(1−p), or empirical from traces.
- Service demand per request: from traces or benchmark.
- Think time: any reasonable distribution; per Principle (viii), choice doesn't matter much.

Generator support:
- **Tsung**: native support for session scripts with think-time distributions.
- **Gatling**: "scenarios" are sessions; `pause()` adds think time; open injection at session level.
- **Locust**: `TaskSet` with `wait_time` methods — closed model by default but can drive open injection via the `users` parameter with session-aware tasks.
- **JMeter**: "Thread Group" is a closed VU pool; plugins add session scripting.

## Anti-patterns

1. **Think time = 0 in a closed test.** Classic. N virtual users pounding as fast as possible. Measures nothing like production; answers "how many requests can N clients issue if they never pause" at best. Fix: at least match think time to production traces.

2. **Uniform short think time.** 500 ms between requests flat. Produces a uniform arrival stream that is unrealistic in variance; Poisson aggregation would be smoother, real traces would be more bursty. Fix: Use the actual distribution or accept the limitation.

3. **Closed-model "adjust think time to get target RPS".** The iteration "decrease think time until we hit 1000 req/s" is reparametrising the workload; it is not testing production-like behaviour. Fix: use an open/partly-open model with session arrival rate as the knob.

4. **Session state machine baked into the generator code.** Every endpoint mix change requires a code change; traces are ignored. Fix: data-driven sessions from exported logs.

5. **Ignoring session length.** "Each VU issues one request and restarts" is not a session; it is an open model with sessions of length 1. Real sessions have length > 1, and many tail behaviours (connection reuse, authenticated-only endpoints, cart state) only exercise under a real session.

## Adversarial reading

- Principle (viii) assumes product-form workloads and exponential distributions. Real distributions deviate, and with sufficient deviation think time *does* start to matter. But the deviation is rarely large enough to overcome the principle — think time remains a secondary knob.
- Think time is not load per se, but it is *a part of the workload model that practitioners over-tune*. Attention is limited; spending it on think time distracts from things that matter more (session shape, endpoint mix, arrival rate distribution).
- For API clients (machine-to-machine), think time is literally zero or very short. The partly-open framing still applies but with session lengths of 1 request, the partly-open model degenerates to open.

## References

- Schroeder, Wierman, Harchol-Balter — "Open Versus Closed: A Cautionary Tale" — NSDI 2006 — `01KNZ4VB6JX0CQ5RFAZDJTQMCS`.
- Arlitt, M. — "Characterizing Web User Sessions" — SIGMETRICS Performance Evaluation Review 28(2):50–63, 2000.
- Meier et al. — Microsoft p&p Performance Testing Guidance, Ch. 12 "Modeling Application Usage".
- Menasce, D., Almeida, V. — *Scaling for E-Business*, Prentice Hall 2000 — the 1800 s timeout convention.
