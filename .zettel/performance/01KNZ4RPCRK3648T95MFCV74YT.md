---
id: 01KNZ4RPCRK3648T95MFCV74YT
title: "REDQUEEN: Fuzzing with Input-to-State Correspondence (Aschermann et al., NDSS 2019)"
type: literature
tags: [paper, fuzzing, redqueen, ndss, 2019, input-to-state, cmplog, magic-bytes, checksums, kafl, intel-pt]
links:
  - target: 01KNZ2ZDMEPBXSH02HFWYAKFE4
    type: extends
  - target: 01KNZ4RPCQ0HJEDD8XQCV5A58N
    type: extends
  - target: 01KNWGA5GD7A7WXW56682R280K
    type: related
  - target: 01KNZ4RPD42FKRBTPDEHDN13WK
    type: related
  - target: 01KNZ4RPCT50A8XWXYC4RVPFT9
    type: related
  - target: 01KNWE2Q9YZBAR140ZX5P36TQ5
    type: related
created: 2026-04-12
modified: 2026-04-12
source: "https://www.ndss-symposium.org/ndss-paper/redqueen-fuzzing-with-input-to-state-correspondence/"
venue: "NDSS 2019"
authors: [Cornelius Aschermann, Sergej Schumilo, Tim Blazytko, Robert Gawlik, Thorsten Holz]
year: 2019
artifact: "https://github.com/RUB-SysSec/redqueen"
---

# REDQUEEN: Fuzzing with Input-to-State Correspondence

**Authors:** Cornelius Aschermann, Sergej Schumilo, Tim Blazytko, Robert Gawlik, Thorsten Holz — all Ruhr-Universität Bochum (Horst Görtz Institute for IT Security).
**Venue:** NDSS Symposium 2019, San Diego.
**Artifact:** https://github.com/RUB-SysSec/redqueen — depends on kAFL / Intel VT-x / Intel Processor Trace.

*Source: https://www.ndss-symposium.org/ndss-paper/redqueen-fuzzing-with-input-to-state-correspondence/ — fetched 2026-04-12.*

## Problem

Coverage-guided greybox fuzzers (AFL and its descendants) drive their search with a simple rule: keep inputs that discover new edges in the control-flow graph. This rule is efficient for programs whose control flow is mostly shallow, but it collapses against two kinds of guards that are ubiquitous in parsers and protocol implementations:

1. **Magic bytes.** `if (*(uint32_t*)buf == 0xdeadbeef) { ... }`. A coverage-guided mutator has to brute-force four specific bytes in a specific position; at `2^32` attempts per guard, fuzzers plateau.
2. **Checksums.** A CRC32, a length field, a hash of the preceding bytes. The mutator mutates a byte, the checksum no longer matches, the input is silently discarded in a sanity-check branch, and the fuzzer sees no new coverage to pursue.

Both problems were classically solved by **concolic execution** (Driller, QSYM, SymCC) — run a symbolic engine alongside the fuzzer, have it solve the magic bytes and checksums symbolically, and feed the concrete solutions back as seeds. This works but requires full instruction semantics, a constraint solver, and the ability to trace symbolic state through a complete binary — an enormous implementation and runtime cost.

## Key observation

Aschermann et al. make an observation that, in retrospect, is almost obvious: **for the overwhelming majority of magic-byte and checksum guards, the required value appears verbatim (or after a simple transform) somewhere in the input bytes, and it appears on one side of a comparison instruction at the point the guard executes.**

Concretely: when the program executes `cmp eax, 0xdeadbeef`, if we can observe the concrete values of `eax` and the immediate at the moment the comparison is evaluated, we learn both what the input needs to contain (`0xdeadbeef`) *and* where the input currently has the wrong value (wherever `eax` came from). That pair (wanted-value, actual-value) is exactly the information a symbolic engine would have produced, at near-zero cost.

The authors call this **input-to-state correspondence**: the observation that state at comparison sites is derived from the input through a traceable chain, and that many guards can be bypassed simply by *substituting* the observed expected value into the input wherever the current actual value appears.

## Technique

REDQUEEN operates in three phases, interleaved with normal greybox fuzzing:

1. **Comparison logging.** Every `cmp`, `sub`, `test`, and every call to `memcmp`/`strcmp`/`strncmp`/`strstr` is traced during execution. For each such instruction, both operand values are recorded along with the program counter and an instance counter. This generates a comparison log (or "cmplog") for the run.

2. **Colorization.** To avoid spurious substitutions, the input is first replaced with a fresh randomised byte sequence at each position and the target is re-run. Only bytes that *change the observed comparison operand* when perturbed are candidates for substitution — this identifies which input bytes actually flow into each comparison.

3. **Guided substitution.** For each logged comparison, REDQUEEN attempts to substitute the "expected" operand value into the input at the colorized positions corresponding to the "actual" operand. It tries several encodings (little-endian / big-endian integer, ASCII decimal, ASCII hex, base64) because real programs parse input through various decoders before comparing.

For **checksums**, the same logic handles the case where a post-decoding byte depends on an earlier byte: REDQUEEN patches the target to accept any checksum during fuzzing, fuzzes normally, then fixes up the checksum in the final reproducer using the comparison log to identify the checksum field.

The whole approach is described in the paper as "lightweight": no SMT solver, no symbolic state, no per-instruction semantics — just operand logging, byte perturbation, and substitution. Implementation on top of kAFL (using Intel PT for tracing and Intel VT-x for snapshotting) keeps the overhead low enough to run REDQUEEN as a secondary stage in the main fuzzing loop.

## Evaluation (as reported)

- **LAVA-M benchmark.** REDQUEEN is the first fuzzer to find **100% of the planted bugs in every LAVA-M target** — the benchmark was designed to stress magic-byte detection and had been an open challenge for greybox fuzzers.
- **Real-world targets.** The authors apply REDQUEEN to a battery of binary parsers and discover **65 previously unknown vulnerabilities, obtaining 16 CVE assignments** in binutils, Linux kernel modules, Wine, ImageMagick, and tcpdump, among others.
- **Throughput.** REDQUEEN's overhead over baseline kAFL is moderate (single-digit multiples on most targets) and pays off immediately on programs with guards — sometimes by three orders of magnitude in bug-finding rate.

## Implementation and reproduction

The artifact is the `RUB-SysSec/redqueen` repository. It is tightly coupled to kAFL (Kernel AFL) and requires:

- An Intel CPU with VT-x and Processor Trace support.
- A Linux host running a modified KVM for kAFL's snapshot fuzzing.
- A prepared target bzImage + initramfs.

Usage example from the README:

```
python kafl_fuzz.py Kernel \
  ~/redqueen/Target-Components/linux_initramfs/bzImage-linux-4.15-rc7 \
  ~/redqueen/Evaluation/lava/packed/who/who_fuzz 500 \
  ~/redqueen/Evaluation/lava/packed/uninformed_seeds \
  /tmp/kafl_workdir -ip0 0x400000-0x47c000 -t10 -hammer_jmp_tables
```

## Legacy: cmplog everywhere

The REDQUEEN idea was rapidly absorbed into the rest of the fuzzing ecosystem. Within two years:

- **AFL++** integrated CMPLOG mode as a first-class feature (see `01KNZ4RPCQ0HJEDD8XQCV5A58N`). AFL++'s CMPLOG is the most widely used descendant of REDQUEEN and is what OSS-Fuzz runs today when confronted with magic-byte and checksum guards.
- **libFuzzer** added `-use_value_profile=1`, which logs comparison operand values for the same purpose, though with a different scoring model.
- **LibAFL** exposes the REDQUEEN loop as a reusable stage over the observer/feedback/mutator abstraction.
- The term "input-to-state correspondence" has become standard terminology for this class of techniques.

## Relevance to APEX G-46

1. **APEX should implement CMPLOG, not concolic execution, as the first line against magic-bytes and checksum guards.** The research literature is clear that input-to-state substitution handles 80%+ of what symbolic execution was previously needed for, at a fraction of the cost.
2. **Performance-fuzzing harnesses often have size-field guards and checksum fields** (e.g., zip, jpeg, tar). Without CMPLOG a G-46 generator will waste budget brute-forcing format gates before it can attack the slow code path behind them. CMPLOG is a prerequisite for reaching the interesting region of the input space.
3. **The colorization step is reusable.** Knowing which input bytes flow into a given decision is useful beyond substitution — it also tells a performance fuzzer which bytes to mutate to amplify a size field or a recursion depth counter.

## Citation

```
@inproceedings{aschermann2019redqueen,
  author    = {Cornelius Aschermann and Sergej Schumilo and Tim Blazytko and Robert Gawlik and Thorsten Holz},
  title     = {{REDQUEEN}: Fuzzing with Input-to-State Correspondence},
  booktitle = {Network and Distributed System Security Symposium (NDSS '19)},
  year      = {2019},
  publisher = {Internet Society}
}
```

## References

- NDSS paper page — [ndss-symposium.org/ndss-paper/redqueen-fuzzing-with-input-to-state-correspondence](https://www.ndss-symposium.org/ndss-paper/redqueen-fuzzing-with-input-to-state-correspondence/)
- Artifact — [github.com/RUB-SysSec/redqueen](https://github.com/RUB-SysSec/redqueen)
- AFL++ CMPLOG integration — see `01KNZ4RPCQ0HJEDD8XQCV5A58N`
- AFL++ project repo — see `01KNZ2ZDMEPBXSH02HFWYAKFE4`
- LAVA-M benchmark — Dolan-Gavitt et al., S&P 2016
