---
id: 01KNWE2QAAKZH8GGZ172HZ9RHS
title: Hash Collision DoS and Modern Defences
type: concept
tags: [hash-collision, dos, siphash, crosby-wallach, security]
links:
  - target: 01KNWE2Q9YZBAR140ZX5P36TQ5
    type: extends
  - target: 01KNWE2QA3FA96G8JKN733K0XP
    type: related
  - target: 01KNWE2QA8H1GKHCVNHYS5QW1F
    type: related
  - target: 01KNWEGYB8807ET2427V3VCRJ3
    type: extends
  - target: 01KNWGA5F9YTQ01C15QNFHQGPT
    type: references
  - target: 01KNWGA5GWXMD72YP3CCKAVT1N
    type: references
created: 2026-04-10
modified: 2026-04-10
---

# Hash Collision DoS and Modern Defences

A **hash collision DoS** is an algorithmic complexity attack in which an adversary supplies keys that all hash to the same bucket of a hash table. Insertion and lookup, normally O(1) expected, degrade to O(n) per operation, and building an n-element table becomes O(n²). Because many web servers use hash tables for HTTP headers, form parameters, and JSON objects — all attacker-controlled — the attack vector is ubiquitous.

This note covers the 20-year history of hash collision DoS, why the 2003 Crosby–Wallach paper landed with a thud before the 2011 28C3 talk made it unignorable, and the modern defences every mainstream hash table now uses.

## The 2003 paper and what it showed

Scott Crosby and Dan Wallach's 2003 USENIX Security paper introduced the hash collision DoS as a general technique and demonstrated it against:

- **Perl 5** hash tables — CGI scripts parsing form parameters.
- **Bro IDS** — flow-tracking tables populated by observed network traffic. An attacker who sends carefully crafted packets can fill the table with colliding keys; Bro spends all its CPU walking collision chains instead of inspecting traffic, effectively blinding the IDS.
- **Squid** and other caches — cache-key collisions drop hit rate and inflate memory.

The paper proposed **universal hashing** — drawing a hash function from a family whose behaviour is unpredictable without a per-process secret — as the general defence. Perl adopted this for its hash tables. Most other languages did not, and the vulnerability stayed dormant.

## 2011: 28C3 ("Effective Denial of Service attacks against web application platforms")

At the Chaos Communication Congress, Alexander Klink and Julian Wälde resurrected the attack and demonstrated practical exploits against **every major web platform at the time**:

- **PHP** (form parameters in `$_POST`)
- **Java** (HTTP headers via `HashMap`)
- **ASP.NET** (`Request.Form`)
- **Python** (CGI form parsing)
- **Ruby** (Rails parameter parser)
- **Node.js** (via `querystring`)

A ~1 MB POST body with carefully chosen colliding keys would burn 10+ seconds of CPU on a single core of the server. At ~100 requests per second, a single attacker could saturate 1000 server cores.

The fallout was rapid: **CVE-2011-4815** (Ruby), **CVE-2011-4838** (Jetty), **CVE-2011-4461** (Jetty), **CVE-2011-3389** (PHP), **CVE-2012-2739** (Java JDK) — and emergency patches from every affected vendor within weeks. This is where universal / keyed hashing finally became mandatory.

## Mathematical root cause

A non-keyed hash function `h(k)` is a **deterministic public function**: given `h`, adversaries can find pre-images (keys that hash to a chosen bucket) via brute-force or meet-in-the-middle. For a table with `b` buckets, finding `n` colliding keys takes:

- `O(b·n)` brute force if keys are enumerable (short strings).
- Much less with meet-in-the-middle birthday attacks.
- Nearly instant if the hash function has algebraic structure (e.g. Java's pre-8 `String.hashCode` was a rolling polynomial — collisions are algebraic).

A **keyed hash function** `h_k(x)` — where `k` is a per-process secret — is a **PRF** (pseudo-random function). Without `k`, adversaries cannot predict which keys collide; the expected collision chain length stays O(1) amortised, and the table's O(1) expected performance is preserved.

## SipHash — the modern workhorse

**SipHash** (Aumasson and Bernstein, INDOCRYPT 2012) is a keyed pseudo-random function optimised for short inputs. It was designed specifically for use as a hash-table mixer and has become the universal standard:

- **Rust** — default `HashMap` uses SipHash-1-3 since 1.0.
- **Python** — `hash()` for strings, bytes, and tuples uses SipHash since 3.4 (PEP 456).
- **Ruby** — since 2.1.
- **Perl** — since 5.18.
- **.NET** — randomised Marvin32 in `String.GetHashCode` since 4.5.
- **Go** — runtime-level randomised hash since 1.0 (initially a custom hash, switched to aes-hash where hardware supports it).
- **Java** — since 8, `HashMap` buckets that exceed a threshold convert to **red-black trees**, bounding worst case to O(log n) even on collision attacks. Key hash itself remains deterministic, which is controversial.

SipHash-1-3 (1 round of compression, 3 rounds of finalisation) is slightly faster than SipHash-2-4 and now the default for hash tables; SipHash-2-4 is used where cryptographic strength matters more.

## Alternatives

- **Marvin32** — Microsoft's competing keyed hash; used in .NET. Faster than SipHash on x86.
- **AHash / Wyhash / HighwayHash** — faster non-cryptographic keyed hashes; Rust's `ahash` crate is a popular drop-in replacement for `HashMap`'s default hasher when speed matters more than worst-case guarantees.
- **Cuckoo hashing** — bounds worst case to O(1) for lookups by using two hash functions and relocating on collision. Used in DPDK and some high-performance stores.
- **Robin Hood hashing** — bounds variance of probe distances; still vulnerable to collision attacks if the base hash is not keyed.

## Defence in depth beyond keyed hashing

Keyed hashing is necessary but not sufficient:

1. **Limit input count** — cap the number of HTTP headers / form parameters / JSON keys per request. Most web frameworks now set sane defaults (e.g. Tomcat `maxParameterCount=10000`).
2. **Limit input size** — `LimitRequestFieldSize`, `client_max_body_size`.
3. **Timeouts** — per-request CPU time budget. If parsing a JSON body takes >100ms, the request is quarantined.
4. **Tree fallback** — Java's HashMap switches collision chains to red-black trees at threshold 8. Caps worst case at O(log n).
5. **Rate limiting** — even with O(n²) per request, if you can only send 1 request per second per IP, the amplification is bounded.

## Lessons for APEX's performance fuzzer

- **Pre-seed the corpus** with known hash-collision generators for popular deterministic hashes (pre-PEP456 Python, pre-Java-8 String.hashCode, Go's old runtime hash). Any parser that still uses them is vulnerable.
- **Static detector**: flag uses of `HashMap` / `HashSet` with non-keyed hashes *on user-controlled keys*. Recommend keyed alternatives.
- **Resource-guided fuzzer**: on a parser that consumes structured input, a performance-fuzzing campaign that maximises `sum-of-probe-length` inside hash inserts should rediscover collision inputs automatically. Worth benchmarking whether PerfFuzz does this in practice.

## References

- Crosby, Wallach — "Denial of Service via Algorithmic Complexity Attacks" — USENIX Security 2003 — [paper](https://www.usenix.org/legacy/event/sec03/tech/full_papers/crosby/crosby.pdf)
- Klink, Wälde — "Effective DoS attacks against web application platforms" — 28C3, 2011 — [events.ccc.de](https://events.ccc.de/congress/2011/Fahrplan/events/4680.en.html)
- Aumasson, Bernstein — "SipHash: A Fast Short-Input PRF" — INDOCRYPT 2012 — [131002.net/siphash](https://www.aumasson.jp/siphash/)
- Python PEP 456 — "Secure and interchangeable hash algorithm" — [peps.python.org/pep-0456](https://peps.python.org/pep-0456/)
- CVE-2011-4815, CVE-2011-4838, CVE-2011-4461, CVE-2012-2739 — 28C3 fallout
