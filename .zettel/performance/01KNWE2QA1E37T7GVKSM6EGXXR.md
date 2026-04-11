---
id: 01KNWE2QA1E37T7GVKSM6EGXXR
title: ReDoS — Regular Expression Denial of Service
type: concept
tags: [redos, regex, cwe-1333, cwe-400, security, catastrophic-backtracking]
links:
  - target: 01KNWE2Q9YZBAR140ZX5P36TQ5
    type: extends
  - target: 01KNWE2QA3FA96G8JKN733K0XP
    type: related
  - target: 01KNWEGYBDMW0V0CFVJ83A4B9J
    type: extends
  - target: 01KNWGA5F4W852RG6C5FJCP204
    type: references
  - target: 01KNWGA5F7VA68B8ZPB6XR0RTE
    type: references
  - target: 01KNWGA5F9YTQ01C15QNFHQGPT
    type: references
  - target: 01KNWGA5FPC1MSYBQQS6GJPRTS
    type: references
  - target: 01KNWGA5FV5S70XY5359FJ97X3
    type: references
  - target: 01KNWGA5G3XDK746J4N59G6VVW
    type: related
  - target: 01KNWGA5G5JNAQP0QEYZXN6T2H
    type: related
  - target: 01KNWGA5G80W4ESMANJM0M2XAV
    type: related
  - target: 01KNWGA5GMWKV6AKP04D964G5H
    type: related
created: 2026-04-10
modified: 2026-04-10
---

# ReDoS — Regular Expression Denial of Service

**ReDoS** is a denial-of-service attack that exploits regular-expression engines whose matching algorithm exhibits **super-linear** (often **exponential**) worst-case complexity on carefully crafted inputs. It is codified by **CWE-1333 (Inefficient Regular Expression Complexity)**, a direct child of **CWE-400 (Uncontrolled Resource Consumption)**.

## Root cause — backtracking NFA engines

Most mainstream regex engines (PCRE, Python `re`, JavaScript V8, Java `java.util.regex`, .NET `System.Text.RegularExpressions` in its default mode, Ruby Onigmo) implement the matcher as a **backtracking NFA**. When a quantifier has multiple ways of consuming a portion of the input, the engine tries each alternative, and on failure it *backtracks* and retries — potentially revisiting exponentially many states.

The canonical trigger pattern is **nested / overlapping quantifiers** where a substring can be partitioned in many ways:

```
^(a+)+$         applied to   aaaaaaaaaaaaaaaaab
^(a|a)*$        applied to   aaaaaaaaaaaaaaaaab
^(a|aa)+$       applied to   aaaaaaaaaaaaaaaaab
^(.*a){N}$      applied to   a long string missing the final a
```

Each extra `a` doubles the number of partitions the engine must try before declaring no-match, so time grows as `O(2ⁿ)`. Polynomial-time versions exist too (e.g. `(a|aa)*b` → `O(n²)`) and are easier to overlook.

By contrast, **non-backtracking** engines (Google RE2, Rust `regex`, Go `regexp`, Intel Hyperscan) use Thompson-NFA / DFA simulation with guaranteed `O(n·m)` matching. They are immune to ReDoS — at the cost of dropping features like backreferences and arbitrary lookaround.

## Scale of the problem

Davis, Coghlan, Servant, Lee (ESEC/FSE 2018, "The Impact of ReDoS in Practice") collected **~500,000 regexes** from npm and PyPI and measured them. **~3.5%** exhibited super-linear worst-case behaviour, with exploitable ReDoS candidates across thousands of popular packages. Follow-up ecosystem scans have continued to surface high-profile CVEs — e.g. the 2024 Keycloak ReDoS found via OSS-Fuzz, the Express `ms` package 2015 CVE, the `marked` library 2017 CVE, `moment.js` CVE-2017-18214.

## Static detection

Static ReDoS analysers look for a regex whose NFA contains an **exploitable ambiguity** — two distinct ways to match the same prefix inside a `*` or `+` quantifier. Well-studied tools:

- **rxxr / rxxr2** — static analysis for exponential-time regex.
- **ReScue** — detection *and* automatic repair.
- **vuln-regex-detector** (Davis et al.) — ensemble: runs rxxr2, Rathnayake and Thielecke, and a dynamic verifier; reports the union.
- **Safe-Regex** (npm) — simple heuristic; high false-positive / false-negative rate.
- **Microsoft SRM / RE#** — research-grade DFA compilation with provable linear matching.

## Dynamic verification

Static tools flag candidates; **dynamic verification** confirms them. For each flagged regex, the verifier:

1. Synthesises a candidate worst-case input (based on the exploitable-ambiguity witness).
2. Runs the regex on increasing sizes `n = 10, 20, 40, 80, ...`.
3. Measures wall-clock time; fits to an exponential / polynomial model.
4. If growth is super-linear, emits a Finding with a concrete witness string.

APEX's G-46 spec explicitly requires the ReDoS finding to include a **concrete worst-case input string** — static flags alone are not sufficient.

## Mitigations

- **Use a non-backtracking engine** (RE2, `rust-regex`) where feature set allows.
- **Atomic groups / possessive quantifiers** — `(?>a+)+` forbids the engine from reconsidering the inner match and breaks the amplification.
- **Input size / time bounds** — cap the matched input length and/or set a regex engine timeout (PCRE `pcre_extra.match_limit`, .NET `Regex.MatchTimeout`, Python does not expose this natively — use `regex` package with `DEFAULT_VERSION=regex.VERSION1` or external watchdog).
- **Linear-time rewrites** — replace `(a+)+` with `a+`, `(a|aa)+` with `a+`, etc.
- **Precompile and validate at CI time** — run a ReDoS linter on every PR that touches regex literals.

## CWE mapping

- **CWE-1333** — Inefficient Regular Expression Complexity (the direct ReDoS weakness).
- **CWE-400** — Uncontrolled Resource Consumption (parent class).
- **CWE-730** — OWASP Top 10 2004 Category A9 — DoS (historical).

## References

- Davis, Coghlan, Servant, Lee — ESEC/FSE 2018 — [DOI 10.1145/3236024.3236027](https://doi.org/10.1145/3236024.3236027)
- MITRE CWE-1333 — [cwe.mitre.org/data/definitions/1333.html](https://cwe.mitre.org/data/definitions/1333.html)
- MITRE CWE-400 — [cwe.mitre.org/data/definitions/400.html](https://cwe.mitre.org/data/definitions/400.html)
- vuln-regex-detector — [github.com/davisjam/vuln-regex-detector](https://github.com/davisjam/vuln-regex-detector)
- Cox, "Regular Expression Matching Can Be Simple and Fast" — [swtch.com/~rsc/regexp/regexp1.html](https://swtch.com/~rsc/regexp/regexp1.html)
