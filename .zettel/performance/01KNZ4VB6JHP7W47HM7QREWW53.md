---
id: 01KNZ4VB6JHP7W47HM7QREWW53
title: "USE Method — Utilisation, Saturation, Errors (Brendan Gregg)"
type: literature
tags: [use-method, brendan-gregg, methodology, resource-analysis, checklist, saturation, utilisation]
links:
  - target: 01KNZ4VB6J08D14Y8P3RWVAABA
    type: extends
  - target: 01KNZ4VB6J6ED6F3YHN1SMDNQ5
    type: related
  - target: 01KNZ4VB6JXAZA2TBRCD5DERK9
    type: related
  - target: 01KNZ5YREWKYWDWQ2MN39KHN5K
    type: related
  - target: 01KNZ666W240KABAHAYZP98C3T
    type: related
  - target: 01KNWE2Q9YZBAR140ZX5P36TQ5
    type: related
created: 2026-04-12
modified: 2026-04-12
source: "https://www.brendangregg.com/usemethod.html"
---

# The USE Method

*Source: https://www.brendangregg.com/usemethod.html — fetched 2026-04-12.*
*Reference text: Gregg, B., "Systems Performance" 2nd ed., §2.5.9.*

## The rule

For every resource, check three metrics, in this order:

- **U — Utilisation**: average fraction of time the resource was busy.
- **S — Saturation**: the degree to which the resource has extra work it can't service, usually queued.
- **E — Errors**: count of error events.

That is it. One sentence. Gregg claims the method "solves about 80% of server issues with 5% of the effort" on typical production systems — the 80/20 observation that makes it the default starting place for diagnosis.

## Why these three, in this order

1. **Errors first** because they are usually faster and easier to interpret. An error spike has a clear cause, and clearing it often resolves the real problem without needing to dig into utilisation/saturation.

2. **Saturation** is the second-cheapest read and captures queue depth — the direct signal of "this resource has more work than it can handle right now". Saturation is often more useful than utilisation because a resource can be 100 % utilised and not saturated (perfectly loaded) or 70 % utilised and saturated (bursty).

3. **Utilisation** is last because it is often misleading on its own. 70 % CPU utilisation is fine; 70 % NIC utilisation means retransmits are starting. The interpretation is resource-specific. But utilisation is the familiar metric most tools report by default, so pairing it with saturation and errors prevents the streetlight anti-method.

## The checklist process

1. **Inventory resources.** Before any measurement, write down every physical and virtual resource. Typical list:
   - **CPUs** — per core and aggregate.
   - **Memory** — physical RAM, swap.
   - **Storage** — block devices, each disk, filesystem.
   - **Network interfaces** — each NIC.
   - **I/O controllers** — HBAs, DMA controllers.
   - **Interconnects** — NUMA links, PCIe.
   - **Software resources** — kernel mutexes, process/thread limits, file descriptors, port ranges, connection pools, thread pools.

2. **For each resource, identify how to measure U, S, E.** Gregg publishes a Linux checklist (usemethodlinux.html) mapping each resource to specific `/proc`, `sar`, `iostat`, `vmstat` commands.

3. **Walk the list.** Run each command; note anomalies.

4. **Anomalies direct further investigation.** USE does not solve the problem; it points at the resource to drill into. Drill-Down Analysis or Latency Analysis takes over from there.

## Example: single-host web server slowdown

- CPU: util 40 %, sat 0 (no runqueue), errors 0 → not CPU.
- Memory: util 90 %, swap pages 120/s, errors 0 → saturated; dig here.
- Disk: util 30 %, sat 0 (iowait low), errors 0 → not disk.
- Network: util 5 %, drops 0 → not network.
- Thread pool: used 45/50, wait queue 0 → not thread pool.
- DB connection pool: used 20/20, wait queue 8 → saturated; dig here too.

Two anomalies: memory pressure causing swap, and DB pool saturation. The two may be related (memory pressure evicting DB query cache, making queries slower, keeping connections busy longer). USE doesn't answer the "why" but it narrows to the two resources to investigate.

## What USE catches that tool-surfing misses

The key contribution of USE is that it *enumerates the question list before you pick tools*. The Tools Method starts from available tools and examines their output; if the problem is in a resource your tools don't see, you never find it. USE starts from the complete resource inventory and asks "what would tell me U/S/E for this?". If no tool exists, you know you have a blind spot — which is itself actionable information (build a tool, add tracing, accept the risk).

Example: a system admin with `top`, `iostat`, `vmstat` may miss a kernel-mutex contention problem because their tools don't show kernel locks. USE prompts them to ask "U/S/E for kernel mutexes?", notice their tools don't answer, and reach for `lockstat` or bpftrace.

## What USE is bad at

Gregg explicitly notes USE's limitations. It is a *tool*, part of a larger toolbox, not a universal solvent.

1. **Bad at problems caused by the *absence* of a resource rather than saturation of one.** e.g. a memory allocator that is fast but gives up locality → cache misses → application-visible slowdown. CPU util is low, no saturation, no errors; USE says "nothing wrong". The problem is latency, not throughput.

2. **Bad at distributed problems.** USE is per-host. A cross-host coordination bug (lock wait on a remote node) doesn't surface as local saturation on either side.

3. **Bad at design-level problems.** "This algorithm is O(n²)" doesn't show up as resource saturation if the workload is small; it shows up as growth over time. USE finds the symptom, not the design flaw.

4. **Biased toward hardware resources.** Software resources (thread pools, connection pools, mutex queues, cache sizes) need to be added explicitly by the operator. Forgetting them is the most common USE failure mode — the resource you forgot is the one that's saturated.

5. **Blind to *cold* problems.** A resource at 5 % utilisation with no queue can still be the bottleneck if its latency per op is 10 ms on every request. Utilisation is about *fraction of time busy*, not *time per op*. Pair USE with Latency Analysis for latency-driven problems.

## Companion methods

Gregg recommends complementing USE with:

- **Workload Characterisation** — answers "why is the load hitting that resource?".
- **Drill-Down Analysis** — zooms into the saturated resource USE identified.
- **Latency Analysis** — for problems where no single resource is saturated but latency is high.
- **Thread State Analysis** — for software-resource breakdowns that USE skims over.

## Adversarial reading

- USE assumes you can measure all three dimensions. On some systems, saturation is unobservable (no queue-depth metric) and you must infer it from utilisation trends. This is where the method starts to feel like guesswork.
- In virtualised / cloud environments, "physical" resources are abstractions. CPU steal time, EBS burst credits, network bandwidth throttles, instance retirement — all of these are resources USE should consider, but they're harder to inventory than local CPU and RAM.
- "80 % of problems with 5 % of effort" is Gregg's rhetoric. Real share varies by shop. USE is a reliable first-pass filter; don't treat it as a complete diagnosis.

## Relevance to APEX

- APEX's resource-profiling feature (recording CPU, wall-clock, peak memory, allocations per test) *is* USE Method data collection applied at function granularity. Reporting "this function's U/S/E triplet is anomalous" would be a natural finding format.
- USE-style checklists are a good scaffold for APEX's own internal analysis: for each resource APEX has access to, verify it reports the three metrics.

## References

- Gregg, B. — "The USE Method" — [brendangregg.com/usemethod.html](https://www.brendangregg.com/usemethod.html)
- Gregg, B. — "USE Method: Linux Performance Checklist" — [brendangregg.com/USEmethod/use-linux.html](https://www.brendangregg.com/USEmethod/use-linux.html)
- Gregg, B. — *Systems Performance*, 2nd ed., Addison-Wesley 2020, §2.5.9.
- Gregg, B. — "Systems Performance in 2018" Netflix Tech Blog — contextual updates for USE in cloud era.
