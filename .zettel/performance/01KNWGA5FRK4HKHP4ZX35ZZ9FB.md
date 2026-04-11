---
id: 01KNWGA5FRK4HKHP4ZX35ZZ9FB
title: "CAPEC-147: XML Ping of the Death"
type: literature
tags: [capec, capec-147, mitre, xml, dos, attack-pattern, security]
links:
  - target: 01KNWE2QA6QKE8Z152WX8D9XYB
    type: references
  - target: 01KNWGA5F4W852RG6C5FJCP204
    type: references
  - target: 01KNWGA5G0GB0F6EZHMWYQW7MP
    type: references
created: 2026-04-10
modified: 2026-04-10
source: "https://capec.mitre.org/data/definitions/147.html"
---

# CAPEC-147: XML Ping of the Death

*Source: https://capec.mitre.org/data/definitions/147.html — fetched 2026-04-10.*

## Description

"An attacker initiates a resource depletion attack where a large number of small XML messages are delivered at a sufficiently rapid rate to cause a denial of service or crash of the target."

The attack leverages SOAP transactions and XML processing overhead to deplete resources more efficiently than basic flooding attacks.

## Attack Steps

**Explore Phase:**
- Survey target to identify web services processing XML requests
- Use automated tools or manual browser analysis to locate vulnerable endpoints

**Exploit Phase:**
- "Send a large number of crafted small XML messages to the target URL" at rapid rates to overwhelm the application

## Prerequisites

The target must receive and process XML transactions.

## Skills Required

- **Low Level:** Sending small XML messages
- **High Level:** Operating distributed networks for large-scale attacks

## Resources Required

Transaction generators and sufficient bandwidth to deliver messages rapidly. Larger targets may require distributed attack infrastructure.

## Consequences

- **Scope:** Availability
- **Impact:** "Resource Consumption" leading to denial of service

## Mitigations

- Implement throttling mechanisms and resource allocation limits
- Deploy timeout mechanisms for incomplete transactions
- Apply network flow control and traffic shaping

## Example Instance

Attack against a `createCustomerBillingAccount` web service receiving simultaneous requests with nonsense billing data, causing service unavailability.

## Related Weaknesses

- CWE-400 — Uncontrolled Resource Consumption
- CWE-770 — Allocation of Resources Without Limits or Throttling

## Parent Attack Pattern

CAPEC-528 — XML Flood

## Relevance to APEX G-46

CAPEC-147 captures the volumetric aspect of XML-DoS, complementary to the billion-laughs structural attack. Both land on CWE-400. Together they illustrate that XML parsers need *two* orthogonal defences: structural entity limits (billion laughs defence) and volumetric rate limits (CAPEC-147 defence). APEX should generate witnesses for both classes.
