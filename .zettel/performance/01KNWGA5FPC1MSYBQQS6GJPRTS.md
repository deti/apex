---
id: 01KNWGA5FPC1MSYBQQS6GJPRTS
title: "CAPEC-492: Regular Expression Exponential Blowup"
type: literature
tags: [capec, capec-492, mitre, redos, regex, attack-pattern, security]
links:
  - target: 01KNWE2QA1E37T7GVKSM6EGXXR
    type: references
  - target: 01KNWGA5F4W852RG6C5FJCP204
    type: references
  - target: 01KNWGA5F7VA68B8ZPB6XR0RTE
    type: references
created: 2026-04-10
modified: 2026-04-10
source: "https://capec.mitre.org/data/definitions/492.html"
---

# CAPEC-492: Regular Expression Exponential Blowup

*Source: https://capec.mitre.org/data/definitions/492.html — fetched 2026-04-10.*

## Description

"An adversary may execute an attack on a program that uses a poor Regular Expression (Regex) implementation by choosing input that results in an extreme situation for the Regex."

The attack exploits Nondeterministic Finite Automaton (NFA) implementations that allow backtracking, causing exponential time complexity relative to input size.

## Extended Description

NFA engines evaluate characters multiple times during backtracking. Attackers craft malicious input where every possible path through the NFA is attempted, all resulting in failures. This causes programs to hang or execute extremely slowly.

## Prerequisites

- Ability to identify hosts running poorly implemented regex systems
- Capability to send crafted input to the vulnerable regex

## Mitigations

- Test custom regex with fuzzing
- Implement timeouts on regex-processing operations
- Rewrite identified problematic regex patterns

## Related Weaknesses

- CWE-400 — Uncontrolled Resource Consumption
- CWE-1333 — Inefficient Regular Expression Complexity

## Taxonomy Mappings

- **OWASP**: Regular expression Denial of Service (ReDoS)
- Inherits ATT&CK mappings from parent pattern CAPEC-130

## Relationships

- **ChildOf**: CAPEC-130 (Excessive Allocation)
- **Domains**: Software attacks
- **Mechanisms**: Abuse of existing functionality

## References

- Bryan Sullivan. "Regular Expression Denial of Service Attacks and Defenses" (MSDN Magazine)

## Relevance to APEX G-46

CAPEC-492 provides the **attack-pattern** perspective on the same vulnerability that CWE-1333 frames as a **weakness**. APEX findings should cite both: CWE-1333 for the code defect, CAPEC-492 for the exploitation vector. The CAPEC also explicitly calls out "Test custom regex with fuzzing" as a mitigation — which is exactly what APEX G-46's ReDoS mode is designed to automate.
