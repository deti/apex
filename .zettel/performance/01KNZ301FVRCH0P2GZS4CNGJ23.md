---
id: 01KNZ301FVRCH0P2GZS4CNGJ23
title: "CVE-2020-5243: uap-core User-Agent ReDoS"
type: incident
tags: [cve, redos, cwe-1333, javascript, npm, uap-core, user-agent, real-world]
links:
  - target: 01KNWE2QA1E37T7GVKSM6EGXXR
    type: references
  - target: 01KNWGA5F7VA68B8ZPB6XR0RTE
    type: extends
  - target: 01KNWGA5F4W852RG6C5FJCP204
    type: extends
  - target: 01KNWGA5FPC1MSYBQQS6GJPRTS
    type: related
  - target: 01KNZ301FV584G004E2SRNW97Z
    type: related
  - target: 01KNZ301FVAC85VSD6QSXHBTBN
    type: related
  - target: 01KNZ301FVV2BBBW67QZV0MWTM
    type: related
  - target: 01KNZ301FVQZCT0JNP97SDY1MH
    type: related
  - target: 01KNZ301FV9DXAXN39MPPAG9JV
    type: related
  - target: 01KNWEGYBDMW0V0CFVJ83A4B9J
    type: related
created: 2026-04-12
modified: 2026-04-12
source: "https://nvd.nist.gov/vuln/detail/CVE-2020-5243"
cve: CVE-2020-5243
cwe: CWE-1333
cvss: 7.5
package: "uap-core"
---

# CVE-2020-5243: uap-core User-Agent ReDoS

## Metadata

- **CVE ID:** CVE-2020-5243
- **Published:** 2020-02-20
- **Last modified:** 2024-11-21
- **CVSS v3.1 (NIST):** 7.5 HIGH — `AV:N/AC:L/PR:N/UI:N/S:U/C:N/I:N/A:H`
- **CVSS v3.1 (CNA):** 5.7 MEDIUM — `AV:N/AC:L/PR:L/UI:R/S:U/C:N/I:N/A:H`
- **Weaknesses:** CWE-1333 (Inefficient Regular Expression Complexity); CWE-20 (Improper Input Validation)

## Official description (NVD, verbatim)

"uap-core before 0.7.3 is vulnerable to a denial of service attack when processing crafted User-Agent strings. Some regexes may take a long time to process a malicious HTTP header."

## Affected versions

- `uap-core` < 0.7.3 (all language bindings; the CVE is filed against the shared YAML regex database, not a single language package)

Because uap-core is the canonical data file consumed by every `ua-parser` implementation across languages (Python, Ruby, Node.js, Go, C++, Java), a flaw here cascades to *every* downstream parser using the shared regex set. Affected downstreams include `ua-parser-js`, `python-user-agents`, `browserscope/ua-parser` (Ruby), and their many indirect dependents.

## Why it matters

Parsing `User-Agent` headers is the textbook example of "run regex on attacker-controlled string on every request." Analytics, abuse detection, bot detection, A/B testing, CDN variant routing, and session-fingerprinting pipelines all call a UA parser on the HTTP request header with no rate limiting and no length cap. A ReDoS in the UA regex database is therefore directly exposed to the open internet on many production services.

## Root cause

A handful of regex entries in the uap-core YAML database contained *overlapping capture groups*: two branches of an alternation or two nested quantifiers that could both consume the same characters. Common forms seen in UA-parsing databases:
- `(\w+ ?\w*)+` — the space is optional inside a repeating group, so for an input of many consecutive spaces the engine has `2^n` ways to partition the whitespace.
- `([a-zA-Z0-9.]+)+` — nested quantifiers over a character class that includes the separator character.

When the regex fails (because the attacker appends a character that violates the trailing constraint), the engine must enumerate every partition before concluding the match fails — classic polynomial or exponential backtracking.

The fix (upstream commit `0afd61ed85396a3b5316f18bfd1edfaadf8e88e1`) rewrites the offending patterns to eliminate overlap, and, critically, adds a CI job that runs a ReDoS detector against every regex added to the database.

## Exploitation

A malicious client sets the `User-Agent` header on their HTTP request to a string several kilobytes long constructed from a known-vulnerable UA regex. On any server that runs uap-core (directly or via a higher-level analytics SDK) as part of the request path, a single request can burn seconds of CPU. The uap-core maintainers' original proof-of-concept showed per-request times exceeding 10 seconds on modest hardware.

Unlike many ReDoS bugs, exploitation requires only the ability to choose your own `User-Agent`, which is trivial for any HTTP client. No authentication, no user interaction, no privileges.

## Remediation

- Upgrade `uap-core` to ≥ 0.7.3 and rebuild any binding that embeds a copy of the YAML (most language implementations vendor the data at build time).
- Alternatively or additionally, cap the length of the `User-Agent` header at the web layer (e.g. 512 bytes is generous for real user agents).
- For defense in depth, wrap calls to `ua_parser.parse` in a timeout guard or run them in a thread/worker with a 50ms budget.

## Key references

- NVD record: https://nvd.nist.gov/vuln/detail/CVE-2020-5243
- GitHub advisory: GHSA-cmcx-xhr8-3w9p
- Fix commit: https://github.com/ua-parser/uap-core/commit/0afd61ed85396a3b5316f18bfd1edfaadf8e88e1

## Relevance to APEX G-46

Uap-core is the paradigm example for "detect ReDoS at the regex database layer, not the consuming language binding." For APEX this is useful in two ways:
1. **Regression oracle.** Any G-46 run against an older `uap-core` tarball should re-surface the known vulnerable patterns. This is a cheap regression test.
2. **Corpus expansion.** The public UA regex database is a ready-made corpus of thousands of regexes against which a static ReDoS analyzer (like `vuln-regex-detector` or `regexploit`) and a dynamic cost-fuzzer can be benchmarked side by side.

This CVE is also one of Davis et al.'s canonical examples in their ecosystem-scale ReDoS empirical studies (ESEC/FSE 2018).
