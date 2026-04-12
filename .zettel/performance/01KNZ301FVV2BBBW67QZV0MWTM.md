---
id: 01KNZ301FVV2BBBW67QZV0MWTM
title: "CVE-2017-16021: uri-js ReDoS in parse()"
type: incident
tags: [cve, redos, cwe-1333, javascript, npm, uri-js, real-world]
links:
  - target: 01KNWE2QA1E37T7GVKSM6EGXXR
    type: references
  - target: 01KNWGA5F7VA68B8ZPB6XR0RTE
    type: extends
  - target: 01KNWGA5F4W852RG6C5FJCP204
    type: extends
  - target: 01KNZ301FV584G004E2SRNW97Z
    type: related
  - target: 01KNZ301FVAC85VSD6QSXHBTBN
    type: related
  - target: 01KNZ301FVRCH0P2GZS4CNGJ23
    type: related
  - target: 01KNZ301FVQZCT0JNP97SDY1MH
    type: related
  - target: 01KNZ301FV9DXAXN39MPPAG9JV
    type: related
  - target: 01KNWEGYBDMW0V0CFVJ83A4B9J
    type: related
created: 2026-04-12
modified: 2026-04-12
source: "https://nvd.nist.gov/vuln/detail/CVE-2017-16021"
cve: CVE-2017-16021
cwe: CWE-1333
cvss: 6.5
package: "uri-js (npm)"
---

# CVE-2017-16021: uri-js ReDoS in parse()

## Metadata

- **CVE ID:** CVE-2017-16021
- **Published:** 2018-06-04
- **Last modified:** 2024-11-20
- **CVSS v3.1:** 6.5 MEDIUM — `AV:N/AC:L/PR:L/UI:N/S:U/C:N/I:N/A:H`
- **CVSS v2.0:** 6.8 MEDIUM — `AV:N/AC:L/Au:S/C:N/I:N/A:C`
- **Weaknesses:** CWE-1333 (Inefficient Regular Expression Complexity); CWE-400 (Uncontrolled Resource Consumption)

## Official description

"uri-js is a module that tries to fully implement RFC 3986. uri-js uses a regular expression that is vulnerable to redos. Applications that use uri-js to parse user input may be vulnerable to denial of service." When the `parse()` function is called with a crafted URI, the regex engine hangs with CPU pegged at 100%.

## Affected versions

`uri-js` ≤ 2.1.1. The first fixed release is `uri-js 3.0.0`, which rewrites the parsing regex and introduces a length cap.

## Why it matters

`uri-js` is a Node.js library that claims RFC 3986 URI parsing compliance. It is a transitive dependency of `ajv` (one of the largest JSON Schema validators in the ecosystem), which in turn is vendored by TypeScript tooling, Webpack, Prettier, ESLint, and virtually every modern JS build chain. Any application that feeds untrusted URIs to `ajv`'s `format: "uri"` validation, or that calls `uri-js.parse` directly on user input, can be stalled with a crafted URI string.

## Root cause

The parsing regex attempts to match the full RFC 3986 grammar (scheme, authority, userinfo, host, port, path, query, fragment) in a single pattern. To tolerate optional components, it contains several alternations where two branches can consume the same characters — the canonical ReDoS recipe. When the pattern fails at the end of the candidate URI, the engine must enumerate every partition of the prefix across the ambiguous alternations, producing super-linear blowup.

Characteristic pathological inputs consist of a long run of unreserved characters followed by a character that violates the grammar (for example `a//` or `://` sequences in unexpected positions), which force the matcher to retry every authority/path split.

## Exploitation

A remote attacker with any endpoint that calls `uri-js.parse` on input they control can submit a ~10 KB URI string and stall the event loop for seconds. In a Node.js HTTP server with a single event loop, this denies service to all concurrent clients. Because URI parsing is usually applied very early in request processing (routing, authentication, origin validation), the attack surface is broad.

## Remediation

- Upgrade `uri-js` to ≥ 3.0.0.
- When using `ajv` with `format: "uri"`, upgrade ajv to a release that vendors a safe `uri-js`.
- At the gateway, cap the Request-URI length (per RFC 7230 this is implementation-defined; many proxies default to 8 KB, which is still enough for exploitation — tighten to 2 KB when possible).

## Key references

- NVD: https://nvd.nist.gov/vuln/detail/CVE-2017-16021
- GitHub issue: https://github.com/garycourt/uri-js/issues/12
- Node Security advisory: https://nodesecurity.io/advisories/100

## Relevance to APEX G-46

This CVE pre-dates most of the modern ReDoS detection literature. It was originally found by manual review rather than fuzzing, and its vulnerable pattern is a good test case for static analyzers (`vuln-regex-detector`, `regexploit`) because it sits inside a pure function that takes a string and returns a parsed structure — no side effects, no I/O, a very clean fuzz harness.

For APEX G-46, `uri-js` is useful as:
1. A second opinion on the `(nested-quantifier + alternation + required-tail)` ReDoS pattern.
2. A benchmark where both static and dynamic detectors should succeed; if either fails, the tool has a gap.
3. A case study in "ReDoS in an RFC parser" — the risk profile is elevated whenever a complex grammar is flattened into a single big regex instead of a proper state-machine parser.
