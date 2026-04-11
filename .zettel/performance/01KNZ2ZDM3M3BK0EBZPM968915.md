---
id: 01KNZ2ZDM3M3BK0EBZPM968915
title: "HotFuzz Paper: Discovering Java AC Vulnerabilities via Micro-Fuzzing (Blair et al., NDSS 2020)"
type: literature
tags: [hotfuzz, java, ndss-2020, micro-fuzzing, algorithmic-complexity, small-step-semantics]
links:
  - target: 01KNWEGYB6AVG1FV1EQVYW3K9Q
    type: extends
  - target: 01KNWE2QA0Z52H8VVFAMSA7KGA
    type: references
created: 2026-04-12
modified: 2026-04-12
source: "https://www.ndss-symposium.org/wp-content/uploads/2020/02/24415.pdf"
---

# HotFuzz — Paper (NDSS 2020)

*Source: https://www.ndss-symposium.org/wp-content/uploads/2020/02/24415.pdf — fetched 2026-04-12.*
*Authors: William Blair, Andrea Mambretti, Sajjad Arshad, Michael Weissbacher, William Robertson, Engin Kirda, Manuel Egele. Published at NDSS 2020.*

## What the paper does

HotFuzz is a guided fuzzer that discovers **algorithmic denial-of-service (AC DoS) vulnerabilities in Java** — i.e., inputs that force a method to run in worst-case rather than average-case time. It differs from SlowFuzz and PerfFuzz in two key ways:

1. **It fuzzes at the *method* level, not the whole-program level.** HotFuzz treats each individual `public` method of a target library as a standalone fuzz target and generates Java objects as inputs directly (instead of serialised byte streams).
2. **It uses "small-step semantics" to generate structured object inputs** — method-call sequences that produce valid, in-bounds Java objects — rather than byte-stream mutation.

## Micro-fuzzing — the core idea

Classical fuzzers need a *driver*: a harness that converts a byte stream into function arguments. Writing a driver for every method in every library is impractical. HotFuzz sidesteps this with **Micro-Fuzzing**: the fuzzer identifies each method's type signature and synthesises a fresh driver for it automatically.

For a target method `public Object foo(List<String> xs, int n)`, HotFuzz needs to produce:
1. A `List<String>` of some shape.
2. An `int`.
3. A loop that calls `foo(xs, n)` and measures CPU time.

The driver is generated from the types. The input values come from the small-step semantic generator.

## Small-step semantics

Java objects are **not** byte strings. A `HashMap` is not a sequence of bytes — it's the result of a sequence of method calls (`new HashMap()`, `put(k1,v1)`, `put(k2,v2)`, ...). HotFuzz represents an input as **the trace of constructor and setter calls** that produced it. A mutation adds, removes, reorders, or perturbs one call in the trace.

This is small-step operational semantics applied to test generation: an object's state is the sum of the operations that built it, and the search space is traversed by editing the operation trace. The benefit over byte-level mutation is that every mutation produces a **structurally valid** Java object — no wasted effort on inputs that crash in deserialisation.

## Feedback signal

HotFuzz uses **execution time** as the fuzzing feedback, similar to SlowFuzz. Inputs that make the target method run longer than current best are added to the corpus; mutations are applied preferentially to high-time inputs. The paper uses wall-clock time with statistical filtering to reject noise, and also tracks CPU cycles via JVM hooks.

## Evaluation and findings

The paper evaluated HotFuzz on:
- **The Java Runtime (JRE 8/11)** — core collections, regex, serialisation.
- **Apache Commons** — the standard Java utility libraries.
- **A selection of Maven Central libraries**.

Reported results include multiple confirmed **CVEs** for algorithmic complexity bugs in well-known Java libraries. The paper specifically highlights:

- A denial-of-service path in Java's `XMLGregorianCalendar` parsing.
- O(n²) in `com.google.javascript.rhino`.
- Hash-collision-like behaviour in a few third-party collection implementations.
- CVEs assigned for several findings; the authors responsibly disclosed to upstream.

The broader claim is that **Java libraries are full of AC DoS bugs** and a domain-independent fuzzer like HotFuzz finds them at low cost once the micro-fuzzing driver is in place.

## Contrast with SlowFuzz / PerfFuzz

| | SlowFuzz (CCS 2017) | PerfFuzz (ISSTA 2018) | HotFuzz (NDSS 2020) |
|---|---|---|---|
| Input model | Raw bytes | Raw bytes | Typed Java objects |
| Feedback | Total instruction count | Per-edge max count | Wall-clock / CPU time |
| Target language | C/C++ (LLVM) | C/C++ (LLVM) | Java (JVM) |
| Driver needed | Yes, manual | Yes, manual | **No, synthesised from method signature** |
| Granularity | Whole program | Whole program | **Single method** |

HotFuzz's contribution is orthogonal to SlowFuzz/PerfFuzz: it answers "how do you run this style of fuzzing on Java" and "how do you fuzz thousands of methods without writing thousands of drivers". In principle one could combine HotFuzz-style micro-fuzzing with PerfFuzz-style multi-dim feedback.

## Limitations

- **Per-method scope misses cross-method bugs.** Some algorithmic vulnerabilities only manifest in specific call sequences across multiple methods (`put` followed by `compute` then `remove`). HotFuzz's single-method driver doesn't explore these.
- **Small-step generators need bootstrapping.** For each *new* class, HotFuzz needs to know which constructors and setters produce meaningful objects. The paper uses heuristics plus handwritten generators for the Java collections; it's unclear how well this generalises.
- **Wall-clock time is noisy.** The paper handles this with statistical filtering but acknowledges the noise floor makes it difficult to detect sub-2x slowdowns.
- **JVM warmup.** JIT-compiled hot methods take ~1000 iterations to reach steady-state. HotFuzz discards the first N iterations as warmup but the exact value is a tuning parameter.

## Relevance to APEX G-46

1. **Java target support.** APEX's language-agnostic performance fuzzer should ship with a HotFuzz-inspired per-method Java driver synthesiser. LibAFL supports JVM instrumentation via a JNI bridge, so the infrastructure exists.
2. **Typed-input fuzzing in general.** The small-step-semantics idea applies to any statically typed language. Rust `struct`s, Go `struct`s, C++ classes all have the same shape — a sequence of constructor and setter calls — and any language APEX targets benefits from a typed-input fuzzer rather than byte-level.
3. **CVE ground truth.** The HotFuzz findings are a **regression benchmark**: APEX's performance fuzzer should re-derive the confirmed CVEs (XMLGregorianCalendar, Rhino, commons-collections cases) from scratch. That's a crisp "does our fuzzer work" smoke test.
4. **Cross-method sequences are a differentiator.** Extend the small-step model to *multiple* methods, and APEX can find cross-method AC bugs that HotFuzz leaves on the table.

## References

- Blair, Mambretti, Arshad, Weissbacher, Robertson, Kirda, Egele — NDSS 2020 — [paper PDF](https://www.ndss-symposium.org/wp-content/uploads/2020/02/24415.pdf)
- NDSS paper page — `01KNWEGYB6AVG1FV1EQVYW3K9Q`
- SlowFuzz — `01KNWEGYB1B15QGYTRC374Z7DQ`
- PerfFuzz — `01KNWEGYB3NXWFB6D4SV4DTD5X`
