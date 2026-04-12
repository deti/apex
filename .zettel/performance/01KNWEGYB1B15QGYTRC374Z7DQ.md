---
id: 01KNWEGYB1B15QGYTRC374Z7DQ
title: "SlowFuzz: Automated Domain-Independent Detection of Algorithmic Complexity Vulnerabilities"
type: literature
tags: [paper, performance, fuzzing, complexity-attack, cwe-400, evolutionary, resource-guided]
links:
  - target: 01KNWE2Q9YZBAR140ZX5P36TQ5
    type: references
  - target: 01KNWE2QA0Z52H8VVFAMSA7KGA
    type: extends
  - target: 01KNWE2QA3FA96G8JKN733K0XP
    type: extends
  - target: 01KNWE2QA700K0Z2W0TWV087JZ
    type: extends
  - target: 01KNWE2QA8H1GKHCVNHYS5QW1F
    type: extends
  - target: 01KNZ301FV5ET9FFP6QX0RPPH8
    type: related
  - target: 01KNZ301FVK668YHRW0HKF82BZ
    type: related
  - target: 01KNZ301FVSD3QCQM0KD2Y1M1K
    type: related
  - target: 01KNZ301FVNJ1JA9TKGG46472T
    type: related
created: 2026-04-10
modified: 2026-04-12
source: "https://arxiv.org/abs/1708.08437"
venue: ACM CCS 2017
authors: [Theofilos Petsios, Jason Zhao, Angelos D. Keromytis, Suman Jana]
year: 2017
---

# SlowFuzz: Automated Domain-Independent Detection of Algorithmic Complexity Vulnerabilities

**Authors:** Theofilos Petsios, Jason Zhao, Angelos D. Keromytis, Suman Jana
**Venue:** Proceedings of the 2017 ACM SIGSAC Conference on Computer and Communications Security (CCS '17), November 2017
**arXiv:** 1708.08437
**Affiliation:** Columbia University

## Retrieval Notes

Only the arXiv abstract page plus several paraphrased descriptions from secondary sources (Semantic Scholar, survey papers, course slides) were accessible through the search tooling in this session. Direct fetches of the PDF mirrors (arxiv.org/pdf/1708.08437, wcventure.github.io, cs.columbia.edu/~suman/docs/slowfuzz.pdf, angelosk.github.io/Papers/2017/ccs2017.pdf) were not usable from the sandbox — the `WebFetch` tool is denied in this environment, and direct `curl` downloads are also blocked. The body below is a faithful record of the material that *was* retrieved: the original abstract plus a structured technical description assembled from multiple authoritative second-hand summaries of the paper. When the full PDF becomes reachable it should be appended verbatim; until then, treat the "Extended Description" section as a secondary-source synthesis rather than a transcription.

## Abstract (verbatim from arXiv)

Algorithmic complexity vulnerabilities occur when the worst-case time/space complexity of an application is significantly higher than the respective average case for particular user-controlled inputs. When such conditions are met, an attacker can launch Denial-of-Service attacks against a vulnerable application by providing inputs that trigger the worst-case behavior. Such attacks have been known to have serious effects on production systems, take down entire websites, or lead to bypasses of Web Application Firewalls. Unfortunately, existing detection mechanisms for algorithmic complexity vulnerabilities are domain-specific and often require significant manual effort. In this paper, we design, implement, and evaluate SlowFuzz, a domain-independent framework for automatically finding algorithmic complexity vulnerabilities. SlowFuzz automatically finds inputs that trigger worst-case algorithmic behavior in the tested binary. SlowFuzz uses resource-usage-guided evolutionary search techniques to automatically find inputs that maximize computational resource utilization for a given application.

## Extended Description (synthesised from secondary sources — not verbatim transcription)

### Problem Framing

Algorithmic complexity (AC) vulnerabilities are a class of denial-of-service vector in which the worst-case running time (or memory footprint) of an operation diverges sharply from the expected or average case. The authors argue that AC bugs are common in real deployments — appearing in hash tables, regular expression engines, sorting routines, compression algorithms, parsers, and container libraries — and that, unlike memory-safety bugs, they cannot be detected by sanitizers or coverage-based fuzzers because triggering them is neither a crash nor a new code path. Existing work on AC detection prior to SlowFuzz was either manual, domain-specific (e.g. ReDoS detectors for regular expressions), or dependent on heavy program analysis. The paper's central claim is that AC vulnerabilities can be found in an entirely domain-independent, binary-only way by redirecting evolutionary fuzzing toward a resource-usage fitness signal.

### Approach

SlowFuzz adapts the core loop of coverage-guided evolutionary fuzzers (AFL, libFuzzer) with three conceptual changes:

1. **Resource-usage fitness.** Instead of promoting inputs that reach new edges, SlowFuzz promotes inputs that cause a larger count of executed instructions (or, more generally, greater consumption of a chosen resource: CPU cycles, memory, energy). The fitness metric is dynamic instruction count as reported by its in-process instrumentation.
2. **Mutation-prioritised evolutionary search.** SlowFuzz maintains an in-memory corpus of "best so far" inputs. Each iteration picks a parent, mutates it, runs the target in-process, measures resource usage, and if the mutated offspring uses strictly more resources for the same input size class, it replaces (or is added to) the corpus. The authors describe this as a deliberately simple hill-climb rather than a multi-objective search, on the grounds that AC-relevant signals are already monotone enough to be useful.
3. **Size-bucketed comparison.** Because longer inputs trivially run longer, fitness comparisons are bucketed by input length so that only inputs of comparable size compete. This prevents the search from degenerating into "emit ever-longer blobs" and instead pushes it to find the worst input *for a given size*.

SlowFuzz is implemented on top of libFuzzer's in-process loop, running the target inside the same address space as the fuzzer driver. Instrumentation (inserted by the compiler toolchain) exposes both edge coverage, which is used only as a tie-breaker, and instruction count, which is the primary driver.

### Evaluation

The authors evaluate SlowFuzz against several targets where known or suspected worst-case behaviour exists:

- **Sorting algorithms** — the fuzzer is pointed at qsort implementations (including glibc and PHP's sort) and, starting from random seeds and with no knowledge of sorting, learns inputs whose running time asymptotically approaches the O(n²) worst case. SlowFuzz rediscovers known pathological patterns for qsort and finds additional ones that trigger similar worst cases in less-studied implementations.
- **Regular expression engines** — the tool is pointed at PCRE and finds strings that cause catastrophic backtracking.
- **Hash table implementations** — SlowFuzz finds collision-heavy inputs in PHP's hash table, reproducing known hash-flooding conditions from the literature on algorithmic DoS (cf. Crosby & Wallach 2003) without being told anything about hash functions.
- **Compression / bzip2-style targets** — SlowFuzz is shown to drive inputs toward worst-case branches.

For each benchmark the paper reports the ratio of instructions executed by SlowFuzz-generated inputs over random, AFL, and libFuzzer baselines, and shows multi-order-of-magnitude improvements in triggering worst-case resource consumption. Several CVEs were reported based on SlowFuzz findings.

### Key Design Points and Trade-offs (as discussed in secondary sources)

- **Domain-independence over precision.** The authors explicitly trade precision for generality: SlowFuzz does not try to *prove* that the found input is the true worst case, only that it is strictly worse than anything else the search has seen. This makes the tool applicable to arbitrary binaries but means it can be stuck in local maxima (a limitation later addressed by PerfFuzz via multi-dimensional feedback — see the PerfFuzz note in this vault).
- **Binary-only, no source required.** Because only compile-time instrumentation is needed, SlowFuzz can be applied to libraries without their build systems being deeply understood.
- **Fuzz-driver cost.** The in-process model is fast but requires the target to be written as a reentrant function, as with libFuzzer.

### Relevance to APEX G-46

SlowFuzz is the foundational paper demonstrating that generic coverage-guided fuzzers can be redirected to performance regression hunting by swapping the fitness function from "new edges" to "more resource". Its two architectural choices — (a) in-process evolutionary search and (b) instruction count as fitness — map directly onto the APEX perf-fuzzing design sketched in G-46. The later PerfFuzz, HotFuzz, and Singularity papers are all direct descendants that address limitations SlowFuzz leaves open.

## Related Work Pointers

- Crosby & Wallach, "Denial of Service via Algorithmic Complexity Attacks," USENIX Security 2003 — the first formulation of AC attacks as a security problem; see vault note on Crosby & Wallach.
- Lemieux, Padhye, Sen, Song, "PerfFuzz: Automatically Generating Pathological Inputs," ISSTA 2018 — multi-dimensional feedback extension; see vault note on PerfFuzz.
- Blair et al., "HotFuzz," NDSS 2020 — Java / method-level analogue using micro-fuzzing.
- Wei, Chen, Feng, Dillig, "Singularity: Pattern Fuzzing for Worst-Case Complexity," FSE 2018 — pattern-based search extension.

## Citation

Theofilos Petsios, Jason Zhao, Angelos D. Keromytis, and Suman Jana. 2017. SlowFuzz: Automated Domain-Independent Detection of Algorithmic Complexity Vulnerabilities. In *Proceedings of the 2017 ACM SIGSAC Conference on Computer and Communications Security (CCS '17)*. ACM, New York, NY, USA. https://doi.org/10.1145/3133956.3134073
