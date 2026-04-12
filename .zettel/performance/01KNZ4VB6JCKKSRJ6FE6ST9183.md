---
id: 01KNZ4VB6JCKKSRJ6FE6ST9183
title: "Spike Testing — Sudden, Short-Duration Overloads"
type: concept
tags: [spike-testing, taxonomy, performance-testing, thundering-herd, meier-2007]
links:
  - target: 01KNZ4VB6JTC1Z9CGYN4Q1CCA6
    type: extends
  - target: 01KNZ4VB6JY38THW04Z3MMGBZ3
    type: related
  - target: 01KNZ4VB6JVSPDK724EZFPA36H
    type: references
  - target: 01KNWE2Q9YZBAR140ZX5P36TQ5
    type: related
created: 2026-04-12
modified: 2026-04-12
source: "Meier, Farre, Bansode, Barber, Rea — Performance Testing Guidance for Web Applications — Microsoft p&p, 2007, Chapter 2"
---

# Spike Testing — Sudden, Short-Duration Overloads

## Question answered

*"What happens when traffic abruptly jumps to N× normal — during a flash sale, a breaking news event, a Super Bowl ad, a viral social-media post — and then falls back?"*

Spike testing is a specialised form of stress testing (Meier et al. 2007 call it a subset) focused on the *arrival-rate discontinuity* rather than sustained overload. The interesting phenomena are transient: queue buildup, cache stampede, thread-pool exhaustion, connection-pool depletion, autoscaler lag, downstream dependency contention.

## Canonical definition

Meier et al. 2007, ch. 2:

> *Spike testing is a subset of stress testing. A spike test is a type of performance test focused on determining or validating the performance characteristics of the product under test when subjected to workload models and load volumes that repeatedly increase beyond anticipated production operations for short periods of time.*

Key phrase: *"repeatedly increase ... for short periods"*. A spike test is not one overload, it is *repeated* overloads, because the interesting failure modes involve the *recovery* between spikes as much as the spike itself.

## Why spike testing is its own thing

A sustained stress test and a spike test can look identical on a rate-vs-time graph at first glance, but they target different bugs:

- **Sustained stress** exposes *steady-state* saturation — queue depth grows until a resource maxes out, errors emerge.
- **Spike testing** exposes *transient* bugs that only occur during rapid rate changes:
  - **Cache stampede.** A warm cache keeps 10 k req/s at 1 ms each. Cache invalidates. Suddenly 10 k requests in flight see a 50 ms cache miss each. All 10 k go to the database at once. DB falls over. Load is not unusually high; the temporal concentration is what kills.
  - **Thundering herd on a lock / leader election.** N clients wake simultaneously and race for a lock. Contention is N² in the worst case. At low steady rate, no contention; at a spike, lock becomes the bottleneck.
  - **Autoscaler lag.** Designed to handle 2 k req/s at 10 instances. Spike to 10 k req/s. Autoscaler takes 2 minutes to provision 40 more instances. In the interim, the existing 10 instances are 5x overloaded and shed or crash. When new instances arrive, they cache-miss, adding another mini-spike.
  - **Connection-pool initialisation.** Spike triggers a process to open 200 new DB connections at once. Each takes 100 ms. For 20 seconds, half the requests hit connection-wait timeouts.
  - **Queue-based throttling that doesn't recover.** Queue grows during spike. Queue policy (FIFO) dequeues oldest first, all of which have client-timed-out. Queue drains into dead work.

## Historical motivation

Spike behaviour is the mechanism behind many high-profile outages:

- **Slashdot effect / Hug of Death.** Early 2000s. A front-page link drives a 100–1000x traffic multiplier over the span of seconds.
- **Black Friday / Cyber Monday.** Retail traffic peaks 10–50x during short windows, with the sharpest rise in the first 30 minutes.
- **Pokémon GO launch, 2016.** Traffic exceeded capacity estimates by 50x during the first week; server availability collapsed.
- **Healthcare.gov launch, 2013.** Not strictly a spike, but the traffic profile had the same shape: announced go-live, millions of simultaneous visitors, system designed for orders of magnitude less.
- **Ticketmaster Taylor Swift sale, 2022.** Scheduled high-cardinality spike with global queueing.
- **Super Bowl commercial effect.** A 30-second ad drives millions of simultaneous visits. Measured in the 1000x-over-baseline range.

## Profiles

Typical spike profile shapes:

- **Square wave.** Steady at 1x, step to 10x for 60 s, step back to 1x. Repeat every 5 minutes. Targets thundering herd and recovery.
- **Sawtooth.** Linear ramp from 1x to 10x over 30 s, step back to 1x. Targets autoscaler lag.
- **Poisson with temporary rate change.** Mostly steady but with rate multiplier 10 applied during short windows. More realistic model of viral-content-driven spikes.
- **Cold spike.** Spike begins after a quiet period during which caches decay. Worst-case stampede trigger.

## Anti-patterns

1. **Spike testing with warm caches.** If the test environment was at 10 k req/s for 10 minutes before the spike, every cache is hot. Real spikes happen after quiet periods (overnight, post-deploy). Fix: drain caches before the spike.

2. **Single spike.** The important signal is *repeatability* and recovery. One spike doesn't reveal whether the system re-cools to normal before the next one arrives. Fix: repeated spikes with varying inter-spike intervals.

3. **Ignoring upstream dependencies.** Spike tests on service A reveal A's behaviour but miss that A cascades load into B, C, D during the spike. Fix: instrument and observe the whole call graph.

4. **No autoscaler in the test environment.** Production has autoscaling; staging does not. The spike test measures "how does the fixed-size staging cluster handle 10x load", not "how does the autoscaler respond". Fix: run autoscaler in staging with matching policies.

5. **Using the breaking-point load for the spike.** If normal load is 1 k and design capacity is 10 k, spiking to 10 k tests "can the system handle peak", not "can it handle sudden transition". Fix: spike to N× *normal*, and vary N to find the spike-specific breaking point (which is typically lower than the steady breaking point).

## Useful to pair with

- **Cache stampede / dogpile mitigation tests** (single-flight, probabilistic early expiration). Spike testing is the easiest way to exercise them.
- **Circuit breakers.** A spike should trigger circuit-breaker opening; the test verifies that the breaker opens at the right threshold and closes at the right time.
- **Shed/throttle admission control.** Load shedding is meant for the *first few seconds* of a spike; a spike test is the right exercise.
- **Autoscaling** end-to-end verification.

## Concrete acceptance criteria

- At 10x normal load for 60 s, error rate stays below X %.
- Recovery to baseline latency within Y seconds of spike end.
- No cascading failures — backends B, C, D all report errors < Z % during and after the spike.
- Autoscaler provisions ≥ N additional instances within M seconds.
- No data inconsistency introduced during the spike.

## Relationship to other test types

- **Subset of stress testing** (per Meier et al.). The distinction: stress is about sustained overload, spike is about transient overload.
- **Inverse of soak.** Spike = short duration × high load. Soak = long duration × normal load.
- **Complement to load testing.** Load test shows design-load behaviour; spike test shows above-design-load behaviour specifically under transient conditions.

## References

- Meier et al. — Microsoft p&p Performance Testing Guidance, Ch. 2 — [learn.microsoft.com/previous-versions/msp-n-p/bb924357(v=pandp.10)](https://learn.microsoft.com/en-us/previous-versions/msp-n-p/bb924357(v=pandp.10))
- Nygard, M. — *Release It!*, 2nd ed., 2018 — chapters on stability patterns cover circuit breakers, bulkheads, and load shedding that spike tests exercise.
- Google SRE book — "Handling Overload" chapter — [sre.google/sre-book/handling-overload](https://sre.google/sre-book/handling-overload/)
