---
id: 01KNWEGYBDMW0V0CFVJ83A4B9J
title: "The Impact of Regular Expression Denial of Service (ReDoS) in Practice: An Empirical Study at the Ecosystem Scale"
type: literature
tags: [paper, performance, redos, regex, cwe-1333, cwe-400, empirical, javascript, python, npm, pypi]
links:
  - target: 01KNWE2Q9YZBAR140ZX5P36TQ5
    type: references
  - target: 01KNWE2QA1E37T7GVKSM6EGXXR
    type: extends
  - target: 01KNWGA5F7VA68B8ZPB6XR0RTE
    type: references
  - target: 01KNWGA5GMWKV6AKP04D964G5H
    type: references
  - target: 01KNZ2ZDMZM2PTFEAZ18TAJ3V0
    type: references
  - target: 01KNZ301FVXKKT846W2GFQ6QZN
    type: related
  - target: 01KNZ301FVEJEFXWZQRNCB36SS
    type: related
  - target: 01KNZ301FV584G004E2SRNW97Z
    type: references
  - target: 01KNZ301FVAC85VSD6QSXHBTBN
    type: references
  - target: 01KNZ301FVRCH0P2GZS4CNGJ23
    type: references
  - target: 01KNZ301FVV2BBBW67QZV0MWTM
    type: references
created: 2026-04-10
modified: 2026-04-12
source: "https://dl.acm.org/doi/10.1145/3236024.3236027"
venue: ESEC/FSE 2018
authors: [James C. Davis, Christy A. Coghlan, Francisco Servant, Dongyoon Lee]
year: 2018
doi: 10.1145/3236024.3236027
---

# The Impact of Regular Expression Denial of Service (ReDoS) in Practice: An Empirical Study at the Ecosystem Scale

**Authors:** James C. Davis, Christy A. Coghlan, Francisco Servant, Dongyoon Lee
**Venue:** ESEC/FSE 2018: Proceedings of the 26th ACM Joint Meeting on European Software Engineering Conference and Symposium on the Foundations of Software Engineering, Lake Buena Vista, FL, USA, November 4–9, 2018
**Affiliation:** Virginia Tech, Stony Brook University
**Artifact:** https://zenodo.org/records/1294301 (Zenodo), https://github.com/SBULeeLab/EcosystemREDOS-FSE18

## Retrieval Notes

The author-hosted PDFs (`https://davisjam.github.io/files/publications/DavisCoghlanServantLee-EcosystemREDOS-ESECFSE18.pdf`, `https://fservant.github.io/papers/Davis_Coghlan_Servant_Lee_ESECFSE18.pdf`, `https://www3.cs.stonybrook.edu/~dongyoon/papers/FSE-18-ReDoS.pdf`), the ACM DL entry, and the Zenodo artifact could not be retrieved in this session: `WebFetch` is denied and direct `curl` downloads are blocked. The body below captures the published abstract and a structured technical description assembled from multiple authoritative secondary sources (ACM DL metadata, the author's companion Medium article, the artifact repository README, and follow-on papers that cite this one). Replace the "Extended Description" with verbatim text when the PDF becomes accessible.

## Abstract (from published venue metadata)

Regular expressions (regexes) are a widely used language for specifying string patterns. Many regex engines implement a backtracking algorithm that can exhibit super-linear (SL) worst-case behaviour on certain inputs. An attacker who can supply input to a vulnerable regex can trigger a Regular Expression Denial of Service (ReDoS) attack. Despite a decade of awareness in the security community, the real-world extent of super-linear regexes and the mechanisms that could be used to identify and repair them remain poorly understood. We perform the first large-scale empirical study of regex use to understand the incidence of super-linear regexes in practice. We analyse the ecosystems of two of the most popular programming languages, JavaScript and Python, covering the core Node.js and Python libraries and 448,402 modules — over 50% of the modules in npm and pypi. We also study reports of ReDoS in these registries to understand the fixes that developers provide for super-linear regexes. We find that super-linear regexes are rather common: they appear in the core Node.js and Python libraries and in thousands of modules in npm and pypi, including popular modules with millions of downloads per month. We also find that conventional wisdom for super-linear regex anti-patterns has few false negatives but many false positives; these anti-patterns appear to be necessary but not sufficient signals of super-linear behaviour. Finally, we find that when faced with a super-linear regex, developers favour revising it over truncating input or developing a custom parser.

## Extended Description (synthesised from secondary sources — not verbatim transcription)

### What "super-linear" means here

A regex engine whose matching algorithm is based on backtracking (as in PCRE, JavaScript's V8 `RegExp`, Python's `re` module, Java's `java.util.regex` by default, Perl, Ruby, and many others) can on some input strings explore a search space that grows super-linearly — polynomially or even exponentially — with the length of the input. The paper uses "super-linear (SL) regex" as a language-agnostic term for any regex whose matching time is not provably linear in input length under a backtracking engine. An attacker who can feed strings to such a regex — e.g., via an HTTP header, a user-supplied form field, or a log line — can cause the regex engine to consume large amounts of CPU, producing a denial-of-service on the host process. This is the mechanism behind many real-world incidents, including the 2016 Stack Overflow outage caused by a regex applied to user posts and the 2019 Cloudflare WAF outage caused by a regex with a nested quantifier.

### Research questions

The paper is organised around four empirical questions (paraphrased from secondary sources):

1. **Prevalence** — how common are super-linear regexes in real code, and specifically in the modules that real applications depend on?
2. **Detector agreement** — do existing static detectors for SL regexes (which look for structural anti-patterns such as nested quantifiers and ambiguous alternations) agree with each other, and do their verdicts match dynamic testing with pumping strings?
3. **Anti-pattern precision/recall** — when conventional "dangerous regex" anti-patterns fire, how often do they correspond to real SL behaviour, and how often do real SL regexes escape them?
4. **Fix behaviour** — when a vulnerability is reported, how do developers actually resolve it: revise the regex, truncate the input, drop the regex entirely, or switch engines?

### Method and scope

- **Corpora.** The authors extract every regex literal from: (a) the core Node.js standard library, (b) the Python standard library, (c) the npm registry (at the time of the study), and (d) the pypi registry. In aggregate this covers 448,402 modules — more than 50% of both registries at the time — which is the "ecosystem scale" the title refers to. This is the first study of this size in the ReDoS literature; prior work had sampled thousands of regexes at most.
- **Static extraction.** The regexes are pulled out by parsing module sources, rather than intercepting them at runtime, because the authors want to see every regex that *could* be exercised at runtime, not only those hit by an existing test suite.
- **SL detection.** Candidates are tested with multiple detectors, including pumping-string-based dynamic tests and structural/static analyses, and disagreement among detectors is treated as a result in its own right — the paper reports where they diverge.
- **Fix study.** A separate subcorpus is built from reported ReDoS advisories in npm and pypi, and the authors trace each report to the commit that fixed it and categorise the fix strategy.

### Headline findings (as summarised in secondary sources)

- **SL regexes are everywhere.** Thousands of modules across npm and pypi contain at least one SL regex, including high-download-count modules relied on by large fractions of their ecosystems. The Node.js core and Python core themselves are not immune: the paper reports finding SL regexes in both, including two SL regexes used to parse UNIX and Windows file paths in Node.js — published as CVE-2018-7158 and fixed by the core team after the authors disclosed.
- **Anti-pattern heuristics are a blunt instrument.** The common "dangerous regex" anti-patterns (nested quantifiers, overlapping alternations, ambiguous unions) have *few false negatives* — most truly SL regexes match at least one anti-pattern — but *many false positives*: many regexes that match an anti-pattern are not SL in practice, because the engine's matching strategy or the practical input domain avoids the worst case. The anti-patterns are necessary-but-not-sufficient signals.
- **Detector disagreement is significant.** Different SL detectors disagree on a non-trivial fraction of regexes, which the paper uses to argue that the field lacks a ground-truth oracle and that ensembling or combined static+dynamic analysis is preferable to relying on any single tool.
- **Developers prefer to rewrite, not to wrap.** When a ReDoS bug is fixed, the overwhelmingly common fix is to revise the regex itself (simplify it, remove the ambiguous construct, switch to an explicit anchored form). Input truncation and replacement with a hand-written parser are used but much less often. The authors interpret this as evidence that developers think of the regex as the bug, not the input that exposed it.

### Broader significance

The paper is the reference citation in modern discussions of ReDoS prevalence. It reframed the ReDoS conversation from "here is a toy example of a bad regex" to "here is how many real dependencies of your real app have this bug." It motivated subsequent work on safer regex engines (Google's RE2, V8's non-backtracking Irregexp mode, Node's switch to V8 irregexp improvements, Python's `regex` module), on static detectors combining multiple signals, and on SL detection as part of CI (including npm's own `safe-regex` line of tools).

### Relevance to APEX G-46

Davis et al. 2018 is the empirical backbone for APEX's ReDoS-focused perf checks. Three specific things the paper tells us APEX needs:

1. **Anti-pattern lists are not enough.** APEX cannot ship a "regex linter" that just flags nested quantifiers and expect that to be useful — the false-positive rate documented by Davis et al. would drown real reports. APEX must combine structural detection with dynamic pumping-string validation.
2. **Ecosystem coverage is worthwhile.** Because SL regexes are common and widely dispersed across dependencies, APEX's perf pass should cover third-party code as well as first-party code; the dependency graph is where most bugs actually live.
3. **Fix recommendations should privilege revision.** When APEX proposes a fix, it should preferentially suggest a rewrite of the regex (anchoring, possessive quantifiers, removal of ambiguity) rather than input truncation, because that is what real maintainers do and it is the fix that eliminates the bug class rather than masking it.

## Related Work Pointers

- Kirrage, Rathnayake, Thielecke, "Static Analysis for Regular Expression Denial-of-Service Attacks," NSS 2013 — early static SL detector.
- Rathnayake, Thielecke, "Static Analysis for Regular Expression Exponential Runtime via Substructural Logics," 2014 — theoretical foundation.
- Weideman et al., "Analyzing Matching Time Behavior of Backtracking Regular Expression Matchers by Using Ambiguity of NFA," CIAA 2016 — ambiguity-based SL detection.
- Davis et al., "Why Aren't Regular Expressions a Lingua Franca? An Empirical Study on the Re-Use and Portability of Regular Expressions," ESEC/FSE 2019 — companion work on regex portability.
- Michael et al., "Regexes are Hard: Decision-making, Difficulties, and Risks in Programming Regular Expressions," ASE 2019 — developer-study companion from overlapping authors.
- Davis, "Rethinking regex engines to address ReDoS," ESEC/FSE 2019.
- ReDoSHunter (USENIX Security 2021) — combined static + dynamic SL detector built on the ideas in this paper.

## Citation

James C. Davis, Christy A. Coghlan, Francisco Servant, and Dongyoon Lee. 2018. The Impact of Regular Expression Denial of Service (ReDoS) in Practice: An Empirical Study at the Ecosystem Scale. In *Proceedings of the 2018 26th ACM Joint Meeting on European Software Engineering Conference and Symposium on the Foundations of Software Engineering (ESEC/FSE '18)*. ACM, 246–256. https://doi.org/10.1145/3236024.3236027
