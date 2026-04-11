---
id: 01KNZ301FVP9C02A01PTQ0WAYT
title: "Tool: hyperfine (Command-Line Benchmarking with Statistical Analysis)"
type: reference
tags: [tool, benchmarking, statistics, performance, regression-detection, cli]
links:
  - target: 01KNWE2Q9YZBAR140ZX5P36TQ5
    type: references
  - target: 01KNWE2QACWYZJXRJ8QE78T043
    type: extends
  - target: 01KNWE2QA5VP0K80TMSABACKWT
    type: extends
  - target: 01KNWGA5GQ08MFV0XJXX3MTFC3
    type: related
  - target: 01KNWGA5H1MNJK8GWPFCZSSW7E
    type: related
created: 2026-04-12
modified: 2026-04-12
source: "https://github.com/sharkdp/hyperfine"
author: "David Peter (sharkdp)"
license: "MIT / Apache-2.0"
---

# Tool: hyperfine (Command-Line Benchmarking with Statistical Analysis)

**Repository:** https://github.com/sharkdp/hyperfine
**Author:** David Peter (`sharkdp`)
**Language:** Rust
**License:** MIT / Apache-2.0 (dual)

## What it is

hyperfine is a command-line benchmarking tool for measuring the wall-clock performance of shell commands. It sits at the "I want to time `cmd-a` vs `cmd-b` and get a principled answer" tier of the performance-tooling stack — simpler than Criterion or JMH (which measure in-process functions), more principled than `time` (which gives you one sample with no variance estimate).

## Core features

**Statistical evaluation.** Each command is run multiple times. hyperfine computes the sample mean, standard deviation, median, min, and max, reports the coefficient of variation, and prints a relative comparison against the fastest command ("1.24 ± 0.03 times faster than..."). This is the feature that separates it from ad-hoc timing: you get both a central estimate and a noise estimate in a single run.

**Automatic run count determination.** By default hyperfine runs each command at least 10 times and continues running for at least 3 seconds of wall time. For fast commands it will run many more iterations to drive down the standard error; for slow commands it runs the minimum. This adaptive scheme is important because it prevents either wildly over-sampling trivial commands or drawing conclusions from a tiny sample of expensive commands.

**Warm-up runs.** The `--warmup N` flag executes N untimed runs before the timed phase begins. This populates the filesystem cache, JIT caches, kernel page tables, and any amortized-construction data structures, so the timed runs measure steady-state rather than cold-start performance. For APEX's purposes this is exactly the right default for measuring algorithmic complexity of a steady-state workload.

**Setup and cleanup hooks.** `--prepare CMD` runs CMD before each timed iteration (e.g., to clear a cache or reset a database). `--setup CMD` runs once at the start of each parameter sweep. `--cleanup CMD` runs at the end. Together these let you measure cold-cache performance scenarios or benchmarks that require per-iteration reset of mutable state.

**Outlier detection.** After collecting samples, hyperfine runs a heuristic outlier test and emits a warning if the spread of measurements suggests interference — e.g. a background process kicked in, thermal throttling occurred, or a disk flush ran during one iteration. This directly implements one of APEX's G-46 requirements: refuse to assert regressions on noise-dominated measurements.

**Parameterized benchmarks.** `--parameter-scan THREADS 1 12` runs a single command template across a range of a single parameter. Output is either flat (one row per parameter) or structured (JSON/CSV) for plotting. Combined with `--parameter-list` you can sweep arbitrary sets.

**Multi-format export.** Results can be exported to CSV, JSON, Markdown, and AsciiDoc. The JSON format is the canonical machine-readable output and is what APEX integration layers should consume.

## Methodology features

**Shell overhead correction.** By default hyperfine runs the target command through `/bin/sh -c`. It measures the startup overhead of that shell in a calibration phase and subtracts it from the reported timings so that very short commands are not dominated by shell startup.

**No-shell mode.** `-N` / `--shell none` executes the command directly via `fork`+`exec` without an intermediate shell. Recommended for commands whose timing is on the order of milliseconds or below, where shell startup is a meaningful fraction of the total.

**Custom shell selection.** `--shell <cmd>` lets you swap in `bash`, `zsh`, `fish`, `dash`, or any wrapper. Useful when the command relies on shell-specific features, or when you want to compare performance across shells.

## Distribution

hyperfine is packaged on every common platform:

- Linux: APT (Debian/Ubuntu), DNF (Fedora), pacman (Arch), APK (Alpine), Zypper (openSUSE), xbps (Void)
- macOS: Homebrew, MacPorts
- Windows: Chocolatey, Scoop, Winget
- Cross-platform: conda, cargo install, nix
- Release binaries on GitHub for every tagged release

The Rust implementation means no runtime dependencies, a static ~2 MB binary, and identical behavior across platforms.

## Relevance to APEX G-46

hyperfine plays two distinct roles in the G-46 pipeline:

1. **Oracle for CI regression gates.** After APEX generates a worst-case input, a downstream CI job should re-measure cost under noise-controlled conditions and assert the difference against a baseline. hyperfine is the correct tool for that step: it gives you a principled mean, a standard deviation, a warning on noisy runs, and machine-readable JSON. A regression gate can then assert `mean_new > mean_old + k * max(σ_new, σ_old)` with well-calibrated numerics.

2. **Loop for Singularity/PerfFuzz-style size sweeps.** When APEX wants to fit a scaling law (empirical Big-O), it can shell out to hyperfine with `--parameter-scan N 1 1024` and parse the JSON for a power-law fit. This is simpler than instrumenting the target in-process with `criterion.rs` when the target is an external binary.

Two caveats are worth recording for future APEX design:

- hyperfine measures *wall-clock* time, not CPU cycles or instructions. On noisy hardware (shared CI runners, cloud VMs) this is a weakness; `perf stat`'s `--instructions` counter is more robust but less portable.
- hyperfine does not measure memory; pair with `/usr/bin/time -v` or `heaptrack` for memory-side worst cases.

## Typical invocations

```
# Compare two binaries
hyperfine --warmup 3 './apex-old' './apex-new'

# Parameter sweep, JSON output for APEX ingestion
hyperfine --warmup 3 --export-json sweep.json \
    --parameter-scan N 1 1024 './parser input_{N}'

# Cold-cache measurement
hyperfine --prepare 'sync; echo 3 > /proc/sys/vm/drop_caches' 'grep foo big.log'
```
