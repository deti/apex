---
id: 01KNWE2QA700K0Z2W0TWV087JZ
title: LibAFL Feedback Architecture and Performance Feedback
type: concept
tags: [libafl, fuzzing, feedback, apex-fuzz, perffuzz]
links:
  - target: 01KNWE2Q9YZBAR140ZX5P36TQ5
    type: extends
  - target: 01KNWE2QA0Z52H8VVFAMSA7KGA
    type: related
  - target: 01KNWEGYB1B15QGYTRC374Z7DQ
    type: extends
  - target: 01KNWEGYB3NXWFB6D4SV4DTD5X
    type: extends
  - target: 01KNWGA5GD7A7WXW56682R280K
    type: related
created: 2026-04-10
modified: 2026-04-10
---

# LibAFL Feedback Architecture and Performance Feedback

LibAFL is the modular, library-based successor to AFL++, developed by Fioraldi, Maier, Zhang, and Balzarotti. APEX uses it as the substrate for its fuzzing pipeline (`crates/apex-fuzz/`). G-46 proposes swapping LibAFL's default coverage feedback for a resource-maximising feedback — this note describes the relevant architecture.

## LibAFL's core abstractions

LibAFL reifies the fuzzing loop as a composition of six first-class traits:

| Abstraction | Role | AFL++ analogue |
|---|---|---|
| `Input` | The mutation target (bytes, structured, grammar tree) | The testcase file |
| `Observer` | Consumes per-execution instrumentation data | Shared-memory coverage map |
| `Feedback` | Decides if an input is "interesting" given observers | `has_new_bits` |
| `Corpus` | Stores interesting inputs | `out/queue/` directory |
| `Scheduler` | Picks the next corpus entry to mutate | Queue cycling + weighting |
| `Stage` | A phase of per-entry work (mutation, trim, calibrate) | `fuzz_one` |

This clean separation is the reason resource-guided fuzzing is expressible as a drop-in replacement rather than a fork: you provide a new `Observer` that captures the resource signal and a new `Feedback` that interprets it.

## Coverage feedback (the default)

The default LibAFL feedback chain for AFL-style fuzzing is:

1. `StdMapObserver<u8>` — points at a shared-memory edge hit-count map populated by compile-time instrumentation (SanitizerCoverage, AFL trampolines).
2. `MaxMapFeedback` wrapped around that observer — maintains a historical max per map cell; declares interesting if any cell exceeds its historical max (AFL's classic bucketed-edge novelty test).
3. `CrashFeedback` / `TimeoutFeedback` — orthogonal feedbacks that detect oracles.

The `MaxMapFeedback` is the key piece. Replacing it (or composing it with something else) changes the fuzzing objective.

## Resource feedback implementations

### SlowFuzz-style (single scalar)

Easiest to implement:

```rust
struct ResourceObserver { total: u64 }
impl Observer for ResourceObserver { /* read from perf counter */ }

struct MaxScalarFeedback { best: u64 }
impl Feedback for MaxScalarFeedback {
    fn is_interesting(&mut self, _state, _manager, _input, observers, _exit_kind) -> bool {
        let r = observers.get::<ResourceObserver>()?.total;
        if r > self.best { self.best = r; true } else { false }
    }
}
```

Fast, deterministic, and easy to explain. But it collapses to a single peak and tends to get stuck there (SlowFuzz's known weakness).

### PerfFuzz-style (per-edge vector)

Harder but much more productive. Instead of a scalar, each observation is an edge-hit-count vector. The feedback keeps the per-edge historical max and marks an input interesting if `any i: new[i] > best[i]`.

This is exactly what `MaxMapFeedback<u64>` already does — **as long as you feed it a 64-bit counter map instead of an 8-bit bucketed map**. LibAFL supports both via generic parameters.

Concretely, to implement PerfFuzz on LibAFL:

1. Use edge instrumentation that produces **unsaturated counts** (not AFL's `log2` bucketing). SanitizerCoverage `-fsanitize-coverage=trace-pc-guard,inline-8bit-counters` works if you widen the counters to `u64` in a post-processing step, or use `-fsanitize-coverage=pc-table,inline-bool-flag` + a custom counter array.
2. Configure `StdMapObserver<u64>` with the extended map.
3. Use `MaxMapFeedback<u64>` with the observer.
4. Compose with the existing `CrashFeedback`/`TimeoutFeedback` for correctness oracles.

### Hybrid (per-edge + global scalar)

A more stable variant composes both: `any i: new[i] > best[i]` OR `sum(new) > best_sum`. This keeps the corpus rich (PerfFuzz) and never loses a strictly better total (SlowFuzz). LibAFL `EagerOrFeedback` / `CombinedFeedback` compose arbitrary feedbacks.

## Cost-signal choices

Which resource to maximise?

| Signal | Pros | Cons |
|---|---|---|
| Instructions retired (hardware perf counter) | Deterministic, low overhead, monotonic | Requires `perf_event_open`; not available in some CI sandboxes |
| Basic-block executions (instrumented counter) | Deterministic, portable | Overhead ~10–30%; dominated by hot loops |
| Wall-clock time | Matches user experience | Noisy; needs statistics; distorted by coexisting load |
| Bytes allocated | Catches memory-complexity bugs | Needs allocator hook (jemalloc, mimalloc) |
| Peak RSS | Catches space complexity | Discretised by page allocator; noisy |

The spec recommends **instruction count** (deterministic) as the primary fuzzing signal, with wall-clock verification only for final "champion" candidates.

## Termination and convergence

A resource fuzzer is not trying to cover a graph; it's trying to find a maximum. Sensible termination criteria:

- **Fixed duration** (G-46 default: 5 minutes).
- **Stagnation** — N iterations without improvement of the best scalar.
- **SLO breach** — a single input exceeds the declared threshold; stop and report.
- **Budget exhaustion** — CPU-seconds limit (important in CI).

## Integration with APEX

APEX's `apex-fuzz` crate already wraps LibAFL with grammar-aware mutation and multi-strategy orchestration. G-46 integration points:

- New `ResourceFeedback` module alongside existing `CoverageFeedback`.
- New CLI flag `apex perf --target X --duration 5m --max-signal instructions` that selects the resource feedback instead of the coverage feedback.
- Output: the champion input, its measured resource consumption, and (if complexity estimation runs too) an asymptotic class.

## References

- Fioraldi, Maier, Zhang, Balzarotti — "LibAFL: A Framework to Build Modular and Reusable Fuzzers" — CCS 2022 — [aflplus.plus/libafl-paper.pdf](https://aflplus.plus/libafl-paper.pdf)
- LibAFL source — [github.com/AFLplusplus/LibAFL](https://github.com/AFLplusplus/LibAFL)
- LibAFL book — [aflplus.plus/libafl-book](https://aflplus.plus/libafl-book/)
- Petsios et al. — SlowFuzz — CCS 2017
- Lemieux et al. — PerfFuzz — ISSTA 2018
