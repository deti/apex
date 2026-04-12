---
id: 01KNZ4VB6JY3QDARVD4N06HR6X
title: "Scalability and Capacity Testing"
type: concept
tags: [scalability-testing, capacity-testing, taxonomy, performance-testing, horizontal-scale, vertical-scale, meier-2007]
links:
  - target: 01KNZ4VB6JY38THW04Z3MMGBZ3
    type: related
  - target: 01KNZ4VB6JTC1Z9CGYN4Q1CCA6
    type: related
  - target: 01KNZ4VB6J4TER1QCE9CKABBED
    type: related
  - target: 01KNZ4VB6JVSPDK724EZFPA36H
    type: references
  - target: 01KNZ4VB6J6R3V3GVBWSAKW8JC
    type: related
  - target: 01KNWE2Q9YZBAR140ZX5P36TQ5
    type: related
created: 2026-04-12
modified: 2026-04-12
source: "Meier, Farre, Bansode, Barber, Rea — Performance Testing Guidance for Web Applications — Microsoft p&p, 2007, Chapter 2"
---

# Scalability and Capacity Testing

Scalability testing and capacity testing are two related but distinct test types that share the question "how does the system change as it grows". They're grouped here because they are easily confused and are the two most common types to be done badly.

## Capacity testing — "how many?"

**Question answered:** *"At the current configuration, how many users / sessions / transactions can the system support while still meeting the SLO?"*

Meier et al. 2007, ch. 2:

> *Capacity test — To determine how many users and/or transactions a given system will support and still meet performance goals. Capacity testing is conducted in conjunction with capacity planning, which you use to plan for future growth, such as an increased user base or increased volume of data. ... Capacity testing helps you to identify a scaling strategy in order to determine whether you should scale up or scale out.*

Capacity is a *scalar* produced by the test: "the system supports 12 500 req/s at p99 < 300 ms on this hardware". The output feeds into capacity planning ("we need 8 of these for 100 k req/s, plus 50 % headroom = 12 instances").

A capacity test is performed *on a fixed configuration* and asks what load it can sustain. It is close kin to load testing but with the load axis as the output, not the input.

## Scalability testing — "how does the system change as we scale it?"

**Question answered:** *"Does doubling the hardware double the capacity? Does adding a shard reduce per-shard latency?"*

Scalability testing is a *sequence* of capacity tests at different system sizes, plotted to reveal the relationship between resource investment and capacity. The output is not a scalar but a curve.

A scalable system's curve is close to linear (or at least sub-linear in cost-per-unit-capacity). A non-scalable system's curve flattens: each added node buys less capacity than the last.

The reasons capacity often grows sub-linearly:

1. **Coordination overhead.** More nodes means more heartbeats, more state synchronisation, more leader elections. Overhead grows in some systems as O(N), in others as O(N²).
2. **Shared bottleneck.** A central service (session store, leader, admission controller) that all nodes talk to. Adding worker nodes pushes more traffic at the central bottleneck without increasing its capacity.
3. **Hot partitions.** Sharding divides load unevenly. The hot shard saturates while others are idle. Capacity is limited by the hottest shard, not the mean.
4. **Network saturation.** At scale, cross-node traffic exceeds NIC capacity before compute saturates.
5. **Amdahl's Law.** Any serial fraction of work caps speedup. A 5 % serial fraction limits speedup to 20x regardless of cluster size.

## Amdahl's and Universal Scalability

Two canonical models for scalability curves:

**Amdahl's Law** (Gene Amdahl, 1967): speedup(N) = 1 / (s + (1-s)/N), where s is the serial fraction. Asymptotic speedup is 1/s. At s = 0.05, max speedup is 20. Amdahl assumes additional nodes cost nothing and contribute cleanly.

**Universal Scalability Law** (Neil Gunther, 1993): C(N) = N / (1 + α(N-1) + βN(N-1)), where α models contention (serial fraction à la Amdahl) and β models coherency / coordination cost. At β > 0 the curve not only flattens but *decreases* past a peak — adding nodes makes the system slower. USL explains the observed "more hardware makes it worse" behaviour that Amdahl can't capture. USL fits measured data well for most real distributed systems and is the standard model for scalability-test analysis.

A scalability test's primary output is the fit coefficients (α, β) for USL. Those coefficients predict the capacity at sizes you can't afford to test directly, and they identify whether the system is contention-limited (reduce serial fraction) or coordination-limited (reduce cross-node talk).

## Scale-up vs scale-out

Meier et al. note that capacity testing informs whether to scale up (bigger nodes, vertical) or scale out (more nodes, horizontal). These have different breaking points:

- **Scale-up**: limited by single-node resources (RAM, NUMA topology, single-socket NIC bandwidth). Good up to the biggest commodity machine; falls apart past it. Simple; no distributed-systems bugs.
- **Scale-out**: limited by coordination overhead (USL β). Good if the workload shards cleanly; falls apart if there's a shared bottleneck. More engineering effort but effectively unbounded if done well.

A scalability test that varies only node count tells you about scale-out. A scalability test that varies node size tells you about scale-up. A complete scalability study varies both.

## Fit in the SDLC

- **Architecture design** — predict capacity at target scale from the USL model fit on small-scale tests.
- **Pre-release** — capacity test to set expected per-instance throughput that autoscalers and dashboards will use.
- **Continuous** — capacity regression gating in CI (see regression-gating note). A PR that reduces per-instance capacity by > X % fails.
- **Post-release** — periodic re-capacity testing as the application evolves. What was 1000 req/s per instance at v1.0 might be 600 req/s at v2.0 with three new features.

## Anti-patterns

1. **Single-size scalability test.** Running one test at 4 nodes and extrapolating linearly. Amdahl and USL both exist because linear extrapolation is usually wrong. Fix: test at ≥ 4 sizes (1, 2, 4, 8 or 1, 3, 9, 27 nodes) and fit USL.

2. **Capacity expressed as a single number.** "Capacity is 10 k req/s" without saying which endpoint mix, which SLO threshold, which environment. Fix: report capacity as a tuple (mix, SLO, environment, value).

3. **"Scaled" but shared state bottleneck.** Adding web-tier nodes while the database is unchanged. The database is still the single point of capacity. Fix: scale the whole stack or measure per-tier capacity independently.

4. **Using averages for capacity thresholds.** "The p99 is still < 300 ms at 12 k req/s". But the capacity curve is not monotonic at the tail; p99 may be fine at 12 k and terrible at 13 k. Fix: find the breaking point, not just the current operating point.

5. **Ignoring data volume.** Capacity at 10 GB of data differs dramatically from capacity at 10 TB. Cold-path operations that fit in RAM at small scale go to disk at large scale. Fix: data volume must be production-parity (see environment parity note).

## Relationship to other test types

- **Capacity testing is a focused load test** with load as the output.
- **Scalability testing is a sequence of capacity tests** with infrastructure size as the independent variable.
- **Stress testing** finds the breaking point; capacity testing finds the *SLO* point (which is below the breaking point).
- **Little's Law** (`01KNZ4VB6J4TER1QCE9CKABBED`) is the skeletal formula for capacity prediction before you test: predict concurrency from target throughput × latency.

## References

- Meier et al. — Microsoft p&p Performance Testing Guidance, Ch. 2.
- Gunther, N. — *Guerrilla Capacity Planning*, Springer 2007, and the USL model — [perfdynamics.blogspot.com](http://perfdynamics.blogspot.com/)
- Amdahl, G. — "Validity of the Single Processor Approach to Achieving Large-Scale Computing Capabilities" — AFIPS 1967.
- Jain, R. — *The Art of Computer Systems Performance Analysis*, Ch. 33 Capacity Planning, Wiley 1991.
