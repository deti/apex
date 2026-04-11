---
id: 01KNZ2ZDKVZ6WW8VF9NXDW0XWG
title: "28C3: Effective DoS Attacks Against Web Application Platforms (Klink and Wälde, 2011)"
type: literature
tags: [hash-collision, dos, 28c3, php, java, aspnet, python, ruby, nodejs, cve]
links:
  - target: 01KNWE2QAAKZH8GGZ172HZ9RHS
    type: references
  - target: 01KNWEGYB8807ET2427V3VCRJ3
    type: extends
  - target: 01KNYZ7YKS8ARHAT2C0GPQCDPX
    type: related
created: 2026-04-12
modified: 2026-04-12
source: "https://fahrplan.events.ccc.de/congress/2011/Fahrplan/events/4680.en.html"
---

# 28C3 — "Effective DoS Attacks Against Web Application Platforms"

*Source: https://fahrplan.events.ccc.de/congress/2011/Fahrplan/events/4680.en.html — fetched 2026-04-12.*
*Speakers: Alexander "alech" Klink and Julian "zeri" Wälde. 28th Chaos Communication Congress (28C3), Berlin, 28 December 2011.*

## The talk in one sentence

A common flaw in how virtually every popular web programming language and framework implements hash tables lets a single attacker saturate 99% of a server's CPU for an extended period using one HTTP request of modest size.

## The eight-year delay

Crosby and Wallach published the same vulnerability class in 2003 (see `01KNWEGYB8807ET2427V3VCRJ3`). Perl patched. Most others didn't — the attack sat dormant for eight years. Klink and Wälde's 28C3 talk is the inflection point: after their demo, every major platform shipped a fix within weeks. The talk is why your Python 3.4, your Java 8, and your Rust `HashMap` use keyed hashes today.

## Affected platforms (all demonstrated live)

| Platform | Vector | Fix in |
|---|---|---|
| PHP 5.3 / 5.4 | `$_POST` parameter parsing | 5.3.9 `max_input_vars` |
| ASP.NET | `Request.Form` | MS11-100 hotfix, randomised hash Dec 2011 |
| Java (Tomcat, Jetty, Glassfish) | HTTP headers + form params via `HashMap` | Java 7u6/8: per-process hash seed + tree-bucketing |
| Python 2.x / 3.2 | CGI form parsing, `dict` literal parsing | PEP 456 / SipHash in 3.4, `-R` randomisation backport |
| Ruby (MRI 1.8/1.9) | Rack parameter parsing | MRI 1.9.3 randomised hash |
| Node.js | `querystring` parser | 0.6.12 randomised hash |
| CRuby, Perl 5 pre-5.18 | Generic `Hash` | 5.18 randomised seed |

The related CVEs: **CVE-2011-4815** (Ruby), **CVE-2011-4838** (Jetty), **CVE-2011-4461** (Glassfish), **CVE-2011-4885** (PHP), **CVE-2012-2739** (Oracle Java JDK), **CVE-2011-5034** (Jetty), **CVE-2011-5036** (Plone/Python). Eight CVEs closing in on the same root cause is characteristic of a cross-ecosystem flaw.

## The proof-of-concept

A ~500 KB to 2 MB POST body with carefully chosen colliding string keys. On a single HTTP worker the CPU spikes to ~100% for 10-60 seconds while the table is built and walked. At a modest 100 requests per second a lone attacker saturates ~1000 server cores. The attack is **amplification-based** — the attacker's cost (bandwidth) grows linearly with the body; the victim's cost grows quadratically with the key count.

## Advisories

- **n.runs-SA-2011.004** — n.runs (Klink and Wälde's employer) vendor-neutral advisory, published 28 December 2011.
- **oCERT-2011-003** — Open Source CERT coordinated disclosure memo.

These are the authoritative technical writeups; they contain the exact colliding-key generators for each affected language.

## Why the attack works (recap of the recipe)

1. A non-keyed hash function is a **deterministic public function**.
2. Given the source code (all affected languages are open source or the algorithms are documented), an attacker can compute pre-images.
3. For rolling polynomial hashes (Java's pre-8 `String.hashCode`, PHP's DJBX33A) the collision set is algebraic — you don't brute-force, you solve.
4. Once you have `n` keys that all hash to the same bucket, hash-table insert is `O(n²)` because every insert re-walks the collision chain.

## Defences demonstrated in the talk

- **Keyed/universal hashing** — make the hash function a pseudo-random function parameterised by a per-process secret. SipHash is the canonical answer; see `01KNWGA5GWXMD72YP3CCKAVT1N` and `01KNZ2ZDKXQR1QP854KBJGKVEC`.
- **Cap number of request parameters** — Tomcat added `maxParameterCount=10000` as a default. PHP 5.3.9 added `max_input_vars=1000`. Simple and effective as defence-in-depth even with a randomised hash.
- **Bucket-to-tree conversion** — Java 8's `HashMap` converts collision chains of length ≥8 to a red-black tree. Caps the worst case at `O(log n)` even if an attacker defeats the randomised hash.

## Relevance to APEX G-46

This talk is the proof the detector exists for a reason. When APEX flags hash-collision-DoS-adjacent code, the ground truth is:

1. **Check the runtime version** — Python <3.4, Ruby <1.9.3, Node <0.6.12, Java <8: vulnerable by default. Any parser from that era is vulnerable unless explicitly patched.
2. **Check the container config** — missing `max_input_vars`, absent `maxParameterCount`, custom hashmap with MurmurHash/CityHash/FNV on attacker keys. These are all high-signal Findings.
3. **Fuzz the parser** — a performance fuzzer targeting a HTTP parser with "sum-of-collision-chain-length" as the feedback signal should re-derive the 28C3 PoC automatically. This is a natural evaluation benchmark for the G-46 prototype.

## References

- 28C3 event page — [fahrplan.events.ccc.de](https://fahrplan.events.ccc.de/congress/2011/Fahrplan/events/4680.en.html)
- n.runs-SA-2011.004 advisory (linked from the event page)
- Crosby, Wallach — USENIX Security 2003 — see `01KNWEGYB8807ET2427V3VCRJ3`
- PEP 456 — Python's eventual SipHash adoption — see `01KNYZ7YKS8ARHAT2C0GPQCDPX`
