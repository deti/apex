---
id: 01KNWGA5GAWV1Y9D6N11D2TP20
title: "Tool: LibAFL (Advanced Fuzzing Library)"
type: literature
tags: [tool, libafl, fuzzing, rust, apex-fuzz]
links:
  - target: 01KNWE2QA0Z52H8VVFAMSA7KGA
    type: related
  - target: 01KNWE2QA700K0Z2W0TWV087JZ
    type: related
  - target: 01KNWGA5GD7A7WXW56682R280K
    type: related
  - target: 01KNZ4RPCT50A8XWXYC4RVPFT9
    type: references
created: 2026-04-10
modified: 2026-04-12
source: "https://github.com/AFLplusplus/LibAFL"
---

# LibAFL — Advanced Fuzzing Library

*Source: https://github.com/AFLplusplus/LibAFL — fetched 2026-04-10.*

## Project Description

LibAFL is an "Advanced Fuzzing Library" that enables developers to "slot your own fuzzers together and extend their features using Rust." It is a collection of reusable fuzzer components offering customisable fuzzing capabilities with industrial-strength features.

## Key Features

1. **Performance** — "We do everything we can at compile time, keeping runtime overhead minimal. Users reach 120k execs/sec in frida-mode on a phone (using all cores)."
2. **Scalability** — Low Level Message Passing (LLMP) enables "almost linear" scaling across CPU cores and multiple machines via TCP.
3. **Modularity** — components like `BytesInput` are replaceable, supporting custom input formats for structured fuzzing.
4. **Cross-platform** — runs on Windows, macOS, iOS, Linux, and Android; supports `no_std` mode for embedded systems and hypervisors.
5. **Instrumentation flexibility** — supports binary-only modes (Frida), source-based compilation passes, and custom backends.

## Supported Instrumentation Backends

- SanitizerCoverage
- Frida (binary-only dynamic instrumentation)
- QEMU (user and system modes with emulation hooks)
- TinyInst

## Setup Requirements

- Rust (install directly, not via distro packages)
- LLVM tools (version 15.0.0 – 18.1.3)
- `just` build tool

## Example Usage

The project provides example fuzzers in `./fuzzers/`, with the best-tested being `libfuzzer_libpng` — "a multicore libfuzzer-like fuzzer using LibAFL for a libpng harness."

## Citation

For academic work, cite: Fioraldi, Maier, Zhang, and Balzarotti — "LibAFL: A Framework to Build Modular and Reusable Fuzzers" — CCS 2022.

## License

Dual-licensed under Apache 2.0 or MIT at the user's option.

## Relevance to APEX G-46

LibAFL is the fuzzing substrate that `crates/apex-fuzz/` builds on. Its modular design — where `Observer`, `Feedback`, `Corpus`, `Scheduler`, and `Stage` are first-class traits — is the reason the G-46 "resource feedback instead of coverage feedback" plan is a **substitution**, not a rewrite. The 120k execs/sec figure sets a realistic ceiling: a resource-guided fuzzer that adds PerfFuzz-style per-edge tracking should still land in the same order of magnitude, because the feedback is computed from the same shared-memory map.

The `no_std` and multi-platform support means APEX's resource fuzzer can target embedded Python/MicroPython, JavaScript-engine targets, or even kernel-space modules if the instrumentation surface exposes the necessary counters.
