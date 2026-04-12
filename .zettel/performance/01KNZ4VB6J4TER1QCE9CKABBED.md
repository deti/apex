---
id: 01KNZ4VB6J4TER1QCE9CKABBED
title: "Little's Law (L = λW) — Statement, Proof, Uses, Misuses"
type: concept
tags: [littles-law, queueing, workload-model, capacity-planning, operations-research, foundation]
links:
  - target: 01KNZ4VB6JX0CQ5RFAZDJTQMCS
    type: related
  - target: 01KNWE2QA5VP0K80TMSABACKWT
    type: extends
  - target: 01KNWE2QACWYZJXRJ8QE78T043
    type: related
  - target: 01KNWE2Q9YZBAR140ZX5P36TQ5
    type: references
  - target: 01KNZ4VB6J6R3V3GVBWSAKW8JC
    type: related
  - target: 01KNZ6WHXWQ1VKJNBA1FYAS8PM
    type: related
  - target: 01KNZ4VB6JDWZF3NFVSD5ATJV8
    type: related
created: 2026-04-12
modified: 2026-04-12
source: "Little, J. D. C. (1961). A Proof for the Queuing Formula: L = λW. Operations Research 9(3): 383–387."
---

# Little's Law (L = λW)

## Statement

For any queueing system that is **stationary** (statistical properties don't drift with time) and **ergodic**:

**L = λW**

where
- **L** is the long-run average number of items (customers, jobs, requests, packets, in-flight RPCs, items of work-in-process) present in the system;
- **λ** is the long-run average effective arrival rate into the system;
- **W** is the long-run average time an item spends in the system (sojourn time).

That is the entire law. No other assumptions: no distribution assumption on arrivals, no distribution assumption on service times, no assumption about scheduling discipline, no assumption about the number of servers, no assumption about FIFO. It holds for M/M/1, G/G/k, priority queues, systems with reneging, systems with feedback, layered systems, entire data centers, entire factories, entire hospitals.

## History

- 1958: **Philip M. Morse**, *Queues, Inventories and Maintenance*, writes down L = λW and challenges readers to find a case where it fails. None exists, but no proof is given.
- 1961: **John D. C. Little**, "A Proof for the Queuing Formula: L = λW", *Operations Research* 9(3), 383–387 — first general proof.
- 1967: **Jewell** gives an alternative proof.
- 1972: **Shaler Stidham** gives the modern, simplest proof via sample-path arguments — the one usually taught today. Stidham's paper shows the law holds "almost surely" under the sample-path definitions of L, λ, W (no need for an underlying stochastic model).

(Source: Wikipedia, *Little's law* — fetched 2026-04-12.)

## Intuition (Stidham-style sample-path argument)

Consider a long interval of time [0, T]. Draw the cumulative-arrivals function A(t) and the cumulative-departures function D(t). The instantaneous number in system is N(t) = A(t) – D(t). The time-average number in system is ∫₀ᵀ N(t) dt / T. By Fubini's theorem, that integral equals the total "customer-seconds" spent in the system, which equals Σ(time spent by customer i) summed over customers who passed through the system during [0, T]. That sum, divided by T, is (number of customers) × (average sojourn time) / T = λW. Hence L = λW. No distributional assumptions needed; only that the system doesn't run off to infinity (stability).

## Applications in performance testing

**1. Throughput from queue depth and latency.** If a server has average 100 in-flight requests (L) and mean response time 50 ms (W = 0.05 s), its effective throughput is λ = L / W = 2000 req/s. Conversely, mean response time can be *inferred* from throughput and queue depth without measuring it directly — a useful sanity check on a load test.

**2. Capacity planning for a microservice.** You want to support 10 000 req/s at p50 = 20 ms. Little's Law tells you you need L ≈ 200 in-flight requests on average. If your thread pool has 100 threads, you are *already* saturated — you'd have to either increase pool size, go non-blocking, or accept additional queueing latency. This single computation catches "number of threads is the bottleneck" failures before they happen.

**3. Consistency check on load-test output.** A load test reports λ = 5000 req/s, mean response time W = 100 ms, and total average concurrency L = 200. By Little, L should equal 500. The 300-concurrency discrepancy means *something is wrong with the measurement*. Common causes: coordinated omission dropping the tail, concurrency measured at the wrong point, request/response counting mismatch. Meier et al.'s P&P guide explicitly calls this out as a standard validation technique.

**4. Queueing network models.** Little's Law can be applied to any subsystem: the entire data center (E[in-flight queries] = λ × E[end-to-end latency]), a single service (E[local concurrency] = local_λ × local_W), a disk I/O queue (E[queue depth] = IOPS × average I/O latency). The same equation, just different scopes.

**5. Derivation of other queueing results.** Little's Law is the trunk from which PASTA, Erlang-C, M/M/1 response-time formulas, and the operational laws of Jain's textbook all hang as branches.

## What Little's Law does *not* say

This is where the footguns live.

- **It says nothing about tails.** L = λW is a statement about *means*. Two systems with identical L, λ, W can have wildly different p99 latencies. Little's Law cannot substitute for measuring percentiles. It is *complementary* to HdrHistogram-style latency capture, not a replacement.
- **It requires stationarity.** During a ramp-up, during a spike, or across a phase change, L ≠ λW even in an ideal system because the "long-run average" doesn't converge. Applying Little's Law across the ramp-up of a load test gives nonsense. You must wait until the system reaches steady state. See JMH's warm-up note for how this is operationalized in microbenchmarks.
- **λ is the *effective* arrival rate, not the *offered* rate.** If the system drops 10% of offered load due to admission control or timeouts, λ in Little's Law is 0.9 × (offered). Using offered rate gives wrong answers for overloaded systems.
- **"In the system" must be defined consistently across L and W.** If L counts both queued and in-service requests, then W must be queue-time + service-time, not just service-time. Mixing scopes is a classic bug.
- **It does not assume Poisson arrivals.** A common misstatement is "Little's Law assumes Poisson". It does not. Confusing it with PASTA (Poisson Arrivals See Time Averages) is the usual origin of this myth.

## Common misuses in practice

1. **Ramp-up averaging.** Running a 5-minute load test, computing mean(N) and mean(W) over the entire run including the ramp, and then "verifying Little's Law". The system was non-stationary; the check is meaningless.
2. **Applying to a single event.** Little's Law is a long-run average. "There are 50 in-flight requests right now and mean response time is 20 ms, so throughput is 2500 req/s *right now*" is wrong — those are instantaneous values, not long-run averages.
3. **Conflating closed-system "users" with "in-system items".** In a closed load test with MPL = 100, N thinks-or-in-system = 100, not N in-system. When applying Little's Law to a closed-loop generator, the denominator includes think time.
4. **Using arrival rate before a bottleneck.** A client generates 10 000 req/s but a router drops 20%. Little's Law for the *server* uses λ = 8000, not 10 000.

## Why Little's Law is underappreciated in load-testing practice

Practitioners see "L = λW" and think "trivial algebra, what could be the fuss". The fuss is:

1. It is the only sanity-check equation that binds the three core load-test outputs — concurrency, throughput, latency — together. If your tool reports all three and they violate Little's Law, *at least one is wrong* and you must find out which before trusting any of them.
2. It is the bridge between "we can generate N virtual users" (closed model) and "we want to simulate λ arrivals per second" (open model). In a closed model, λ is an output, λ = MPL / (W + Z) where Z is think time. That formula is just Little's Law applied to the whole "think + system" box.
3. It is the foundation of every operational law in Jain's *The Art of Computer Systems Performance Analysis*, including the bottleneck bound, the interactive response-time law, and the asymptotic bound.

## Adversarial reading

Little's Law is famously distribution-free, but stationarity is doing a lot of work. In a production system, true stationarity is rare: traffic has diurnal patterns, traffic spikes, failover events. Practitioners treat 10-minute windows as approximately stationary, which is a useful-but-not-rigorous hack. For very spiky workloads (flash-sale, social-media-driven), even 1-minute windows may not be stationary, and Little's Law "fails" in the sense that its inputs don't converge.

## References

- Little, J. D. C. (1961). "A Proof for the Queuing Formula: L = λW". *Operations Research* 9(3): 383–387. [DOI 10.1287/opre.9.3.383](https://doi.org/10.1287/opre.9.3.383)
- Stidham, S. (1974). "A Last Word on L = λW". *Operations Research* 22(2): 417–421. — simpler sample-path proof.
- Jain, R. (1991). *The Art of Computer Systems Performance Analysis*. Wiley. — practical performance-engineering framing, chapter on operational laws.
- Little, J. D. C. (2011). "Little's Law as Viewed on Its 50th Anniversary". *Operations Research* 59(3): 536–549. — retrospective.
- Wikipedia, "Little's law" — [en.wikipedia.org/wiki/Little%27s_law](https://en.wikipedia.org/wiki/Little%27s_law) — fetched 2026-04-12.
