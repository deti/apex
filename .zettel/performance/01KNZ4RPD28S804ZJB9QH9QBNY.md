---
id: 01KNZ4RPD28S804ZJB9QH9QBNY
title: "ISSTAC: Information Security through Symbolic Transformation of Algorithmic Constructs (GitHub org)"
type: resource
tags: [darpa, issstac, symbolic-execution, worst-case, side-channel, java, spf, kelinci, badger, diffuzz, canopy, org]
links:
  - target: 01KNZ301FV7BZB2X9338XPDNK0
    type: extends
  - target: 01KNZ301FVSD3QCQM0KD2Y1M1K
    type: extends
  - target: 01KNZ4RPCXXK9RRRWN1X0Y12Q6
    type: related
  - target: 01KNZ301FVVKJ3YGQC55B3N754
    type: related
  - target: 01KNWE2QA8H1GKHCVNHYS5QW1F
    type: related
  - target: 01KNWE2QA0Z52H8VVFAMSA7KGA
    type: related
  - target: 01KNWE2Q9YZBAR140ZX5P36TQ5
    type: related
created: 2026-04-12
modified: 2026-04-12
source: "https://github.com/isstac"
---

# ISSTAC: Information Security through Symbolic Transformation of Algorithmic Constructs

**Organisation:** https://github.com/isstac
**Sponsor:** DARPA ISSTAC programme (Information Security through Symbolic Transformation of Algorithmic Constructs), a sub-programme of the DARPA STAC (Space/Time Analysis for Cybersecurity) initiative, 2015–2019.
**Affiliated institutions:** Carnegie Mellon University (Corina S. Păsăreanu and NASA Ames collaborators), Stony Brook University, Humboldt-Universität zu Berlin, Synopsys.

*Source: https://github.com/isstac — fetched 2026-04-12.*

## What it is

`isstac` is the GitHub organisation under which the DARPA ISSTAC team released the public artifacts from their research programme on algorithmic-complexity and side-channel analysis of Java programs. The organisation hosts the tooling that powers several papers already in the vault: **SPF-WCA** (ISSTA 2017 worst-case analysis, see `01KNZ301FV7BZB2X9338XPDNK0`), **Badger** (ISSTA 2018 hybrid complexity analysis, see `01KNZ301FVSD3QCQM0KD2Y1M1K`), and **HyDiff** (ICSE 2020 differential analysis, see `01KNZ4RPCXXK9RRRWN1X0Y12Q6`).

The programme's technical focus is **symbolic-execution-driven analysis of Java bytecode**, building on top of Symbolic PathFinder (SPF) from NASA Ames. The repositories are the reference implementations behind the DARPA STAC engagement evaluations — a structured red-team / blue-team exercise in which participants competed on a common set of Java challenge programs ("Smart Ticketing System," "Smart Mail Client," "AirPlan," "Smart Video Chat," "Battleboats," etc.) to find algorithmic complexity and side-channel vulnerabilities that the programme organisers had deliberately planted.

## Repositories

As of 2026 the organisation hosts seven repositories:

| Repo | Description | License | Paper link |
|---|---|---|---|
| **kelinci** | AFL-based fuzzing interface for Java bytecode. Bridges AFL's shared-memory instrumentation to an instrumented JVM so AFL can drive Java targets. | Apache-2.0 | Kersten, Luckow, Păsăreanu — ISSRE 2017 |
| **badger** | Hybrid complexity analysis combining SPF symbolic execution with Kelinci fuzzing. Two cooperating workers trading concrete inputs and path prefixes. | MIT | Noller, Kersten, Păsăreanu — ISSTA 2018 (`01KNZ301FVSD3QCQM0KD2Y1M1K`) |
| **spf-wca** | Symbolic PathFinder extension for worst-case algorithmic complexity analysis. Generalises WISE to structured inputs by learning policy functions over decision histories. | MIT | Luckow, Kersten, Păsăreanu — ISSTA 2017 (`01KNZ301FV7BZB2X9338XPDNK0`) |
| **spf-sca** | Symbolic PathFinder extension for side-channel analysis. Uses symbolic execution to identify secret-dependent resource differences (timing, memory). | MIT | Chen, Feng, Whalley — ISSTA 2017 / PLDI 2019 adjacent |
| **diffuzz** | Differential fuzzing for side-channel analysis. Fuzzes a program twice under different secrets, maximising observed cost difference. Direct predecessor to HyDiff. | Apache-2.0 | Nilizadeh, Noller, Păsăreanu — ICSE 2019 |
| **canopy** | Java-based symbolic-execution-assisted program analysis utility. Generalisation of the symbolic analysis layer used by Badger and SPF-WCA. | MIT | used by Badger |
| **cogito** | Smaller utility for symbolic execution; supporting tool for the larger artifacts. | MIT | (utility) |

## The DARPA STAC engagement corpus

A key artifact the ISSTAC team contributed to the community is the **Engagement Challenge Programs** — Java programs deliberately seeded with planted algorithmic-complexity vulnerabilities and side-channel leaks, organised in tiers of increasing difficulty. The programs were originally used for the STAC evaluation but have since become a de facto **benchmark suite for worst-case analysis tools**: Singularity, SlowFuzz, Badger, SPF-WCA, HyDiff, PathFuzzing and others all report on subsets of this corpus.

The programs are hosted not under `isstac` but under a separate DARPA STAC repository (varying links depending on year); the ISSTAC tooling is designed to consume the STAC input format directly.

## Affiliated researchers

Cross-referencing the paper authors whose work appears under `isstac`:

- **Corina S. Păsăreanu** (CMU / NASA Ames): principal investigator for the symbolic execution side; lead on SPF-WCA, Badger, HyDiff.
- **Yannic Noller** (Humboldt-Universität zu Berlin, later NUS): lead on Badger, Diffuzz, HyDiff.
- **Rody Kersten** (Synopsys): co-author on Badger and SPF-WCA; industrial adopter.
- **Kasper Luckow** (Amazon, previously CMU): lead on SPF-WCA.
- **Marcel Böhme** (Monash, later MPI-SP): co-author on HyDiff; brought fuzzing expertise.
- **Lars Grunske** (HU Berlin): senior author on Noller's dissertation work.

Papers produced under the programme are collectively the "ISSTAC corpus of techniques" and form a coherent research programme spanning eight years and four major conferences.

## Relationship to other G-46 literature

ISSTAC represents the **symbolic-execution lineage** of the worst-case analysis research tree:

- **WISE** (Burnim, Juvekar, Sen — ICSE 2009, see `01KNZ301FVVKJ3YGQC55B3N754`) is the historical root: pure symbolic execution for worst-case complexity.
- **SPF-WCA** generalises WISE to structured inputs and feeds it into SPF.
- **Badger** cooperates SPF-WCA with Kelinci fuzzing.
- **HyDiff** generalises Badger to differential analysis (regressions, side-channels, DNN robustness).
- **PathFuzzing** (arXiv 2025, see `01KNZ301FVK668YHRW0HKF82BZ`) extends the cooperation further.

In parallel, the **fuzzing-first lineage** developed SlowFuzz, PerfFuzz, MemLock, HotFuzz, and Singularity — approaches that start from byte-level fuzzing and add feedback channels. The two lineages have converged: modern tools mix symbolic handoffs and fuzzing feedback in a single loop. The ISSTAC organisation is the single best public reference point for the symbolic-execution side of that history.

## Relevance to APEX G-46

1. **Benchmark access.** APEX's G-46 evaluation should include a subset of the STAC engagement programs. The ISSTAC repositories document how to run them, and their results provide calibration points.
2. **Kelinci-style JVM harness.** APEX's Java target support can reuse Kelinci's forkserver bridge rather than reinventing JVM instrumentation.
3. **Diffuzz/HyDiff for side channels.** If APEX expands from performance DoS to timing side channels, the Diffuzz and HyDiff sources are the starting implementations to study.
4. **Citation hygiene.** Every paper in the ISSTAC family cites its predecessors; APEX's own writeups in the G-46 space should follow the same chain to avoid reinventing terminology or missing obvious related work.

## Repositories at a glance

- `kelinci` — https://github.com/isstac/kelinci
- `badger` — https://github.com/isstac/badger
- `spf-wca` — https://github.com/isstac/spf-wca
- `spf-sca` — https://github.com/isstac/spf-sca
- `diffuzz` — https://github.com/isstac/diffuzz
- `canopy` — https://github.com/isstac/canopy
- `cogito` — https://github.com/isstac/cogito

## References

- Org page — [github.com/isstac](https://github.com/isstac)
- DARPA STAC programme page (archived) — [darpa.mil/program/space-time-analysis-for-cybersecurity](https://www.darpa.mil/program/space-time-analysis-for-cybersecurity)
- SPF-WCA — see `01KNZ301FV7BZB2X9338XPDNK0`
- Badger — see `01KNZ301FVSD3QCQM0KD2Y1M1K`
- HyDiff — see `01KNZ4RPCXXK9RRRWN1X0Y12Q6`
- WISE — see `01KNZ301FVVKJ3YGQC55B3N754`
- PathFuzzing — see `01KNZ301FVK668YHRW0HKF82BZ`
