---
id: 01KNZ2ZDKXQR1QP854KBJGKVEC
title: "SipHash Reference Implementation (veorq/SipHash)"
type: literature
tags: [siphash, hash, prf, aumasson, bernstein, cryptography, hash-collision]
links:
  - target: 01KNWGA5GWXMD72YP3CCKAVT1N
    type: extends
  - target: 01KNWE2QAAKZH8GGZ172HZ9RHS
    type: references
  - target: 01KNYZ7YKS8ARHAT2C0GPQCDPX
    type: related
  - target: 01KNZ2ZDKVZ6WW8VF9NXDW0XWG
    type: related
created: 2026-04-12
modified: 2026-04-12
source: "https://github.com/veorq/SipHash"
---

# SipHash — Reference Implementation

*Source: https://github.com/veorq/SipHash (README) — fetched 2026-04-12. Canonical reference implementation by JP Aumasson, one of the co-designers. The other co-designer is Daniel J. Bernstein.*

## What it is

SipHash is a family of **pseudo-random functions (PRFs)** optimised for short messages, with a 128-bit secret key and (by default) a 64-bit output. It was designed in 2012 specifically as a **keyed hash for hash-table mixers** — the direct response to the 2003 Crosby/Wallach paper and the 2011 28C3 talk.

The key property: without the 128-bit secret, an attacker cannot predict which inputs hash to which bucket, which reduces hash-table performance from an adversarial `O(n²)` back to the expected `O(n)` amortised.

## Variants

| Variant | Compression rounds | Finalisation rounds | Output | Typical use |
|---|---|---|---|---|
| **SipHash-2-4** | 2 | 4 | 64 bits | Conservative default; original spec |
| **SipHash-1-3** | 1 | 3 | 64 bits | Rust/Python `HashMap` default since ~2015; ~2x faster than 2-4 |
| **SipHash-4-8** | 4 | 8 | 64 bits | Conservative; when you want more crypto margin |
| **SipHash-128** | 2 or 4 | 4 or 8 | 128 bits | Short MAC |
| **HalfSipHash-2-4** | 2 | 4 | 32 or 64 | 32-bit words; 64-bit key; for low-power/32-bit CPUs |

Rounds are the number of SipRound operations applied. Fewer rounds = faster but less cryptographic margin.

## Design philosophy

> "Simpler and faster on short messages than previous cryptographic algorithms" while remaining "competitive in performance with insecure non-cryptographic algorithms."

That sentence captures why SipHash displaced MurmurHash, CityHash, FNV, and DJBX33A as the default: on the short-string regime that matters for hash-table keys (1-64 bytes) SipHash is within 2-3x of the fastest non-cryptographic hash, and it is the only one with a security proof against the adversarial setting.

## Internal structure (summary)

- Four 64-bit internal state words: `v0, v1, v2, v3`, initialised from the 128-bit key XOR'd with four constants.
- Message is padded and split into 64-bit chunks.
- For each chunk: XOR into `v3`, run `c` SipRounds, XOR into `v0`.
- Finalisation: XOR 0xff into `v2`, run `d` SipRounds, output `v0 ⊕ v1 ⊕ v2 ⊕ v3`.
- A SipRound is a sequence of `ADD`, `ROT` and `XOR` operations on the four words — "ARX" construction, the same family as Salsa20.

Crucially, it is **ARX on 64-bit words**, so it is fast on modern 64-bit CPUs (1-2 cycles per byte in the amortised long-message regime; 10-30 ns constant overhead for short messages).

## Adoption

| Ecosystem | Where | Version |
|---|---|---|
| Linux kernel | `lib/siphash.c`, packet scheduler, IP fragment hash | 4.11+ (2017) |
| OpenBSD | `siphash24()` throughout kernel | 5.6+ |
| FreeRTOS | Network stack | — |
| Python | `str.__hash__`, `bytes.__hash__`, `tuple.__hash__` (PEP 456) | 3.4+ (March 2014) |
| Perl 5 | All hash tables | 5.18+ |
| Ruby (MRI) | `Hash` | 2.1+ |
| Rust | `std::collections::HashMap` (SipHash-1-3) | 1.0+ |
| Redis | Dict mixer | 4.0+ |
| WireGuard | Cookie MAC | 1.0 |
| libsodium / NaCl | `crypto_shorthash` | — |
| OpenSSL libcrypto | EVP wrapper | 3.0+ |

## Licensing

Multi-licensed: CC0, MIT, Apache-2.0 with LLVM exceptions. The reference implementation is ~100 lines of C.

## Relevance to APEX G-46

1. **Ground truth for "is this hash DoS-resistant?"** — if a codebase's hash table uses SipHash (or a wrapper that does), APEX's hash-collision detector should **downgrade or suppress** the finding. This requires a small allowlist: `std::collections::HashMap` in Rust, CPython `dict`, Perl `%h`, Redis dict, etc.

2. **Detector targets** — APEX should *flag*:
   - Use of **DJBX33A** or other rolling-polynomial hashes on attacker-controlled keys.
   - Explicit `mmh3`, `cityhash`, `xxhash` Python/Rust packages when the keys come from request parsing.
   - Custom hash maps seeded with a constant, or with `std::hash::SeaHasher::new()` instead of `DefaultHasher` in Rust.
   - Anywhere `std::collections::HashMap::with_hasher` is called with a non-keyed hasher.

3. **Config check** — Python runtime should have `PYTHONHASHSEED` unset or `random`, not a fixed number. A fixed seed defeats SipHash's randomness and reopens the attack (CI systems sometimes set `PYTHONHASHSEED=0` for reproducibility, introducing latent DoS exposure in prod).

## References

- Aumasson, Bernstein — "SipHash: A Fast Short-Input PRF" — INDOCRYPT 2012 — paper at [aumasson.jp/siphash/](https://www.aumasson.jp/siphash/)
- Reference implementation — [github.com/veorq/SipHash](https://github.com/veorq/SipHash)
- Python PEP 456 — see `01KNYZ7YKS8ARHAT2C0GPQCDPX`
- Linux siphash commit — `torvalds/linux@3c79107`
