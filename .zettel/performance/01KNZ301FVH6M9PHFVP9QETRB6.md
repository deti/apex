---
id: 01KNZ301FVH6M9PHFVP9QETRB6
title: "Tool: libFuzzer (LLVM in-process coverage-guided fuzzing)"
type: reference
tags: [tool, fuzzing, libfuzzer, llvm, coverage, memory-limits, performance]
links:
  - target: 01KNWE2Q9YZBAR140ZX5P36TQ5
    type: references
  - target: 01KNWE2QA0Z52H8VVFAMSA7KGA
    type: extends
  - target: 01KNWGA5GD7A7WXW56682R280K
    type: related
  - target: 01KNWGA5GFHMDSYKRHEE5BJXKJ
    type: related
  - target: 01KNWEGYB3NXWFB6D4SV4DTD5X
    type: related
  - target: 01KNZ301FVNJ1JA9TKGG46472T
    type: related
created: 2026-04-12
modified: 2026-04-12
source: "https://llvm.org/docs/LibFuzzer.html"
license: "Apache-2.0 with LLVM exceptions"
---

# Tool: libFuzzer (LLVM in-process coverage-guided fuzzing)

**Documentation:** https://llvm.org/docs/LibFuzzer.html
**Source:** https://github.com/llvm/llvm-project/tree/main/compiler-rt/lib/fuzzer
**License:** Apache-2.0 with LLVM exceptions

## What it is

libFuzzer is an in-process, coverage-guided, evolutionary fuzzing engine that ships with LLVM's Clang toolchain. Unlike AFL which forks a new process for every test case, libFuzzer links directly into the target and runs the fuzzing loop inside a single long-lived process. This eliminates fork overhead and makes it one of the fastest general-purpose fuzzers available — iteration rates of 10^5–10^6 executions per second per core are routine for small in-memory targets.

libFuzzer is the reference fuzzer for OSS-Fuzz, Google's continuous fuzzing service for open source projects, and is widely used as a first-class member of SanitizerCoverage toolchains alongside AFL and honggfuzz.

## Execution model

The fuzz target is a single C/C++ function:

```c
extern "C" int LLVMFuzzerTestOneInput(const uint8_t* data, size_t size);
```

At link time, libFuzzer supplies its own `main`. At runtime, the harness maintains a corpus on disk and a working queue in memory; each iteration picks a corpus entry, applies mutation, calls `LLVMFuzzerTestOneInput`, collects coverage feedback from SanitizerCoverage-instrumented edges, and decides whether to retain the new input based on new coverage or new interesting value-profile observations. Every crash (sanitizer report or signal) is reported and the minimized crashing input is written to disk.

## Performance-relevant options

The following flags are documented in the LLVM libFuzzer reference and are directly relevant to performance test generation:

- **`-max_total_time=<seconds>`** — stop fuzzing after this many seconds of wall time. Default 0 (unbounded). This is the canonical time budget knob for APEX-style timeboxed runs.
- **`-max_len=<bytes>`** — cap on the length of generated inputs. Default 0, in which case libFuzzer auto-picks based on the corpus. For performance fuzzing, setting `-max_len` low (e.g. 128 B) concentrates budget on short inputs and is often the best heuristic for finding ReDoS and pathological small-input algorithmic complexity bugs.
- **`-rss_limit_mb=<mb>`** — terminate the run (and report a memory OOM) if resident set size exceeds this in MB. Default 2048. Setting to 0 disables. This is the direct feedback channel for memory-side DoS testing: drop it to e.g. 64 MB to convert any input that drives the target past that threshold into a reportable "slow-input / OOM" crash.
- **`-malloc_limit_mb=<mb>`** — trip on any single allocation larger than this. Catches CVE-2021-21419-style "one big malloc" bugs even if total RSS stays small.
- **`-timeout=<seconds>`** — treat any input that runs longer than this many seconds as a failure. Default 1200. For performance testing, drop this to 1 or even 100 ms: any input that takes longer than that is a worst-case candidate and should be reported as a timeout crash.
- **`-timeout_exitcode=<n>`** — exit code used when `-timeout` trips. Default 77. Lets upstream orchestration distinguish timeout crashes from memory and assertion crashes.
- **`-use_value_profile=1`** — enables libFuzzer's value profile, which hashes operands of comparison instructions into the coverage signal. This lets the fuzzer discover "magic constants" (length fields, sentinels, checksums) and satisfy guards that pure edge coverage cannot. The docs note this can increase corpus size and impose up to 2× slowdown; for most targets the hit rate improvement is worth it.
- **`-reduce_inputs=1`** (default) — after discovering a new input, try to minimize it while preserving its coverage signature.
- **`-detect_leaks=1`** — catch memory leaks via LeakSanitizer integration; indirectly useful for long-running-process memory-exhaustion bugs.

## Performance caveats from the docs

The LLVM documentation explicitly warns that libFuzzer is tuned for targets where each input completes in **under ~10 ms**, and that targets with cubic or worse algorithmic complexity should be avoided *or* explicitly bounded with `-timeout`. This is a subtle but important note for G-46: libFuzzer's own feedback loop assumes a fast target. To repurpose it for finding slow inputs, you set `-timeout` very low so that "takes longer than the threshold" becomes a reportable event. Otherwise the fuzzer will happily run a quadratic input for 20 minutes and quietly continue, wasting the budget.

This is precisely the motivation for the extension libraries (SlowFuzz, PerfFuzz, MemLock, Badger) that add *non-coverage* feedback channels to the coverage-guided core — standard libFuzzer retains an input only if it hits new edges, not if it is merely expensive.

## Integration with sanitizers

libFuzzer composes with AddressSanitizer (ASan), UndefinedBehaviorSanitizer (UBSan), MemorySanitizer (MSan), ThreadSanitizer (TSan), and LeakSanitizer (LSan). The standard compile line is:

```
clang -g -O1 -fsanitize=fuzzer,address -o fuzz_target fuzz_target.cc
```

Adding `-fsanitize=fuzzer` enables SanitizerCoverage edge tracking and links in libFuzzer's `main`.

## Relevance to APEX G-46

libFuzzer plays three roles in a G-46 stack:

1. **Backbone for extensions.** Both SlowFuzz and MemLock are implemented as libFuzzer/AFL forks. Any APEX implementation that ships a dedicated performance fuzzer will need to either extend libFuzzer's mutation loop or interoperate with its corpus format.
2. **Existing harness reuse.** The C/C++ ecosystem has thousands of pre-written `LLVMFuzzerTestOneInput` harnesses (OSS-Fuzz alone hosts thousands). An APEX G-46 integration that can consume existing libFuzzer harnesses unchanged — and simply re-run them under a cost-aware feedback loop — inherits this enormous corpus for free.
3. **Timebox + RSS cap as cheap DoS oracle.** Even without a custom feedback channel, a stock libFuzzer run with aggressive `-timeout=1 -rss_limit_mb=64` will catch blatant DoS regressions. This is a cheap "tier-0" G-46 check that APEX can offer to users before they commit to a full pattern synthesizer.
