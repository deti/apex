---
id: 01KNZ4VB6JNFK2XFWP9N1HEJ5M
title: "Poisson Arrival Processes in Load Generation"
type: concept
tags: [poisson, arrival-process, workload-model, queueing, mmpp, pasta]
links:
  - target: 01KNZ4VB6JX0CQ5RFAZDJTQMCS
    type: related
  - target: 01KNZ4VB6J4TER1QCE9CKABBED
    type: related
  - target: 01KNZ4VB6JSR9RJ0RTWXB9P6FV
    type: related
  - target: 01KNZ4VB6J6R3V3GVBWSAKW8JC
    type: related
  - target: 01KNZ5F8TVW837C1YJKKXFH504
    type: related
  - target: 01KNWE2Q9YZBAR140ZX5P36TQ5
    type: related
created: 2026-04-12
modified: 2026-04-12
source: "multiple — Kleinrock 'Queueing Systems' vol 1; Wikipedia Poisson point process"
---

# Poisson Arrival Processes in Load Generation

## The default arrival process

When a load generator says "arrival rate 1000 req/s" without further qualification, what arrival process does it generate? The cleanest default is a Poisson process: requests arrive independently, inter-arrival times are exponentially distributed with rate parameter λ = 1000/s, mean inter-arrival 1 ms. Most open-model generators (httperf, k6 constant-arrival-rate, Gatling open injection, wrk2's constant-throughput mode) default to Poisson or constant-rate; a few offer user-defined distributions.

Understanding *why* Poisson is the default — and *when* it is wrong — is the difference between a load test whose results you can trust and a load test that produces defensible-looking numbers on the wrong workload.

## Definition

A Poisson point process with rate λ has three equivalent definitions:

1. **Counts**: the number of arrivals in an interval of length t is Poisson-distributed with mean λt.
2. **Inter-arrival times**: consecutive inter-arrival times are independent and exponentially distributed with rate λ.
3. **Infinitesimal**: in an interval (t, t+dt), the probability of exactly one arrival is λ·dt + o(dt); of more than one is o(dt); arrivals in disjoint intervals are independent.

## Key properties

### Memoryless

The exponential distribution has the memoryless property: P(next arrival is within Δ | we have waited for time t already) = P(next arrival is within Δ). "Nothing that happened so far changes the probability of what happens next." This is the property that makes Poisson analytically tractable — every Markov chain for a Poisson-arrival queue simplifies.

### Independent increments

Counts of arrivals in disjoint intervals are independent. No correlation across time scales. This is the property that real traffic most often violates (see self-similar note).

### Merging and splitting

Two independent Poisson processes with rates λ₁ and λ₂ merge into a Poisson process with rate λ₁ + λ₂. Conversely, splitting a Poisson process with rate λ by marking each arrival independently with probability p gives two independent Poisson processes with rates pλ and (1−p)λ. These properties make Poisson the only arrival process that composes cleanly across aggregation boundaries.

### PASTA — Poisson Arrivals See Time Averages

Wolff's theorem (1982): under a Poisson arrival process, the time-average distribution of the system state is the same as the distribution seen by arrivals. In plain language: if you sample the queue's state at Poisson arrival instants, you see a statistically unbiased view of the queue's time-average state. This is a sampling theorem, not a queueing result; it holds regardless of service distribution.

PASTA is why Poisson simulations can report "arrival sees mean queue depth X" and get the correct answer without a separate time-average calculation. It is also why Poisson arrivals have special theoretical status in queueing theory — many results that are exact under Poisson are only approximate under other arrival distributions.

### Why Poisson is the "default"

1. **Maximum-entropy distribution for a fixed mean inter-arrival time.** No correlations, no clumping, no structure. It's the "least informative" arrival process compatible with a specified rate, which makes it the principle-of-indifference default.
2. **Closed-form queueing results.** M/M/1, M/G/1, M/M/c — the tractable queueing models start with M (Markov arrivals = Poisson). If you want Little's-Law back-of-envelope capacity estimates, Poisson lets you use published formulas.
3. **Burglar's friend**: many real arrival patterns *look* Poisson at the aggregate level over short enough windows.

## Why Poisson is often wrong

### Self-similarity / long-range dependence

Leland, Taqqu, Willinger, Wilson (1994) analysed Ethernet LAN traffic and found that it was *self-similar* across time scales from tens of ms to hours: the burstiness at 1 s looks like the burstiness at 10 s looks like the burstiness at 100 s. Poisson is *not* self-similar; aggregated Poisson over longer intervals looks smoother. Self-similar traffic is characterised by a Hurst parameter H ∈ (0.5, 1); Poisson has H = 0.5.

The practical consequence: a Poisson-driven load test underestimates queue depths, tail latencies, and buffer overflow risks on real traffic by factors of 2–100x depending on H.

Crovella & Bestavros (1996) extended this to web traffic. Park & Willinger's book *Self-Similar Network Traffic and Performance Evaluation* (2000) is the standard reference.

### Flash crowds / burst arrivals

A viral news event, a Super Bowl commercial, a flash sale — arrivals jump by 10x or more over seconds. No stationary Poisson process captures this. A non-stationary (time-varying λ) Poisson can capture a single event, but the *burst structure* is typically clumpier than Poisson even with time-varying rate.

### Batch arrivals

Real requests often arrive in bursts from a single client (a browser loading 50 subresources, a mobile app pulling sync data). These are not independent arrivals but correlated batches. Use MMPP (Markov-modulated Poisson process) or BMAP (batch Markov arrival process) if the batch structure matters.

### Closed-loop feedback

If the arrival rate depends on the completion rate (i.e., closed system; see Schroeder et al.), the arrival process is not Poisson at all — it is coupled to the system state. This is orthogonal to the open/closed distinction and is the primary reason not to use a Poisson-based analytical formula for a closed load test.

## Practical test generators and Poisson

- **k6**: `constant-arrival-rate` executor launches at configured rate; requests are *not* strictly Poisson but are close to constant-rate (deterministic inter-arrival). To get true Poisson, use the `ramping-arrival-rate` with a Poisson-distributed rate function or `xk6-chaos` plugins.
- **Gatling**: `openInjection` supports `atOnceUsers`, `rampUsers`, `constantUsersPerSec`, and `poissonPausing` for think times. Arrivals can be pulled from a Poisson process via custom injection.
- **httperf**: supports uniform and Poisson arrival processes natively.
- **wrk2**: implements a constant-rate (deterministic) arrival schedule, *not* Poisson, to enable coordinated-omission correction. This is a deliberate trade-off — Gil Tene argued that for latency measurement the scheduling regularity outweighs the "Poisson is more realistic" argument.
- **JMeter**: "Constant Throughput Timer" is constant-rate (deterministic); a Gaussian timer approximates a Normal distribution of delays. No native Poisson; needs a plugin.

## When to use Poisson and when to use something else

**Use Poisson when:**
- You need closed-form analytical results via M/M/1 etc.
- The aggregated traffic pattern is many independent sources each hitting at low rates (traffic aggregation theorem → approximately Poisson in aggregate).
- You are doing an initial feasibility test, before trace data is available.

**Use constant-rate deterministic when:**
- You need to correct for coordinated omission (wrk2's choice).
- You are stress-testing a specific throughput target and want minimal variance in the offered load.
- You want the simplest possible generator for CI regression tests.

**Use trace-based replay when:**
- You have real production traffic and can afford to capture and replay it.
- The trace will capture correlations, batches, self-similarity, and diurnal patterns that no analytical model reproduces.
- This is the gold standard for realism.

**Use self-similar generators (FARIMA, Pareto on-off) when:**
- You are modelling WAN / Internet traffic where self-similarity is established.
- You need to stress queueing behaviour at the tail.
- Standard tools: libraries like ns-3's self-similar source model; research prototypes around Pareto on-off.

## Anti-patterns

1. **"Load generator produces 1000 req/s" without specifying the distribution.** Uniform, Poisson, and constant-rate produce different queue behaviour on the same mean rate. Tool documentation should say which.
2. **Reporting Poisson results for flash-sale scenarios.** Flash sales are about burst and spike; a Poisson generator literally cannot simulate them.
3. **Assuming Poisson is "realistic" because real traffic "looks random".** Looking random at the mean level doesn't mean it's Poisson at the burst level. Self-similarity hides here.

## References

- Kleinrock, L. — *Queueing Systems, Volume 1: Theory*, Wiley 1975 — foundation for Poisson queueing analysis.
- Leland, W., Taqqu, M., Willinger, W., Wilson, D. — "On the self-similar nature of Ethernet traffic (extended version)" — IEEE/ACM Trans Networking 2(1), 1994.
- Park, K., Willinger, W. — *Self-Similar Network Traffic and Performance Evaluation*, Wiley 2000.
- Wolff, R. W. — "Poisson Arrivals See Time Averages" — Operations Research 30(2), 1982.
- Wikipedia — "Poisson point process" — [en.wikipedia.org/wiki/Poisson_point_process](https://en.wikipedia.org/wiki/Poisson_point_process) — fetched 2026-04-12.
- Self-similar traffic note — `01KNZ4VB6JSR9RJ0RTWXB9P6FV`.
- Open vs closed — `01KNZ4VB6JX0CQ5RFAZDJTQMCS`.
