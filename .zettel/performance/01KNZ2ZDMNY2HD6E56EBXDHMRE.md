---
id: 01KNZ2ZDMNY2HD6E56EBXDHMRE
title: "Google RE2: Safe Linear-Time Regex Library (README)"
type: literature
tags: [re2, regex, linear-time, google, nfa, pcre, backreferences]
links:
  - target: 01KNWGA5G80W4ESMANJM0M2XAV
    type: extends
  - target: 01KNYZ7YKH344XCTAFQAHQNYHG
    type: references
  - target: 01KNWE2QA1E37T7GVKSM6EGXXR
    type: related
created: 2026-04-12
modified: 2026-04-12
source: "https://github.com/google/re2"
---

# RE2 — A Safe Regular Expression Library

*Source: https://github.com/google/re2 (README and wiki) — fetched 2026-04-12.*
*Original author: Russ Cox. Maintained by Google since 2006.*

## Core promise

RE2 guarantees that **match time is linear in the length of the input string** (technically `O(m·n)` where `m` is the regex size and `n` is the input length). This is not a heuristic or a best-effort; it is a property of the matching algorithm. Consequently, **catastrophic backtracking is impossible** in RE2, regardless of the pattern or input.

The library was written at Google starting in 2006 for production services that had to accept patterns from untrusted users (Code Search being the motivating example). It has since become the default safe-regex library in many environments.

## What RE2 supports

RE2 aims to be a mostly-compatible subset of the PCRE/Perl regex language:

- Character classes, POSIX classes, Perl classes (`\d`, `\w`, `\s`, `\p{L}`, etc.)
- Unicode — full property and script support
- Quantifiers — `*`, `+`, `?`, `{n}`, `{n,m}`, lazy variants
- Grouping — `(…)`, `(?:…)`, `(?P<name>…)`
- Alternation — `|`
- Anchors — `^`, `$`, `\A`, `\z`, `\b`, `\B`
- Word boundaries and multiline mode
- Named captures via Python-style `(?P<name>…)`
- Case-insensitive matching via `(?i)`

## What RE2 deliberately does NOT support

RE2 rejects by design any regex feature whose correct semantics **require** backtracking:

- **Backreferences** — `\1`, `\g{name}`, etc. Matching `(a*)b\1` requires remembering what `\1` matched and re-matching it later. Matching languages of this form is at least PSPACE-complete in general; there is no known linear-time algorithm. **Not supported.**
- **Lookahead/lookbehind assertions** — `(?=…)`, `(?!…)`, `(?<=…)`, `(?<!…)`. Context-sensitive and require multi-pass; **not supported**. (Note: Go's standard-library `regexp` is RE2-based and also lacks these.)
- **Possessive quantifiers** — `*+`, `++`, `?+`. In an NFA simulation these are redundant; in a backtracker they exist to prevent catastrophic backtracking. RE2 never backtracks, so they are moot. **Not supported.**
- **Atomic groups** — `(?>…)`. Same rationale.
- **Recursive patterns** — `(?R)`, `(?0)`, `(?&name)`. **Not supported.**
- **Conditional patterns** — `(?(cond)yes|no)`. **Not supported.**

The README's justification: *"features ... for which only backtracking solutions are known to exist"*. If an efficient algorithm is discovered for any of these, RE2 will reconsider.

## The algorithm

RE2 runs one of three engines depending on the pattern shape and the match requested:

1. **DFA** — the fastest. Compiled lazily from the NFA on demand; cached states; falls back to NFA if the DFA cache overflows. Used when only "does it match?" or "where does it match?" is needed, not full capture groups.
2. **NFA** — parallel-state simulation (Thompson's algorithm). `O(m·n)` time, `O(m)` state memory. Used when captures are required but backtracking is not.
3. **One-pass** — a special optimisation for regexes whose NFA has at most one active state per input position. Fastest captures engine when the pattern qualifies.

All three are worst-case linear-time. RE2 picks the fastest applicable engine automatically.

## Language bindings and ports

The C++ library has bindings or reimplementations in most mainstream languages:

| Language | Binding/port |
|---|---|
| **C++** | `google/re2` (native) |
| **Python** | `google-re2` (PyPI, official bindings) |
| **Go** | `regexp` (standard library, RE2 semantics) |
| **Rust** | `regex` crate (independent implementation, RE2 semantics) |
| **Java** | `re2j` — Google's pure-Java port |
| **JavaScript** | `re2` (npm, native bindings) |
| **Ruby** | `re2` gem |
| **Node.js** | `node-re2` |
| **OCaml**, **Perl**, **R**, **D**, **Erlang**, **WebAssembly** | Community ports |

Notably, Go's `regexp` and Rust's `regex` are independently-written RE2-style engines, not wrappers around the C++ code. The semantics and guarantees are the same.

## Tradeoffs

- **Backreferences** are genuinely useful for patterns like HTML tag matching (`<(\w+)>.*?</\1>`). RE2 cannot do this; users must either accept a less precise pattern or switch to a full PCRE parser.
- **Lookaheads** can be simulated in many cases by rewriting the pattern, but the rewrite is sometimes non-obvious.
- **Alternative orderings**: Perl's leftmost-first (matches `a|ab` as `a` even when `ab` would fit); RE2 matches leftmost-longest in "longest" mode and leftmost-first in "first" mode. Users migrating patterns must check carefully.

## Performance data

On the canonical catastrophic-backtracking benchmark — `(a?)ⁿaⁿ` matched against `aⁿ`:

| n | Perl | Python `re` | PCRE | **RE2** | Go `regexp` | Rust `regex` |
|---:|---:|---:|---:|---:|---:|---:|
| 10 | 0.1 ms | 0.1 ms | 0.2 ms | **< 1 µs** | < 1 µs | < 1 µs |
| 20 | 40 ms | 70 ms | 60 ms | **< 1 µs** | < 1 µs | < 1 µs |
| 29 | ~60 s | ~100 s | ~70 s | **< 1 µs** | < 1 µs | < 1 µs |

RE2 is approximately **a million times faster** at `n = 29` because the cost is constant in the pathological input. (Source: Russ Cox's "Regular Expression Matching Can Be Simple And Fast", `01KNYZ7YKH344XCTAFQAHQNYHG`.)

## Adoption notes

- **Cloudflare's WAF** switched to RE2 and Rust regex after the July 2019 outage (`01KNWGA5G3XDK746J4N59G6VVW`). This was the explicit remediation.
- **Stack Exchange** switched after their July 2016 outage.
- **GitHub** uses RE2 via Go `regexp` for its content inspection.
- **Google Code Search** was the original RE2 use case.

## Relevance to APEX G-46

1. **RE2 is the "safe" half of APEX's ReDoS detector.** A regex that *compiles* in RE2 or Go `regexp` is guaranteed non-catastrophic. APEX can use this as a fast pre-filter: if the pattern compiles in Rust's `regex` crate, there is no ReDoS Finding to emit (modulo pattern differences).
2. **Rewriting guidance.** When APEX flags a regex as ReDoS-vulnerable, the remediation should include "try rewriting to RE2 syntax" with an automated transformer that identifies and removes unsupported features where possible.
3. **Language-migration Findings.** A Python codebase using the `re` module on untrusted inputs can switch to `google-re2` (the PyPI binding) with nearly no code change. APEX should suggest this explicitly for high-severity ReDoS Findings.
4. **Negative test corpus.** Any pattern that RE2 rejects as "unsupported feature" is, by definition, a candidate for ReDoS review. APEX's static ReDoS checker can use RE2 rejection as a "worth checking further" heuristic.

## References

- RE2 — [github.com/google/re2](https://github.com/google/re2)
- Russ Cox — "Regular Expression Matching Can Be Simple And Fast" — `01KNYZ7YKH344XCTAFQAHQNYHG`
- Russ Cox — "Regular Expression Matching: The Virtual Machine Approach" — [swtch.com/~rsc/regexp/regexp2.html](https://swtch.com/~rsc/regexp/regexp2.html)
- Cloudflare July 2019 post-mortem — `01KNWGA5G3XDK746J4N59G6VVW`
