---
id: 01KNZ2ZDMXYG1HE0S46R66QFA9
title: "PerfFuzz Repository (carolemieux/perffuzz)"
type: literature
tags: [perffuzz, fuzzing, llvm, afl, insertion-sort, benchmark]
links:
  - target: 01KNWEGYB3NXWFB6D4SV4DTD5X
    type: extends
  - target: 01KNWEGYB1B15QGYTRC374Z7DQ
    type: related
  - target: 01KNZ2ZDMEPBXSH02HFWYAKFE4
    type: related
created: 2026-04-12
modified: 2026-04-12
source: "https://github.com/carolemieux/perffuzz"
---

# PerfFuzz — Source Repository

*Source: https://github.com/carolemieux/perffuzz (README) — fetched 2026-04-12.*
*Author: Caroline Lemieux (UC Berkeley). This is the artifact accompanying Lemieux, Padhye, Sen, Song — ISSTA 2018.*

## What the repo contains

The repository is a **fork of AFL (Google's American Fuzzy Lop)** at roughly the 2017 version, with the following PerfFuzz-specific modifications:

- An additional instrumentation that records **per-edge execution counts** (not just binary coverage).
- A modified coverage feedback loop that retains a test case if it maximises **any** edge's execution count, not just if it discovers a new edge.
- A new output file suffix `+max` for saved inputs that represent a per-edge maximum.
- A helper tool `afl-showmax` for querying maximum execution counts on a given input.
- A new AFL command-line flag `-p` that switches the fuzzer into PerfFuzz mode.
- A new flag `-N` that caps input file size (essential to prevent the fuzzer from simply making huge inputs).

The modifications are relatively small — the README implies the delta from upstream AFL is on the order of a few hundred lines.

## Build

```bash
make                            # builds the main afl-fuzz binary
cd llvm_mode && make && cd ..   # builds the LLVM instrumentation
```

Requires `clang-3.8.0` on Linux or clang 8 on macOS. The original AFL is Linux-only for production; PerfFuzz follows this constraint.

## Running on the insertion-sort benchmark

The repo ships one example: `insertion-sort.c`, a plain O(n²) insertion sort over a byte buffer. The PerfFuzz demo is:

```bash
./afl-clang-fast insertion-sort.c -o isort
./afl-fuzz -p -N 64 -i in -o out -- ./isort @@
```

- `-p` enables PerfFuzz mode.
- `-N 64` caps input size at 64 bytes.
- `-i in` is the seed corpus directory (usually one file: `aa\n`).
- `-o out` is the output directory. `out/queue/` accumulates test cases; those with `+max` in the filename are per-edge maxima.

After 5-15 minutes PerfFuzz converges on a **reverse-sorted 64-byte input** — the pathological worst case for insertion sort. The paper reports that on the insertion-sort benchmark, PerfFuzz finds the O(n²) worst case **5-69× faster** than SlowFuzz (whose single-dim feedback gets stuck on a local optimum).

## Usage flags specific to PerfFuzz

| Flag | Effect |
|---|---|
| `-p` | Enable PerfFuzz mode (multi-dim edge-count feedback). |
| `-N N` | Cap test case size at N bytes. **Essential**; without it the fuzzer "cheats" by making inputs bigger rather than more pathological. |
| `-d` | Fidgety mode — skips some deterministic stages, usually faster on large targets. |
| `afl-showmax` | Report maximum per-edge execution count on a given input. Useful for ranking saved queue entries. |

## The "+max" file convention

In the output queue, PerfFuzz uses a filename suffix to mark saved inputs:

- `id:000123,src:000098,op:havoc+max:edge-id-4712` — this input is the current maximum-execution-count input for edge 4712.

One input can be the maximum for multiple edges simultaneously (its name will list them). The corpus retention policy keeps an input alive as long as it is the maximum for at least one edge; this is the "multi-dim" feedback mechanism.

## Limitations acknowledged in the README

- **LLVM-only.** No binary-only support (unlike AFL++'s QEMU/Frida modes). You must be able to recompile the target.
- **C/C++ only.** No JVM/CLR/Python/JS instrumentation.
- **Research prototype.** No released versions, no CI, not maintained at production quality.
- **Single-machine.** No parallel fuzzing across cores or nodes.
- **Not integrated with AFL++ upstream.** The PerfFuzz ideas are implemented more modularly in LibAFL now (via a `MaxMapFeedback` with per-edge counts), so the original repo is mostly of historical interest.

## The seed corpus matters a lot

Because PerfFuzz's feedback rewards inputs that increase *any* edge count, a poor seed can lead it down unproductive paths. The recommended seed for most targets is a **small valid input** — as small as you can construct something that exercises the target meaningfully. Tiny seeds let PerfFuzz explore the space of sizes up to the `-N` cap without getting stuck on a pre-existing large-but-mediocre seed.

## Performance relative to SlowFuzz

The paper's headline number is **5-69×** speedup over SlowFuzz on per-edge maximum discovery for the insertion sort, bzip2 decompression, LZMA, and PCRE regex benchmarks. The source of the speedup:

- SlowFuzz maximises a **single** global count (typically total instructions).
- PerfFuzz maximises **every** per-edge count simultaneously.
- Pathological inputs typically saturate *one specific edge*, not the global sum. Single-dim feedback has no gradient towards saturating edge X if edge Y's count is what drives the global sum up. Multi-dim feedback retains inputs that saturate each edge individually, so the search surface is convex in each edge's direction.

## Relevance to APEX G-46

1. **This is the reference implementation of multi-dim perf feedback.** The algorithm is the baseline APEX's performance fuzzer should match or beat. The LibAFL equivalent (`MaxMapFeedback` with per-edge counts) is already present and reusable.
2. **The insertion-sort benchmark is a smoke test.** A G-46 perf fuzzer that can't rediscover a reverse-sorted input for an O(n²) insertion sort in under 15 minutes is broken; this benchmark takes ~5 minutes on the reference PerfFuzz. It's a trivial regression test.
3. **Corpus size cap is essential.** Without an analogue of `-N`, resource-guided fuzzing degenerates into "make the input bigger". APEX's fuzzer must have an input-size constraint per benchmark, declared explicitly or derived from the SLO shape (e.g., "parse 10KB in 100ms" means the fuzzer explores the 0-10KB range).
4. **`+max` naming convention is a clean output format.** APEX's perf fuzzer should save worst-case inputs with an analogous convention that records *which* metric (edge count, total time, allocations) the input maximised. This lets downstream tools rank and de-duplicate.
5. **The research-prototype quality is a feature, not a bug**, for this sort of artifact. The value is the algorithm, not the code. APEX doesn't need to run PerfFuzz; it needs to re-implement PerfFuzz-style feedback in LibAFL's idiom.

## References

- PerfFuzz repo — [github.com/carolemieux/perffuzz](https://github.com/carolemieux/perffuzz)
- Lemieux, Padhye, Sen, Song — "PerfFuzz: Automatically Generating Pathological Inputs" — ISSTA 2018 — `01KNWEGYB3NXWFB6D4SV4DTD5X`
- SlowFuzz (the paper PerfFuzz benchmarks against) — `01KNWEGYB1B15QGYTRC374Z7DQ`
- LibAFL (modular refactor of this idea) — `01KNWGA5GAWV1Y9D6N11D2TP20`
