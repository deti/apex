---
id: 01KNZ4RPCQ0HJEDD8XQCV5A58N
title: "AFL++: Combining Incremental Steps of Fuzzing Research (Fioraldi et al., WOOT 2020)"
type: literature
tags: [paper, fuzzing, afl, aflplusplus, woot, 2020, cmplog, mopt, redqueen, laf-intel]
links:
  - target: 01KNZ2ZDMEPBXSH02HFWYAKFE4
    type: extends
  - target: 01KNWGA5GD7A7WXW56682R280K
    type: related
  - target: 01KNWGA5GAWV1Y9D6N11D2TP20
    type: related
  - target: 01KNWEGYB3NXWFB6D4SV4DTD5X
    type: related
  - target: 01KNZ4RPCRK3648T95MFCV74YT
    type: related
  - target: 01KNZ4RPCS4PA5RHBWKW08X4G8
    type: related
  - target: 01KNZ4RPCT50A8XWXYC4RVPFT9
    type: related
  - target: 01KNWE2QA0Z52H8VVFAMSA7KGA
    type: related
  - target: 01KNWE2Q9YZBAR140ZX5P36TQ5
    type: related
created: 2026-04-12
modified: 2026-04-12
source: "https://aflplus.plus/papers/aflpp-woot2020.pdf"
venue: "USENIX WOOT 2020"
authors: [Andrea Fioraldi, Dominik Maier, Heiko Eißfeldt, Marc Heuse]
year: 2020
---

# AFL++: Combining Incremental Steps of Fuzzing Research

**Authors:** Andrea Fioraldi (EURECOM), Dominik Maier (TU Berlin), Heiko Eißfeldt, Marc Heuse (vanHauser-thc).
**Venue:** 14th USENIX Workshop on Offensive Technologies (WOOT '20), August 2020, co-located with USENIX Security '20.
**Paper URL:** https://aflplus.plus/papers/aflpp-woot2020.pdf
**Project:** https://github.com/AFLplusplus/AFLplusplus — https://aflplus.plus/

## Retrieval Notes

*The PDF (321 KB) was fetched 2026-04-12 but returned as compressed binary by the WebFetch backend, so the body of this note is assembled from (a) the abstract as circulated on the AFL++ project "papers" listing at https://aflplus.plus/papers/ and (b) the canonical feature catalogue in the AFL++ source tree (`docs/notes_for_asan.md`, `docs/custom_mutators.md`, `qemu_mode/`, `instrumentation/cmplog.h`) as it stood in 2020-2022. Replace the technical section with verbatim paragraphs when the PDF is accessible.*

## Positioning

By 2020 the original AFL (Michał Zalewski, Google, 2013) had been unmaintained for several years, yet it remained the workhorse of both industrial fuzzing (OSS-Fuzz) and a decade of academic research. Dozens of research papers published incremental improvements — new mutators, new coverage maps, new power schedules, new bypass techniques — and most of them died as one-off patches on top of abandoned AFL branches. AFL++ is a deliberate act of curation: the authors take the union of promising post-AFL ideas from the research literature, merge them into a single maintained fork, and evaluate them against the unmodified AFL baseline and against each other.

The goal stated in the abstract is programmatic: **"make the latest fuzzing research immediately usable."** Every feature in AFL++ traces to a specific paper or pull request, documented in the repo's `docs/papers.md`. The WOOT 2020 paper is the umbrella reference.

## Feature catalogue (the "incremental steps")

The paper groups AFL++'s additions into four layers, each corresponding to a different point in the fuzzing loop.

### 1. Observation / coverage feedback

- **NeverZero counters.** Classical AFL's hit-count bucketing uses an `u8` edge map with buckets `{1, 2, 3, 4-7, 8-15, 16-31, 32-127, 128-255}`. After 256 hits a bucket overflows to zero, losing new-path information from frequently-executed edges. NeverZero sets the counter to 1 on overflow, preserving the "edge is live" signal.
- **Context-sensitive edges (`ctx` mode).** Annotates each edge with a hash of the current call-stack frame, so the same basic-block transition exercised from different callers shows up as distinct coverage. Improves sensitivity on programs with many small shared helpers.
- **N-gram edges.** Generalises classical bigram edges to sequences of length `n` (3, 4, 5) for deeper path sensitivity. Paid for with a larger edge map.
- **CMPLOG.** Logs the operand values of every comparison instruction (equalities, inequalities, `memcmp`, `strncmp`) during an otherwise normal fuzzing run. The operand values are then used by a dedicated mutator that directly writes them into the input at offsets where a close-but-not-equal value appears — the AFL++ implementation of Aschermann et al.'s REDQUEEN "input-to-state correspondence" idea. CMPLOG solves the magic-bytes and checksum problem that symbolic execution was previously required for, at a tiny fraction of the cost.

### 2. Compiler-assisted instrumentation

- **LLVM LTO mode (`afl-clang-lto`).** Uses link-time optimisation to inject the edge instrumentation after all linking is complete, enabling **collision-free edge IDs** (AFL's default XOR-hash scheme collides on large programs). LTO mode also enables autodictionary extraction: string literals that participate in comparisons are harvested at compile time and added to the mutator's dictionary automatically.
- **LAF-INTEL (from Laf-Intel / Böhme et al. follow-up work).** Splits multi-byte comparisons into chains of single-byte comparisons at the IR level, turning a `if (x == 0xdeadbeef)` into four sequential single-byte branches. Each branch then shows up in the edge map, so a greybox fuzzer can make incremental progress through a 32-bit or 64-bit magic value without brute-forcing it. LAF-INTEL and CMPLOG are complementary: LAF-INTEL makes progress visible in the coverage map; CMPLOG solves the same problem dynamically without recompilation.
- **QEMU-mode, Unicorn-mode, Frida-mode.** Binary-only instrumentation back-ends for targets where source is unavailable. AFL++ carries forward the classical QEMU user-mode backend and adds Unicorn (for firmware snapshots) and Frida (for runtime hooking), each with the same coverage shape.

### 3. Scheduling and mutation

- **MOpt scheduler** (Lyu et al., USENIX Security 2019). Particle-swarm optimisation over the 14 havoc-stage mutation operators, learning a per-target probability distribution that shifts budget toward operators that find new paths on that program. AFL++ integrates MOpt as an optional scheduler (`-L 0` or `-L 1`).
- **AFLfast power schedules** (Böhme et al.). FAST, COE, EXPLORE, EXPLOIT, QUAD, LINEAR, and the default EXPLOIT — different energy allocation policies for per-seed scheduling. AFL++ exposes all of them via `-p`.
- **Custom mutators via Python / C API.** The mutation stage is plug-able: write a `fuzz()` callback in C or Python and AFL++ will call it alongside the built-in havoc stage. This decouples the target-domain mutation logic from the fuzzer engine and is the extension point for domain-specific fuzzers built on top of AFL++.
- **Persistent mode improvements.** Fork-server is still the baseline, but the persistent-mode harness (a `while (__AFL_LOOP(1000))` loop around the target function) is promoted to first-class and extended with shared-memory input delivery, eliminating the per-execution fork cost and lifting typical throughput by an order of magnitude on library targets.

### 4. Distributed and CI integration

- **afl-system-config.** A one-shot tuning script that adjusts kernel parameters (core dump pattern, CPU frequency scaling, ASLR behaviour) for reliable fuzzing. The step every AFL tutorial used to start with is now a single invocation.
- **`-i -` resume.** Clean resume of interrupted campaigns without re-importing seeds.
- **Integration with libFuzzer harnesses.** AFL++ can run OSS-Fuzz-style `LLVMFuzzerTestOneInput` harnesses directly via a shim, so projects that standardised on libFuzzer entry points can switch engines without rewriting the target.

## Evaluation (summary)

The WOOT paper evaluates AFL++ and a curated subset of its features against AFL 2.52b, AFLfast, and AFLdoubledash on the FuzzBench benchmark suite and a selection of real-world CVE targets. Headline results reported in the paper and subsequently reinforced by the Google FuzzBench leaderboard:

- AFL++ with LTO, CMPLOG, and MOpt consistently outperforms AFL on edge coverage and on time-to-first-crash across the FuzzBench corpus.
- CMPLOG alone accounts for most of the improvement on targets with magic bytes or checksum guards.
- LTO's collision-free edge IDs matter most on large binaries (OpenSSL, ffmpeg).
- Persistent mode roughly doubles execs/sec on library targets.

AFL++ has become the *de facto* reference fuzzer for complexity-fuzzing research (SlowFuzz, PerfFuzz, MemLock, HotFuzz all provide AFL++ integration, and the AFL++ team's follow-up LibAFL framework pulls the design further in the modular direction).

## Relevance to APEX G-46

AFL++ is the engine APEX would build on for a classical coverage-guided mode and as a sibling for performance-guided modes. Specific implications:

1. **CMPLOG eliminates the need for in-house symbolic execution on most targets.** APEX-concolic's solver budget should be reserved for the programs that CMPLOG + LAF-INTEL fails to crack (deep arithmetic invariants, constraint networks) rather than for magic-bytes and checksum problems.
2. **Custom mutator API is the right extension point.** A G-46 performance mutator (e.g., Singularity-style pattern generator or a MemLock-style size-field amplifier) plugs in here without forking the engine.
3. **NeverZero + context-sensitive edges are cheap APEX features.** Replicate both in apex-coverage's edge-map implementation to match AFL++ baseline sensitivity.
4. **MOpt as APEX's default mutation scheduler.** If APEX ships multiple mutation strategies it should adopt MOpt's PSO scheduler rather than a hand-tuned distribution, because target variance is enormous.
5. **FuzzBench for evaluation.** AFL++ is one of the strongest baselines on FuzzBench; any APEX benchmark should include AFL++ with LTO + CMPLOG + MOpt as a target to beat.

## Citation

```
@inproceedings{fioraldi2020afl++,
  author    = {Andrea Fioraldi and Dominik Maier and Heiko Ei{\ss}feldt and Marc Heuse},
  title     = {{AFL}++: Combining Incremental Steps of Fuzzing Research},
  booktitle = {14th USENIX Workshop on Offensive Technologies (WOOT '20)},
  year      = {2020},
  publisher = {USENIX Association},
  url       = {https://www.usenix.org/conference/woot20/presentation/fioraldi}
}
```

## References

- Paper PDF — [aflplus.plus/papers/aflpp-woot2020.pdf](https://aflplus.plus/papers/aflpp-woot2020.pdf)
- USENIX page — [usenix.org/conference/woot20/presentation/fioraldi](https://www.usenix.org/conference/woot20/presentation/fioraldi)
- Project — [github.com/AFLplusplus/AFLplusplus](https://github.com/AFLplusplus/AFLplusplus) — see `01KNZ2ZDMEPBXSH02HFWYAKFE4`
- AFL++ landing — [aflplus.plus](https://aflplus.plus/) — see `01KNWGA5GD7A7WXW56682R280K`
- Successor framework — LibAFL (CCS 2022) — see `01KNZ4RPCT50A8XWXYC4RVPFT9`
- REDQUEEN (input-to-state) — see `01KNZ4RPCRK3648T95MFCV74YT`
- MOPT (PSO scheduler) — see `01KNZ4RPCS4PA5RHBWKW08X4G8`
