---
id: 01KNZ4RPCWXJR5HATBVGZCKEKQ
title: "SipHash: a fast short-input PRF (Aumasson and Bernstein, INDOCRYPT 2012)"
type: literature
tags: [paper, siphash, prf, mac, hash-flooding, indocrypt, 2012, aumasson, bernstein]
links:
  - target: 01KNZ2ZDKXQR1QP854KBJGKVEC
    type: extends
  - target: 01KNWGA5GWXMD72YP3CCKAVT1N
    type: extends
  - target: 01KNZ2ZDKVZ6WW8VF9NXDW0XWG
    type: related
  - target: 01KNWEGYB8807ET2427V3VCRJ3
    type: related
  - target: 01KNWE2QAAKZH8GGZ172HZ9RHS
    type: related
  - target: 01KNWE2Q9YZBAR140ZX5P36TQ5
    type: related
created: 2026-04-12
modified: 2026-04-12
source: "https://eprint.iacr.org/2012/351"
doi: "10.1007/978-3-642-34931-7_28"
venue: "INDOCRYPT 2012"
authors: [Jean-Philippe Aumasson, Daniel J. Bernstein]
year: 2012
---

# SipHash: a fast short-input PRF

**Authors:** Jean-Philippe Aumasson (Kudelski Security), Daniel J. Bernstein (University of Illinois at Chicago / Technische Universität Eindhoven).
**Venue:** INDOCRYPT 2012, 13th International Conference on Cryptology in India, December 2012, pp. 489–508.
**DOI:** 10.1007/978-3-642-34931-7_28.
**IACR eprint:** https://eprint.iacr.org/2012/351 (published June 2012, several months before the conference presentation).
**Canonical page:** https://www.aumasson.jp/siphash/ — with reference implementations, test vectors, and adoption table.

*Source: https://eprint.iacr.org/2012/351 and https://www.aumasson.jp/siphash/siphash.pdf — PDFs fetched 2026-04-12 but returned as compressed binary; the body is assembled from the eprint abstract, the SipHash companion page, and the reference implementation README at `github.com/veorq/SipHash` (already in the vault as `01KNZ2ZDKXQR1QP854KBJGKVEC`).*

## Why this paper exists: hash flooding

Between 2003 and 2012 the cryptographic and systems communities independently realised that **non-keyed hash functions used in general-purpose hash tables are a denial-of-service vector**. If an attacker knows the hash function (and in open-source or standardised systems they do), the attacker can compute pre-images that all land in the same bucket, turning an expected `O(1)` hash-table insert into `O(n)` and the overall insertion of `n` colliding keys into `O(n²)`. Crosby and Wallach demonstrated this at USENIX Security 2003 against Perl, Squid, and Bro (see `01KNWEGYB8807ET2427V3VCRJ3`). Klink and Wälde re-demonstrated it at 28C3 in 2011 against PHP, ASP.NET, Java, Python, Ruby, and Node.js (see `01KNZ2ZDKVZ6WW8VF9NXDW0XWG`), forcing the industry to ship keyed hashes within weeks.

The emergency response in every language was the same: **replace the non-keyed hash with a keyed pseudorandom function (PRF)**, seeded by a per-process random secret. The technical problem was that no existing MAC/PRF was fast enough to replace `strhash` or `djb33` in a hash table: HMAC-SHA1 was orders of magnitude too slow; CBC-MAC with AES required hardware acceleration that most platforms did not have; UMAC and Poly1305 were fast but were designed for long messages and were not clearly safe on the short keys typical of hash-table use.

Aumasson and Bernstein designed SipHash specifically to fill this gap: a **short-input PRF** fast enough to run on every hash-table lookup, secure under standard PRF assumptions, and simple enough to ship in every language's standard library.

## Design

SipHash is an **ARX** (Add, Rotate, XOR) construction with a 128-bit key `(k0, k1)` and a 256-bit internal state `(v0, v1, v2, v3)`. A single round, called `SipRound`, consists of eight additions modulo 2^64, seven 64-bit rotations, and six XORs — all operations that are vectorised and branch-free on every 64-bit CPU. The round is applied `c` times per input block (compression rounds) and `d` times at the end (finalisation rounds). The paper specifies two variants:

- **SipHash-2-4** (c=2, d=4): the default recommendation. Fast enough for hash tables; ~1 cycle/byte on x86-64.
- **SipHash-4-8** (c=4, d=8): a conservative variant with higher security margin, at roughly half the speed.

The message is parsed into 64-bit blocks with a length-encoded final block (so empty and length-8 messages differ). Each block is XORed into `v3`, `SipRound` is applied `c` times, and then the block is XORed into `v0`. After the last block an additional `SipRound^d` finalisation is performed and the output `v0 ⊕ v1 ⊕ v2 ⊕ v3` is returned as the 64-bit PRF output. (A later 128-bit variant was added.)

## Security claim

The paper proves, under a standard PRF argument on `SipRound`, that SipHash-2-4 is a secure PRF with the full 128-bit key as the security parameter. No attack better than generic key guessing is known as of 2026. Since the release in 2012:

- The SipHash reference has been independently audited multiple times.
- Cryptanalysis papers have studied reduced-round variants but have not reduced the security of SipHash-2-4 below the birthday bound.
- No practical attack on any SipHash deployment has been published.

For the hash-table use case, the relevant security property is not collision resistance (which would require `n/2` bits of security for an `n`-bit output) but **indistinguishability from a random function with access only to the output of a PRF evaluated on a short input**. SipHash-2-4 meets this property with a security margin well above the brute-force boundary for any realistic attacker who observes only hash-table output and cannot extract the seed.

## Performance

The paper's own benchmarks show SipHash-2-4 at approximately:
- **1.0 cycles per byte on long inputs** on x86-64 (Sandy Bridge).
- **~18 cycles for a 1-byte input, ~22 for 8-byte, ~28 for 16-byte** — i.e. the fixed finalisation cost dominates on very short inputs, which is exactly the regime hash tables care about.

These numbers are comparable to or better than fast non-cryptographic hashes like Google's CityHash and MurmurHash3, while providing a proven PRF property that the non-cryptographic hashes lack.

## Deployments (by 2026)

SipHash has become the default keyed hash for hash-table implementations across the mainstream ecosystem:

| Language / runtime | SipHash variant | Since |
|---|---|---|
| Python | SipHash-2-4 → SipHash-1-3 (since 3.4 / PEP 456) | 2014 |
| Ruby MRI | SipHash-2-4 (since 1.9.3) | 2012 |
| Rust `std::collections::HashMap` | SipHash-1-3 (default hasher `RandomState`) | 2015 |
| PHP | SipHash for `array` keys since 8.1 | 2021 |
| Perl | SipHash-2-4 (since 5.18) | 2013 |
| OpenBSD | SipHash in kernel hash tables | 2015 |
| Linux kernel | SipHash in various in-kernel hash tables (netfilter, IPv6) | 2016 |
| Redis | SipHash for dictionary hashing | 2014 |
| Haskell, OCaml, Erlang, Go | SipHash or keyed variants in standard libraries | various |

The SipHash paper is the citation of record for all of these.

## Relevance to APEX G-46

1. **Hash-flooding DoS is a real and historically critical vulnerability class.** Any APEX finding that flags a hash-table implementation using a non-keyed hash on attacker input (CWE-407 / algorithmic complexity weakness) should cite SipHash as the canonical remediation: swap to a keyed PRF. A performance fuzzer against a non-keyed hash table should re-derive the 2011 Klink-Wälde result automatically.
2. **Detector signal.** An APEX detector can flag imports of `MurmurHash`, `FNV`, `CityHash`, `djb33` on attacker-influenced keys as a high-confidence finding for CWE-407 and point to SipHash as the remediation.
3. **Ground truth for a G-46 prototype.** A reference test is: "given a hash table using MurmurHash2, can APEX's performance fuzzer produce a set of keys that collide into one bucket?" The answer should be "yes within minutes" using PerfFuzz-style max-per-edge feedback.

## Citation

```
@inproceedings{aumasson2012siphash,
  author    = {Jean-Philippe Aumasson and Daniel J. Bernstein},
  title     = {{SipHash}: a fast short-input {PRF}},
  booktitle = {Progress in Cryptology -- INDOCRYPT 2012},
  year      = {2012},
  pages     = {489--508},
  publisher = {Springer},
  doi       = {10.1007/978-3-642-34931-7_28}
}
```

## References

- IACR eprint — [eprint.iacr.org/2012/351](https://eprint.iacr.org/2012/351)
- Canonical page — [aumasson.jp/siphash](https://www.aumasson.jp/siphash/)
- Reference C implementation — [github.com/veorq/SipHash](https://github.com/veorq/SipHash) — see `01KNZ2ZDKXQR1QP854KBJGKVEC`
- Wikipedia (SipHash) — [en.wikipedia.org/wiki/SipHash](https://en.wikipedia.org/wiki/SipHash) — see `01KNWGA5GWXMD72YP3CCKAVT1N`
- Crosby & Wallach (original hash-flooding paper) — see `01KNWEGYB8807ET2427V3VCRJ3`
- Klink & Wälde 28C3 — see `01KNZ2ZDKVZ6WW8VF9NXDW0XWG`
