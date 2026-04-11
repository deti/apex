---
id: 01KNZ301FVGJ4SY6Q3W0XWQXEP
title: "Tool: honggfuzz (Hardware- and Software-Feedback Fuzzer)"
type: reference
tags: [tool, fuzzing, honggfuzz, intel-pt, hardware-coverage, persistent-mode, coverage-guided]
links:
  - target: 01KNWE2Q9YZBAR140ZX5P36TQ5
    type: references
  - target: 01KNWE2QA0Z52H8VVFAMSA7KGA
    type: extends
  - target: 01KNWGA5GD7A7WXW56682R280K
    type: related
  - target: 01KNZ301FVH6M9PHFVP9QETRB6
    type: related
  - target: 01KNWGA5GAWV1Y9D6N11D2TP20
    type: related
created: 2026-04-12
modified: 2026-04-12
source: "https://github.com/google/honggfuzz"
author: "Robert Swiecki / Google"
license: "Apache-2.0"
---

# Tool: honggfuzz (Hardware- and Software-Feedback Fuzzer)

**Repository:** https://github.com/google/honggfuzz
**Original author:** Robert Swiecki (Google)
**License:** Apache-2.0

## What it is

honggfuzz is a security-oriented, multi-process and multi-threaded fuzzer developed at Google. It differentiates itself from AFL++ and libFuzzer by offering **hardware-based coverage feedback** as a first-class option: it can drive the fuzzing loop using Intel Processor Trace (Intel PT) or Intel Branch Trace Store (BTS) instead of (or in addition to) compiler-inserted software coverage. This makes it the preferred choice for fuzzing closed-source binaries or when source-level instrumentation is impractical.

The README describes honggfuzz as a tool that "uses code coverage (software and hardware-based) to find bugs," with a "multi-process and multi-threaded engine" and the ability to "test APIs directly in-process with iteration speeds up to 1M/sec" in persistent mode.

## Coverage modes

honggfuzz supports four coverage feedback modes, selectable at invocation time:

1. **SanitizerCoverage (software)** — identical to libFuzzer and AFL++'s `-fsanitize=fuzzer-no-link` mode. Requires compiler instrumentation but gives fine-grained edge coverage.
2. **Intel BTS** — the older Intel feature that records a fixed-size buffer of taken branches. honggfuzz reads the buffer between iterations and treats new branches as new coverage.
3. **Intel PT** — the newer Intel feature that produces a full branch trace in a compressed format. honggfuzz decodes the trace per iteration to reconstruct executed edges. This is strictly more powerful than BTS but has higher decode overhead.
4. **None** — run as a pure blackbox fuzzer (closer to classical random fuzzing). Useful as a baseline or when no coverage source is available.

Hardware-based modes work on unmodified binaries, which is the key advantage over source-instrumented fuzzers. This is critical for fuzzing proprietary libraries, kernel drivers, firmware, and any target where you cannot recompile.

## Performance features

**Persistent fuzzing.** Like libFuzzer, honggfuzz supports in-process persistent fuzzing by calling a user-supplied `LLVMFuzzerTestOneInput` function. Iteration rates in this mode can reach 1M/sec per core, making it competitive with libFuzzer in raw throughput.

**Multi-process + multi-threaded.** Unlike AFL's simpler fork-per-iteration model, honggfuzz spawns multiple worker threads within a single process, each driving its own target. This amortizes fuzzer bookkeeping costs across cores.

**Low-level monitoring via ptrace.** Honggfuzz uses `ptrace(2)` to observe target crashes, including asynchronous signals and crashes that ordinary in-process fuzzers miss (e.g. signal-delivered crashes in child threads, or race-condition crashes where the stack is torn down before a sanitizer can catch it).

## Platform support

Linux, macOS, Android, NetBSD, FreeBSD, and Windows via Cygwin. The hardware coverage modes are Linux-specific (Intel PT/BTS) and require recent kernels.

## Notable discoveries

The honggfuzz project's hall of fame includes critical bugs in major projects, most notably CVE-2016-6309 (an OpenSSL use-after-free triggering potential remote code execution) and numerous Apache HTTPD, cryptographic library, and media processor vulnerabilities. honggfuzz is also one of the backend engines for OSS-Fuzz.

## Relevance to APEX G-46

Honggfuzz is an important alternative engine in the G-46 fuzzing menu for two reasons:

1. **Closed-source / binary-only targets.** Some APEX targets (firmware, proprietary libraries, JITs) cannot be recompiled with SanitizerCoverage. honggfuzz's Intel PT mode is the practical escape hatch: give it the binary and a harness script, and it will drive coverage-guided fuzzing with no source modifications.

2. **Hardware-backed cost signals.** Intel PT records the complete branch history per iteration. This is not only a coverage signal but also a cost signal: the total number of recorded branches is a fast, deterministic proxy for instruction count. An APEX performance fuzzer could drive a PerfFuzz-style max-per-edge counter using honggfuzz's PT decoder rather than adding source-level instrumentation. Because PT is implemented in hardware, the feedback loop is essentially free at the target side.

## Comparison to peer fuzzers in the vault

| Feature | AFL++ | libFuzzer | honggfuzz |
|---|---|---|---|
| In-tree source-level coverage | yes (via SanCov) | yes | yes |
| Hardware coverage (Intel PT/BTS) | no (officially) | no | **yes** |
| Persistent mode | yes | yes (native) | yes |
| Closed-source binaries | QEMU mode (slower) | no | yes (PT mode) |
| Works as OSS-Fuzz engine | yes | yes | yes |
| Cost-feedback extensions in literature | AFLFast, MemLock, PerfFuzz | SlowFuzz | none published |

The gap at the bottom of the honggfuzz column is notable: no one has (yet) published a cost-feedback extension of honggfuzz analogous to MemLock or PerfFuzz. That is a research opportunity — and a concrete place where APEX could make a novel contribution by building a hardware-trace-driven worst-case fuzzer on top of honggfuzz's Intel PT mode.
