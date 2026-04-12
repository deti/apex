---
id: 01KNZ6FPSNP9N8SFZMH3B8ZK13
title: Hardware Noise Sources in Performance Benchmarking — Taxonomy
type: permanent
tags: [benchmarking, noise, cpu-frequency, turbo, thermal-throttling, aslr, cache, tlb, numa, mitigations]
links:
  - target: 01KNZ6FPT0VDMJ7J1R5PEBM0DX
    type: related
  - target: 01KNZ6FPQ32R61BEN1K1WNZGPX
    type: related
  - target: 01KNZ6FPS6S89303Q1F11G6M1A
    type: related
  - target: 01KNWE2QACWYZJXRJ8QE78T043
    type: related
  - target: 01KNZ4VB6JP254YSHY7N9PX4HQ
    type: related
  - target: 01KNWE2Q9YZBAR140ZX5P36TQ5
    type: related
created: 2026-04-11T21:17:08.789087+00:00
modified: 2026-04-11T21:17:08.789089+00:00
---

A taxonomy of the confounders that make wall-clock performance measurement noisy on modern hardware, with mitigation strategies for each. This is a reference note for practitioners building CI perf infrastructure — the "what could possibly go wrong" checklist.

*Canonical source: LLVM's benchmarking guide at llvm.org/docs/Benchmarking.html. Related guidance: the criterion.rs book, Google Benchmark's "Reducing Variance" guide, Intel's performance analysis documentation, and the Linux `perf` tool documentation.*

## Frequency-related noise

### Dynamic frequency scaling (CPUfreq, P-states, Intel SpeedStep, AMD Cool'n'Quiet)

Modern CPUs vary their clock frequency dynamically to save power. Under default governors (`powersave`, `ondemand`, `schedutil` on Linux), the frequency depends on load, temperature, and hints from the kernel. The same instruction sequence can run at 800MHz one measurement and 3.6GHz the next, producing a 4.5× variation in wall-clock time.

**Mitigation:**
- Set the governor to `performance`: `sudo cpupower frequency-set -g performance`
- Disable `intel_pstate` in favour of `acpi_cpufreq` + userspace governor.
- Pin frequencies to base clock via BIOS / MSRs.

### Turbo boost / dynamic overclocking

Turbo frequencies depend on thermal and power headroom — one core running alone can boost higher than eight cores running together. This makes single-threaded benchmarks especially variable: if another core on the same package decides to do work (a background service), your benchmark's turbo budget shrinks.

**Mitigation:**
- Disable turbo (`echo 1 > /sys/devices/system/cpu/intel_pstate/no_turbo`) or Boost on AMD. Measurements become lower-peak but more reproducible.

### Thermal throttling

Under sustained load, CPUs downclock to stay within thermal limits (TjMax). Throttling kicks in after tens of seconds to minutes — a short benchmark runs at base+turbo; a long one runs at base or lower. This produces a systematic bias where "long" benchmarks look slower per iteration than "short" ones even for identical code.

**Mitigation:**
- Pre-warm the CPU to steady-state temperature before measurement.
- Keep rooms cool; use server chassis with adequate cooling.
- Disable turbo so throttling is less aggressive.
- For ARM / laptop hardware, monitor `/sys/class/thermal/thermal_zone*/temp` during runs and reject samples taken during throttle.

## Core- and thread-level noise

### Context switches and scheduler jitter

The Linux scheduler periodically preempts your benchmark to run other tasks — kernel threads, RCU work, interrupt handlers. Each context switch adds hundreds of nanoseconds and cold-caches the benchmark.

**Mitigation:**
- **CPU pinning**: `taskset -c 3 ./benchmark` — bind to a single core.
- **`cset shield`** (from `cpuset`): reserve a core so the kernel doesn't schedule anything else on it.
- **`isolcpus` kernel boot parameter**: exclude cores from the scheduler entirely, then manually pin benchmarks.
- **`nohz_full` + `rcu_nocbs`**: eliminate timer ticks and RCU callbacks on isolated cores.

### Interrupts and softirqs

IRQs are delivered to whichever core IRQ affinity says, possibly including your benchmark core. Each interrupt is a mini context switch.

**Mitigation:**
- Move IRQs away from benchmark cores: `echo <mask> > /proc/irq/<n>/smp_affinity`.
- Disable `irqbalance` or set it to ignore benchmark cores.
- For ultimate control, use an RT kernel with CPU shielding.

### Hyperthreading (SMT)

Two logical threads on one physical core share L1, L2, and execution units. If a neighbour HT runs concurrently, performance of your benchmark is degraded unpredictably.

**Mitigation:**
- Disable SMT in BIOS, or offline sibling logical CPUs at runtime: `echo 0 > /sys/devices/system/cpu/cpu<n>/online`.
- Pin to one HT sibling and leave the other idle.

## Memory-related noise

### ASLR (Address Space Layout Randomization)

Linux randomizes the starting addresses of stack, heap, mmap regions, and (for PIE binaries) the text segment. This is a security feature but it means every run has a different memory layout, which changes cache alignment, TLB coverage, and branch predictor state.

**Mitigation:**
- `setarch $(uname -m) -R ./benchmark` disables ASLR for a single run.
- Or `echo 0 > /proc/sys/kernel/randomize_va_space`.
- **Note**: disabling ASLR makes layout *one deterministic value* rather than *many random values*, which means you've frozen the bias at a single point (cf. Mytkowicz et al. 2009). Stabilizer's response is the opposite: randomize *more*, not less.

### NUMA effects

On multi-socket systems, memory local to the CPU's own NUMA node is faster to access than memory on a remote node. The kernel's first-touch policy places pages on whichever node touches them first, but cross-socket migration and remote allocations can happen silently.

**Mitigation:**
- `numactl --cpunodebind=0 --membind=0 ./benchmark` pins both CPU and memory allocation to a single NUMA node.
- Avoid multi-socket machines for benchmarking unless you're explicitly measuring NUMA behaviour.

### THP (Transparent Huge Pages)

Linux may opportunistically promote 4KB pages to 2MB huge pages to reduce TLB pressure, but the promotion happens asynchronously and can cause stalls mid-benchmark.

**Mitigation:**
- Set `madvise` or `never` mode: `echo never > /sys/kernel/mm/transparent_hugepage/enabled`.

### Cache state (cold/warm, pollution)

The first iteration of a benchmark runs on a cold cache, the hundredth on a warm one. Background activity can evict your data between runs.

**Mitigation:**
- **Warm-up runs**: discard the first N iterations.
- **Flush or prime the cache** deliberately to pin the initial state.
- Shield the core so nothing else pollutes the cache during measurement.

### TLB state

Similar to cache — cold TLB, warm TLB. Especially matters for workloads with large working sets.

**Mitigation:**
- Warm-up; shield; use huge pages if appropriate.

### Branch predictor state

BP is process-shared on some microarchitectures (until Spectre mitigations). Predictor state from prior processes can bleed into yours, causing noisy first-run measurements.

**Mitigation:**
- Warm-up and shield.
- Disable Intel IBPB / IBRS at your own risk (security trade-off).

### Hardware prefetchers

Prefetchers can mask latency on some access patterns and not others, leading to bimodal behaviour depending on microarchitectural state.

**Mitigation:**
- Disable prefetchers via MSR (`wrmsr`) if reproducibility is critical. Understand that you're now measuring a non-representative scenario.

## System-level noise

### Background services

`systemd`, cron, auto-update daemons, `packagekit`, logging rotators, crash reporters, indexers. Any of these can wake up during your benchmark and eat cycles and caches.

**Mitigation:**
- Boot into a minimal environment (`systemctl isolate multi-user.target`, or a dedicated benchmark user with only essential services).
- Kill or mask services: `systemctl mask tracker-miner-fs.service` etc.
- Run on dedicated hardware, not a shared workstation.

### Filesystem and disk noise

Page cache state, dirty writeback, fsync, journal commits. A filesystem benchmark's variance can be driven by unrelated I/O on the same disk.

**Mitigation:**
- Dedicated disk or ramfs for benchmark data.
- Drop caches before each run: `echo 3 > /proc/sys/vm/drop_caches`.
- Disable automatic filesystem maintenance (fstrim timers, scrub jobs).

### Network noise

NICs can interrupt, `iwlwifi` firmware can retrain, NTP can adjust the clock.

**Mitigation:**
- Offline the NIC during benchmarks where it's not needed.
- Use `CLOCK_MONOTONIC_RAW` instead of `CLOCK_REALTIME` to avoid NTP skew.

## What CI runners don't control

Most public CI services (GitHub Actions, GitLab CI, CircleCI) run on shared virtual machines on hyperscaler clouds. They typically have:
- **None** of the mitigations above applied.
- **Noisy neighbours** on the hypervisor.
- **No frequency pinning**; P-states are under hypervisor control.
- **Variable hardware** from the pool.
- **Cloud-provider interruptions** (live migration, maintenance).

This is why naive wall-clock benchmarks on public CI produce garbage. The industry has converged on three responses:
1. **Bare-metal perf runners** (Google Perflab, Mozilla's dedicated Talos hardware, MongoDB's Evergreen perf fleet).
2. **Change-point detection** (MongoDB, Hunter) — accept the noise and use robust statistics on long time series.
3. **Instruction counting** (CodSpeed, iai-callgrind) — sidestep wall-clock entirely.

## Connections

- Mytkowicz et al. 2009 — layout noise specifically.
- Stabilizer — randomize to handle layout noise.
- Chen & Revels 2016 — robust statistics for handling residual noise.
- CodSpeed / iai-callgrind — instruction-count alternative.
- Chrome's perf waterfall — dedicated hardware lab.

## Reference

LLVM benchmarking guide: llvm.org/docs/Benchmarking.html.
Google Benchmark: github.com/google/benchmark/blob/main/docs/reducing_variance.md.
