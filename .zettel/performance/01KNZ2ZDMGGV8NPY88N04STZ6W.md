---
id: 01KNZ2ZDMGGV8NPY88N04STZ6W
title: "Wikipedia: Algorithmic Complexity Attack"
type: literature
tags: [wikipedia, algorithmic-complexity, attack, redos, zip-bomb, billion-laughs]
links:
  - target: 01KNWE2QA3FA96G8JKN733K0XP
    type: references
  - target: 01KNWE2Q9YZBAR140ZX5P36TQ5
    type: related
  - target: 01KNWEGYB8807ET2427V3VCRJ3
    type: related
  - target: 01KNYZ7YKMA92CCFZ16HD1H8J5
    type: references
  - target: 01KNZ2ZDMJNCKQ2AZEYXENBX53
    type: references
  - target: 01KNZ301FV5ET9FFP6QX0RPPH8
    type: related
  - target: 01KNZ3XK3QDTG1BB60XBMTNYFE
    type: references
created: 2026-04-12
modified: 2026-04-12
source: "https://en.wikipedia.org/wiki/Algorithmic_complexity_attack"
---

# Wikipedia — Algorithmic Complexity Attack

*Source: https://en.wikipedia.org/wiki/Algorithmic_complexity_attack — fetched 2026-04-12.*

## Definition

An **algorithmic complexity attack (ACA)** is a denial-of-service attack in which a carefully crafted input forces an algorithm into its worst-case time or space complexity, even though normal inputs would execute in average-case time. The attacker wins by consuming far more server resources per byte of malicious input than legitimate traffic would.

This class of attack does **not** require bandwidth advantage. A single attacker on a dial-up connection can saturate a multi-core server if the server's algorithms are worst-case-vulnerable on parser-accessible inputs.

## The three textbook categories

The Wikipedia article highlights three canonical examples, each of which has its own dedicated note in this vault:

1. **ReDoS** — catastrophic regex backtracking (see `01KNYZ7YKF7DHTFHJ50C7AE403`, `01KNYZ7YKH344XCTAFQAHQNYHG`).
2. **Zip bombs** — small archives that decompress to large outputs (see `01KNZ2ZDMJNCKQ2AZEYXENBX53`).
3. **Billion laughs** — XML entity expansion DoS (see `01KNWGA5G0GB0F6EZHMWYQW7MP`).

The article is a stub; it omits several equally important categories that should be covered for completeness:

- **Hash collision DoS** (Crosby & Wallach 2003, 28C3 2011) — `01KNWE2QAAKZH8GGZ172HZ9RHS`.
- **Quadratic string accumulation** — Schlemiel the Painter, `01KNZ2ZDM1XWWE840ADNCGKBMP`.
- **Quicksort pivot DoS** — McIlroy 1999's "A Killer Adversary for Quicksort".
- **Recursion bombs** — parsers with unbounded recursion depth.
- **Slowloris and slow-read** — per-request resource exhaustion through pacing rather than content.

## What makes this class distinctive

Unlike buffer overflows or SQL injection, algorithmic complexity attacks:

- Leave the algorithm **correct** — the program produces the right answer, just slowly. Sanity-check tests pass.
- Don't need **unusual** inputs — the malicious input is often a syntactically valid and semantically trivial thing (e.g., a short regex, a valid XML document, a well-formed HTTP POST).
- Exploit **complexity phase transitions** — the same code is fast on 99.9% of inputs and glacial on the other 0.1%. Benchmarking average-case gives the developer a false sense of security.
- Reward the attacker with **amplification** — the ratio of victim cost to attacker cost scales with the input size, which rewards attackers with more bandwidth but isn't required for the attack to work.

## History — the key inflection points

- **1974** — Aho, Hopcroft, Ullman's "The Design and Analysis of Computer Algorithms" introduces the theoretical framework of worst-case complexity that adversarial inputs exploit.
- **1999** — Douglas McIlroy, "A Killer Adversary for Quicksort" — constructs an `O(n²)` quicksort adversary. The first "adversarial complexity" result with a hands-on exploit.
- **2003** — Crosby & Wallach at USENIX Security demonstrate hash-collision DoS against Perl, Bro, Squid. Coins the term "algorithmic complexity attack".
- **2011** — Klink & Wälde's 28C3 talk generalises the hash-collision attack to every major web platform; triggers industry-wide patching wave.
- **2012** — SipHash published; becomes the new universal default.
- **2016** — Stack Exchange outage from a single `^[\s\u200c]+|[\s\u200c]+$` regex eating 100% CPU (`01KNWGA5G5JNAQP0QEYZXN6T2H`).
- **2019** — Cloudflare 30-minute global outage from a single WAF regex (`01KNWGA5G3XDK746J4N59G6VVW`).
- **2022** — Google Cloud blocks a 46M-req/s Layer 7 DDoS (noted in the Wikipedia article) — a bandwidth-class DDoS rather than an ACA, but illustrates how often the two are confused.
- **2024** — CWE-400 enters the CWE Top 25 at rank 24 (`01KNZ2ZDM5DNHY26MRQ9V2BPKT`).

## Defensive categories

- **Choose non-adversarial algorithms** — linear-time regex (RE2), randomised pivot quicksort, keyed hash tables.
- **Resource limits** — caps on input size, recursion depth, entity count, quantifier nesting, timeouts.
- **Per-request accounting** — track CPU and memory per request; kill requests that exceed budget.
- **Defence in depth** — even with all of the above, rate limiting and WAF-level inspection bound the amplification.

## Relevance to APEX G-46

1. **This note is the umbrella** — every APEX performance Finding should roll up to the ACA category with a pointer to either the Wikipedia article or the specific sub-class note in this vault.
2. **The history timeline is marketing material.** When making the case to a user for why G-46 matters, the history of industry-wide incidents is more persuasive than theoretical CWE discussions.
3. **Gap analysis.** The sub-classes the Wikipedia article omits (hash collision, quadratic accumulation, McIlroy's quicksort, recursion bombs, slow-pacing attacks) are all in G-46's scope. The vault has dedicated notes for several; APEX detectors should exist for all of them.
4. **Amplification is the policy hook.** APEX Findings should include the amplification ratio (malicious input size ÷ time-to-process) where known; users can triage based on whether the worst-case is 10x or 10000x.

## References

- Wikipedia — [en.wikipedia.org/wiki/Algorithmic_complexity_attack](https://en.wikipedia.org/wiki/Algorithmic_complexity_attack)
- Crosby, Wallach — USENIX Security 2003 — `01KNWEGYB8807ET2427V3VCRJ3`
- 28C3 talk — `01KNZ2ZDKVZ6WW8VF9NXDW0XWG`
- McIlroy — "A Killer Adversary for Quicksort" — 1999 — www.cs.dartmouth.edu/~doug/mdmspe.pdf
