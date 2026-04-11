---
id: 01KNWGA5FYGP8SMYTVYNRY8W62
title: OWASP Denial of Service Cheat Sheet
type: literature
tags: [owasp, dos, cheat-sheet, defence, mitigation, rate-limiting]
links:
  - target: 01KNWE2QA6QKE8Z152WX8D9XYB
    type: references
created: 2026-04-10
modified: 2026-04-10
source: "https://cheatsheetseries.owasp.org/cheatsheets/Denial_of_Service_Cheat_Sheet.html"
---

# OWASP Denial of Service Cheat Sheet

*Source: https://cheatsheetseries.owasp.org/cheatsheets/Denial_of_Service_Cheat_Sheet.html — fetched 2026-04-10.*

## Introduction

This cheat sheet presents a methodology for addressing denial-of-service (DoS) attacks across multiple layers. It acknowledges that anti-DoS methods cannot be one-step solutions and emphasises that developers and infrastructure architects must develop DoS solutions carefully.

### Fundamentals

"Availability" is a fundamental component of the CIA triad. The guidance stresses that "if every part of the computing system within the interoperability flow does not function correctly, your infrastructure suffers."

A successful DoS attack hinders system availability and can render entire systems inaccessible. The guidance strongly recommends "a thorough analysis on components within your inventory based on functionality, architecture and performance (i.e. application-wise, infrastructure and network related)."

The DoS system inventory should identify potential attack points and single points of failure, ranging from programming errors to resource exhaustion. "A solid understanding of your environment is essential to develop suitable defence mechanisms" aligned with:

1. Scaling options (vertical through hardware, horizontal through additional components)
2. Conceptual / logical techniques like redundancy and bulk-heading
3. Cost analysis appropriate to the situation

### Analysing DoS Attack Surfaces

The cheat sheet uses CERT-EU's DDOS classification based on the seven-layer OSI model, examining three main attack categories: Application, Session, and Network.

#### Overview of Potential DoS Weaknesses

Three primary attack categories:

- **Application attacks**: focus on rendering applications unavailable by exhausting resources or making them functionally unusable
- **Session attacks**: target server resources or intermediary equipment like firewalls and load-balancers
- **Network attacks**: focus on saturating network bandwidth

**Physical Layer DoS**: system destruction, obstruction, and malfunction of networking hardware transmission technologies.

**Data Layer DoS**: protocol-level attacks. MAC flooding fills a switch's MAC-to-port table, purging valid entries and forcing the switch into hub mode ("all data is forwarded to all ports, resulting in a data leakage"). **ARP Poisoning**: spoofed ARP messages link the attacker's MAC to a legitimate device's IP, letting them intercept, modify, or stop data. Mitigation: packet filtering or static ARP tables.

## Application Attacks

"Application layer attacks usually make applications unavailable by exhausting system resources or by making it unusable in a functional way." These attacks need not consume network bandwidth to be effective; they place operational strain on application servers to render them unavailable or non-functional. All OSI layer 7 protocol stack weaknesses are categorised as application attacks and are "the most challenging to identify/mitigate."

**Slow HTTP Attacks**: "Slow HTTP attacks deliver HTTP requests very slow and fragmented, one at a time." The server maintains stalled resources while waiting for complete delivery. Once maximum concurrent connections are reached, DoS occurs. These attacks are "cheap to perform because they require minimal resources." Slowloris is the canonical example.

### Software Design Concepts

- **Using validation that is cheap in resources first**: reduce resource impact as soon as possible; perform more expensive CPU, memory, and bandwidth validation afterwards.
- **Employing graceful degradation**: continue some level of functionality when system portions fail. Fault-tolerant design enables continued operation "possibly at a reduced level, rather than failing completely if parts of the system fails".
- **Prevent single point of failure**: "Detecting and preventing single points of failure (SPOF) is key to resisting DoS attacks." Employ stateless components, redundant systems, bulkheads to prevent failure spread, and ensure survival when external services fail.
- **Avoid highly CPU-consuming operations**: when DoS occurs, CPU-intensive operations become performance drags and failure points. Review code performance issues, including language-inherent problems.
- **Handle exceptions**: DoS attacks likely trigger exceptions; systems must handle them gracefully.
- **Protect overflow and underflow**: since these vulnerabilities lead to exploits, prevention is essential.
- **Threading**: avoid operations requiring large task completion before proceeding; use asynchronous operations.
- **Identify resource-intensive pages and plan ahead**.

### Session

- **Limit server side session time based on inactivity and a final timeout**: "also an important measure to prevent resource exhaustion".
- **Limit session bound information storage**: the less data linked to a session, the less burden it places on webserver performance.

### Input Validation

- **Limit file upload size and extensions**: prevents DoS on file storage or web application functions using uploads (image resizing, PDF creation, etc.).
- **Limit total request size**: prevents resource-consuming DoS attacks.
- **Prevent input based resource allocation**: prevents resource exhaustion through DoS.
- **Prevent input based function and threading interaction**: user input can determine function execution frequency and CPU intensity. Unfiltered input for resource allocation enables DoS through resource exhaustion.
- **Input-based puzzles**: captchas and math problems protect web forms from functionality abuse (mailbox flooding, for example), but "this kind of technology will not help defend against DoS attacks".

### Access Control

- **Authentication as a means to expose functionality**: the principle of least privilege prevents DoS by denying attackers access to potentially damaging functions.
- **User lockout**: attackers can exploit login failure mechanisms to cause DoS by triggering account lockouts.

## Network Attacks

Network attacks involve bandwidth saturation and volumetric attacks utilising amplification techniques.

### Network Design Concepts

- **Preventing single point of failure** (as above).
- **Caching**: data storage enabling faster future request serving. Increased cached data makes applications more resilient to bandwidth exhaustion.
- **Static resources hosting on a different domain**: reduces HTTP requests on web applications; images and JavaScript typically load from separate domains.

### Rate Limiting

Rate limiting controls traffic rates from and to servers or components at infrastructure or application levels, potentially based on offending IPs, IP block lists, geolocation, etc.

- **Define a minimum ingress data rate limit** and drop connections below that rate. Setting the limit too low impacts legitimate clients; inspect logs to establish genuine traffic baselines (protects against slow HTTP attacks).
- **Define an absolute connection timeout**.
- **Define a maximum ingress data rate limit** and drop connections exceeding it.
- **Define a total bandwidth size limit** to prevent bandwidth exhaustion.
- **Define a load limit** specifying maximum concurrent users accessing any given resource.

### ISP-Level Remediations

- **Filter invalid sender addresses using edge routers**, consistent with RFC 2267, to filter IP-spoofing attacks bypassing block lists.
- **Check your ISP services regarding DDOS beforehand**: evaluate support for multiple internet access points, sufficient bandwidth (xx-xxx Gbit/s), and special hardware for traffic analysis and application-level defence.

### Global-Level Remediations: Commercial Cloud Filter Services

- Consider filter services for resisting larger attacks (up to 500 GBit/s).
- **Filter services** employ different mechanics to filter malicious or non-compliant traffic.
- **Comply with relevant data protection / privacy laws**: many providers route traffic through USA / UK.

## Relevance to APEX G-46

This cheat sheet is the defender's view of the problem APEX is attacking from the other side. Its guidance on "software design concepts" (cheap-first validation, CPU-budget awareness, exception handling, input-size limits) maps directly to the sort of code-smells APEX's static pre-analysis phase should flag. The session and input-validation sections are particularly relevant: any route that allocates unbounded resources from user-controllable parameters is a prime APEX target.
