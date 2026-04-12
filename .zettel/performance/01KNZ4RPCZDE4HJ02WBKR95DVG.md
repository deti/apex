---
id: 01KNZ4RPCZDE4HJ02WBKR95DVG
title: "Regexes are Hard: Decision-making, Difficulties, and Risks in Programming Regular Expressions (Michael, Donohue, Davis, Lee, Servant, ASE 2019)"
type: literature
tags: [paper, regex, cognition, ase, 2019, davis, empirical, developer-study, redos-awareness, best-paper]
links:
  - target: 01KNZ301FVXKKT846W2GFQ6QZN
    type: extends
  - target: 01KNWEGYBDMW0V0CFVJ83A4B9J
    type: related
  - target: 01KNWGA5F7VA68B8ZPB6XR0RTE
    type: related
  - target: 01KNWE2QA1E37T7GVKSM6EGXXR
    type: related
  - target: 01KNZ4RPCY9XVZE6M8PZQKJHJA
    type: related
  - target: 01KNWE2Q9YZBAR140ZX5P36TQ5
    type: related
created: 2026-04-12
modified: 2026-04-12
source: "https://arxiv.org/abs/2303.02555"
doi: "10.1109/ASE.2019.00105"
venue: "ASE 2019"
authors: [Louis G. Michael IV, James Donohue, James C. Davis, Dongyoon Lee, Francisco Servant]
year: 2019
---

# Regexes are Hard: Decision-making, Difficulties, and Risks in Programming Regular Expressions

**Authors:** Louis G. Michael IV, James Donohue, James C. Davis, Dongyoon Lee (Stony Brook), Francisco Servant (Virginia Tech).
**Venue:** 34th IEEE/ACM International Conference on Automated Software Engineering (ASE '19), San Diego, November 2019.
**Status:** ASE 2019 Best Paper Award (New Ideas / Emerging Results track recipient). Note: The ICSE vs ASE confusion comes up in follow-up citations; the paper was published at **ASE 2019**, not ICSE 2019.
**DOI:** 10.1109/ASE.2019.00105.
**arXiv mirror (extended version):** 2303.02555 (posted 2023, same content).
**Author commentary:** https://medium.com/ase-conference/regexes-are-hard-e7933ae3122d

## Why this paper exists

By 2019 the ReDoS literature had made a clear case that **vulnerable regexes are common** in open source. The Davis group's own ESEC/FSE 2018 paper (see `01KNWEGYBDMW0V0CFVJ83A4B9J`) had shown that thousands of npm and PyPI packages contained super-linear regexes, often hidden in transitive dependencies. But a question was untouched: *why do developers write vulnerable regexes in the first place?* The ReDoS detector literature implicitly assumed developers would fix vulnerabilities once they were pointed out, but nobody had studied how developers actually think about regexes — how they design them, how they debug them, what risks they perceive, and whether they even know what ReDoS is.

The Michael et al. study is the first empirical investigation of that question. Its thesis, summarised bluntly in the title, is that regex programming is harder than the community has acknowledged: it is not just a readability problem but a **full-cycle cognitive problem** covering design, search, validation, documentation, and risk assessment, and developers struggle with every stage.

## Methodology

A mixed-methods study combining scale and depth:

1. **Quantitative survey.** 279 professional developers recruited from diverse backgrounds, explicitly including engineers from top tech firms. The survey asked about frequency of regex use, debugging strategies, awareness of ReDoS, sources of regex knowledge, reuse patterns, and perceived difficulty.
2. **Qualitative interviews.** 17 follow-up interviews with a subset of survey respondents, lasting 30–60 minutes each, exploring the survey answers in depth and allowing the authors to probe for anecdotes, workflow details, and misconceptions.
3. **Coding and analysis.** Standard qualitative coding by two authors independently, with inter-rater reliability checks. Quantitative summaries tabulated per question.

The sample is weighted toward developers who use regexes regularly; the authors caveat that self-selected survey respondents may overstate both confidence and difficulty relative to a uniform random sample of professional developers.

## Findings

The paper's findings, organised around the **lifecycle** of a regex from conception to retirement:

### 1. Design

Developers design regexes **iteratively and by example**. Very few start from a formal specification. The dominant workflow is: write a first attempt, paste it into a tool (online regex tester, IDE regex playground), feed in example strings, tweak until the examples pass, ship. Interviewees described this as "painful and experimental," without the closed-loop feedback they get from imperative code.

### 2. Search for existing regexes

A majority of respondents said they first **search for an existing regex** rather than writing one from scratch — typically on Stack Overflow, RegExr, regex101, or a library like `validator.js`. However, they report that **searching for regexes is hard**: it is difficult to describe the string language you want in a search query, and retrieved results from different sources often use incompatible regex dialects (JavaScript vs PCRE vs POSIX vs .NET). The authors note that this suggests a tool opportunity: *semantic regex search* — retrieval by the language a regex matches rather than by its syntactic form.

### 3. Validation and testing

Developers rely on **manual example-based testing** and do not systematically generate adversarial or edge-case inputs. None of the interview subjects reported running a property-based testing tool or a ReDoS detector against their regexes before shipping them. Some used unit tests with a handful of positive and negative examples; most relied on the regex tester interaction during design as sufficient validation.

### 4. Documentation

Regexes are rarely commented or documented, even in codebases with otherwise strong documentation norms. Interviewees acknowledged this as a problem but described regex comments as **low priority** — "if you can't read the regex, you won't trust the comment either."

### 5. Risk awareness

The single most striking finding. **A majority of studied developers were unaware of the critical security risks that can occur when using regexes.** Even among those who *did* know about ReDoS, few reported taking effective countermeasures. Common misconceptions:

- "ReDoS only affects PHP / only affects old languages / only affects the regex in the tutorial"
- "If my regex works on my test inputs, it's safe"
- "The regex engine has a timeout built in" (rarely true; Python's default engine has no timeout, Node.js has none, Java has one only in specific APIs)
- "Only evil regexes with `(a+)+` are vulnerable" (also false; alternation overlap and nested quantifiers in many innocuous-looking patterns are equally dangerous)

Developers who knew about ReDoS still often failed to mitigate it because they did not know their specific regex was vulnerable and did not run any tool to check.

## Implications

The paper argues for several tool-building directions:

- **Semantic regex search.** Retrieve regexes by the language they match, not by their string form. Would reduce the "reinvent-the-wheel vs. copy-from-Stack-Overflow" dichotomy.
- **Better regex testing frameworks.** Property-based testing for regexes with automatic generation of adversarial inputs is an obvious gap.
- **IDE ReDoS warnings.** Integrate ReDoS detectors (the Davis group's own, or Regexploit, or node-re2 migration linters) into IDEs and CI by default so developers are warned at write time rather than after deployment.
- **Regex debugger UX.** Existing regex testers (regex101, RegExr) are good at showing what a regex matches but poor at showing how the engine spends its time — no equivalent of a CPU profiler for regex execution.

## Relevance to APEX G-46

1. **Developer cognition gap validates APEX's reporting strategy.** A G-46 finding for a vulnerable regex cannot assume the developer already understands why the pattern is dangerous. The report must include (a) a worked example of a pathological input, (b) a link to the ReDoS explanation, (c) a rewrite suggestion or migration path. Without these, developers are likely to dismiss or misdiagnose the finding, per the Michael et al. survey results.
2. **Static detection is not enough.** Even when ReDoS detectors (Regexploit, ReDoSHunter, the Davis group's own static checker) exist, developers do not run them. G-46 has an opening to ship ReDoS detection as a default, friction-free scan — integrated into CI, producing automatic PR comments — so that detection happens whether or not the developer remembers to run the tool.
3. **Benchmark corpus.** The survey's recruited developers provided a long list of regexes in their own codebases; that corpus (published as artifact) is a useful ground-truth benchmark for a G-46 regex analyser.
4. **The "search for existing regexes" workflow is a propagation channel for ReDoS.** A vulnerable regex posted to Stack Overflow can spread to thousands of codebases; APEX flagging imports of well-known-bad patterns is a cheap high-value detector.

## Citation

```
@inproceedings{michael2019regexes,
  author    = {Louis G. Michael IV and James Donohue and James C. Davis and Dongyoon Lee and Francisco Servant},
  title     = {Regexes are Hard: Decision-making, Difficulties, and Risks in Programming Regular Expressions},
  booktitle = {34th IEEE/ACM International Conference on Automated Software Engineering (ASE '19)},
  year      = {2019},
  pages     = {415--426},
  doi       = {10.1109/ASE.2019.00105}
}
```

## References

- arXiv — [arxiv.org/abs/2303.02555](https://arxiv.org/abs/2303.02555)
- IEEE Xplore — [ieeexplore.ieee.org/document/8952499](https://ieeexplore.ieee.org/document/8952499/)
- ASE 2019 page — [2019.ase-conferences.org/details/ase-2019-papers/21/Regexes-are-Hard-Decision-making-Difficulties-and-Risks-in-Programming-Regular-Exp](https://2019.ase-conferences.org/details/ase-2019-papers/21/Regexes-are-Hard-Decision-making-Difficulties-and-Risks-in-Programming-Regular-Exp)
- Author commentary — [medium.com/ase-conference/regexes-are-hard-e7933ae3122d](https://medium.com/ase-conference/regexes-are-hard-e7933ae3122d)
- Davis memoization (S&P 2021) — see `01KNZ301FVXKKT846W2GFQ6QZN`
- Davis ecosystem study (ESEC/FSE 2018) — see `01KNWEGYBDMW0V0CFVJ83A4B9J`
