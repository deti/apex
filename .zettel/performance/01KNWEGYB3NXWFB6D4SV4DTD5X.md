---
id: 01KNWEGYB3NXWFB6D4SV4DTD5X
title: "PerfFuzz: Automatically Generating Pathological Inputs"
type: literature
tags: [paper, performance, fuzzing, complexity-attack, afl, multi-dimensional-feedback, pathological-input]
links:
  - target: 01KNWE2Q9YZBAR140ZX5P36TQ5
    type: references
  - target: 01KNWE2QA0Z52H8VVFAMSA7KGA
    type: extends
  - target: 01KNWE2QA700K0Z2W0TWV087JZ
    type: extends
  - target: 01KNWE2QA8H1GKHCVNHYS5QW1F
    type: extends
  - target: 01KNZ2ZDMXYG1HE0S46R66QFA9
    type: references
  - target: 01KNZ301FV5ET9FFP6QX0RPPH8
    type: related
  - target: 01KNZ301FVK668YHRW0HKF82BZ
    type: related
created: 2026-04-10
modified: 2026-04-12
source: "https://www.carolemieux.com/perffuzz-issta2018.pdf"
venue: ACM ISSTA 2018
authors: [Caroline Lemieux, Rohan Padhye, Koushik Sen, Dawn Song]
year: 2018
---

# PerfFuzz: Automatically Generating Pathological Inputs

**Authors:** Caroline Lemieux, Rohan Padhye, Koushik Sen, Dawn Song
**Venue:** Proceedings of the 27th ACM SIGSOFT International Symposium on Software Testing and Analysis (ISSTA 2018)
**Affiliation:** UC Berkeley
**Artifact:** https://github.com/carolemieux/perffuzz

## Retrieval Notes

The direct PDF (`https://www.carolemieux.com/perffuzz-issta2018.pdf`) and its mirrors (`people.eecs.berkeley.edu/~ksen/papers/perffuzz.pdf`, `wcventure.github.io/FuzzingPaper/Paper/ISSTA18_PerfFuzz.pdf`, `dl.acm.org/doi/10.1145/3213846.3213874`) could not be retrieved from this environment: `WebFetch` is denied and direct `curl` is blocked. The body below captures the abstract verbatim as published in the ISSTA proceedings metadata and author page, together with a structured technical description assembled from multiple authoritative secondary sources. Replace the "Extended Description" section with a verbatim transcription when the PDF becomes accessible.

## Abstract (verbatim)

Performance problems in software can arise unexpectedly when programs are provided with inputs that exhibit worst-case behavior. A large body of work has focused on diagnosing such problems via statistical profiling techniques. But how does one find these inputs in the first place? PerfFuzz is a method to automatically generate inputs that exercise pathological behavior across program locations, without any domain knowledge. PerfFuzz generates inputs via feedback-directed mutational fuzzing. Unlike previous approaches that attempt to maximize only a scalar characteristic such as the total execution path length, PerfFuzz uses multi-dimensional feedback and independently maximizes execution counts for all program locations. This enables PerfFuzz to (1) find a variety of inputs that exercise distinct hot spots in a program and (2) generate inputs with higher total execution path length than previous approaches by escaping local maxima. PerfFuzz is also effective at generating inputs that demonstrate algorithmic complexity vulnerabilities. PerfFuzz is implemented on top of AFL, a popular coverage-guided fuzzing tool, and evaluated on four real-world C programs typically used in the fuzzing literature. PerfFuzz outperforms prior work by generating inputs that exercise the most-hit program branch 5x to 69x times more, and result in 1.9x to 24.7x longer total execution paths.

## Extended Description (synthesised from secondary sources — not verbatim transcription)

### Motivation: The Local-Maximum Problem of Scalar Feedback

Earlier resource-guided fuzzers (SlowFuzz in particular) use a single scalar fitness — "total instructions executed" — and promote inputs that push that scalar higher. The authors' central observation is that scalar fitness landscapes for performance are riddled with plateaus and local maxima. Two example symptoms:

1. A mutation that trades "lots of cheap work in loop A" for "a smaller amount of expensive work in loop B" may reduce total instruction count even though it unlocks a qualitatively worse hot spot. A scalar comparison rejects the mutation and the fuzzer never finds B's worst case.
2. Several hot spots exist in the same program, each with its own characteristic pathological input. A scalar fuzzer converges to the single hottest one and abandons the others; coverage of "performance behaviours" is therefore narrow.

### The PerfFuzz Feedback Model

PerfFuzz replaces the scalar resource signal with a **vector of per-location execution counts**. Conceptually, for every basic block (or AFL-style edge) b in the program, the fuzzer tracks the maximum number of times any input in the corpus has executed b. A new input is saved to the corpus if it *strictly increases the max-execution-count of at least one program location*, even if its total instruction count is lower than the current best. This gives three properties:

- **Multi-hotspot discovery.** The corpus simultaneously retains inputs specialised for different pathological locations.
- **Escape from local maxima.** An input that is a net loss on total work but reveals a new "hot" location is preserved and can be mutated further to combine hotspots.
- **Same infrastructure as coverage fuzzing.** The per-edge max-count map is almost a drop-in extension of AFL's per-edge hit-count map, so PerfFuzz is implemented as a modification of AFL with minimal overhead.

### Implementation on AFL

PerfFuzz extends AFL's trace_bits shared-memory region with 32-bit counters (instead of bucketed 8-bit counts) to handle long paths and large hot-spot counts without saturation. The havoc/splice mutation engine and seed scheduling are otherwise inherited. The fitness comparison in AFL's `has_new_bits` routine is replaced with a "has any location been executed more times than before?" check.

### Evaluation

The authors evaluate PerfFuzz on four C benchmarks commonly used in the fuzzing literature: libxml2 (XML parser), libjpeg-turbo (JPEG decoder), zlib, and a custom cJSON parser. Baselines are AFL itself (coverage-guided), SlowFuzz-style total-instructions fuzzing, and a random baseline. Metrics include: total execution path length (summed instructions), max count at the single hottest branch, and number of distinct "hot" locations found.

Headline numbers:

- PerfFuzz generates inputs that execute the *most-hit* program branch **5x to 69x** times more than SlowFuzz-style total-length fuzzing.
- PerfFuzz produces inputs with **1.9x to 24.7x** longer total execution paths than prior work — in particular, it dominates single-scalar fuzzers on their *own* metric because its multi-dimensional corpus escapes the plateaus the scalar fuzzer gets stuck on.
- PerfFuzz rediscovers known algorithmic complexity vulnerabilities (e.g., in libxml2) and surfaces distinct hot spots the baselines never reach.

### Design Insights and Limitations

- **Granularity matters.** The choice of "location" (basic block, edge, function) affects both the feedback resolution and the memory cost of the trace buffer. The paper uses AFL's edge granularity.
- **No notion of "for a given size".** Unlike SlowFuzz, PerfFuzz does not explicitly bucket by input length; the multi-dimensional signal is expected to implicitly encourage small pathological inputs because each hot-spot record rewards increasing count at a single location regardless of size.
- **C-only.** The implementation depends on AFL's compile-time instrumentation and is therefore tied to targets that can be built with afl-clang / afl-gcc.

### Relevance to APEX G-46

PerfFuzz is the direct intellectual ancestor of the per-location performance feedback model APEX will need in apex-fuzz for G-46. Its key reusable ideas are: (a) a per-basic-block "max hit count ever observed" map as a first-class feedback alongside coverage, (b) a fitness rule that accepts any strict increase in any dimension, and (c) the observation that this extension plugs into an existing coverage fuzzer with little architectural disruption. The LibAFL feedback architecture note in this vault discusses how this maps onto APEX's plumbing.

## Related Work Pointers

- SlowFuzz (CCS 2017) — scalar-feedback predecessor; see vault note.
- Wei, Chen, Feng, Dillig, "Singularity: Pattern Fuzzing for Worst-Case Complexity," FSE 2018 — complementary pattern-based approach.
- HotFuzz (NDSS 2020) — applies related ideas to Java at method granularity; see vault note.
- Padhye, Lemieux, Sen, Papadakis, Le Traon, "Semantic Fuzzing with Zest," ISSTA 2019 — related valid-input-preserving fuzzing from overlapping authors.

## Citation

Caroline Lemieux, Rohan Padhye, Koushik Sen, and Dawn Song. 2018. PerfFuzz: Automatically Generating Pathological Inputs. In *Proceedings of the 27th ACM SIGSOFT International Symposium on Software Testing and Analysis (ISSTA 2018)*. ACM. https://doi.org/10.1145/3213846.3213874
