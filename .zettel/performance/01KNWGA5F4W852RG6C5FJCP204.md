---
id: 01KNWGA5F4W852RG6C5FJCP204
title: "CWE-400: Uncontrolled Resource Consumption"
type: literature
tags: [cwe, cwe-400, mitre, resource-exhaustion, dos, security, top25-2024]
links:
  - target: 01KNWE2Q9YZBAR140ZX5P36TQ5
    type: references
  - target: 01KNWE2QA1E37T7GVKSM6EGXXR
    type: references
  - target: 01KNWE2QA3FA96G8JKN733K0XP
    type: references
  - target: 01KNWE2QA6QKE8Z152WX8D9XYB
    type: references
  - target: 01KNWGA5F9YTQ01C15QNFHQGPT
    type: references
  - target: 01KNWGA5FCBGJCSJJ3XPH1H1DG
    type: references
  - target: 01KNWGA5FEAC0QN3PK6CAYP7T8
    type: references
  - target: 01KNWGA5FPC1MSYBQQS6GJPRTS
    type: references
  - target: 01KNWGA5FRK4HKHP4ZX35ZZ9FB
    type: references
  - target: 01KNWGA5G0GB0F6EZHMWYQW7MP
    type: references
  - target: 01KNYZ7YKMA92CCFZ16HD1H8J5
    type: references
  - target: 01KNZ301FV1M4DYXV6PN9BD1YZ
    type: references
  - target: 01KNZ2ZDMJNCKQ2AZEYXENBX53
    type: references
  - target: 01KNZ2ZDM5DNHY26MRQ9V2BPKT
    type: references
created: 2026-04-10
modified: 2026-04-12
source: "https://cwe.mitre.org/data/definitions/400.html"
---

# CWE-400: Uncontrolled Resource Consumption

*Source: https://cwe.mitre.org/data/definitions/400.html — fetched 2026-04-10.*

## Description
The product does not properly control the allocation and maintenance of a limited resource, creating conditions where attackers can exhaust system resources and cause denial of service.

## Common Consequences

**Availability Impact:** If attackers trigger unlimited resource allocation without controls, the most common result is denial of service — preventing valid users from accessing the product. The system may slow down, crash due to unhandled errors, or lock out legitimate users.

**Access Control Impact:** In some cases, resource exhaustion may force the product to "fail open," potentially compromising security functionality and the overall system state.

## Potential Mitigations

**Architecture and Design:**
- Design throttling mechanisms into system architecture
- Implement strong authentication and access control models
- Protect login applications against DoS attacks
- Limit database access through caching result sets
- Track request rates from users and block those exceeding thresholds
- Recognize attacks and deny further access for defined periods
- Ensure protocols have specific scale limits

**Implementation:**
- Ensure all resource allocation failures place the system into a safe posture

## Key Weaknesses (Parent-Child Relationships)

**Parents:** CWE-664 (Improper Control of a Resource Through its Lifetime)

**Children include:**
- CWE-405: Asymmetric Resource Consumption (Amplification)
- CWE-770: Allocation of Resources Without Limits or Throttling
- CWE-771: Missing Reference to Active Allocated Resource
- CWE-779: Logging of Excessive Data
- CWE-920: Improper Restriction of Power Consumption

## Common Scenarios

Resource exhaustion occurs through:
- Lack of throttling for allocated resource numbers
- Loss of all references to resources before shutdown
- Failure to close/return resources after processing
- Error conditions and exceptional circumstances not properly handled

## Demonstrative Examples (from the CWE entry)

- **Example 1 (Java):** A Worker class executes runnables with no limits on the number created, allowing rapid resource exhaustion.
- **Example 2 (C):** A socket server forks processes for each connection without tracking or limiting connections, exhausting CPU and memory.
- **Example 3 (C):** File writing from socket data with no size limits can exhaust disk resources.
- **Example 4 (C):** Processing message bodies with unsanitized length values causes excessive memory allocation.
- **Example 5 (Java):** Creating unlimited client threads without a thread pool overwhelms system resources.
- **Example 6 (Go):** Reading entire request bodies without size limits causes memory exhaustion.

## Observed Examples (Selected CVEs)

- **CVE-2020-7218** — Go orchestrator lacks resource limits with unauthenticated connections
- **CVE-2020-3566** — Insufficient IGMP queue management causes exhaustion
- **CVE-2009-2874** — Large connection numbers trigger crashes
- **CVE-2009-1928** — Malformed requests trigger uncontrolled recursion
- **CVE-2008-2121** — TCP SYN flood attacks exhaust CPU resources

## Detection Methods (per CWE)

- **Automated Static Analysis:** Limited utility except for system resources (files, sockets, processes)
- **Automated Dynamic Analysis:** Moderate effectiveness through generating high-volume requests
- **Fuzzing:** May inadvertently reveal resource exhaustion when tests aren't restarted between cases

## Mapping Notes

**Status:** DISCOURAGED for mapping real-world vulnerabilities due to frequent misuse. Analyse the specific underlying mistake causing resource consumption and map to more precise child CWEs like CWE-770, CWE-771, CWE-410, or CWE-834.

## Relevance to APEX G-46

This is the umbrella CWE the performance-test-generation spec targets. The "Detection Methods" section explicitly acknowledges that fuzzers sometimes reveal resource exhaustion as a side effect of crash-hunting runs — APEX G-46 proposes making it a *first-class* target rather than an accident, by replacing the coverage feedback with a resource-maximising feedback. CWE-400 findings produced by APEX should map to a more specific child (770/834/1333) when possible, per MITRE's own guidance.
