---
id: 01KNYZ7YKH344XCTAFQAHQNYHG
title: "Cox: Regular Expression Matching Can Be Simple And Fast"
type: literature
tags: [article, cox, regex, thompson-nfa, re2, non-backtracking]
links:
  - target: 01KNWE2QA1E37T7GVKSM6EGXXR
    type: related
  - target: 01KNWGA5G80W4ESMANJM0M2XAV
    type: references
  - target: 01KNWGA5F7VA68B8ZPB6XR0RTE
    type: references
  - target: 01KNYZ7YKF7DHTFHJ50C7AE403
    type: related
created: 2026-04-10
modified: 2026-04-10
source: "https://swtch.com/~rsc/regexp/regexp1.html"
---

# Russ Cox — "Regular Expression Matching Can Be Simple And Fast" (2007)

*Source: https://swtch.com/~rsc/regexp/regexp1.html — fetched 2026-04-10. This is the founding article of the modern anti-ReDoS argument and the intellectual basis for RE2, Go's `regexp`, Rust's `regex`, and Hyperscan.*

## Core Argument

Russ Cox's 2007 article demonstrates that "Regular expression matching can be simple and fast, using finite automata-based techniques that have been known for decades."

## The Performance Problem

Cox presents a dramatic comparison between two approaches. When matching the pattern `a?ⁿaⁿ` against the string `aⁿ`, **Perl requires exponential time while Thompson's NFA algorithm executes in linear time**. Specifically, "the Thompson NFA implementation is a million times faster than Perl when running on a minuscule 29-character string."

The iconic graph in the original article (not reproduced here because fetch is text-only) shows Perl climbing off the top of the chart past n=23, while Thompson-NFA remains flat near-zero at n=100. The performance gap is not a constant factor but the difference between two complexity classes.

## Why This Matters

The article explains that popular languages including "Perl, PCRE, Python, Ruby, Java, and many other languages have regular expression implementations based on recursive backtracking that are simple but can be excruciatingly slow."

Cox argues these backtracking implementations suffer from pathological cases where they must explore 2ⁿ possible execution paths. In contrast, Thompson's approach maintains "state lists of length approximately n and processes the string ... for a total of O(n²) time."

## The Solution

Cox provides a complete C implementation (under 400 lines) demonstrating Thompson's algorithm using non-deterministic finite automata (NFAs). The technique involves:

1. Converting regular expressions to NFAs
2. Simulating multiple states simultaneously rather than backtracking
3. Optionally caching results to construct a deterministic finite automaton (DFA)

## Historical Context

Ironically, early Unix tools like `grep` and `awk` used these efficient algorithms, but later implementations abandoned them. **Henry Spencer's widely-adopted backtracking library became the foundation for modern slow implementations**, despite warnings about performance limitations.

This is a nice piece of software archaeology: the world once had fast regex, then traded it away for features (backreferences, lookaround) that turned out to be rarely needed but enormously expensive in the worst case. RE2 and its descendants are a deliberate rollback of that trade.

## Relevance to APEX G-46

Cox's article is the best single piece of writing to cite when explaining to APEX users *why* a ReDoS finding matters and *why* RE2 (or Rust's `regex`, or Hyperscan) is the correct mitigation. The claim is not "backtracking is a little slower" — it is a **complexity-class change** that an adversary can weaponise. Every APEX ReDoS Finding's "recommended mitigation" section should link to this article.

The article also foreshadows the whole APEX G-46 feature: the *millionfold* gap between exponential and polynomial regex is exactly the sort of asymmetry a resource-guided fuzzer is designed to surface.
