---
id: 01KNZ6GWB2T03BDYP2SNGG8XJR
title: Open vs. Closed Workload Models — Schroeder et al. (NSDI 2006)
type: literature
tags: [open-workload, closed-workload, arrival-process, schroeder, wierman, harchol-balter, nsdi-2006, capacity-planning]
links:
  - target: 01KNZ4VB6JX0CQ5RFAZDJTQMCS
    type: related
  - target: 01KNZ4VB6J4TER1QCE9CKABBED
    type: related
  - target: 01KNZ4VB6J3AB4QA4YZVDPMFWY
    type: related
  - target: 01KNZ4VB6JDWZF3NFVSD5ATJV8
    type: related
  - target: 01KNZ56MPVSZD05KM395ZKAM5J
    type: related
  - target: 01KNWE2Q9YZBAR140ZX5P36TQ5
    type: related
created: 2026-04-11T21:17:47.234323+00:00
modified: 2026-04-11T21:17:47.234329+00:00
source: "https://www.usenix.org/legacy/events/nsdi06/tech/schroeder/schroeder.pdf"
---

# Open Versus Closed: A Cautionary Tale — Schroeder, Wierman, Harchol-Balter (NSDI 2006)

*Bianca Schroeder, Adam Wierman, Mor Harchol-Balter. NSDI 2006. One of the most cited papers in load-testing theory and the single best paper to read if you want to understand why most perf tests are wrong about concurrency.*

## The core distinction

**Closed workload model:** There are a fixed number of "users" (N). Each user issues a request, waits for the response, thinks for Z seconds, then issues the next request. The system sees at most N outstanding requests at any time. Throughput is bounded by N × (1 / (service time + think time)).

**Open workload model:** Requests arrive at some rate λ independent of how the system responds. New arrivals happen regardless of outstanding load. Queue lengths can grow without bound if λ > service rate.

These are **fundamentally different systems** with fundamentally different performance characteristics. The same service under the same offered load measured both ways gives different numbers — sometimes by orders of magnitude in the tails.

## What the paper shows

Schroeder et al. run the same server under closed and open workloads and demonstrate:

1. **Response time distributions differ by orders of magnitude.** Closed workloads have bounded queue lengths, so tail latencies are bounded. Open workloads can have unbounded tails as the system approaches saturation.
2. **The effect of scheduling policy is different.** Shortest-job-first helps open workloads enormously. It barely helps closed workloads because queue depth is bounded.
3. **Optimisations that help one model can *hurt* the other.** An admission-control policy that rejects excess requests improves open-workload tail latency at the cost of closed-workload throughput.
4. **Measurement artefacts.** Reporting "average response time" without saying which model you're in produces numbers that are not comparable.

The paper's name for the problem is blunt: **coordinated omission** — when a closed-loop load generator waits for the server to respond before sending the next request, it *underreports* slow responses because the client-side clock stops while the server is slow. Open-loop generators don't have this problem; closed-loop ones do unless they explicitly compensate.

## Why most load tests are wrong

Most off-the-shelf load tools (k6 with VUs and `sleep`, JMeter thread groups with default constant timer, Gatling simulations with VU-count ramps) use **closed workload by default**. They model N virtual users doing sequential work. Production web services usually experience something closer to **open workload** because user arrivals are externally driven and don't wait for the server.

Result: a CI load test that says "p99 is 200 ms" is measuring the closed-loop behaviour of a service whose real production behaviour is open-loop. The closed-loop number is an optimistic lower bound. When the service hits saturation in production, p99 explodes in ways the test didn't predict.

This is the most common subtle bug in load testing and almost nobody discusses it.

## How the modern tools handle it (or don't)

- **wrk2** (Gil Tene's fork of wrk) explicitly implements coordinated-omission-corrected open-workload load with constant throughput. It is the reference correct-by-construction open-loop HTTP benchmark. Every perf engineer should know it.
- **k6's `ramping-arrival-rate` executor** (and its siblings `constant-arrival-rate`) are open-loop. The default `constant-vus` executor is closed-loop. The docs call this out but the default is still closed-loop in tutorials and most generated scripts.
- **JMeter Constant Throughput Timer** is a partial open-loop model — it tries to hit a target RPS but underlying behaviour is still thread-gated.
- **Gatling** supports both `atOnceUsers` (closed) and `constantUsersPerSec` (open).
- **ghz** has `--rps` but it is applied to the underlying fixed-concurrency model, not a true Poisson arrival process.

## Implications for test generation

Any automated perf-test generator has to decide whether to emit open-loop or closed-loop scenarios. The right default is **open-loop** for user-facing services (because that's what production looks like) and closed-loop for internal batch systems (where a fixed pool of workers is the real model). LLM-generated tests today almost always emit closed-loop because it's the more common k6/JMeter example in the training data — which is exactly the wrong default.

A production-grade test generator should:

1. **Ask about the workload type** (user-facing vs. internal) or infer it from the API description.
2. **Default to open-loop for user-facing services.**
3. **Use coordinated-omission-corrected pacing** for open-loop tests (wait for send time, not for response).
4. **Report both open and closed metrics when the test is ambiguous** — running the same workload both ways surfaces more failure modes.
5. **Explain the choice to the engineer** so they can override.

## Adversarial reading of the paper

1. **Model space is binary.** The paper presents open and closed as distinct. Real systems are often **partly-open**: a request has a response dependency (closed-like) but the *session* has a think-time distribution that's independent of the server (open-like). The paper acknowledges this and introduces "partly-open" models, but most readers take only the binary away.
2. **Mean service time is the benchmark target.** The specific numbers in the paper are circa-2006 scheduling for web servers. The *relative* magnitudes of the difference matter more than the specific numbers.
3. **The implicit recommendation is "use open."** The paper is explicit that both are valid; the real lesson is "know which one you're measuring." Readers who take "always use open" as the takeaway are missing half the paper.

## Why this paper deserves a note

This is the single most influential paper on workload modelling for performance testing that is cited in capacity-planning literature. Any test generation tool that doesn't explicitly handle the open/closed distinction is producing partly meaningless numbers. The paper is short, old, and free. Everyone building perf tooling should have read it.

## Citations

- NSDI 2006 paper: https://www.usenix.org/legacy/events/nsdi06/tech/schroeder/schroeder.pdf
- wrk2 (Gil Tene): https://github.com/giltene/wrk2
- k6 executors (open vs closed): https://grafana.com/docs/k6/latest/using-k6/scenarios/executors/
- Gil Tene's coordinated-omission talk: https://www.youtube.com/watch?v=lJ8ydIuPFeU
- HdrHistogram (Tene): http://hdrhistogram.org/