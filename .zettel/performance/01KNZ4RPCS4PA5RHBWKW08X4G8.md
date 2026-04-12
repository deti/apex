---
id: 01KNZ4RPCS4PA5RHBWKW08X4G8
title: "MOPT: Optimized Mutation Scheduling for Fuzzers (Lyu et al., USENIX Security 2019)"
type: literature
tags: [paper, fuzzing, mutation, scheduling, pso, particle-swarm, usenix-security, 2019, afl, mopt]
links:
  - target: 01KNZ2ZDMEPBXSH02HFWYAKFE4
    type: extends
  - target: 01KNZ4RPCQ0HJEDD8XQCV5A58N
    type: extends
  - target: 01KNWEGYB1B15QGYTRC374Z7DQ
    type: related
  - target: 01KNZ4RPD3AV67Q8W1D6G6EW12
    type: related
  - target: 01KNZ4RPCT50A8XWXYC4RVPFT9
    type: related
  - target: 01KNWE2Q9YZBAR140ZX5P36TQ5
    type: related
created: 2026-04-12
modified: 2026-04-12
source: "https://www.usenix.org/conference/usenixsecurity19/presentation/lyu"
pdf: "https://www.usenix.org/system/files/sec19-lyu.pdf"
venue: "USENIX Security 2019"
authors: [Chenyang Lyu, Shouling Ji, Chao Zhang, Yuwei Li, Wei-Han Lee, Yu Song, Raheem Beyah]
year: 2019
artifact: "https://github.com/puppet-meteor/MOpt-AFL"
---

# MOPT: Optimized Mutation Scheduling for Fuzzers

**Authors:** Chenyang Lyu (Zhejiang U), Shouling Ji (Zhejiang U / Alibaba-ZJU Joint Institute), Chao Zhang (Tsinghua BNRist), Yuwei Li (Zhejiang U), Wei-Han Lee (IBM Research), Yu Song (Zhejiang U), Raheem Beyah (Georgia Tech).
**Venue:** 28th USENIX Security Symposium (USENIX Security '19), Santa Clara, pp. 1949–1966.
**ISBN:** 978-1-939133-06-9. DOI: 10.5555/3361338.3361473.
**PDF:** https://www.usenix.org/system/files/sec19-lyu.pdf
**Artifact:** https://github.com/puppet-meteor/MOpt-AFL (and mirror https://github.com/Microsvuln/MOpt-AFL).

*Source: https://www.usenix.org/conference/usenixsecurity19/presentation/lyu — not fetchable in sandbox (403); this note is assembled from the USENIX metadata, the dblp record, the README of the `puppet-meteor/MOpt-AFL` repository, and the canonical summary published in the AFL++ `docs/papers.md` and the USENIX Security 2019 program.*

## The problem: mutation operator selection

AFL's havoc stage mutates a single seed by applying a sequence of 14 mutation operators — bit flips, byte flips, arithmetic increments, known-interesting-value overwrites, random byte overwrites, splicing, and so on. At each havoc step AFL picks an operator uniformly at random. This is a simple and venue-independent default, but it is plainly suboptimal: the operator that produces new coverage on a PNG parser is not the same as the one that produces new coverage on a JSON parser, and a good mutator should bias its choices toward the operators that work on the current target.

The authors observe that no principled approach for online mutation operator selection existed before MOPT. Prior work either hand-tuned probabilities per target (not scalable) or picked uniformly at random (suboptimal). MOPT fills this gap.

## Core technique: Particle Swarm Optimization over havoc operators

MOPT frames mutation scheduling as a continuous optimization problem. The decision variable is the **probability distribution** over the 14 havoc operators (a point in the 13-simplex). The fitness function is the number of new coverage edges (or new crashes) discovered in a unit of fuzzing work under that distribution. The authors apply **Particle Swarm Optimization (PSO)** to search the simplex for a high-fitness distribution.

PSO maintains a swarm of `N` candidate distributions ("particles"), each with a velocity vector. At each iteration every particle is evaluated — run the target with that particle's distribution for a fixed budget and measure new coverage — and then each particle's velocity is updated toward (a) its own best-known position and (b) the swarm's global best-known position, with a random perturbation term. The swarm converges on distributions that work well for the current target, and because the velocity has a stochastic component, MOPT keeps exploring other regions of the simplex as the target's hot paths shift.

The customization to fuzzing that MOPT introduces:

- **Discretization of the simplex to positive rationals.** Probabilities are tracked as integers summing to a fixed total; velocity updates are clamped to keep all probabilities positive.
- **Swarm per fuzzing task.** A separate PSO instance runs per fuzzer stage (bit flip, arithmetic, havoc) so that each stage's operator weights evolve independently.
- **A distinct "core" vs. "pilot" vs. "pacemaker" execution mode.** Pilot mode runs the PSO evaluation on short fixed budgets to explore the distribution space; core mode then runs the currently best distribution for a longer budget to extract paths; pacemaker mode is triggered when the fuzzer has not found a new path or crash within a user-specified time window `t`, after which MOPT kicks in to attempt an operator-distribution shift.

The `-L t` flag controls the pacemaker threshold. `-L 0` puts MOPT in continuous mode from the start (recommended for short evaluations); `-L 1` waits one minute of no progress before activating MOPT.

## Evaluation

The paper evaluates MOPT as an add-on to three baseline fuzzers:

- **MOpt-AFL**: on top of AFL 2.52b.
- **MOpt-AFLFast**: on top of AFLFast.
- **MOpt-VUzzer**: on top of VUzzer (a binary-only fuzzer with taint tracking).

Benchmarks: 13 real-world open-source programs including `binutils` (objdump, readelf, nm), `libtiff`, `libjpeg`, `libxml2`, `infotocap`, `sqlite3`, `tcpdump`, and others. Each run: 24 hours, five repetitions, pooled results.

Headline numbers reported in the paper and echoed in the artifact README:

| Fuzzer | Paths (24h) | Bugs found | CVEs |
|---|---|---|---|
| AFL (baseline) | 1× | 1× | 1× |
| **MOpt-AFL `-L 1`** | **~2-5×** | **+170%** | **+3× new CVEs** |
| | | (compared to AFL) | |
| MOpt-AFL crashes | | | **+350%** |

Concrete example from the repository README on the `infotocap` target (24 h): AFL found 1,821 unique paths while MOpt-AFL `-L 1` found 3,983 — a 2.2× improvement. On `objdump` the gap is 5× (1,099 → 5,499). On `sqlite3` it is 2× (4,949 → 9,975).

The practical observation: MOPT's benefit is largest on targets with asymmetric mutation needs (e.g. protocols that require many bit flips to cross a guard but then want arithmetic to exercise a size field). On targets where havoc is already near-optimal (simple data-driven parsers) the improvement is marginal.

## Integration and legacy

- **AFL++ integrates MOPT as the built-in scheduler `-L` flag** (see `01KNZ4RPCQ0HJEDD8XQCV5A58N`). AFL++'s integration is the most-run version of MOPT today; the original MOpt-AFL branch is maintained only for reproducibility.
- **LibAFL** provides a `MOpt`-style scheduler over its generic stage abstraction; any LibAFL-built fuzzer can drop MOPT in.
- **The PSO-over-operator-weights idea has been picked up beyond AFL lineage**, e.g. in grammar-based fuzzers that learn production weights online.

## Relevance to APEX G-46

1. **Default mutation scheduler for APEX should be MOPT-style.** Hand-picking probabilities is a non-starter for a general-purpose performance fuzzer that must handle heterogeneous targets.
2. **Performance-guided objective.** The PSO fitness can be replaced from "new edges" to "increase in PerfFuzz max-per-edge counter" or "increase in MemLock memory counter". This is a natural extension and would produce a performance-scheduling MOPT variant.
3. **Pacemaker mode is a useful template.** APEX's fuzz crate should have a "progress detector" that activates adaptive policies (mutation re-weighting, schedule change, solver handoff) only after the baseline has plateaued.
4. **Benchmark baseline.** When APEX evaluates its own mutation scheduling, MOPT is the standard comparison point alongside plain AFL havoc.

## Citation

```
@inproceedings{lyu2019mopt,
  author    = {Chenyang Lyu and Shouling Ji and Chao Zhang and Yuwei Li and Wei-Han Lee and Yu Song and Raheem Beyah},
  title     = {{MOPT}: Optimized Mutation Scheduling for Fuzzers},
  booktitle = {28th USENIX Security Symposium (USENIX Security '19)},
  year      = {2019},
  pages     = {1949--1966},
  publisher = {USENIX Association},
  isbn      = {978-1-939133-06-9},
  url       = {https://www.usenix.org/conference/usenixsecurity19/presentation/lyu}
}
```

## References

- USENIX page — [usenix.org/conference/usenixsecurity19/presentation/lyu](https://www.usenix.org/conference/usenixsecurity19/presentation/lyu)
- PDF — [usenix.org/system/files/sec19-lyu.pdf](https://www.usenix.org/system/files/sec19-lyu.pdf)
- Artifact — [github.com/puppet-meteor/MOpt-AFL](https://github.com/puppet-meteor/MOpt-AFL)
- dblp — [dblp.org/rec/conf/uss/LyuJZLLSB19](https://dblp.org/rec/conf/uss/LyuJZLLSB19.html)
- AFL++ paper — see `01KNZ4RPCQ0HJEDD8XQCV5A58N`
- AFL++ project — see `01KNZ2ZDMEPBXSH02HFWYAKFE4`
