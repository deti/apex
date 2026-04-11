---
id: 01KNYZ7YKS8ARHAT2C0GPQCDPX
title: "Python PEP 456: Secure and Interchangeable Hash Algorithm"
type: literature
tags: [python, pep, pep-456, siphash, hash-collision, security]
links:
  - target: 01KNWE2QAAKZH8GGZ172HZ9RHS
    type: references
  - target: 01KNWGA5GWXMD72YP3CCKAVT1N
    type: references
  - target: 01KNWEGYB8807ET2427V3VCRJ3
    type: related
created: 2026-04-10
modified: 2026-04-10
source: "https://peps.python.org/pep-0456/"
---

# Python PEP 456 — Secure and Interchangeable Hash Algorithm

*Source: https://peps.python.org/pep-0456/ — fetched 2026-04-10.*

## Overview

PEP 456, authored by Christian Heimes and approved by BDFL-Delegate Alyssa Coghlan, proposes adopting SipHash as Python's default string and bytes hashing algorithm. The PEP was finalised for Python 3.4.

## Motivation

Despite previous attempts to address vulnerabilities, CPython remained susceptible to **hash collision denial-of-service attacks**. The existing FNV-based algorithm lacked cryptographic properties necessary to protect against attacks. Researchers Jean-Philippe Aumasson and Daniel J. Bernstein demonstrated how attackers could recover randomisation seeds from the current implementation — meaning even the partial defence of randomised hash seeds was insufficient.

Additional concerns included:

- Hard-coded, duplicated implementations across multiple Unicode representations
- Poor performance on modern processors
- Inflexibility for embedders needing alternative algorithms

## Hash Function Requirements

The proposal established these criteria:

1. Process memory blocks from 1 byte to maximum `ssize_t` values
2. Produce at least 32 bits on 32-bit systems, 64 bits on 64-bit systems
3. Support unaligned memory hashing
4. Ensure input length influences output

## Algorithm Evaluation

**SipHash** — Cryptographic pseudo-random function with 128-bit seed and 64-bit output, designed by Aumasson and Bernstein. Widely adopted by Ruby, Perl, OpenDNS, Rust, and Redis.

**MurmurHash** — Non-cryptographic but fast. However, "Aumasson, Bernstein and Boßlet have shown ... Murmur3 is not resilient against hash collision attacks." This ruled out MurmurHash as a standalone default.

**CityHash** — Similar vulnerabilities identified as MurmurHash.

**FNV (Fowler–Noll–Vo)** — Maintained as fallback for platforms lacking 64-bit support.

## Conclusion

SipHash emerged as optimal, balancing speed and security. The proposal enables compile-time algorithm selection while maintaining backward compatibility for hash outputs across ASCII strings and bytes.

## Relevance to APEX G-46

PEP 456 is the "end of the story" for Python hash-collision DoS — once Python 3.4 shipped (March 2014), every Python deployment started getting the SipHash defence automatically. This has two consequences for APEX's detector:

1. **Version gating** — a Python project running 3.3 or earlier should get a "you are vulnerable by default" warning from APEX, independent of code analysis. Anything newer has the defence unless the dev explicitly disabled `PYTHONHASHSEED` or used a custom hashmap.
2. **MurmurHash / CityHash usage** — APEX should still flag Python libraries that use MurmurHash or CityHash explicitly (e.g. `mmh3` package, `cityhash`) on user-controlled keys, since the PEP evaluation explicitly showed these are breakable.

The PEP also illustrates the point made in the Hash Collision note: a non-keyed hash is fundamentally unsafe for user-controlled keys, no matter how fast or well-distributed it is. "Fast" and "collision-resistant-against-adversaries" are different design axes, and you have to pick both if you need both.
