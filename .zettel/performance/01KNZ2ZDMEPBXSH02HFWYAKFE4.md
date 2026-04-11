---
id: 01KNZ2ZDMEPBXSH02HFWYAKFE4
title: "AFL++: Community Fuzzer Superset of AFL (README)"
type: literature
tags: [aflplusplus, fuzzing, afl, libafl, qemu, mopt, redqueen, laf-intel]
links:
  - target: 01KNWGA5GD7A7WXW56682R280K
    type: extends
  - target: 01KNWGA5GAWV1Y9D6N11D2TP20
    type: related
  - target: 01KNWE2QA700K0Z2W0TWV087JZ
    type: references
created: 2026-04-12
modified: 2026-04-12
source: "https://github.com/AFLplusplus/AFLplusplus"
---

# AFL++ — Community Fuzzer Superset of AFL

*Source: https://github.com/AFLplusplus/AFLplusplus (README) — fetched 2026-04-12.*
*Maintainers: Andrea Fioraldi, Dominik Maier, Heiko Eißfeldt, Marc Heuse, et al.*

## What it is

AFL++ is a fork of Google's AFL (American Fuzzy Lop) that integrates years of academic fuzzing research and community patches into a single production-quality tool. Google discontinued active development on AFL in 2020; AFL++ is the community-maintained successor and is the de-facto default coverage-guided fuzzer for 2020+ C/C++ projects.

The project's tagline: **"more speed, more and better mutations, more and better instrumentation, custom module support"**.

## Headline features over original AFL

### Performance
- **Persistent mode** ported to all instrumentation backends (~10× faster than fork-exec)
- **Parallel fuzzing** improvements and better CPU binding
- **Faster forkserver** with reduced overhead per exec
- **Deferred forkserver** pattern for heavy libraries

### Mutations
- **MOpt mutator** (Lyu et al., USENIX Security 2019) — a particle-swarm-optimisation-guided mutator that adapts mutation probabilities at runtime.
- **Custom mutators** via a C API and Python bindings, allowing language-specific mutation strategies (JSON-aware, PCAP-aware, LISP-aware, etc.).
- **Grammar-aware mutators** — support for Nautilus, Grammatron, and custom grammar harnesses.
- **Stacked mutations** — combine multiple mutation kinds per test case for deeper exploration.

### Instrumentation
- **LAF-Intel** (Laurent, Kordian 2016) — the "divide-and-conquer" transform that splits multi-byte comparisons into chains of 1-byte comparisons so the coverage feedback can track progress through a magic-value check (e.g., `if (x == 0xCAFEBABE)` compiles to four byte-compare edges).
- **Redqueen / cmplog** (Aschermann et al., NDSS 2019) — input-to-state correspondence: records operand values at comparison sites during execution, then replaces matching byte sequences in the input to progress through conditionals without bruteforce.
- **Collision-free coverage** — an improved edge map that avoids the hash collisions in AFL's original 64K map.
- **CompCov** — byte-level coverage for comparisons, making magic-byte discovery tractable.
- **Context-sensitive coverage** — call-stack hashed into the edge ID.

### Target support
- **Source-available targets** — `afl-clang-fast` and `afl-clang-fast++` for LLVM-based instrumentation.
- **Binary-only targets** via:
  - **QEMU mode** (QEMU 5.1-based) — full-system or user-mode emulation.
  - **Frida mode** — dynamic instrumentation via Frida Gum, for macOS/iOS/Android and targets where QEMU is awkward.
  - **Unicorn mode** — CPU emulation only, for embedded firmware where no OS is available.
- **Networked services** — `afl-persistent-config` and preeny/desock_multi for wrapping a network server into a persistent fuzzing harness.
- **Multi-target persistent mode** — specific features for fuzzing multiple entry points in one binary.

### Scheduling
- **AFLfast++** power schedules — `fast`, `coe`, `explore`, `quad`, `lin`, `exploit`. Prioritise test cases by a power function of their execution count; inputs in rare paths get more mutation time.

## AFL++ vs LibAFL

AFL++ is the **production fuzzer** — a monolithic tool you can point at a target and run. LibAFL (also from the AFL++ team, see `01KNWGA5GAWV1Y9D6N11D2TP20`) is the **fuzzer construction framework** — a Rust library from which you compose a custom fuzzer. Design-wise, LibAFL's objects (Observer, Feedback, Scheduler, Stage) generalise the AFL++ loop; AFL++ could in principle be reimplemented on LibAFL, and some components are.

The team published "LibAFL: A Framework to Build Modular and Reusable Fuzzers" at CCS 2022 — that paper is the theoretical treatment of the split.

## Performance numbers (from community benchmarks)

- On LAVA-M (standard fuzzing benchmark): AFL++ finds all 4 CVEs in `base64`, `md5sum`, `uniq`, `who` within 24 hours, where vanilla AFL typically finds 1-2.
- On the FuzzBench suite (Google): AFL++ ranks consistently in the top 3 of ~15 fuzzers on bug-discovery metrics.
- Persistent mode over fork-exec: 10-50x speedup for small target functions.

## Relevance to APEX G-46

1. **LibAFL is APEX's substrate; AFL++ is the feature reference.** Anything AFL++ ships that is relevant to performance fuzzing is likely already present as a LibAFL component. In particular:
   - **MOpt** mutator template generalises to resource-optimisation mutators.
   - **LAF-Intel** transform is essential for making PerfFuzz-style feedback useful on real-world parsers — without it, the fuzzer wastes huge effort guessing magic bytes that dominate the perf profile.
   - **Redqueen / cmplog** is essential on binary targets where symbolic information is absent.
2. **Binary-only support via QEMU/Frida.** APEX G-46 should support performance fuzzing of a closed-source binary — a common realistic setup for customers who don't own the target. AFL++ already has this; LibAFL's `QemuExecutor` inherits it.
3. **AFLfast++ power schedules as a template for APEX's "exploit" phase.** Once APEX finds a slow input, the scheduler should prioritise mutations around it — exactly the `exploit` power schedule.
4. **Grammar-aware mutation.** For performance fuzzing, structural inputs matter disproportionately — a JSON parser that's quadratic on deeply-nested objects needs structured JSON inputs. AFL++'s Nautilus and Grammatron integrations are the state of the art.

## References

- AFL++ — [github.com/AFLplusplus/AFLplusplus](https://github.com/AFLplusplus/AFLplusplus)
- Fioraldi et al. — "AFL++: Combining Incremental Steps of Fuzzing Research" — WOOT 2020
- Fioraldi, Maier, Zhang, Balzarotti — "LibAFL: A Framework to Build Modular and Reusable Fuzzers" — CCS 2022
- Aschermann et al. — "REDQUEEN: Fuzzing with Input-to-State Correspondence" — NDSS 2019
- Lyu, Ji, Zhang, Li, Wu, Beyah, Yu — "MOPT: Optimized Mutation Scheduling for Fuzzers" — USENIX Security 2019
