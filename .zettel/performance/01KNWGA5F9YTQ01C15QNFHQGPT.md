---
id: 01KNWGA5F9YTQ01C15QNFHQGPT
title: "CWE-407: Inefficient Algorithmic Complexity"
type: literature
tags: [cwe, cwe-407, mitre, complexity-attack, quadratic, dos, security]
links:
  - target: 01KNWE2Q9YZBAR140ZX5P36TQ5
    type: references
  - target: 01KNWE2QA1E37T7GVKSM6EGXXR
    type: references
  - target: 01KNWE2QA3FA96G8JKN733K0XP
    type: references
  - target: 01KNWE2QA6QKE8Z152WX8D9XYB
    type: references
  - target: 01KNWE2QAAKZH8GGZ172HZ9RHS
    type: references
  - target: 01KNWE2QABV7943DKAXTARJHXA
    type: references
  - target: 01KNWEGYB8807ET2427V3VCRJ3
    type: references
  - target: 01KNWGA5F4W852RG6C5FJCP204
    type: references
  - target: 01KNWGA5F7VA68B8ZPB6XR0RTE
    type: references
created: 2026-04-10
modified: 2026-04-10
source: "https://cwe.mitre.org/data/definitions/407.html"
---

# CWE-407: Inefficient Algorithmic Complexity

*Source: https://cwe.mitre.org/data/definitions/407.html — fetched 2026-04-10.*

## Description

"An algorithm in a product has an inefficient worst-case computational complexity that may be detrimental to system performance and can be triggered by an attacker, typically using crafted manipulations that ensure that the worst case is being reached."

## Alternate Terms

**Quadratic Complexity** — Used when algorithmic complexity relates to the square of the number of inputs (N²).

## Common Consequences

| Impact | Details |
|---|---|
| DoS: Resource Consumption (CPU); DoS: Resource Consumption (Memory); DoS: Resource Consumption (Other) | Scope: Availability. The typical consequence is CPU consumption, but memory and other resource consumption can also occur. |

## Relationships

**ChildOf:** CWE-405 (Asymmetric Resource Consumption)

**ParentOf:** CWE-1333 (Inefficient Regular Expression Complexity)

**MemberOf:** CWE-1003 (Weaknesses for Simplified Mapping of Published Vulnerabilities)

## Observed Examples (selected)

- **CVE-2021-32617** — C++ image metadata parsing
- **CVE-2020-10735** — Python string-to-int conversion
- **CVE-2020-5243** — ReDoS via User-Agent strings
- **CVE-2014-1474** — Perl email parser
- **CVE-2003-0244** — Hash table collisions

## Relevance to APEX G-46

CWE-407 is the direct home for the Crosby-Wallach hash-collision class, quadratic-parser bugs, and sort worst-case attacks. CWE-1333 (ReDoS) is a child. Findings from APEX's resource-guided fuzzer that are not ReDoS-specific should map here rather than to the umbrella CWE-400.

The notable observed example CVE-2020-10735 is worth internalising: a pure Python built-in (`int(s)`) had O(n²) worst-case behaviour for decimal string→int conversion and was weaponisable against any Python web service that parsed numeric form parameters. It was fixed in Python 3.11 by imposing a default `sys.set_int_max_str_digits(4300)` limit — an interesting *defence-in-depth via input-size cap* rather than an algorithmic fix.
