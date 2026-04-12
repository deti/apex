---
id: 01KNWGA5FEAC0QN3PK6CAYP7T8
title: "CWE-789: Memory Allocation with Excessive Size Value"
type: literature
tags: [cwe, cwe-789, mitre, memory, allocation, dos, security]
links:
  - target: 01KNWE2Q9YZBAR140ZX5P36TQ5
    type: references
  - target: 01KNWE2QA6QKE8Z152WX8D9XYB
    type: references
  - target: 01KNWGA5F4W852RG6C5FJCP204
    type: references
  - target: 01KNZ301FV1M4DYXV6PN9BD1YZ
    type: references
  - target: 01KNZ2ZDMJNCKQ2AZEYXENBX53
    type: references
created: 2026-04-10
modified: 2026-04-12
source: "https://cwe.mitre.org/data/definitions/789.html"
---

# CWE-789: Memory Allocation with Excessive Size Value

*Source: https://cwe.mitre.org/data/definitions/789.html — fetched 2026-04-10.*

## Description

"The product allocates memory based on an untrusted, large size value, but it does not ensure that the size is within expected limits, allowing arbitrary amounts of memory to be allocated."

## Common Consequences

**Impact: DoS: Resource Consumption (Memory)**

Scope: Availability

"Not controlling memory allocation can result in a request for too much system memory, possibly leading to a crash of the application due to out-of-memory conditions, or the consumption of a large amount of memory on the system."

## Potential Mitigations

**Phase: Implementation; Architecture and Design**

"Perform adequate input validation against any value that influences the amount of memory that is allocated. Define an appropriate strategy for handling requests that exceed the limit, and consider supporting a configuration option so that the administrator can extend the amount of memory to be used if necessary."

**Phase: Operation**

"Run your program using system-provided resource limits for memory. This might still cause the program to crash or exit, but the impact to the rest of the system will be minimized."

## Relationships

- **ChildOf:** CWE-770 (Allocation of Resources Without Limits or Throttling)
- **PeerOf:** CWE-1325 (Improperly Controlled Sequential Memory Allocation)
- **CanFollow:** CWE-129 (Improper Validation of Array Index) and CWE-1284 (Improper Validation of Specified Quantity in Input)
- **CanPrecede:** CWE-476 (NULL Pointer Dereference)

## Observed Examples

- **CVE-2019-19911** — Python library fails to limit resources for images with excessive bands, causing memory exhaustion or integer overflow
- **CVE-2010-3701** — Program using alloca() for encoding triggers segfault with large messages
- **CVE-2008-1708** — Large length field values cause memory consumption and daemon exit
- **CVE-2008-0977** — Large length field leads to memory consumption and crash when memory depleted
- **CVE-2006-3791** — Large key size in game program crashes when resizing function cannot allocate memory
- **CVE-2004-2589** — Large Content-Length HTTP header crashes instant messaging application due to allocation failure

## Relevance to APEX G-46

CWE-789 is triggered by a single input field (a size/length) directly controlling an allocation — billion-laughs, "Content-Length exhaustion", and "image dimensions exhaustion" all land here. APEX's static pre-analysis should flag any allocation size that flows from an untrusted source without a clamp; the resource-guided fuzzer should then generate maximum legal values for that field to verify exploitability. This is one of the highest-precision detection targets in the G-46 portfolio.
