---
id: 01KNWGA5G80W4ESMANJM0M2XAV
title: "Tool: Google RE2 (Non-Backtracking Regex)"
type: literature
tags: [tool, re2, google, regex, non-backtracking, redos-mitigation]
links:
  - target: 01KNWE2QA1E37T7GVKSM6EGXXR
    type: related
  - target: 01KNWGA5G3XDK746J4N59G6VVW
    type: references
created: 2026-04-10
modified: 2026-04-10
source: "https://github.com/google/re2"
---

# Google RE2 — Non-Backtracking Regex Library

*Source: https://github.com/google/re2 — fetched 2026-04-10.*

## Overview

RE2 is an efficient, principled regular expression library used in production at Google since 2006. The project prioritises safety above all else, designed to handle regular expressions from untrusted users without risk.

## Core Guarantee

RE2's defining characteristic is its **linear-time matching guarantee**. The match time scales linearly with input string length, preventing the catastrophic backtracking that plagues traditional regex engines. This safety comes from a fundamental architectural difference: while backtracking engines test alternatives sequentially, RE2 evaluates them in parallel using a Thompson-NFA / DFA simulation.

## Key Tradeoff Philosophy

The library embodies a "pessimistic" approach contrasting with backtracking engines' "optimism." Traditional engines excel when early alternatives match frequently but can slow dramatically on worst-case inputs. RE2 accepts higher constant-factor overhead to guarantee consistent linear performance, trading potential speed advantages for predictability and security.

## Design Principles

RE2 intentionally omits features requiring only backtracking solutions for correctness — specifically **backreferences** and **lookaround assertions**. This principled limitation ensures safety guarantees remain mathematically sound.

The implementation manages memory through configurable budgets and avoids stack overflow by eliminating recursion, addressing production deployment concerns.

## Language & Availability

Written in C++. Includes an official Python wrapper (published as `google-re2` on PyPI) and numerous community ports to languages including Go, Rust, Java, JavaScript, Ruby, Perl, and others. The Go standard library's `regexp` package is derived from the same design and implementation approach.

## Related family

- **Rust `regex` crate** — same Thompson-NFA foundations, adds NFA + literal-search optimisations; standard for Rust and recommended by Cloudflare in their 2019 remediation plan.
- **Hyperscan** (Intel) — non-backtracking, multi-pattern matching at line rate, used in Suricata and other network-scanning tools.
- **Google RE2/J** — Java port.
- **RE2/JS** — JavaScript port.

## Relevance to APEX G-46

RE2 is the canonical answer to the "how do I fix this ReDoS?" question. APEX's ReDoS findings should include RE2 (or a language-specific analogue) as the recommended mitigation. The spec's CWE-1333 CVSS-scoring step should downgrade severity for patterns that are already served by a guaranteed-linear engine, since they are not exploitable even with catastrophic inputs.

There's also an interesting use case in reverse: APEX could offer a **"simulate with RE2"** verification mode for every detected regex. If RE2 fails to compile a pattern (because of backreferences or lookaround), that's a signal the pattern is *inherently* backtracking — a strong positive ReDoS-risk indicator, independent of quantifier analysis.
