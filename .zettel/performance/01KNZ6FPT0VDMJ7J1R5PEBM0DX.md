---
id: 01KNZ6FPT0VDMJ7J1R5PEBM0DX
title: "Linux Perf Runner Mitigations — cset shield, isolcpus, tuned profiles"
type: permanent
tags: [cset-shield, isolcpus, tuned, benchmarking, linux, cpu-pinning, mitigations, perf-runner]
links:
  - target: 01KNZ6FPSNP9N8SFZMH3B8ZK13
    type: related
  - target: 01KNZ6FPQ32R61BEN1K1WNZGPX
    type: related
  - target: 01KNZ6FPS6S89303Q1F11G6M1A
    type: related
  - target: 01KNWE2QACWYZJXRJ8QE78T043
    type: related
  - target: 01KNWE2Q9YZBAR140ZX5P36TQ5
    type: related
created: 2026-04-11T21:17:08.800659+00:00
modified: 2026-04-11T21:17:08.800660+00:00
---

The operational playbook for turning a noisy Linux machine into a reproducible performance measurement platform. This note is the "how-to" complement to the noise taxonomy note — it lists the specific commands and kernel parameters that modern perf infrastructure uses.

## The layered approach

Modern benchmark rigs apply mitigations in layers, each narrowing the noise budget further:

1. **BIOS layer** — disable turbo, SMT, C-states, EIST/SpeedStep where possible.
2. **Kernel boot parameters** — `isolcpus`, `nohz_full`, `rcu_nocbs`, `intel_pstate=disable`.
3. **Runtime layer** — `cset shield`, `taskset`, `numactl`, `cpupower`.
4. **Process layer** — `setarch -R` (no ASLR), `nice -n -20`, `ionice`.
5. **Statistical layer** — warm-up runs, outlier rejection, robust estimators.

Each layer attacks specific noise sources; skipping a layer leaves that source uncontrolled.

## `cset shield` — cpuset-based core isolation

`cset` (from the `cpuset` package) provides a user-friendly wrapper around Linux cpusets. The `shield` subcommand creates two cpusets:
- `system` — holds all existing tasks; gets the "leftover" cores.
- `user` — the "shielded" cpuset; reserved for your benchmark.

```bash
# Shield cores 2-7 for benchmarking; cores 0-1 handle system tasks
sudo cset shield --cpu=2-7 --kthread=on
# Run benchmark inside the shield
sudo cset shield --exec -- ./bench --n 1000
# Restore
sudo cset shield --reset
```

`--kthread=on` migrates movable kernel threads out of the shielded cores. Non-movable kthreads (per-cpu timers, etc.) remain, which is why full isolation requires `isolcpus` at boot.

**Caveat:** `cset` was last actively maintained around 2011 and is marked deprecated in some distros. The modern alternative is direct cpuset manipulation via `cgcreate`/`cgset` or via `systemd` slice units.

## `isolcpus`, `nohz_full`, `rcu_nocbs` — kernel boot isolation

For reproducibility beyond what cpuset can offer, isolate cores at boot. Add to the kernel command line:

```
isolcpus=2-7 nohz_full=2-7 rcu_nocbs=2-7
```

- **`isolcpus=2-7`** — excludes these CPUs from the scheduler's load-balancing. Nothing runs on them unless explicitly pinned (via `taskset` or sched affinity).
- **`nohz_full=2-7`** — disables the periodic tick on these cores when they have a single runnable task. Removes a ~1kHz source of jitter.
- **`rcu_nocbs=2-7`** — moves RCU callback processing off these cores to housekeeping cores.

Combined, these give you cores that look almost like bare-metal from a single-threaded benchmark's perspective. You pay in capacity — those cores can't run general workloads without explicit pinning.

**Caveat:** `isolcpus` is marked as a deprecated interface in newer kernels; it still works but future changes may require migration to `cpuset.cpu_exclusive=1` + `sched_setaffinity`. `nohz_full` and `rcu_nocbs` remain supported.

## `cpupower frequency-set` — P-state control

```bash
sudo cpupower frequency-set -g performance
# Confirm
cpupower frequency-info
```

Sets the CPU frequency governor to `performance` (max frequency at all times). On Intel P-state-driven systems, you may also need:

```bash
echo 1 | sudo tee /sys/devices/system/cpu/intel_pstate/no_turbo
echo performance | sudo tee /sys/devices/system/cpu/cpu*/cpufreq/scaling_governor
```

to pin at the base (non-turbo) clock and disable turbo. Running at base clock sacrifices peak performance for reproducibility — the turbo budget is variable, base is stable.

## `tuned-adm` — Red Hat's tuned profiles

RHEL/Fedora ship with the `tuned` daemon which bundles many of the above into named profiles:

```bash
sudo tuned-adm profile latency-performance
# or for ultra-low jitter workloads
sudo tuned-adm profile realtime
```

`latency-performance` pins the governor to performance, disables most power saving, and raises `vm.swappiness` to 10. `realtime` additionally uses `isolcpus` and RT scheduling. For benchmarking, `latency-performance` is usually the right starting point.

## `taskset` + `numactl` — per-process pinning

Once the environment is prepared, pin the benchmark process:

```bash
numactl --cpunodebind=0 --membind=0 taskset -c 4 ./benchmark
```

- `numactl --cpunodebind=0 --membind=0` pins both CPU and memory allocation to NUMA node 0.
- `taskset -c 4` pins to a specific core (core 4 in this example, ideally one inside your shield).

## `setarch -R` — disable ASLR

```bash
setarch $(uname -m) -R ./benchmark
```

Runs the benchmark with ASLR disabled. As noted in the Mytkowicz discussion, this trades "variable layout bias" for "one fixed layout bias" — reproducibility improves but you may be measuring an unlucky layout. The alternative is Stabilizer-style randomization (expensive but unbiased).

## `perf` and diagnostic sampling

During benchmark runs, collect PMU counters to diagnose noise after the fact:

```bash
perf stat -e context-switches,cpu-migrations,L1-dcache-load-misses,branch-misses ./benchmark
```

Elevated `cpu-migrations` means your pinning failed. Elevated `context-switches` means a kthread or softirq visited your core. These diagnostics let you detect when a "regression" is actually a failed mitigation.

## What this does not solve

- **Hardware heterogeneity.** If your CI pool contains multiple SKUs, results from different runs are on different machines even if each is individually quiet. Mitigation: separate perf-runner pool with identical hardware.
- **Neighbour noise on shared hosts.** Cloud VMs share physical cores via the hypervisor; `cset` runs inside the VM and cannot shield against the host. Dedicated (bare-metal) instances or on-prem perf runners are needed for tight noise control.
- **Long-term drift.** Over days, cooling performance degrades as dust accumulates; over months, thermal paste dries. Long-running baselines drift even with all mitigations applied. Change-point detection handles this.
- **Code layout (Mytkowicz) effects.** None of the kernel/BIOS mitigations address link-order and stack-offset sensitivity. Stabilizer does; CodSpeed sidesteps it.

## Canonical perf rig checklist

For a new Linux-based perf runner, the minimum competent setup:

- [ ] BIOS: turbo off, SMT off, C-states off, SpeedStep off.
- [ ] Kernel cmdline: `isolcpus=<N> nohz_full=<N> rcu_nocbs=<N> intel_pstate=disable`.
- [ ] `cpupower frequency-set -g performance`.
- [ ] ASLR off: `kernel.randomize_va_space = 0`.
- [ ] `tuned-adm profile latency-performance`.
- [ ] `echo never > /sys/kernel/mm/transparent_hugepage/enabled`.
- [ ] Swappiness low: `vm.swappiness = 1`.
- [ ] Filesystem for benchmark data mounted with `noatime`, on dedicated disk or ramfs.
- [ ] Killed / masked: unnecessary services (tracker, packagekit, updaters).
- [ ] Bench process: `numactl --cpunodebind=0 --membind=0 taskset -c <N> setarch -R ./benchmark`.
- [ ] Warm-up iterations discarded.
- [ ] Multiple repetitions with robust statistics (min, median, trimmed mean — not just mean).

Even with all this, expect residual 0.5–2% noise on most benchmarks. Below 0.5% you are likely in over-control territory and need bare-metal.

## Connections

- Noise sources taxonomy (sibling note).
- Mytkowicz et al. 2009 — layout sensitivity that these mitigations don't address.
- Stabilizer (Curtsinger & Berger 2013) — orthogonal layout randomization.
- Chen & Revels 2016 — robust statistics for residual noise.

## References

- `man cset-shield(1)`.
- Linux kernel documentation: `Documentation/admin-guide/kernel-parameters.txt` (`isolcpus`, `nohz_full`, `rcu_nocbs`).
- Red Hat Performance Tuning Guide — `tuned-adm` profiles.
- LLVM benchmarking guide: llvm.org/docs/Benchmarking.html.
