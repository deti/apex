---
id: 01KNWGA5GWXMD72YP3CCKAVT1N
title: "Wikipedia: SipHash (Hash-Flood Defence)"
type: literature
tags: [wikipedia, siphash, aumasson, bernstein, hash-collision, prf, mac]
links:
  - target: 01KNWE2QAAKZH8GGZ172HZ9RHS
    type: references
  - target: 01KNWEGYB8807ET2427V3VCRJ3
    type: related
  - target: 01KNZ2ZDKXQR1QP854KBJGKVEC
    type: references
created: 2026-04-10
modified: 2026-04-12
source: "https://en.wikipedia.org/wiki/SipHash"
---

# SipHash (Wikipedia)

*Source: https://en.wikipedia.org/wiki/SipHash — fetched 2026-04-10.*

## Overview

SipHash is an ARX (add–rotate–xor) based pseudorandom function family created by Jean-Philippe Aumasson and Daniel J. Bernstein in 2012. It emerged as a response to widespread "hash flooding" denial-of-service attacks in late 2011 — most famously demonstrated at the 28C3 conference against every major web platform.

## Key Characteristics

**Design Purpose**: SipHash functions as a secure pseudorandom function (PRF) and message authentication code (MAC). Critically, it differs from general-purpose hash functions like SHA: "SipHash instead guarantees that, having seen Xᵢ and SipHash(Xᵢ, k), an attacker who does not know the key k cannot find (any information about) k or SipHash(Y, k)."

**Technical Specifications**: The algorithm produces either a 64-bit or 128-bit message authentication code from variable-length messages using a 128-bit secret key. Functions are denoted as **SipHash-c-d**, where `c` represents rounds per message block and `d` represents finalisation rounds. The recommended variant is **SipHash-2-4** for performance, with **SipHash-4-8** for conservative security.

## Defence Against Hash Flooding

SipHash protects hash table implementations against denial-of-service attacks. Traditional unkeyed hash functions are vulnerable: attackers can deliberately craft inputs producing identical hash values, overwhelming servers through collision-based attacks.

By requiring a secret key, SipHash prevents adversaries from predicting collisions, thereby securing hash table operations against such exploits.

## Widespread Adoption

**Programming Languages**: Python (3.4+, PEP 456), Ruby, Rust, Swift, Node.js, OCaml, and Perl all incorporate SipHash as the default string-hashing primitive for hash tables.

**Operating Systems**: Linux (systemd), OpenBSD, and FreeBSD use it for hash-table protection.

**Other Applications**: Bitcoin uses it for transaction IDs; IPFS employs it in Bloom filter implementations.

## Licensing

"The reference code of SipHash is released under CC0 licence, a public domain-like licence."

## Relevance to APEX G-46

SipHash is the defence side of the hash-collision DoS story. APEX's static detector for hash-collision vulnerabilities should:

1. **Identify the hash function in use** — CPG analysis of the `HashMap` / `HashSet` / equivalent construction and its hasher argument.
2. **Classify as safe or unsafe** — a keyed PRF like SipHash, Wyhash (keyed mode), aHash, or Marvin32 = safe; anything deterministic and public (Java's pre-8 `String.hashCode`, FNV, MurmurHash2 unkeyed, CRC32) = unsafe on attacker-controlled keys.
3. **Check for attacker reachability** — if the unsafe map's keys are tainted from an untrusted source, emit a CWE-407 finding.
4. **Recommend mitigation** — specifically name the language-appropriate keyed alternative (Rust's default `HashMap`, Python's `dict` post-PEP-456, Java's HashMap with tree-bucket fallback).

SipHash's adoption history is also a useful timeline for reasoning about legacy code: anything on Python < 3.4, Perl < 5.18, Ruby < 2.1, or Java < 8 predates widespread keyed-hash rollout and may still be vulnerable.
