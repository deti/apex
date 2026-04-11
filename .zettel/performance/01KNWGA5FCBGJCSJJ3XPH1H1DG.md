---
id: 01KNWGA5FCBGJCSJJ3XPH1H1DG
title: "CWE-834: Excessive Iteration"
type: literature
tags: [cwe, cwe-834, mitre, loop, iteration, dos, security]
links:
  - target: 01KNWE2Q9YZBAR140ZX5P36TQ5
    type: references
  - target: 01KNWE2QA6QKE8Z152WX8D9XYB
    type: references
  - target: 01KNWE2QABV7943DKAXTARJHXA
    type: references
  - target: 01KNWGA5F4W852RG6C5FJCP204
    type: references
created: 2026-04-10
modified: 2026-04-10
source: "https://cwe.mitre.org/data/definitions/834.html"
---

# CWE-834: Excessive Iteration

*Source: https://cwe.mitre.org/data/definitions/834.html — fetched 2026-04-10.*

## Description

"The product performs an iteration or loop without sufficiently limiting the number of times that the loop is executed."

## Extended Description

"If the iteration can be influenced by an attacker, this weakness could allow attackers to consume excessive resources such as CPU or memory. In many cases, a loop does not need to be infinite in order to cause enough resource consumption to adversely affect the product or its host system; it depends on the amount of resources consumed per iteration."

## Common Consequences

**Impact: DoS: Resource Consumption (CPU); DoS: Resource Consumption (Memory); DoS: Amplification; DoS: Crash, Exit, or Restart**

Scope: Availability

"Excessive looping will cause unexpected consumption of resources, such as CPU cycles or memory. The product's operation may slow down, or cause a long time to respond. If limited resources such as memory are consumed for each iteration, the loop may eventually cause a crash or program exit due to exhaustion of resources, such as an out-of-memory error."

## Relationships

- **ChildOf:** CWE-691 (Insufficient Control Flow Management)
- **ParentOf:** CWE-674 (Uncontrolled Recursion), CWE-835 (Infinite Loop), CWE-1322 (Blocking Code in Non-blocking Context)
- **CanFollow:** CWE-606 (Unchecked Loop Condition), CWE-1339 (Floating Point Precision Issues)

## Observed Examples

- **CVE-2011-1027** — Off-by-one error leading to infinite loop using invalid hex characters
- **CVE-2006-6499** — Web browser crash from "bad looping logic relying on floating point math to exit the loop"

## Relevance to APEX G-46

CWE-834 is the natural target for any fuzzer-discovered input that drives a loop to iterate super-linearly in input size, *without* the loop being wrapped in a regex. It's also the mapping for uncontrolled-recursion discoveries (via CWE-674 child). The APEX static pre-analysis step described in the G-46 spec — "nested loops with data-dependent bounds" — directly feeds this CWE.
