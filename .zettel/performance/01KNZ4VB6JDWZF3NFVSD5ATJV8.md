---
id: 01KNZ4VB6JDWZF3NFVSD5ATJV8
title: "Common Workload Modeling Mistakes That Invalidate Results"
type: concept
tags: [workload-model, anti-patterns, validity, closed-loop, think-time, coordinated-omission, workflow]
links:
  - target: 01KNZ4VB6JX0CQ5RFAZDJTQMCS
    type: related
  - target: 01KNZ4VB6JB4Q5H3NPS72MZZ2A
    type: related
  - target: 01KNZ4VB6J3AB4QA4YZVDPMFWY
    type: related
  - target: 01KNZ4VB6JNFK2XFWP9N1HEJ5M
    type: related
  - target: 01KNZ4VB6J4TER1QCE9CKABBED
    type: related
  - target: 01KNZ4VB6JHJSARKD8E9XVGVRC
    type: related
  - target: 01KNWE2Q9YZBAR140ZX5P36TQ5
    type: related
created: 2026-04-12
modified: 2026-04-12
source: "synthesis from Schroeder et al. 2006, Tene 'How NOT to Measure Latency', Meier et al. 2007, and accumulated practice"
---

# Common Workload Modeling Mistakes

A catalogue of the recurring errors that make load-test results useless or actively misleading. Every item below corresponds to a real production failure pattern or to published literature that explains the bug in detail.

## 1. Closed-loop masking saturation

**Bug**: the load generator is a fixed pool of virtual users, each waiting for the previous response before issuing the next. When the SUT slows down, the generator slows down with it. Throughput "stabilises" at the system's capacity, and latency "looks fine" because the generator isn't offering more.

**Why it matters**: Schroeder, Wierman, Harchol-Balter NSDI 2006 Principles (i)–(iii) and Principle (v) (`01KNZ4VB6JX0CQ5RFAZDJTQMCS`). Closed-loop response times can be an order of magnitude better than the same load under an open model; scheduling policies look useless in closed that are critical in open. The load test reports everything is fine; production has a different arrival model and everything is not fine.

**Fix**: use an open or partly-open model. Choose based on session length of your real workload per Schroeder et al. Principle (vii): short sessions → open, long sessions → closed, in-between → partly-open. Default to open when in doubt.

## 2. Coordinated omission during generation

**Bug**: a closed-loop generator records latency as the time between send and receive. When the SUT stalls, no requests are sent, so no latency samples are recorded during the stall. The stall is invisible to the histogram.

**Why it matters**: see Gil Tene's talk (`01KNZ4VB6JB4Q5H3NPS72MZZ2A`). A real 100-second stall can appear in the histogram as a single 100 s sample amid 99 zero-latency samples — a median of 0 s when the true median was 50 s.

**Fix**: use an open-model generator, which cannot coordinate-omit by construction. Or use a closed-model generator that records intended vs actual time and back-fills synthetic samples via `recordValueWithExpectedInterval` (HdrHistogram).

## 3. Think-time zeroing

**Bug**: to produce high throughput from a closed virtual-user pool, the operator sets think time to 0. Users hammer endpoints at full speed with no pause.

**Why it matters**: Users don't hammer. Real workloads have inter-request pauses, cache effects during pauses, connection reuse, and TLS session lifetime that all differ from a zero-pause loop. The test exercises the server's ability to handle back-to-back requests on warm connections, not the realistic mix. Results over-report cache hit rates and connection reuse.

**Fix**: match think time to production traces, *or* switch to an open model where think time is replaced by session arrival rate.

## 4. Using offered rate when effective rate is different

**Bug**: the SUT is rate-limited to 1000 req/s. The generator offers 1200 req/s. Under admission control, 200 req/s are dropped. The test reports "throughput 1000, latency X". But applying Little's Law with λ = 1200 gives wrong numbers; with λ = 1000 gives right numbers.

**Why it matters**: measurements that confuse offered and effective rate cannot be reasoned about with queueing theory. SLO oracles are evaluated against wrong baselines.

**Fix**: track offered, accepted, and completed rates separately. Little's Law uses accepted (or completed in steady state). SLOs should be specified in terms of the rate at which the test was *expecting* to be served, not the rate at which requests happened to arrive.

## 5. Uniform inputs masking hot-key behaviour

**Bug**: the generator draws user IDs or keys uniformly at random. The real workload is heavily skewed (Zipf, power-law) — a small number of users generate most of the traffic.

**Why it matters**: uniform inputs produce uniform cache usage, uniform DB access, no hot spots. The test does not exercise hot-shard contention, hot-row lock contention, or cache invalidation stampede. Production does.

**Fix**: use empirical distributions from traces, or explicit skew (Zipf with exponent ~1.0 for the user-facing cases). Hot keys should be in the generator's input corpus at their real frequency.

## 6. Warm state in test, cold state in production (or vice versa)

**Bug**: the test runs in a loop for 10 minutes, warming caches, growing JIT profiles, warming connection pools. The first seconds of production after a deploy are cold, and production behaves differently. Or the reverse: test starts cold on every run, production has been running for weeks.

**Why it matters**: the state you measure is the state you test. State mismatch is a sub-bug of environment-parity mismatch (`01KNZ4VB6J22PTMXAYQ3V2WYAZ`).

**Fix**: be explicit about the state you're measuring. If you care about post-deploy behaviour, reset state at the start of each run. If you care about steady-state, warm up deliberately and exclude warm-up from the window.

## 7. Single endpoint as the workload

**Bug**: the load test targets `/health` or `/api/v1/ping` or a single trivial endpoint, and reports the SUT's capacity on that endpoint.

**Why it matters**: single-endpoint tests measure nothing about cross-endpoint contention, data-path contention, or the realistic mix. A system with 1000 req/s on /ping may fail at 200 req/s on the realistic mix.

**Fix**: realistic endpoint mix derived from production logs. Weight by count, not by endpoint importance.

## 8. Conflating concurrency with throughput

**Bug**: "the test achieved 1000 concurrent users" is reported as a measure of throughput. But Little's Law says throughput = concurrency / mean response time, so 1000 concurrent users with 500 ms mean response time is *2000 req/s throughput* — not 1000.

**Why it matters**: concurrency and throughput are related by response time; they are not interchangeable.

**Fix**: report throughput and concurrency separately, along with latency. Any two define the third via Little's Law.

## 9. Running too short to hit steady state or too long to stay stationary

**Bug**: a 2-minute test that includes 60 s of warm-up and 60 s of measurement measures mostly warm-up transients. A 24-hour test crosses thermal, GC, and fragmentation phases whose means drift.

**Why it matters**: measurements require stationarity. Too-short runs have no stationary window; too-long runs have multiple, and averaging across them is meaningless.

**Fix**: explicit warm-up phase, explicit steady-state window, explicit end. Soak-scale measurements are reported as trends, not means.

## 10. Ignoring background work

**Bug**: the test environment runs *only* the SUT and the load generator. Production has cron jobs, log processors, backup tasks, metric agents, garbage collectors running at unrelated schedules. They contend for CPU, memory, disk, network.

**Why it matters**: the test's 100 % CPU is available for the SUT; production's is 70 %, with the rest eaten by background. Results are non-transferable.

**Fix**: run background workloads in the test environment that match production's. Or explicitly document the "clean environment" assumption and know the gap.

## 11. Poisson where heavy-tailed is appropriate

**Bug**: the generator draws inter-arrival times from an exponential distribution. Real traffic is self-similar (`01KNZ4VB6JSR9RJ0RTWXB9P6FV`), and the tails of queueing response times are 10–100x worse under heavy-tailed arrivals.

**Fix**: use a Pareto ON-OFF generator, or replay real traces, or accept the limitation and flag it.

## 12. Reporting the mean when SLO is percentile

**Bug**: the SLO says "p99 < 400 ms"; the test report says "mean = 180 ms"; the report is interpreted as passing.

**Why it matters**: mean and p99 are different. A service with mean 180 ms and p99 of 800 ms is failing the SLO.

**Fix**: report the percentile the SLO targets. Percentile-aware tools (HdrHistogram) from the start.

## 13. No record of workload mix, arrival rate, or configuration

**Bug**: the test output is "latency = 150 ms", no record of how many users, which endpoints, which version of the code. Comparing across runs is impossible.

**Fix**: full metadata per run (workload mix, arrival model, rate, SUT version, environment, build SHA, operator, date). Checked into version control alongside results.

## 14. Reusing input data across runs

**Bug**: input data is loaded from a fixed file. Each run processes the same data in the same order. Caches warm on second-and-later runs. Results improve over repeated runs without any code change.

**Fix**: randomise inputs per run, or drop caches between runs, or accept the bias and know the gap.

## 15. Ignoring the warm-up phase in the result

**Bug**: a 10-minute load test includes 2 minutes of ramp-up. The overall latency reported is averaged over all 10 minutes, including the ramp-up transient.

**Fix**: explicit steady-state window. Measurements exclude ramp-up.

## 16. Trusting a single test run

**Bug**: run the test once, report the number, make a decision. No repetition, no CI, no check on variance.

**Why it matters**: every performance measurement is noisy. A single run is a single sample; its value is meaningful only within a noise envelope, and that envelope is invisible without replication.

**Fix**: at least 3 runs for any decision, more for tight decisions. Report mean, CI, and range.

## 17. Using a cloud instance without controlling for neighbour noise

**Bug**: running on a cloud VM that may share a physical host with other tenants. Tenant noise varies day-to-day, with a CoV of 10–20 %.

**Fix**: dedicated instances for perf runs (at 3–5x the cost), or very long runs that average out the noise, or explicit acceptance of the noise envelope.

## 18. Conflating load test and stress test

**Bug**: a test labelled "load test" that actually pushes the system to 5x design capacity and reports what happens. The report is interpreted as "this is how it behaves under load", when it's really "how it behaves at the breaking point".

**Fix**: separate tests for separate questions. Load tests at design load, stress tests past it.

## References

- Schroeder, Wierman, Harchol-Balter NSDI 2006 — `01KNZ4VB6JX0CQ5RFAZDJTQMCS`
- Tene "How NOT to Measure Latency" — `01KNZ4VB6JB4Q5H3NPS72MZZ2A`
- Meier et al. 2007 Microsoft p&p — especially Chapters 12 (Modeling Application Usage) and 14 (Test Execution).
- Barber, S. — "The Test Isn't the Enemy" — PerfTestPlus, 2008 — practitioner-oriented summary of the common mistakes.
