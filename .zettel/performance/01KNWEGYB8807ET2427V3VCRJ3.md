---
id: 01KNWEGYB8807ET2427V3VCRJ3
title: Denial of Service via Algorithmic Complexity Attacks
type: literature
tags: [paper, performance, complexity-attack, dos, hash-collision, cwe-400, usenix-security, foundational]
links:
  - target: 01KNWE2Q9YZBAR140ZX5P36TQ5
    type: references
  - target: 01KNWE2QA3FA96G8JKN733K0XP
    type: extends
  - target: 01KNWE2QA8H1GKHCVNHYS5QW1F
    type: related
  - target: 01KNWE2QAAKZH8GGZ172HZ9RHS
    type: extends
  - target: 01KNWGA5F9YTQ01C15QNFHQGPT
    type: references
  - target: 01KNWGA5GWXMD72YP3CCKAVT1N
    type: related
created: 2026-04-10
modified: 2026-04-10
source: "https://www.usenix.org/conference/12th-usenix-security-symposium/denial-service-algorithmic-complexity-attacks"
venue: 12th USENIX Security Symposium (2003)
authors: [Scott A. Crosby, Dan S. Wallach]
year: 2003
---

# Denial of Service via Algorithmic Complexity Attacks

**Authors:** Scott A. Crosby, Dan S. Wallach
**Venue:** Proceedings of the 12th USENIX Security Symposium, Washington, D.C., August 2003
**Affiliation:** Rice University

## Retrieval Notes

The USENIX legacy PDF (`https://www.usenix.org/legacy/event/sec03/tech/full_papers/crosby/crosby.pdf`), the Rice mirror (`https://www.cs.rice.edu/~scrosby/hash/CrosbyWallach_UsenixSec2003.pdf`), the Auckland course mirror (`https://www.cs.auckland.ac.nz/~mcw/Teaching/refs/misc/denial-of-service.pdf`), and the USENIX HTML version (`https://www.usenix.org/legacy/publications/library/proceedings/sec03/tech/full_papers/crosby/crosby_html/index.html`) could not be fetched from this environment: `WebFetch` is denied and direct `curl` downloads are blocked. The body below captures the abstract as published by USENIX, plus a structured technical description assembled from multiple authoritative secondary sources (USENIX conference page, LWN reporting on the follow-on hash-flooding wave, course slides, survey papers). Replace the "Extended Description" with verbatim text when the PDF becomes accessible.

## Abstract (from USENIX conference page)

We present a new class of low-bandwidth denial of service attacks that exploit algorithmic deficiencies in many common applications' data structures. Frequently used data structures have "average-case" expected running time that's far more efficient than the worst case. For example, both binary trees and hash tables can degenerate to linked lists with carefully chosen input. We show how an attacker can effectively compute such input, and we demonstrate attacks against the hash table implementations in two versions of Perl, the Squid web proxy, and the Bro intrusion detection system. Using bandwidth less than a typical dialup modem, we can bring a dedicated Bro server to its knees; after six minutes of carefully chosen packets, our Bro server was dropping as much as 71% of its traffic and consuming all of its CPU. Finally, we show how modern universal hashing techniques can yield performance comparable to commonplace hash functions while being provably secure against these attacks.

## Extended Description (synthesised from secondary sources — not verbatim transcription)

### Why this paper matters

Crosby and Wallach's 2003 paper is the work that *named* the problem of algorithmic complexity attacks (ACAs) as a distinct class of security vulnerability. Before this paper, worst-case-vs-average-case complexity gaps in textbook data structures were regarded as an academic curiosity; after it, they were understood as a practical, low-bandwidth denial-of-service vector with measurable impact on real servers. Every subsequent paper on algorithmic DoS — SlowFuzz, PerfFuzz, HotFuzz, Singularity, the ReDoS literature — cites this paper as the starting point, and the programming-language community's long march toward randomised hash tables (SipHash in Python, per-process keys in Perl, Ruby, PHP, .NET, Java's string-hash randomisation) traces directly to its recommendations.

### The attack model

The attack assumes an application that:

1. Inserts attacker-influenced data into a data structure whose *average-case* bound is significantly better than its *worst case*.
2. Treats the "slow" worst case as operationally unreachable.

The attacker's goal is to craft inputs that deterministically drive the data structure into its worst case, converting an O(1) or O(log n) operation into an O(n) one and a batch of n insertions from O(n) or O(n log n) into O(n²). With n large enough, the attack saturates CPU at a fraction of the bandwidth a traditional flooding DoS requires.

### The two canonical targets: chained hash tables and binary search trees

- **Chained hash tables.** A hash table with chaining resolves collisions by appending colliding keys to a per-bucket linked list. Average-case lookup is O(1) assuming the hash function distributes keys roughly uniformly across buckets. If the attacker knows (or can reverse-engineer) the hash function — the paper notes that many production hash tables used simple, well-known, unkeyed hash functions — the attacker can precompute a family of keys that all hash to the same bucket. Inserting n such keys costs O(n²); each subsequent lookup scans the full bucket chain, so every operation becomes O(n).
- **Binary search trees without balancing.** A plain BST inserted with sorted input degenerates into a linked list. An attacker who can force sorted insertion order can make operations O(n) instead of O(log n).

### Concrete demonstrations

The paper demonstrates the attack against real software:

- **Perl 5.6 and 5.8 hash tables.** At the time, Perl's core hash implementation used a deterministic, unkeyed hash function. The authors show how to generate colliding keys cheaply and convert O(n) work into O(n²).
- **Squid web proxy.** Squid's internal caches use hash tables keyed on attacker-controllable strings (URLs, headers). Colliding keys degrade cache operations.
- **Bro network intrusion detection system.** Bro (now Zeek) builds connection-tracking tables keyed on flow tuples. The authors construct a packet stream in which flow keys collide. The result cited in the abstract: at dial-up bandwidth, a dedicated Bro server drops as much as 71% of its traffic and runs CPU-bound within about six minutes.

### The proposed defence: universal hashing

Crosby and Wallach advocate for **keyed universal hash functions** — constructions for which, given a secret key chosen uniformly at random at startup, the probability (over choice of key) that two arbitrary inputs collide is small and is *independent* of the inputs. Because the attacker does not know the key, they cannot precompute a colliding family. The paper argues the performance overhead versus commonplace non-cryptographic hash functions is modest and the security benefit is substantial. This argument is the intellectual seed of the widespread later adoption of SipHash, AES-based hashes, and per-process random seeds as the default in most language runtimes.

### Broader legacy

- The hash-flooding wave of 2011–2012, in which researchers at 28C3 demonstrated the same attack against PHP, Java, Python, Ruby, and .NET hash tables, was a direct re-application of the Crosby–Wallach ideas nearly a decade later and led to coordinated vendor patches.
- CWE-407 ("Inefficient Algorithmic Complexity") and CWE-400 ("Uncontrolled Resource Consumption") are the modern classifications for exactly the class of bug this paper introduced.
- All subsequent fuzzing work on AC vulnerabilities (SlowFuzz, PerfFuzz, HotFuzz, Singularity) treats Crosby–Wallach as the canonical "why this matters" reference.

### Relevance to APEX G-46

Crosby and Wallach provide two things APEX depends on: (1) the formal definition of an AC attack as a security-relevant bug class, which is what makes "performance test generation" a security feature and not just a QA feature; and (2) a catalogue of the data-structure-shaped patterns APEX must recognise in source: unkeyed hashes, unbalanced trees, and other containers whose worst case diverges from their average case. The vault note on "Algorithmic Complexity Attacks" consolidates the pattern list this paper bootstrapped.

## Related Work Pointers

- McIlroy, "A Killer Adversary for Quicksort," 1999 — the canonical earlier demonstration of a specific worst-case input generator for sorting.
- "Efficient Denial of Service Attacks on Web Application Platforms," Klink & Wälde, 28C3 2011 — the hash-flooding replay that re-popularised the attack a decade later.
- Aumasson & Bernstein, "SipHash: a fast short-input PRF," INDOCRYPT 2012 — the modern construction most language runtimes adopted in direct response to hash-flooding.
- SlowFuzz (CCS 2017), PerfFuzz (ISSTA 2018), HotFuzz (NDSS 2020) — see the respective vault notes.

## Citation

Scott A. Crosby and Dan S. Wallach. 2003. Denial of Service via Algorithmic Complexity Attacks. In *Proceedings of the 12th USENIX Security Symposium*, Washington, D.C., August 2003.
