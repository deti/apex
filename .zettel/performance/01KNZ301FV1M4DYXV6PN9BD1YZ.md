---
id: 01KNZ301FV1M4DYXV6PN9BD1YZ
title: "CVE-2021-21419: Eventlet Memory Exhaustion via Websocket Frame"
type: incident
tags: [cve, cwe-400, python, eventlet, websocket, memory-exhaustion, real-world]
links:
  - target: 01KNWE2QA6QKE8Z152WX8D9XYB
    type: references
  - target: 01KNWGA5F4W852RG6C5FJCP204
    type: extends
  - target: 01KNWGA5FEAC0QN3PK6CAYP7T8
    type: related
  - target: 01KNZ301FVNJ1JA9TKGG46472T
    type: related
created: 2026-04-12
modified: 2026-04-12
source: "https://nvd.nist.gov/vuln/detail/CVE-2021-21419"
cve: CVE-2021-21419
cwe: CWE-400
cvss: 5.3
package: "eventlet"
---

# CVE-2021-21419: Eventlet Memory Exhaustion via Websocket Frame

## Metadata

- **CVE ID:** CVE-2021-21419
- **Published:** 2021-05-07
- **Last modified:** 2024-11-21
- **CVSS v3.1:** 5.3 MEDIUM — `AV:N/AC:L/PR:N/UI:N/S:U/C:N/I:N/A:L`
- **Weakness:** CWE-400 (Uncontrolled Resource Consumption)
- **Package:** `eventlet` (Python asynchronous networking)

## Official description

A websocket frame parsing flaw in Eventlet allows a malicious websocket peer to exhaust memory on the server. A peer can either (a) advertise a very large uncompressed frame length, causing the server to pre-allocate a buffer that size, or (b) send a highly compressed "zip-bomb"-style compressed frame whose decompressed size is orders of magnitude larger than the on-the-wire size. Either path lets a remote attacker force unbounded memory allocation inside the server process.

## Affected versions

Eventlet 0.10.0 through 0.30.x (first fixed release: 0.31.0).

## Why it matters

Eventlet is the default asynchronous networking stack under OpenStack, GNU Mailman, SaltStack, Home Assistant, and many Python services that need cooperative-multitasking socket I/O. Any Eventlet-based websocket endpoint exposed to the internet is directly vulnerable: a single TCP connection can OOM-kill the process with a few kilobytes of crafted websocket frames.

## Root cause

Two sub-bugs share one CVE:

1. **Unbounded frame length.** The websocket framing protocol (RFC 6455) carries an explicit payload length that can be up to 2^63 bytes. A conformant server should cap the length before allocating. Eventlet's parser did not enforce a cap: it trusted the peer's declared length and called `recv` into a buffer of that size. A remote peer could therefore force the server to `malloc` an arbitrary amount without ever sending matching bytes.

2. **Compression amplification.** RFC 7692's `permessage-deflate` extension allows a peer to send a deflate-compressed payload. Eventlet decompressed the payload without a cap on the *decompressed* size. Because deflate achieves arbitrary compression ratios on repetitive inputs (the classical ~1000× "zip bomb"), a small on-the-wire frame can decompress to gigabytes of memory.

## Exploitation

Both sub-bugs are trivially exploitable by any peer that can complete the websocket handshake. On a typical Eventlet-backed service:

- To trigger sub-bug (1), send a handshake-completed frame with the extended 64-bit length field set to 10^9. Eventlet will attempt to allocate ~1 GB of buffer and typically OOM-kill the process immediately.
- To trigger sub-bug (2), negotiate `permessage-deflate`, then send a few kilobytes of deflate-compressed zeroes whose decompressed form is gigabytes.

Neither requires any application-level authentication; the websocket handshake only needs to succeed at the TCP+HTTP upgrade level.

## Remediation

- Upgrade to `eventlet >= 0.31.0`. The fix imposes a hard cap on both raw and decompressed frame sizes and rejects frames exceeding the cap at parse time.
- If the process cannot be upgraded, disable `permessage-deflate` (to close sub-bug 2) and put the endpoint behind a reverse proxy that enforces a size limit on websocket frames.

## Key references

- NVD: https://nvd.nist.gov/vuln/detail/CVE-2021-21419
- GitHub advisory: GHSA-9p9m-jm8w-94p2
- Fix commits in `eventlet/websocket.py` (restrict frame size and decompression length).

## Relevance to APEX G-46

Eventlet's websocket flaw illustrates a pattern that pure ReDoS/CPU fuzzers miss entirely: **length-controlled allocation**. The bug is not about a slow code path, it is about a code path that dutifully allocates whatever the attacker says to allocate. Detecting it requires:

1. A memory-consumption feedback channel like MemLock's `memlock-heap-fuzz`, which tracks allocation bytes rather than execution count.
2. A protocol-aware harness: a byte-level fuzzer against a random TCP stream will almost never reach the `permessage-deflate` negotiation state. The harness needs to be able to construct a valid RFC 6455 frame header so the mutator can vary only the length field.
3. A sandbox with a hard RSS cap so runaway memory does not kill the fuzzer itself.

All three requirements are explicitly in scope for APEX G-46. This CVE is a good "harness literacy" regression test: the performance generator must be able to reach stateful websocket code via a minimal handshake script.
