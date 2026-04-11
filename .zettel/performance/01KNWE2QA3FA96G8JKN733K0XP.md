---
id: 01KNWE2QA3FA96G8JKN733K0XP
title: Algorithmic Complexity Attacks
type: concept
tags: [security, dos, complexity-attack, cwe-400, hash-collision]
links:
  - target: 01KNWE2Q9YZBAR140ZX5P36TQ5
    type: extends
  - target: 01KNWE2QA0Z52H8VVFAMSA7KGA
    type: related
  - target: 01KNWE2QA1E37T7GVKSM6EGXXR
    type: related
  - target: 01KNWE2QA6QKE8Z152WX8D9XYB
    type: related
  - target: 01KNWE2QAAKZH8GGZ172HZ9RHS
    type: related
  - target: 01KNWEGYB1B15QGYTRC374Z7DQ
    type: extends
  - target: 01KNWEGYB8807ET2427V3VCRJ3
    type: extends
  - target: 01KNWGA5F4W852RG6C5FJCP204
    type: references
  - target: 01KNWGA5F9YTQ01C15QNFHQGPT
    type: references
  - target: 01KNZ2ZDMGGV8NPY88N04STZ6W
    type: references
  - target: 01KNZ3XK3QDTG1BB60XBMTNYFE
    type: references
created: 2026-04-10
modified: 2026-04-12
---

# Algorithmic Complexity Attacks

An **algorithmic complexity attack** is a denial-of-service attack in which an adversary supplies input specifically engineered to drive a target program onto its **worst-case** execution path, consuming disproportionate CPU, memory, or other resources. The class was formally introduced by Scott Crosby and Dan Wallach in their USENIX Security 2003 paper *"Denial of Service via Algorithmic Complexity Attacks"*.

## The seminal result

Crosby and Wallach observed that many programs use data structures whose **average-case** performance is excellent (e.g. hash tables with `O(1)` expected lookup) but whose **worst-case** is terrible (`O(n)` if all inputs land in a single bucket). If the hash function is public and deterministic, an attacker can compute inputs that collide in the same bucket, forcing the data structure onto its worst-case path on every insert and lookup.

They demonstrated the attack practically against:

- **Perl 5** hash tables — a DoS on any CGI script using Perl hashes with user-controlled keys. Could reduce Perl to a crawl with a few thousand crafted strings.
- **Bro IDS** — an intrusion detection system using open-addressed hash tables. Specially crafted network traffic would cause the IDS to spend all its time rebalancing instead of inspecting traffic, effectively blinding it.
- **Squid** and various web caches.

The fix that followed: **randomised or keyed hash functions** (SipHash, Aumasson & Bernstein 2012) that make per-bucket collisions unpredictable without knowing a per-process secret. Perl, Python (3.3+, PEP 456), Ruby, Rust's default `HashMap`, Go's map, and Java's `HashMap` (from 8 onwards for buckets that become trees) all adopted keyed hashing.

## The broader class

Crosby-Wallach hash attacks are the canonical example but the class is much larger:

| Attack vector | Mechanism | Example |
|---|---|---|
| Hash collision DoS | Inputs all hash to same bucket; `O(1)` → `O(n)` per op | CVE-2003 Perl; CVE-2011 many Java/Tomcat/PHP frameworks (28C3) |
| ReDoS | Catastrophic regex backtracking | `^(a+)+$` on `aaaa...b` |
| Quadratic parsing | Parser builds result by repeated concatenation / re-tokenisation | `lxml` pre-2015 text concat; pandoc early versions |
| Sort worst case | Deterministic pivot picks the worst possible partition | Quicksort DoS (killer adversary); triggered by McIlroy's 1999 "A Killer Adversary for Quicksort" |
| Zip bomb / XML bomb / JSON bomb | Decompression expands to astronomical size vs. tiny input | 42.zip; "billion laughs" XML; nested JSON arrays |
| Exponential regex / backref | Regex with backreferences → NP-hard in general | PCRE backreference with nesting |
| Tree rebalancing storms | Sequential insert pattern triggers worst-case rebalance | Red-black tree / AVL under monotonic input |
| Memoisation poisoning | Cache-key collision evicts hot entries → O(computation) per miss | Web cache pollution |
| Resource exhaustion via unbounded structures | Unlimited list / connection / FD growth | Slowloris, billion laughs, zip bombs |

Each of these shares a common shape: **the attacker chooses the input, not the code path; the code path follows from the input.** If the worst-case path exists in the code, the attacker can find a way to it.

## Why static analysis doesn't catch them

A complexity attack is not a memory-safety bug, not a type error, not a logic bug. The code is perfectly correct — it just has a super-linear worst case that the adversary can reach. Static analysers trained on buffer overflows, tainted data flow, or null dereferences don't flag these patterns unless they specifically look for complexity risk signatures (nested loops with data-dependent bounds, regex with nested quantifiers, hash operations on user-controlled keys, recursive parsers without depth limits).

## CWE / CVSS mapping

- **CWE-400** — Uncontrolled Resource Consumption (the umbrella class, 2024 CWE Top 25 rank 24).
- **CWE-1333** — Inefficient Regular Expression Complexity (ReDoS; child of 400).
- **CWE-407** — Inefficient Algorithmic Complexity (direct match for Crosby-Wallach).
- **CWE-789** — Memory Allocation with Excessive Size Value.
- **CWE-834** — Excessive Iteration.

CVSS for these usually lands at **Availability: High**, **Attack Vector: Network** (if the input reaches from untrusted sources), with Confidentiality and Integrity both `None` — so base scores land in the 5.3–7.5 range.

## Mitigations catalogue

- Keyed / randomised hash functions (SipHash, Wyhash).
- Non-backtracking regex engines (RE2, Rust `regex`, Hyperscan).
- Input size limits at trust boundaries.
- Recursion depth limits, timeout walls, per-request CPU budgets.
- Algorithms with **deterministic worst case** (IntroSort for pivot-hardening, Timsort for data-sensitive sorting).
- Streaming / bounded-memory parsers.
- Quotas: per-IP, per-connection, per-tenant.

## References

- Crosby, Wallach — "Denial of Service via Algorithmic Complexity Attacks" — USENIX Security 2003 — [paper](https://www.usenix.org/legacy/event/sec03/tech/full_papers/crosby/crosby.pdf)
- Aumasson, Bernstein — "SipHash: A Fast Short-Input PRF" — INDOCRYPT 2012
- McIlroy — "A Killer Adversary for Quicksort" — SP&E 1999
- 28C3 — "Effective Denial of Service attacks against web application platforms" — [CCC 2011](https://events.ccc.de/congress/2011/Fahrplan/events/4680.en.html)
- MITRE CWE-400 — [cwe.mitre.org/data/definitions/400.html](https://cwe.mitre.org/data/definitions/400.html)
- MITRE CWE-407 — [cwe.mitre.org/data/definitions/407.html](https://cwe.mitre.org/data/definitions/407.html)
