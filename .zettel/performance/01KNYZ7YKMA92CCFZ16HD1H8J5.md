---
id: 01KNYZ7YKMA92CCFZ16HD1H8J5
title: "Wikipedia: Slowloris (Slow HTTP DoS)"
type: literature
tags: [wikipedia, slowloris, slow-http, dos, cwe-400, apache, nginx]
links:
  - target: 01KNWE2QA6QKE8Z152WX8D9XYB
    type: related
  - target: 01KNWGA5F4W852RG6C5FJCP204
    type: references
  - target: 01KNWGA5FYGP8SMYTVYNRY8W62
    type: references
created: 2026-04-10
modified: 2026-04-10
source: "https://en.wikipedia.org/wiki/Slowloris_(computer_security)"
---

# Slowloris — Slow HTTP DoS (Wikipedia)

*Source: https://en.wikipedia.org/wiki/Slowloris_(computer_security) — fetched 2026-04-10.*

## Description

Slowloris is denial-of-service software that "allows a single machine to take down another machine's web server with minimal bandwidth." The tool gets its name from slow lorises — primates known for deliberate, measured movement.

## Attack Mechanism

The attack works by maintaining numerous open connections to a target server. It "send[s] a partial request" and periodically adds HTTP headers "never completing the request." This exhausts the server's connection pool, preventing legitimate users from connecting. Crucially, each attacker connection consumes one server-side worker thread or connection slot, but effectively zero bandwidth on the attacker side — a 1 KB/minute dribble is enough to hold the connection open.

## Vulnerable Software

Multiple web servers are susceptible, including:

- Apache 1.x/2.x
- IIS 6.0 and earlier
- Nginx versions up to 1.5.9
- Flask's development server

Some systems have partial vulnerabilities during TLS handshake processes.

## Mitigation Strategies

Defences include:

- **Apache modules** like `mod_reqtimeout` (official solution since 2.2.15)
- **Reverse proxies, firewalls, and load balancers** with their own slow-connection detection
- **Alternative servers** — nginx, lighttpd, Caddy resist this attack type by design (event-driven architectures)
- **Per-IP connection limits** and **minimum transfer-speed thresholds**

## Notable Usage

During 2009 Iranian election protests, activists deployed Slowloris against government websites. Its low-bandwidth nature made it preferable to traditional DDoS attacks that would disrupt broader internet access — an interesting case of the asymmetry being useful to the *attacker* for collateral-damage reasons rather than just efficiency.

## Relevance to APEX G-46

Slowloris is a *workload-shape* attack rather than a single-input attack. You cannot catch it with APEX G-46's resource-guided fuzzer because the pathology is in the *arrival pattern* (many slow connections held open simultaneously), not in any individual request. This is why the G-46 spec explicitly marks full load/stress testing with concurrent request simulation as **out of scope**.

However, APEX can still help:

1. **Static detection** — flag any HTTP server configuration that lacks request-receive timeouts (`mod_reqtimeout`, nginx `client_header_timeout`, etc.) or per-IP connection limits.
2. **Code-level detection** — flag threaded / blocking server patterns where one slow request holds a worker thread (`socket.recv()` on a request thread without a timeout).
3. **Architecture recommendation** — suggest event-driven alternatives.

So Slowloris belongs in APEX's "recognise the pattern, recommend mitigation" static-analysis bucket, not its dynamic-fuzzing bucket.
