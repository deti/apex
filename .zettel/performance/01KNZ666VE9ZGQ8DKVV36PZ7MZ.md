---
id: 01KNZ666VE9ZGQ8DKVV36PZ7MZ
title: "Dean & Barroso, \"The Tail at Scale\" (CACM 2013) — why tail latency dominates at scale"
type: literature
tags: [paper, tail-at-scale, dean-barroso, cacm-2013, seminal, tail-latency, fan-out, hedged-requests, percentiles, distributed-systems]
links:
  - target: 01KNZ4VB6JMSSE4E40PBE23S3M
    type: related
  - target: 01KNZ666VRMB0N00T1E5GRPHT4
    type: related
  - target: 01KNZ666W240KABAHAYZP98C3T
    type: related
  - target: 01KNZ4VB6JB4Q5H3NPS72MZZ2A
    type: related
  - target: 01KNZ4VB6JPYBYW64S7NNYS1CM
    type: related
  - target: 01KNWE2Q9YZBAR140ZX5P36TQ5
    type: related
created: 2026-04-11T21:11:57.550241+00:00
modified: 2026-04-11T21:11:57.550243+00:00
---

Source: "The Tail at Scale", Jeffrey Dean and Luiz André Barroso. Communications of the ACM, Vol. 56 No. 2 (February 2013), pp. 74-80. https://research.google/pubs/the-tail-at-scale/ — fetched 2026-04-12. The paper is also widely available at https://web.archive.org/web/ and at the CACM mirror.

This is the single most important paper for understanding why performance testing of distributed systems must focus on latency distributions rather than averages. It is the foundational statement of the "tail latency" problem in large-scale services and is mandatory reading for anyone building, operating, or testing high-scale distributed software.

## The core problem statement

In large-scale online services, many individual components respond to each user request — frontends fan out to search clusters, cache tiers, backends, storage layers. A single user-visible request may touch hundreds or thousands of component servers before a response is returned. In such **fan-out architectures**, the latency a user experiences is the slowest response among all touched components, not the average.

The result is mathematical: even if every component is fast *on average*, a small fraction of slow responses (the tail of the latency distribution) **dominate** the user-visible latency as fan-out increases. Dean and Barroso work the numbers: if a single service responds in 10ms on average but has a 1% probability of responding in 1 second, then a fan-out to 100 servers means the user request almost certainly hits at least one slow server and the user-visible latency is dominated by that one-in-a-hundred event. **A component at the 99th percentile becomes the median when requests fan out across 100 servers.**

Quote from the paper: *"Systems that respond to user actions very quickly (within 100 milliseconds) feel more fluid and natural to users than those that take longer."* And the core insight: latency perception at user scale depends on the *tail*, not the average.

## Sources of latency variability

The paper catalogues the reasons any individual component occasionally responds slowly, even when its code is correct and its hardware is healthy:

- **Shared resources contention** — two processes on one machine competing for CPU cache, memory bandwidth, or network bandwidth.
- **Daemon activity** — background maintenance daemons (cron jobs, log rotators, health-checkers) consuming resources unpredictably.
- **Garbage collection pauses** — JVM, Go runtime, and other managed runtimes occasionally pause for GC; the pause duration is bimodal.
- **Cache misses** — cold data paths occasionally trigger a disk read or remote fetch that takes 100-1000x longer than cache hits.
- **Queueing** — transient bursts overflow a service's work queue; requests wait.
- **Network switch buffer overflows, reboots, thermal throttling** — hardware-level disturbances that affect individual machines briefly.
- **Global synchronization events** — compactions in LSM-tree storage engines, memory balancer migrations, log checkpoint flushes.

The takeaway: variability is not a bug to be fixed. It is an emergent property of running at scale on commodity hardware with shared resources, and the right engineering posture is to **design around it** — the way fault-tolerance designs around unreliable components.

## Tail-tolerant techniques

The paper's second half proposes techniques for building predictably-fast services out of less-predictably-fast components. These techniques define an entire subfield of distributed systems performance engineering:

### Hedged requests

Send a request to one server. If a response doesn't arrive within a short delay (e.g. the 95th-percentile expected latency), send a second request to a different server. Accept whichever response arrives first. The cost is ~5% extra traffic (for the small fraction of requests that fire the hedge); the benefit is that the long tail is truncated to approximately the p95 of the underlying service.

### Tied requests

Send requests to two (or more) servers simultaneously, but give each server the identifier of the other. As soon as one server picks up the work, it tells the other to cancel. This reduces the extra work to the cost of scheduling decisions rather than full duplicate execution. Tied requests shift the distribution's tail dramatically with minimal extra work.

### Micro-partitions

Split data into many small partitions (more partitions than machines), and reassign partitions dynamically as load changes. When a machine slows down, its partitions migrate to healthy machines in seconds, not minutes. This is the ancestor of modern service-mesh load shedding and of dynamic resharding in systems like Bigtable.

### Selective replication

Keep extra copies of the most-accessed data. When a hot partition is experiencing tail latency, reads can be served from the replicas. The cost scales with the degree of replication; the benefit is that hot partitions are never single-points of slowness.

### Latency-induced probation

Monitor each backend server's latency distribution. When a server's responses are consistently in the tail, temporarily remove it from the load-balancing pool. Health checks run continuously; the server is returned to the pool when its latency recovers.

## The key reframing

The paper's most influential contribution is a conceptual reframe:

> "Large online services need to create a predictably responsive whole out of less predictable parts."

This is identical in spirit to the fault-tolerance reframe of the 1980s and 1990s — build reliable systems from unreliable components. Dean & Barroso argue for the same mental model applied to latency: build predictable latency from unpredictable latency, using redundancy and active mitigation rather than attempting to make individual components faster.

## Implications for performance testing

The practical consequences for any performance testing practice — and directly for APEX G-46:

1. **Mean latency is actively misleading.** A performance test that reports only averages will miss every single effect described in the paper. Percentiles are mandatory; p99, p999, and p9999 are the ones that matter at scale.
2. **Constant-arrival-rate load models are required.** Closed-loop load generators (wrk, Siege, hey, closed-mode JMeter) under-report the tail via coordinated omission — they slow down in sync with the server they are testing. See wrk/wrk2 notes for the canonical demonstration. The "Tail at Scale" problem and the coordinated-omission problem are the same problem viewed from two angles.
3. **Single-server performance tests are not enough.** If your production architecture fans out to N backends, testing a single backend in isolation gives results that do not predict the user-visible latency. Some form of integration testing with realistic fan-out is necessary.
4. **Latency variability is the target, not just latency.** A service with mean 10ms and p99 10ms is categorically better than one with mean 8ms and p99 500ms. Minimising the variance (or equivalently: minimising the tail) is a distinct engineering goal from minimising the mean.
5. **Resource-guided fuzzing should target the tail.** APEX G-46's "worst-case input generation" is precisely the search for inputs that drag the latency distribution to the right. PerfFuzz and SlowFuzz are reaching for the same goal Dean & Barroso identify: find the inputs and conditions that push a component into its tail, because those will dominate the user experience once the component is deployed in a fan-out service.

## Cross-linking

This note is the primary reference for:
- Any discussion of percentile reporting versus average reporting in the performance-testing comparative notes.
- The coordinated-omission note pair (wrk / wrk2).
- The continuous-profiling notes (Pyroscope, Parca, GWP) — continuous profiling exists because the conditions that produce tail latency are not reproducible on demand in a test environment, so you must observe them in production.
- The SLO notes (Google SRE, `implementing-slos`) — SLOs are typically phrased in percentile terms precisely because of the tail-at-scale argument.

## One-line summary

If you read only one paper about performance in distributed systems, read this one. Dean and Barroso wrote the textbook description of why tail latency dominates user experience, why averages lie, and how to build predictable latency from unpredictable components. Every tail-latency technique in current use — from Envoy's outlier detection to Istio's retry-with-timeout to hedged requests in gRPC client libraries — traces directly to this paper.