---
id: 01KNWEGYB6AVG1FV1EQVYW3K9Q
title: "HotFuzz: Discovering Algorithmic Denial-of-Service Vulnerabilities Through Guided Micro-Fuzzing"
type: literature
tags: [paper, performance, fuzzing, complexity-attack, java, jvm, micro-fuzzing, ndss]
links:
  - target: 01KNWE2Q9YZBAR140ZX5P36TQ5
    type: references
  - target: 01KNWE2QA0Z52H8VVFAMSA7KGA
    type: extends
  - target: 01KNWE2QA8H1GKHCVNHYS5QW1F
    type: extends
created: 2026-04-10
modified: 2026-04-10
source: "https://www.ndss-symposium.org/ndss-paper/hotfuzz-discovering-algorithmic-denial-of-service-vulnerabilities-through-guided-micro-fuzzing/"
venue: NDSS 2020
authors: [William Blair, Andrea Mambretti, Sajjad Arshad, Michael Weissbacher, William Robertson, Engin Kirda, Manuel Egele]
year: 2020
arxiv: 2002.03416
---

# HotFuzz: Discovering Algorithmic Denial-of-Service Vulnerabilities Through Guided Micro-Fuzzing

**Authors:** William Blair, Andrea Mambretti, Sajjad Arshad, Michael Weissbacher, William Robertson, Engin Kirda, Manuel Egele
**Venue:** Network and Distributed System Security Symposium (NDSS) 2020
**Affiliation:** Boston University, Northeastern University
**arXiv:** 2002.03416

## Retrieval Notes

Neither the NDSS paper landing page nor any of the PDF mirrors (wcventure.github.io/FuzzingPaper/Paper/NDSS20_HotFuzz.pdf, megele.io/hot-fuzz-ndss2020.pdf, arxiv.org/abs/2002.03416, ar5iv.labs.arxiv.org/html/2002.03416) could be retrieved in this session: `WebFetch` is denied and direct `curl` is blocked. The body below records the abstract as published on the NDSS site plus a structured technical description assembled from multiple authoritative secondary sources and the first author's blog post. Replace the "Extended Description" with verbatim text once the PDF is reachable. See also the follow-up TOPS 2022 extension ("HotFuzz: Discovering Temporal and Spatial Denial-of-Service Vulnerabilities Through Guided Micro-Fuzzing", ACM TOPS) which generalises the approach to memory as well as CPU.

## Abstract (verbatim)

Contemporary fuzz testing techniques focus on identifying memory corruption vulnerabilities that allow adversaries to achieve either remote code execution or information disclosure. Meanwhile, Algorithmic Complexity (AC) vulnerabilities, which are a common attack vector for denial-of-service attacks, remain an understudied threat. In this paper, we present HotFuzz, a framework for automatically discovering AC vulnerabilities in Java libraries. HotFuzz uses micro-fuzzing, a genetic algorithm that evolves arbitrary Java objects in order to trigger the worst-case performance for a method under test. We define Small Recursive Instantiation (SRI) as a technique to derive seed inputs represented as Java objects to micro-fuzzing. After micro-fuzzing, HotFuzz synthesizes test cases that triggered AC vulnerabilities into Java programs and monitors their execution in order to reproduce vulnerabilities outside the fuzzing framework. We evaluate HotFuzz over the Java Runtime Environment (JRE), the 100 most popular Java libraries on Maven, and challenges contained in the DARPA Space and Time Analysis for Cybersecurity (STAC) program. In this evaluation, we verified known AC vulnerabilities, discovered previously unknown AC vulnerabilities that we responsibly reported to vendors, and received confirmation from both IBM and Oracle.

## Extended Description (synthesised from secondary sources — not verbatim transcription)

### Why Micro-Fuzzing

Conventional file-/network-style fuzzing is a poor fit for large Java libraries: there is no single "main" to feed bytes to, each library contains hundreds to thousands of independently interesting methods, and many methods take structured object graphs rather than byte arrays. HotFuzz reframes the problem at the *method* level: for each public method `m` of interest, treat `m` as a fuzzing target, construct arguments directly as Java objects, invoke `m` many times, and look for arguments that make `m` run for a disproportionately long time relative to their "size" as perceived by a developer. The authors call this micro-fuzzing: the fuzzing entry point is a single method rather than a program.

### Small Recursive Instantiation (SRI)

The central technical problem is: given an arbitrary Java method signature (which can involve interfaces, generics, collections, arrays, user-defined classes, nullable references, and deep object graphs), how do you produce a *population of valid, diverse initial inputs* cheaply, without a harness written by hand? HotFuzz introduces Small Recursive Instantiation:

- For primitive parameters, pick values randomly within each type's domain (booleans, int ranges, characters, etc.).
- For reference parameters with a concrete type, recursively instantiate the type by (a) locating a constructor (preferring the simplest), (b) recursively generating arguments for that constructor, and (c) assigning each field's value — again recursively — before returning.
- For interfaces and abstract classes, enumerate concrete implementations visible on the classpath and pick one.
- For collections and arrays, choose a small initial size and populate recursively.
- Cap recursion depth to keep seed construction tractable and to bias toward small seeds that can be mutated outward during evolution.

SRI gives the fuzzer a population of legal object graphs that satisfy the method's static type constraints; subsequent genetic operators mutate these graphs (field-flip, collection-grow/shrink, primitive-bitflip, type swap within subclass set, etc.).

### EyeVM and the Fitness Signal

HotFuzz needs a high-resolution CPU-consumption signal per method invocation, stable enough to drive evolution. The authors build EyeVM, a modified OpenJDK HotSpot JVM that exposes precise per-invocation CPU-cycle / instruction-count measurements to the fuzzer through a side channel. This removes the noise that would come from wall-clock timing or from JVM warm-up effects. Micro-fuzzing workers run candidate inputs inside EyeVM and report the measurement back to the genetic algorithm, which ranks offspring accordingly.

### Two-Phase Architecture: Micro-Fuzzing + Witness Validation

Running only inside EyeVM is good for speed and signal, but raises false-positive risk: a "slow" measurement can be caused by I/O blocking, socket polling, JIT warm-up, or other externalities the instrumented VM can't fully isolate. HotFuzz therefore adds a second phase:

1. **Micro-fuzz** the method inside EyeVM with SRI seeds and a genetic algorithm until the fitness stops improving.
2. **Synthesise a standalone Java witness program** that reconstructs the winning input object graph and invokes the method under test exactly once.
3. **Run the witness on an unmodified JVM** and measure end-to-end CPU time. If the witness reproduces the slowdown outside the instrumented environment, it is reported as a real AC vulnerability; if not, it is discarded as an artefact.

The authors emphasise that this validation step is necessary precisely because the per-invocation micro-signal, while sensitive, cannot on its own prove that a production deployment would be affected.

### Evaluation Corpus and Findings

- **Target 1 — the JRE.** HotFuzz was run over public methods of the OpenJDK class library. It found an AC vulnerability in `java.math` that was assigned CVE-2018-1517 and confirmed by IBM and Oracle.
- **Target 2 — Top-100 Maven libraries.** Across the 100 most-downloaded Java libraries on Maven, HotFuzz reports 132 AC vulnerabilities in 47 libraries (numbers as cited in secondary sources).
- **Target 3 — DARPA STAC challenges.** The authors ran HotFuzz against the DARPA Space/Time Analysis for Cybersecurity (STAC) engagement programs used as ground truth. HotFuzz rediscovered planted vulnerabilities that STAC had built as benchmarks.

### Design Points and Trade-offs

- **Method-granularity fits libraries, not whole programs.** HotFuzz is an excellent fit for library AC analysis but does not address AC bugs that require a specific sequence of public API calls, a warmed cache, or stateful coordination across methods.
- **Reflective object synthesis is the hard part.** The paper's real contribution is less the genetic algorithm and more the SRI machinery for producing legal, diverse object graphs from nothing but type signatures. This is the component APEX would need to reimplement for any JVM-targeted perf fuzzing.
- **Instrumented-VM fitness is noisy.** The two-phase design implicitly acknowledges that per-invocation measurements are not trustworthy in isolation. Any APEX equivalent will need both a cheap fitness signal for the search loop and an expensive out-of-VM confirmation step.

### Relevance to APEX G-46

HotFuzz is the canonical reference for method-level performance fuzzing of managed-runtime libraries. For APEX's G-46 work, its most reusable ideas are: (a) "micro-fuzzing" as a unit-of-work, targeting individual functions/methods rather than whole programs, (b) typed-structure seed synthesis (the JVM equivalent of what `apex-synth` already does for typed test inputs), and (c) the two-phase fitness + witness-validation pipeline, which maps naturally onto APEX's existing test-synthesis pipeline plus a separate performance assertion step.

## Related Work Pointers

- SlowFuzz (CCS 2017) — origin of resource-guided evolutionary fuzzing; see vault note.
- PerfFuzz (ISSTA 2018) — multi-dimensional feedback; see vault note.
- Singularity (FSE 2018) — pattern-based complementary approach.
- HotFuzz extension, ACM TOPS 2022 — extends HotFuzz to *spatial* (memory) as well as temporal AC vulnerabilities; same research group.
- DARPA STAC program — ground-truth dataset referenced for evaluation.

## Citation

William Blair, Andrea Mambretti, Sajjad Arshad, Michael Weissbacher, William Robertson, Engin Kirda, and Manuel Egele. 2020. HotFuzz: Discovering Algorithmic Denial-of-Service Vulnerabilities Through Guided Micro-Fuzzing. In *Network and Distributed System Security Symposium (NDSS) 2020*. Internet Society. https://www.ndss-symposium.org/ndss-paper/hotfuzz-discovering-algorithmic-denial-of-service-vulnerabilities-through-guided-micro-fuzzing/
