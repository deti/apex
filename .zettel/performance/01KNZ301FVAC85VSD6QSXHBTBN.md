---
id: 01KNZ301FVAC85VSD6QSXHBTBN
title: "CVE-2023-26115: word-wrap (Node.js) ReDoS"
type: incident
tags: [cve, redos, cwe-1333, javascript, npm, word-wrap, real-world]
links:
  - target: 01KNWE2QA1E37T7GVKSM6EGXXR
    type: references
  - target: 01KNWGA5F7VA68B8ZPB6XR0RTE
    type: extends
  - target: 01KNWGA5F4W852RG6C5FJCP204
    type: extends
  - target: 01KNZ301FV584G004E2SRNW97Z
    type: related
  - target: 01KNZ301FVQZCT0JNP97SDY1MH
    type: related
  - target: 01KNZ301FV9DXAXN39MPPAG9JV
    type: related
  - target: 01KNZ301FVEJEFXWZQRNCB36SS
    type: related
  - target: 01KNWEGYBDMW0V0CFVJ83A4B9J
    type: related
created: 2026-04-12
modified: 2026-04-12
source: "https://nvd.nist.gov/vuln/detail/CVE-2023-26115"
cve: CVE-2023-26115
cwe: CWE-1333
cvss: 7.5
package: "word-wrap (npm)"
---

# CVE-2023-26115: word-wrap (Node.js) ReDoS

## Metadata

- **CVE ID:** CVE-2023-26115
- **Published:** 2023-06-22
- **Last modified:** 2025-02-13
- **CVSS v3.1 (NIST):** 7.5 HIGH — `AV:N/AC:L/PR:N/UI:N/S:U/C:N/I:N/A:H`
- **CVSS v3.1 (Snyk):** 5.3 MEDIUM — `AV:N/AC:L/PR:N/UI:N/S:U/C:N/I:N/A:L`
- **Weakness:** CWE-1333 (Inefficient Regular Expression Complexity)

## Official description (NVD, verbatim)

"All versions of the package word-wrap are vulnerable to Regular Expression Denial of Service (ReDoS) due to the usage of an insecure regular expression within the result variable."

## Affected versions

All `word-wrap` releases up to (excluding) 1.2.4, for both the Node.js package and the Java WebJars mirror.

## Why it matters

`word-wrap` is a tiny (<200 LOC) utility that splits long strings into line-wrapped text at a target column width. It is a transitive dependency of very large toolchains — notably ESLint (via `optionator`), which is in turn a dependency of every major JS/TS linter setup. At the time of disclosure, `word-wrap` had ~25M weekly npm downloads. Although the user-facing application rarely passes attacker-controlled strings to word-wrap directly, the sheer breadth of its inclusion made it a high-visibility supply-chain incident.

## Root cause

The vulnerable regex sits in the core `result` computation of `word-wrap/index.js`, line 39. It matches a run of non-whitespace characters followed by an optional trailing separator inside a repeating group. The pattern contains overlapping quantifiers in the form `(\S+\s+)+` (simplified): once the engine reaches the anchor that ends the substring and fails, it must enumerate every partition of whitespace across the matched prefix — classic polynomial ReDoS. With long, whitespace-free input the regex's execution time grows super-linearly in the input length.

The fix (`word-wrap 1.2.4`) rewrites the core matcher without overlapping quantifiers and adds a bound on processed input length.

## Exploitation

A crafted string of several kilobytes of pathologically repeated tokens is enough to push `word-wrap` CPU usage into seconds-per-call, stalling the calling process. Because `word-wrap` runs synchronously inside a caller's event loop (typical of small JS string utilities), a successful stall freezes the enclosing process entirely. This is particularly nasty for services that run `word-wrap` on descriptions, error messages, or help text fed through a templating layer.

## Remediation

- Upgrade `word-wrap` to ≥ 1.2.4.
- If pinned for compatibility, pre-truncate the input string before calling `wrap()` and reject any string containing very long whitespace-free runs.
- At the host level, wrap synchronous string-manipulation calls that originate from untrusted input in worker threads with a timeout.

## Key references

- NVD record: https://nvd.nist.gov/vuln/detail/CVE-2023-26115
- Snyk JS advisory: SNYK-JS-WORDWRAP-3149973
- Snyk Java advisory: SNYK-JAVA-ORGWEBJARSNPM-5537278
- NetApp advisory: ntap-20240621-0006
- GitHub release: https://github.com/jonschlinkert/word-wrap/releases/tag/1.2.4

## Relevance to APEX G-46

`word-wrap` is an archetype for the "small utility with obvious tainted input" class. It has a compact function boundary, deterministic output, and a pure (no I/O) implementation. A coverage-plus-cost fuzzer (PerfFuzz, MemLock, SlowFuzz, or an APEX descendant) should find this bug in minutes starting from any non-trivial seed corpus. Including `word-wrap 1.2.3` in APEX's regression benchmark provides a minimal polynomial-ReDoS target where expected time-to-discovery should be in the tens of seconds.
