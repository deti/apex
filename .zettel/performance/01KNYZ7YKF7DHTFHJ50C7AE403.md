---
id: 01KNYZ7YKF7DHTFHJ50C7AE403
title: "Wikipedia: ReDoS (Regular Expression Denial of Service)"
type: literature
tags: [wikipedia, redos, regex, backtracking, dos, security]
links:
  - target: 01KNWE2QA1E37T7GVKSM6EGXXR
    type: related
  - target: 01KNWGA5F7VA68B8ZPB6XR0RTE
    type: references
  - target: 01KNYZ7YKH344XCTAFQAHQNYHG
    type: references
created: 2026-04-10
modified: 2026-04-10
source: "https://en.wikipedia.org/wiki/ReDoS"
---

# ReDoS — Regular Expression Denial of Service (Wikipedia)

*Source: https://en.wikipedia.org/wiki/ReDoS — fetched 2026-04-10.*

## Overview

ReDoS is an algorithmic complexity attack that exploits vulnerabilities in regular expression engines. By providing specially crafted regex patterns or inputs, attackers can cause programs to consume excessive computational resources, resulting in denial of service.

## Description

The vulnerability stems from how regex engines process patterns. When converting regular expressions to automata, engines may employ problematic approaches:

- Converting non-deterministic finite automata (NFAs) to deterministic finite automata (DFAs) can require exponential time
- Backtracking approaches may explore an exponential number of paths

"The time taken can grow polynomially or exponentially in relation to the input size" depending on the regex implementation and input combination.

## How Exponential Backtracking Occurs

Three conditions create severe vulnerabilities:

1. Repetition operators (`+`, `*`) applied to subexpressions
2. Subexpressions matching input in multiple ways or matching prefixes of longer matches
3. Following expressions that don't match what the subexpression matches

Examples include patterns like `(a|a)+$`, `(a+)*$`, and `(a|aa)*c`. When tested against strings like `aaaaaaaaaaaaaaaaaaaaaaaax`, "the runtime will approximately double for each extra `a`" before the terminating character.

## Attack Vectors

ReDoS can occur through two mechanisms:

**User-Supplied Patterns:** Web services allowing clients to provide search patterns enable attackers to inject malicious regex.

**User-Supplied Input:** If vulnerable regex already exists server-side, attackers provide crafted input triggering worst-case behaviour. Email scanners and intrusion detection systems face this risk.

## Mitigation Strategies

**Timeouts:** Setting execution time limits prevents hangs during untrusted input processing.

**Non-Backtracking Libraries:** Implementations using deterministic finite automata, like Google's RE2, run in linear time and resist ReDoS attacks.

**Pattern Analysis:** Linters and fuzzing tools can detect vulnerable regexes. Many problematic patterns can be rewritten — for instance, `(.*a)+` becomes `([^a]*a)+`.

**Possessive Matching:** Atomic grouping and possessive quantifiers disable backtracking for pattern sections.

## Relevance to APEX G-46

Wikipedia's taxonomy of the three necessary conditions for exponential backtracking is a clean checklist for a static detector: any regex containing a repeated subexpression that matches an input in multiple overlapping ways AND is followed by a pattern that can fail matches the signature. APEX's ReDoS analyser can use this as a first-pass filter before running the more expensive ensemble-of-detectors approach used by vuln-regex-detector.
