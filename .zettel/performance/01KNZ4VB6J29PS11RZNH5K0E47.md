---
id: 01KNZ4VB6J29PS11RZNH5K0E47
title: "Soak / Endurance Testing — Long-Duration Exposure"
type: concept
tags: [soak-testing, endurance, taxonomy, memory-leak, resource-leak, mttf, meier-2007]
links:
  - target: 01KNZ4VB6JY38THW04Z3MMGBZ3
    type: extends
  - target: 01KNZ4VB6JTC1Z9CGYN4Q1CCA6
    type: related
  - target: 01KNZ4VB6JVSPDK724EZFPA36H
    type: references
  - target: 01KNWE2Q9YZBAR140ZX5P36TQ5
    type: related
created: 2026-04-12
modified: 2026-04-12
source: "Meier, Farre, Bansode, Barber, Rea — Performance Testing Guidance for Web Applications — Microsoft p&p, 2007, Chapter 2"
---

# Soak / Endurance Testing — Long-Duration Exposure

## Question answered

*"Does the system remain healthy when run at normal load for hours, days, or weeks? Does it leak resources, drift, or accumulate state that eventually takes it down?"*

Soak testing (Meier et al. use "endurance testing") exposes bugs that are invisible at short time scales because they accumulate linearly or super-linearly with wall-clock time or request count. The test signal is not *whether the SLO is met in a five-minute window* — a correctly-specced system will pass that easily — but *whether the SLO is still met after hour 6, hour 24, hour 72*.

## Canonical definition

Meier et al. 2007, ch. 2:

> *Endurance testing is a subset of load testing. An endurance test is a type of performance test focused on determining or validating the performance characteristics of the product under test when subjected to workload models and load volumes anticipated during production operations over an extended period of time. Endurance testing may be used to calculate Mean Time Between Failure (MTBF), Mean Time To Failure (MTTF), and similar metrics.*

"Soak test" is the more common informal term; "endurance test" is the formal Meier-era term. ISTQB uses both; IEEE 829 prefers "endurance".

## What soak testing exposes that shorter tests miss

1. **Memory leaks.** A leak of 4 KB per request is invisible in 5 minutes. At 1 k req/s, 24 hours, that's 345 GB — OOM long before the soak completes. Classic bugs: unbounded caches, forgotten listener references, finaliser chains, connection pools that grow without shrinking.

2. **File-descriptor and socket leaks.** Each leaked FD is slow to reach the ulimit — 64 k FDs at 0.1 leaks/s takes 180 hours. Soak tests at realistic rates expose them within a day.

3. **Log-file growth and log rotation bugs.** At 10 k req/s with 2 KB per log line, that's 20 MB/s or 1.7 TB per 24 hours. Faulty rotation policies fill the disk.

4. **Database bloat.** Tombstones in Cassandra, unvacuumed dead tuples in Postgres, unmerged SSTables in LSM stores. Steady-state query performance drifts slower on hour 48 than hour 1 even though workload is identical.

5. **Clock drift, cert rotation, secret rotation.** Operational events that only happen on multi-day cycles. A soak test spanning a cert renewal catches "service segfaults during TLS cert reload" bugs that shorter tests can't even reach.

6. **Fragmentation.** Heap fragmentation in glibc malloc or jemalloc. Disk fragmentation on XFS/ext4. Both accumulate over time and only cause observable slowdown after many allocation cycles.

7. **GC pathology.** Short-lived objects that escape to the old generation and eventually trigger full GCs. At start of test, young GC is fast; at hour 12, full GCs start dominating. Visible only over long windows.

8. **Counter overflows.** 32-bit counters overflow at 4.3 G. At 10 k increments/s, that's 5 days. Shorter tests never hit it. 64-bit counters overflow at 5.8 billion years and are safe.

9. **Lease / token / session expiration.** Services that use tokens with TTL often have subtle bugs at the first renewal. Soak tests crossing the TTL boundary reveal them.

10. **Accumulating drift in distributed state.** A small per-request skew in replica sync that compounds over hours until replicas diverge.

## Fit in the SDLC

- **Pre-release**, always. A release candidate goes through a 24–72 hour soak before shipping. This is the one type of perf test that cannot be "run tomorrow" — its signal is the elapsed time.
- **Continuous (nightly)** — a 6–12 hour soak as part of CI. Long enough to catch the fastest leaks, short enough to fit in a CI window. Not a replacement for pre-release soak.
- **Post-release** — production itself *is* the continuous soak test, if it is instrumented to detect the drift patterns the offline soak tests were meant to catch.

## Profiles

- **Flat steady load at ~50–70 % of capacity** for N hours. The canonical profile. Low enough that headroom exists, high enough that leaks are exercised at realistic rates.
- **Diurnal variation.** 2x the nightly load during "day", 0.5x during "night", matching production. Catches bugs that appear at load transitions as well as at steady state.
- **Soak-plus-stress.** Steady load for 24 h, then a brief stress burst. Tests whether degradation during stress is worse after long uptime (typical with fragmentation).

## Key metrics to watch over the soak window

These are *trend* metrics, not steady-state values:

- Resident memory (RSS) over time — linear growth = leak.
- Heap used / committed — saw-tooth should stay in a stable range; a rising envelope is a leak.
- File descriptor count — flat; rising = leak.
- Thread count — flat; rising = leak.
- Log volume — flat or matches request rate.
- GC duration and frequency — should be stable; rising full-GC frequency is a smell.
- Query latency distribution — compare hour 1 histogram to hour 24 histogram. They should be indistinguishable (KS test). Drift is the bug.
- Disk usage — matches retention policy; if growing without bound, rotation is broken.

## Anti-patterns

1. **Too short.** A 30-minute "soak" is not a soak. The minimum useful duration is the one that would catch your fastest-growing leak. At a 4 KB/req leak and 1 k req/s, a 30-min run grows 7 GB — enough to spot. At a 40 B/req leak, 30 min = 70 MB, lost in noise. Fix: size duration to the leak you're hunting.

2. **Running the soak on a quiet machine.** A soak test environment under 10 % of production load tests "does the system idle cleanly", not "does it run cleanly under production load". Fix: target realistic production rate.

3. **No instrumentation for trend.** Soak test ends, report says "no crashes". But was memory rising? Nobody knows. Fix: before starting, decide which trend metrics will be plotted and what counts as "drift".

4. **Ignoring variance at the end.** Latency histogram at hour 24 has higher p99 than hour 1, but average is similar; the report says "pass" because averages match. Fix: compare histograms, not scalars.

5. **Restarting the system before measurement.** "Soaked for 24 h, restarted, measured latency — it's fine." Of course it is. Fix: measure *without* restart.

6. **Soak testing in an environment that gets itself restarted.** Kubernetes eviction, cloud instance rotation, kernel auto-updates. If the test environment itself restarts during soak, the soak isn't a soak. Fix: long-lived dedicated infra.

7. **Using soak as the only leak detection.** Runtime leak detectors (Valgrind, ASan, LeakSanitizer, the JVM HotSpot NMT, Go's `runtime.ReadMemStats`) catch leaks faster and more precisely. Soak tests confirm, don't diagnose.

## Acceptance criteria

- Run for D hours at L % of design capacity.
- At the end of the run, the following trends are within tolerance:
  - RSS growth < X MB/hour (or flat).
  - p50, p99 latency histograms at the end statistically indistinguishable from the start.
  - Error rate flat.
  - No restarts, no OOMs, no deadlocks.
- Post-run system still accepts traffic and serves it at baseline performance.

## Adversarial reading

- Soak tests are expensive (long wall-clock, tied-up infra). Each org chooses a duration that's a compromise between coverage and cost. 24 hours is common. 72 hours catches more. One week catches even more but rarely fits.
- "No observed leak" does not mean "no leak". It means "no leak of magnitude detectable in D hours at L % load". A leak of 1 byte/request is genuinely hard to find in soak; it shows up only in production after weeks.
- Soak testing does not find memory-leak *root cause*, only its *existence*. Root cause typically needs heap dump analysis or a runtime leak sanitiser.

## References

- Meier et al. — Microsoft p&p Performance Testing Guidance, Ch. 2.
- IEEE Std 829 — Software Test Documentation (endurance test definition).
- Barber, S. — "The Evils of Coordinated Omission" and "Stability Patterns for Long-Running Load Tests" — PerfTestPlus, 2009.
