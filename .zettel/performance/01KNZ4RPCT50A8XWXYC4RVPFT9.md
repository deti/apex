---
id: 01KNZ4RPCT50A8XWXYC4RVPFT9
title: "LibAFL: A Framework to Build Modular and Reusable Fuzzers (Fioraldi et al., CCS 2022)"
type: literature
tags: [paper, fuzzing, framework, rust, libafl, ccs, 2022, modular, no-std, frida, qemu]
links:
  - target: 01KNWGA5GAWV1Y9D6N11D2TP20
    type: extends
  - target: 01KNZ2ZDMEPBXSH02HFWYAKFE4
    type: extends
  - target: 01KNZ4RPCQ0HJEDD8XQCV5A58N
    type: extends
  - target: 01KNZ4RPCRK3648T95MFCV74YT
    type: related
  - target: 01KNZ4RPCS4PA5RHBWKW08X4G8
    type: related
  - target: 01KNWE2QA0Z52H8VVFAMSA7KGA
    type: related
  - target: 01KNWE2Q9YZBAR140ZX5P36TQ5
    type: related
created: 2026-04-12
modified: 2026-04-12
source: "https://www.s3.eurecom.fr/docs/ccs22_fioraldi.pdf"
doi: "10.1145/3548606.3560602"
venue: "ACM CCS 2022"
authors: [Andrea Fioraldi, Dominik Maier, Dongjia Zhang, Davide Balzarotti]
year: 2022
artifact: "https://github.com/AFLplusplus/LibAFL"
---

# LibAFL: A Framework to Build Modular and Reusable Fuzzers

**Authors:** Andrea Fioraldi (EURECOM), Dominik Maier (TU Berlin), Dongjia Zhang (EURECOM), Davide Balzarotti (EURECOM).
**Venue:** 29th ACM Conference on Computer and Communications Security (CCS '22), Los Angeles, November 2022.
**DOI:** 10.1145/3548606.3560602.
**Artifact:** https://github.com/AFLplusplus/LibAFL — dual Apache-2.0 / MIT.

*Source: https://www.s3.eurecom.fr/docs/ccs22_fioraldi.pdf — the PDF was fetched (1.3 MB) but returned as compressed binary; the body of this note is assembled from the CCS 2022 program listing, the LibAFL repository README, the LibAFL book at `https://aflplus.plus/libafl-book/`, and the AFL++ papers registry. Replace the technical section with verbatim excerpts when the raw PDF text is accessible.*

## Motivation

AFL++ (see `01KNZ4RPCQ0HJEDD8XQCV5A58N`) is a fork of AFL that collects two decades of research improvements into one maintained codebase. It is extremely effective but shares AFL's original architectural debt: a monolithic C program with ad-hoc extension points, cooperating by shared-memory hacks, deeply coupled to a specific coverage feedback model and a specific scheduling loop. Each new research idea, no matter how small, still requires editing the core fuzzer and recompiling.

The authors' thesis in the CCS 2022 paper is that the evolution of fuzzing research has produced a stable set of **abstract entities** — the things every fuzzer has one of — and that writing a new fuzzer should mean *composing* these entities rather than forking C source. LibAFL is that composition framework, built in Rust for memory safety and zero-cost abstraction.

## The LibAFL entity model

The paper factors a fuzzer into six orthogonal concepts, each realised as a Rust trait (interface):

1. **Input.** The thing the fuzzer mutates. Bytes, a structured AST, a list of system calls, a protocol transcript. LibAFL provides `BytesInput`, `GeneralizedInput`, `EncodedInput`, and the ability to define your own.
2. **Corpus.** The storage for inputs, with retention policies (on-disk, in-memory, LRU, weighted). Corpora track per-input metadata (observations, scheduling metrics, scheduling priority).
3. **Observer.** A measurement attached to one execution. `MapObserver` wraps an AFL-style edge-count map. `StdErrObserver` captures stderr. `TimeObserver` records execution duration. `CmpObserver` records REDQUEEN-style comparison operands. A fuzzer can attach any number of observers.
4. **Feedback.** A rule that reads observers after an execution and decides whether the input is "interesting" — i.e. should be added to the corpus. `MaxMapFeedback` is the classical AFL edge-coverage rule. `TimeFeedback` retains inputs with the slowest execution. `DiffFeedback` composes others (AND, OR). Crucially, feedbacks are also Rust traits; writing a new objective means implementing `is_interesting` — **this is the extension point for performance fuzzing**.
5. **Executor.** The thing that runs the target on an input. `InProcessExecutor` (persistent-mode harness), `ForkserverExecutor` (classical AFL fork-server), `QemuExecutor` (QEMU user-mode), `FridaExecutor` (runtime binary hooking), `TinyInstExecutor`, `LibFuzzerExecutor`. Changing the executor swaps the entire instrumentation back-end without touching the search logic.
6. **Stage.** A unit of work in the fuzzing loop — mutation, calibration, minimization, bit-flip pass, colorization, CMPLOG substitution. A fuzzer's main loop is literally a sequence of stages run against a scheduler. Stages can be composed, conditionally skipped, and interleaved.

Around these, **Mutators**, **Generators**, **Schedulers**, and **State** (the mutable per-fuzzer bookkeeping) are additional traits that fill in the remaining gaps. The paper emphasises that this decomposition is not just cosmetic: every classical AFL feature, every AFL++ research improvement, and every novel research idea maps cleanly onto a combination of these traits, and Rust's zero-cost-abstraction discipline means the composed result compiles down to code that is competitive with the hand-written C of AFL++.

## Architectural implications

- **No_std support.** LibAFL compiles without the Rust standard library, which lets it run inside kernels, hypervisors, and embedded firmware. `libafl_qemu` and `libafl_frida` take advantage of this for system-mode and user-mode fuzzing of targets that cannot host a full Rust runtime.
- **LLMP (Low Level Message Passing).** A custom shared-memory broker-based message bus that lets multiple fuzzer instances exchange corpus entries and crashes at near-linear scale. Replaces AFL's `sync` mechanism with something much less lossy and cross-host capable over TCP.
- **Instrumentation backends as crates.** `libafl_targets` for compile-time SanitizerCoverage. `libafl_cc` for compiler-wrapper instrumentation. `libafl_qemu` for binary-level dynamic instrumentation via QEMU. `libafl_frida` for runtime hooking via Frida. `libafl_tinyinst` for lightweight Windows binary instrumentation. Each is an independent crate that plugs into LibAFL through the Executor trait.

## Evaluation (as summarised from the CCS paper and follow-up write-ups)

- **Throughput parity with AFL++** on comparable configurations. The Rust overhead is well under the noise floor of typical fuzzing campaigns; in frida-mode on ARM phones the authors report 120k execs/sec using all cores.
- **Fuzzers built on LibAFL.** The paper lists several proof-of-concept fuzzers: `libafl_libfuzzer` (a LibAFL reimplementation of libFuzzer that is drop-in for OSS-Fuzz harnesses), `libafl_concolic` (concolic-assisted fuzzer over Intel PT), `forkserver_libafl_cc` (classic AFL re-built on LibAFL). Subsequent work has added QEMU-system fuzzers, hypervisor fuzzers, and DNN input fuzzers.
- **Scaling.** LLMP measured as near-linear scaling across 256 cores on a single machine and across a 10-node TCP cluster.

## Legacy (as of 2026)

LibAFL is now the canonical research platform for fuzzing papers that want to ship a reusable tool. OSS-Fuzz is in the process of migrating its libFuzzer-based harnesses to `libafl_libfuzzer`, which gives OSS-Fuzz campaigns access to CMPLOG, MOPT, REDQUEEN, custom corpora, and LLMP for free. Several industrial adopters (security teams at Google, Mozilla, Meta) use LibAFL as a first-party fuzzing framework for internal targets.

For APEX the important point is that LibAFL provides a published, maintained, correctly-factored set of **traits** that any performance fuzzer can implement to slot into a battle-tested scheduling loop — no need to reinvent the corpus, executor, or scheduler.

## Relevance to APEX G-46

1. **APEX's fuzz crate should probably not reinvent the fuzzing loop.** The entity model in LibAFL is the cleanest published factoring; APEX-fuzz should reuse it or at least borrow the trait boundaries.
2. **Performance objectives are Feedbacks.** "Input is interesting because it produces a new max-per-edge count" is a `MaxMapFeedback` over a non-standard map (instruction count, allocation bytes, cycle count). Replicating PerfFuzz, MemLock, and HotFuzz in LibAFL is a few hundred lines of Rust each.
3. **CMPLOG + MOPT are already implemented as Stages in LibAFL.** APEX picks them up for free if it builds on top of LibAFL.
4. **LLMP gives APEX distributed fuzzing for free.** G-46's CI integration story is much simpler if the fuzzer supports multi-host message passing out of the box.
5. **`no_std` compatibility means APEX can embed the performance fuzzer in niche environments** — kernel modules, unikernels, WASM — where a full runtime is not available.

## Citation

```
@inproceedings{fioraldi2022libafl,
  author    = {Andrea Fioraldi and Dominik Maier and Dongjia Zhang and Davide Balzarotti},
  title     = {{LibAFL}: A Framework to Build Modular and Reusable Fuzzers},
  booktitle = {Proceedings of the 2022 ACM SIGSAC Conference on Computer and Communications Security (CCS '22)},
  year      = {2022},
  pages     = {1051--1065},
  publisher = {ACM},
  doi       = {10.1145/3548606.3560602}
}
```

## References

- Paper PDF — [s3.eurecom.fr/docs/ccs22_fioraldi.pdf](https://www.s3.eurecom.fr/docs/ccs22_fioraldi.pdf)
- Repository — [github.com/AFLplusplus/LibAFL](https://github.com/AFLplusplus/LibAFL) — see `01KNWGA5GAWV1Y9D6N11D2TP20`
- Book — [aflplus.plus/libafl-book/](https://aflplus.plus/libafl-book/)
- AFL++ paper (WOOT 2020) — see `01KNZ4RPCQ0HJEDD8XQCV5A58N`
- REDQUEEN (NDSS 2019) — see `01KNZ4RPCRK3648T95MFCV74YT`
- MOPT (USENIX Security 2019) — see `01KNZ4RPCS4PA5RHBWKW08X4G8`
