---
id: 01KNZ4RPD42FKRBTPDEHDN13WK
title: "REDQUEEN reference implementation (RUB-SysSec/redqueen)"
type: tool
tags: [tool, fuzzing, redqueen, cmplog, kafl, intel-pt, vt-x, repo, github]
links:
  - target: 01KNZ4RPCRK3648T95MFCV74YT
    type: extends
  - target: 01KNZ2ZDMEPBXSH02HFWYAKFE4
    type: related
  - target: 01KNWGA5GD7A7WXW56682R280K
    type: related
  - target: 01KNWE2Q9YZBAR140ZX5P36TQ5
    type: related
created: 2026-04-12
modified: 2026-04-12
source: "https://github.com/RUB-SysSec/redqueen"
---

# REDQUEEN reference implementation (RUB-SysSec/redqueen)

**Repository:** https://github.com/RUB-SysSec/redqueen
**Canonical paper:** Aschermann et al., NDSS 2019 — see `01KNZ4RPCRK3648T95MFCV74YT`
**Base fuzzer:** kAFL (Kernel AFL), Ruhr-Universität Bochum.

*Source: https://github.com/RUB-SysSec/redqueen (README) — fetched 2026-04-12.*

## What it is

`RUB-SysSec/redqueen` is the reference implementation of the REDQUEEN NDSS 2019 paper. It is **not** a drop-in fuzzer you can pip install — it is a tightly-coupled stack that requires specific hardware (Intel VT-x + Intel Processor Trace), a modified Linux host KVM, and the kAFL snapshot-fuzzing framework. The repository is intended for researchers reproducing the paper's LAVA-M results and for practitioners fuzzing closed-source x86-64 binaries using hardware-assisted tracing.

The approach's **algorithmic contribution** (input-to-state substitution via comparison operand logging) was subsequently ported into AFL++, LibAFL, libFuzzer, and honggfuzz as "cmplog" mode and is how most REDQUEEN-style fuzzing happens today. The RUB-SysSec repository remains the canonical citation and the reference for the paper's specific LAVA-M reproduction numbers.

## Architecture as documented in the README

The REDQUEEN stack has three layers:

1. **kAFL** — a coverage-guided fuzzing harness that drives a snapshotted VM. Uses Intel Processor Trace (Intel PT) to collect execution traces with negligible overhead and KVM VT-x snapshots to reset state between test cases. Shared memory between fuzzer and VM carries the test input and the PT trace.
2. **REDQUEEN instrumentation** — a set of VM-introspection hooks that log the arguments to every `cmp`, `sub`, `test`, `memcmp`, and `strcmp` during execution. These become the cmplog trace.
3. **REDQUEEN mutator** — a post-execution stage that reads the cmplog, identifies colourization-confirmed input bytes flowing to each comparison, and constructs new test inputs by substituting the comparison's "expected" value into the corresponding input bytes. The substitution tries multiple encodings (little-endian int, big-endian int, ASCII hex, ASCII decimal, base64) to handle programs that decode input before comparing.

## Hardware and host requirements

The README is explicit about hardware: **Intel VT-x and Intel Processor Trace are mandatory**. In practice this means a 2015-or-later Intel CPU (Broadwell or newer); AMD and ARM are unsupported. The host must run a patched KVM with kAFL's extensions. The targets are prepared bzImage + initramfs bundles built by `kafl_user_prepare.py`.

A typical invocation (from the README):

```
python kafl_fuzz.py Kernel \
  ~/redqueen/Target-Components/linux_initramfs/bzImage-linux-4.15-rc7 \
  ~/redqueen/Evaluation/lava/packed/who/who_fuzz 500 \
  ~/redqueen/Evaluation/lava/packed/uninformed_seeds \
  /tmp/kafl_workdir -ip0 0x400000-0x47c000 -t10 -hammer_jmp_tables
```

The `-ip0 0x400000-0x47c000` flag gives the target's instrumented address range (so PT filtering can drop unrelated traces); `-t10` sets the per-execution timeout to 10 seconds; `-hammer_jmp_tables` enables additional mutation of jump-table offsets, one of the specific techniques described in the paper.

## Provided artifacts

The repository includes:

- **Target components.** A prepared Linux 4.15-rc7 bzImage and matching initramfs with a set of LAVA-M fuzzing harnesses.
- **LAVA-M evaluation scripts.** Reproduction of the paper's 100% bug-finding result on every LAVA-M target.
- **Real-world packing scripts.** Examples of how to prepare other binaries (who, uniq, base64, md5sum — the LAVA-M canonical targets) for fuzzing.
- **Seed corpora.** An "uninformed" seed set (`uninformed_seeds` — single-byte or empty files) and an "informed" corpus with minimal valid inputs.

## CVE track record

The paper reports REDQUEEN discovered 65 previously unknown vulnerabilities and obtained 16 CVE assignments, on targets including **binutils**, the **Linux kernel**, **Wine**, **ImageMagick**, and **tcpdump**. The README links to the specific CVE IDs for the disclosed bugs.

## Status

As of 2026 the reference implementation is maintained for reproduction purposes but is not actively extended — most REDQUEEN-descended development happens in AFL++ (CMPLOG stage), LibAFL (`CmpObserver` + `CmpLogObserver`), and libFuzzer (`-use_value_profile=1`). Practitioners seeking input-to-state fuzzing should use AFL++ or LibAFL for new work; `RUB-SysSec/redqueen` is the canonical reproduction target.

## Relevance to APEX G-46

1. **Do not reimplement REDQUEEN from scratch.** The mature downstream integrations in AFL++, LibAFL, and honggfuzz already expose cmplog feedback; APEX should integrate against one of them rather than re-doing the VM introspection layer.
2. **Hardware-assisted tracing is orthogonal.** Intel PT based tracing remains the lowest-overhead way to collect full execution traces on x86-64; APEX's binary-only targets might benefit from a PT-based executor in LibAFL terms.
3. **LAVA-M is a smoke test.** If APEX's CMPLOG-equivalent cannot solve LAVA-M to 100% within a few CPU-hours, something is wrong with the substitution logic — REDQUEEN showed the benchmark is tractable with the right approach in 2019.

## References

- Repository — [github.com/RUB-SysSec/redqueen](https://github.com/RUB-SysSec/redqueen)
- NDSS 2019 paper — see `01KNZ4RPCRK3648T95MFCV74YT`
- AFL++ CMPLOG integration — see `01KNZ4RPCQ0HJEDD8XQCV5A58N`
- kAFL — [github.com/RUB-SysSec/kAFL](https://github.com/RUB-SysSec/kAFL)
