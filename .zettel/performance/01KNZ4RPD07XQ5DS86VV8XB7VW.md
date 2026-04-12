---
id: 01KNZ4RPD07XQ5DS86VV8XB7VW
title: "Fifield: A Better Zip Bomb (USENIX WOOT 2019)"
type: literature
tags: [paper, zip-bomb, compression, deflate, woot, 2019, fifield, file-format, parser-dos]
links:
  - target: 01KNZ2ZDMJNCKQ2AZEYXENBX53
    type: extends
  - target: 01KNWGA5G0GB0F6EZHMWYQW7MP
    type: related
  - target: 01KNWGA5F4W852RG6C5FJCP204
    type: related
  - target: 01KNZ301FVNJ1JA9TKGG46472T
    type: related
  - target: 01KNWE2Q9YZBAR140ZX5P36TQ5
    type: related
created: 2026-04-12
modified: 2026-04-12
source: "https://www.bamsoftware.com/hacks/zipbomb/"
venue: "USENIX WOOT 2019"
authors: [David Fifield]
year: 2019
artifact: "https://www.bamsoftware.com/git/zipbomb.git"
---

# A Better Zip Bomb

**Author:** David Fifield (independent researcher, previously UC Berkeley).
**Venue:** 13th USENIX Workshop on Offensive Technologies (WOOT '19), August 2019.
**Paper / technique description:** https://www.bamsoftware.com/hacks/zipbomb/
**USENIX listing:** https://www.usenix.org/conference/woot19/presentation/fifield
**Tool:** `zipbomb`, Python script in the `zipbomb` git repo.

*Source: https://www.bamsoftware.com/hacks/zipbomb/ — fetched 2026-04-12. The technique description and compression ratios below are quoted or paraphrased from the canonical page, which is the paper's companion write-up.*

## The problem with classical zip bombs

The infamous **42.zip** (the "classical" zip bomb of the early 2000s) worked by **recursive nesting**: a zip archive containing 16 zip archives, each containing 16 more, and so on, five layers deep. Each inner file was a ~4 GB file of all-zero bytes, compressed via DEFLATE to a few kilobytes. A naive decompressor that recurses into inner archives would ultimately unpack ~4.5 petabytes from a 42 kB input.

Recursive bombs are easy to defend against: any modern decompressor simply refuses to recurse, or caps recursion depth at a small number. By 2019 recursive zip bombs were essentially harmless — archive managers, antivirus engines, web browsers, and cloud ingestion pipelines all rejected them. The practical amplification ratio of a single-level zip file is bounded by DEFLATE's own compression ratio limits, which max out at roughly **1032×** for a single stored file. A non-recursive zip file could therefore amplify at most ~1000×, which is annoying but not catastrophic.

Fifield's contribution is a **non-recursive zip bomb** that achieves amplification ratios of tens of millions to hundreds of millions, using only standard ZIP features that every decompressor must support. The core output of the paper is:

- **zbsm.zip** — 42 KB → 5.5 GB (~129,000× amplification)
- **zblg.zip** — 10 MB → 281 TB (~28 million× amplification)
- **zbxl.zip** — 46 MB → 4.5 PB (~98 million× amplification), using Zip64 extensions

These are single-level archives. A decompressor that has already disabled recursion sees no reason to refuse them; they are structurally indistinguishable from a legitimate archive containing many similar files.

## Technique 1: file overlap via quoted local headers

The key insight exploits two properties of the ZIP file format:

1. **Multiple central-directory entries can point to the same local file header.** The ZIP format has two indexes into the file: the "local file headers" inline in the archive stream, and the "central directory" at the end of the file. Each central-directory entry contains an offset that points to a local file header. Nothing in the ZIP spec forbids two central-directory entries from pointing to the same offset. A back-to-front parser (which finds the central directory first and then reads each local file) sees N distinct "files," all of which happen to share the same physical bytes.
2. **DEFLATE has a "stored block" (non-compressed block) type that can quote raw bytes.** A DEFLATE stream can mix compressed blocks and stored blocks. A stored block contains a length and then that many bytes of literal output.

Combining the two: Fifield lays out the archive as a chain of local file headers, where the DEFLATE stream for file `k` is a stored block that "quotes" the local file header for file `k+1`, followed by another stored block that quotes the local file header for file `k+2`, and so on, until it reaches the **kernel** — a single highly-compressed DEFLATE stream of repeated zeros at the end. Every earlier file, when decompressed, expands to: the quoted local header bytes for the following files *plus* the kernel's output. Because all N central-directory entries ultimately chain through to the same kernel, the kernel's decompressed output is **counted N times in the total output size**.

If the kernel expands from `k` compressed bytes to `K` decompressed bytes (near the DEFLATE limit of 1032×), and the archive contains `N` files that all reference it, the total decompression cost is `O(N * K)`. The archive file size itself grows linearly with `N` because each file only adds a small local header. Amplification therefore grows **quadratically in `N`**, which is what produces the 28-million-fold ratio.

## Technique 2: Zip64 for truly massive output

Zip64 is an official ZIP extension that allows file sizes beyond 4 GB. Many parsers support it. Using Zip64, the kernel's per-file expanded output can be made enormous (many gigabytes), and Fifield's `zbxl.zip` combines Zip64 with the quoting trick to reach **4.5 petabytes** of decompressed output from a 46 MB input.

## Compatibility and defenses

The constructions are designed to parse cleanly under common decompressor implementations. Fifield enumerates which parsers accept which variant:

- **Back-to-front parsers** (consult central directory first, then local file headers): accept all variants.
- **Streaming parsers** (read sequentially from the start): may reject variants that require seeking back.
- **Strict mode parsers** that require local file header filenames to match central directory filenames: reject variants where the trick relies on mismatched names.

The defensive recommendation is to **cap total decompressed output during extraction** rather than trying to detect zip bombs structurally. Most modern archive-processing code now has such a cap — typical values are 100× or 1000× input size, configurable per use case — and that cap is sufficient to defang both recursive and non-recursive zip bombs regardless of construction.

## Relevance to APEX G-46

1. **File-format parsers are a natural G-46 target.** Zip, tar, jpeg, png, pdf, and protobuf parsers have per-file or per-frame expansion ratios that can be attacker-controlled. A performance fuzzer against a parser should measure **allocated-bytes growth relative to input-bytes growth** — which is precisely MemLock's heap feedback channel (see `01KNZ301FVNJ1JA9TKGG46472T`) applied to compression parsers.
2. **Ground-truth regression test.** Any G-46 implementation should, within its standard benchmark, be able to re-derive Fifield's construction given a ZIP parser without size capping. If it cannot, the amplification-seeking feedback channel is misaligned.
3. **"Amplification fuzzing" is a distinct objective from classical coverage fuzzing.** The zip-bomb example makes it plain: the attacker's goal is not to trigger a crash but to exponentially inflate the decompression-to-input ratio. A detector's feedback signal should directly reward that ratio.
4. **Remediation citation.** When APEX flags a decompression parser without a size cap, the report should cite this paper as the canonical demonstration of why recursive-bomb defenses alone are not enough.

## Citation

```
@misc{fifield2019zipbomb,
  author = {David Fifield},
  title  = {A better zip bomb},
  year   = {2019},
  howpublished = {USENIX WOOT 2019},
  url    = {https://www.bamsoftware.com/hacks/zipbomb/}
}
```

## References

- Technique page — [bamsoftware.com/hacks/zipbomb](https://www.bamsoftware.com/hacks/zipbomb/)
- USENIX page — [usenix.org/conference/woot19/presentation/fifield](https://www.usenix.org/conference/woot19/presentation/fifield)
- Tool — `zipbomb` Python script at the same URL
- Classical zip bomb / 42.zip — see `01KNZ2ZDMJNCKQ2AZEYXENBX53`
- Billion laughs (XML analogue) — see `01KNWGA5G0GB0F6EZHMWYQW7MP`
- MemLock (memory-consumption feedback) — see `01KNZ301FVNJ1JA9TKGG46472T`
- CWE-400 (Uncontrolled Resource Consumption) — see `01KNWGA5F4W852RG6C5FJCP204`
