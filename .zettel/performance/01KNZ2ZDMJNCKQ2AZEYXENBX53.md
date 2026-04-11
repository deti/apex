---
id: 01KNZ2ZDMJNCKQ2AZEYXENBX53
title: "Wikipedia: Zip Bomb"
type: literature
tags: [wikipedia, zip-bomb, decompression-bomb, 42.zip, antivirus, archive]
links:
  - target: 01KNWGA5G0GB0F6EZHMWYQW7MP
    type: related
  - target: 01KNZ2ZDMGGV8NPY88N04STZ6W
    type: references
  - target: 01KNWGA5FEAC0QN3PK6CAYP7T8
    type: references
created: 2026-04-12
modified: 2026-04-12
source: "https://en.wikipedia.org/wiki/Zip_bomb"
---

# Wikipedia — Zip Bomb

*Source: https://en.wikipedia.org/wiki/Zip_bomb — fetched 2026-04-12.*

## Definition

A **zip bomb**, sometimes called a **decompression bomb**, is a malicious archive file crafted so that its uncompressed size vastly exceeds what the system unpacking it can handle. The archive is small (kilobytes), the expansion is enormous (gigabytes or petabytes), and the target process runs out of disk, RAM, or time before realising what it's doing.

Zip bombs are a subclass of the broader **decompression bomb** category, which also includes gzip bombs, XZ bombs, bzip2 bombs, PNG bombs (via the IDAT chunk), and so on. Any format with a compression ratio that can exceed the target's resource budget is vulnerable to the same idea.

## The classic: 42.zip

The most famous zip bomb, **42.zip**, is a 42-kilobyte file that expands to approximately **4.5 petabytes** when fully unpacked — a compression ratio of ~107,000,000,000:1.

Structure:
- 5 nested layers of zip files.
- Each layer contains 16 copies of the next layer down.
- 16⁵ = 1,048,576 bottom-layer archives.
- Each bottom archive contains a 4.3 GB file (highly compressible — often a stream of a single byte).
- Total uncompressed size: 16⁵ × 4.3 GB ≈ 4.5 PB.

42.zip exploits two compounding effects:
1. **DEFLATE is highly efficient on low-entropy input.** A stream of `0x00` bytes compresses to <0.1% of its original size.
2. **Recursive unpacking multiplies compression.** Each layer of the nested structure adds roughly another ×16 factor; five layers give ×16⁵ ≈ 10⁶.

## Non-recursive zip bombs

Recursive unpacking can be defeated by a simple "don't follow nested archives" rule. Modern zip bombs therefore aim to achieve the same pathological expansion **within a single archive layer**.

David Fifield's 2019 paper demonstrated this: a zip file with overlapping local file headers can produce 4.5 GB of output from a 10 MB archive without any nesting. The trick exploits the zip format's ability to point multiple directory entries at the same compressed data. No recursion required — mainstream archivers still fall for it.

## Why they still matter

Despite being a 1996-era attack, zip bombs remain a real threat surface because:

- **Antivirus scanners** must decompress archives to inspect the contents. Naive implementations are vulnerable; a zip bomb can disable the scanner before the payload is delivered.
- **Email gateways and WAFs** inspect attachments. Same vector.
- **Upload handlers** — any service that accepts a user upload and decompresses it (backup services, package managers, content management systems) is a potential target.
- **CI/CD pipelines** that unpack build artifacts.
- **Docker/container registries** that unpack image layers on pull.

## Mitigation strategies

Modern archivers and scanners apply several techniques:

1. **Depth limits on recursive unpacking.** Typical: 4-8 nested archives maximum.
2. **Ratio limits.** Reject unpacking if the ratio of expected output to input exceeds a threshold (e.g., 100:1 for general-purpose tools, 1000:1 for scientific data). The `unzip -l` tool already reports the planned expansion so this can be a pre-flight check.
3. **Stream-with-budget unpacking.** Abort unpacking after N bytes of output regardless of how far into the archive you are.
4. **Dynamic programming (single-file recursion).** The Wikipedia article notes that advanced scanners "follow only one file recursively per layer, converting dangerous exponential expansion into linear traversal."
5. **Sandbox with quota.** Run the unpacker in a namespace with a strict memory cgroup and disk quota.

Linux `unzip(1)` enforces neither ratio nor byte budget by default — you can still drop a zip bomb on most systems and have it fill the disk. The BSD `unzip` from the FreeBSD base system is only marginally better. Defence-in-depth is mandatory.

## The APEX-adjacent static signature

A piece of code calling `ZipFile.extractall()`, `zipfile.ZipFile(...).extractall()`, `tarfile.extractall()`, `shutil.unpack_archive()`, `zipInputStream.getNextEntry()` + `read` loop without a byte limit, or the equivalent in any language — all are zip-bomb vulnerable by default. APEX should flag these as CWE-409 (Improper Handling of Compressed Data) or CWE-789 (Memory Allocation with Excessive Size) findings. The fix is a helper that streams with an explicit budget:

```python
def safe_extract(zf, dest, max_bytes=100_000_000):
    total = 0
    for info in zf.infolist():
        if total + info.file_size > max_bytes:
            raise ValueError("archive too large")
        total += info.file_size
    zf.extractall(dest)
```

Even this is insufficient against declared-size-lying archives; the proper fix reads each file chunk-wise and counts actual bytes consumed.

## Relevance to APEX G-46

1. **Detector rule: unbounded `extractall`.** Any Python/Java/Go code calling the archive library's "extract everything" helper without a size or ratio cap is a high-confidence Finding. CWE-409 or CWE-789.
2. **Detector rule: `gzip.decompress()` on network input without a budget.** Same pattern for a simpler format.
3. **Synthetic corpus.** APEX's fuzzing corpus for performance testing should include 42.zip, Fifield's overlapping-header bomb, and per-format equivalents. Any parser that doesn't recognise them as pathological is worth investigating.
4. **Empirical complexity benchmark.** Zip bomb handling is a clean example for the empirical-complexity classifier: a safe `extractall` runs in `O(input size + output size)`; an unsafe one in `O(output size)` which is unbounded relative to input size. The ratio is measurable.

## References

- Wikipedia — [en.wikipedia.org/wiki/Zip_bomb](https://en.wikipedia.org/wiki/Zip_bomb)
- David Fifield — "A better zip bomb" — USENIX WOOT 2019 — [bamsoftware.com/hacks/zipbomb](https://www.bamsoftware.com/hacks/zipbomb/)
- CWE-789 note — `01KNWGA5FEAC0QN3PK6CAYP7T8`
- Billion laughs note — `01KNWGA5G0GB0F6EZHMWYQW7MP`
