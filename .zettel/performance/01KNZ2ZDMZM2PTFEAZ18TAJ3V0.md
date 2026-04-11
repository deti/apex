---
id: 01KNZ2ZDMZM2PTFEAZ18TAJ3V0
title: "Davis et al. ReDoS Ecosystem Artifact (Zenodo 1294301)"
type: literature
tags: [zenodo, dataset, redos, davis, npm, pypi, artifact, fse18]
links:
  - target: 01KNWEGYBDMW0V0CFVJ83A4B9J
    type: extends
  - target: 01KNWGA5GMWKV6AKP04D964G5H
    type: related
created: 2026-04-12
modified: 2026-04-12
source: "https://zenodo.org/records/1294301"
---

# Davis et al. ReDoS Ecosystem Artifact — Zenodo 1294301

*Source: https://zenodo.org/records/1294301 — fetched 2026-04-12.*
*Title: "Artifact (software + dataset) for 'The Impact of Regular Expression Denial of Service (ReDoS) in Practice: an Empirical Study at the Ecosystem Scale'"*
*Authors: James C. Davis, Christy A. Coghlan, Francisco Servant, Dongyoon Lee (Virginia Tech)*
*Published: 4 November 2018. DOI: 10.5281/zenodo.1294301. License: CC-BY-4.0.*

## What this artifact is

This is the reproducibility package for the Davis et al. ESEC/FSE 2018 paper (primary note: `01KNWEGYBDMW0V0CFVJ83A4B9J`). The Zenodo record is the authoritative distribution point for:

1. The **dataset** — the full corpus of regexes extracted from npm and pypi packages.
2. The **analysis pipeline** — scripts that reproduce each figure in the paper.
3. A **Docker container** — the reproducibility environment for the pipeline.

The artifact was evaluated and awarded the ESEC/FSE 2018 Artifact Evaluation Committee's "Reusable" badge.

## Contents

The single file `FSE18Artifact-DavisCoghlanServantLee.zip` (~15.5 MB) contains:

- **Regex corpus** — unique regexes collected from packages on npm and pypi as of mid-2018.
- **Metadata** — for each regex: the source package, the source file, the line, the construction site (literal, compiled, variable).
- **Vulnerability labels** — per-regex classification from the ensemble detector (Weideman, Rathnayake, Wüstholz, RXXR2) with agreement flags.
- **Performance annotations** — measured super-linearity under Node.js's `RegExp` and Python's `re`.
- **Anti-pattern matches** — per-regex match against the paper's taxonomy of smelly construction patterns.
- **Scripts** — Python + bash pipeline that takes a package set, extracts regexes, runs the detectors, and produces CSV summaries.

The regexes are **not** attributed to source packages in the public dataset — the authors made a responsible-disclosure choice to delay package-level attribution. Researchers can re-run the extraction on a current snapshot if they want live attribution.

## Key numbers from the paper (reproduced from artifact)

These are the headline statistics the artifact lets you verify:

- **~500,000** unique regexes collected across npm and pypi.
- **~3.5%** of regexes classified as super-linear (polynomial or exponential worst case).
- **~1.4%** classified as strictly exponential — the subset where ReDoS is most acute.
- **~40%** of top-500 npm packages contain at least one super-linear regex somewhere in their transitive dependencies.
- **~339** distinct super-linear regexes appear in **≥10** packages each — the "reuse debt" of ReDoS patterns through copy-paste.

## The detector ensemble

The artifact uses an ensemble of four detectors:

| Detector | Approach | What it's good at |
|---|---|---|
| **Weideman** | SMT-based pumping | Exponential patterns |
| **Rathnayake** | Regex-to-NFA + backreferences | Exponential with state explosion |
| **Wüstholz** | Symbolic execution on regex engine | Engine-specific quadratic/polynomial |
| **RXXR2** | Analysis of NFA structure | Both classes, higher false-positive rate |

Each detector has different false-positive and false-negative characteristics. The ensemble reports a regex as vulnerable only if ≥1 detector flags it and the dynamic validator confirms super-linear growth. This is the "high precision, lower recall" approach Davis later generalised into `vuln-regex-detector` (see `01KNWGA5GMWKV6AKP04D964G5H`).

## Anti-pattern taxonomy

The paper identifies a small catalogue of **textual anti-patterns** whose presence in a regex source is a strong predictor of ReDoS vulnerability:

1. **Nested quantifiers** — `(a+)+`, `(a*)*`, `(a|a)+`.
2. **Overlapping alternatives inside a quantifier** — `(a|ab|abc)*`.
3. **Optional repeat before a fixed tail** — `.*a.*b` with `.*` that can over-consume.
4. **Repeated capture groups** — `([^/]+)+` is a classic.
5. **Unbounded `.*` at the start of an anchored expression** — `^.*…` can force huge backtracking windows.

The artifact includes a standalone "anti-pattern grep" script that finds these patterns in source code without running any regex engine. It's ~30 lines of Python. APEX's cheapest static detector can be a port of this file.

## Reproducibility status

The artifact earned the ESEC/FSE Reusable badge, meaning the reviewers successfully re-ran the pipeline on a fresh machine using only the Docker container and documentation. That said, six years later:

- Docker Hub URLs may have rotted (the paper's container is `jamiedavis/ecosystem-redos` — last verified 2020).
- The Node.js version used (v8.x) is EOL; modern V8's `irregexp` is somewhat faster on some patterns, so exact number reproduction requires pinning the Node version.
- pypi and npm have both evolved dramatically; a fresh extraction would yield a very different (larger, differently distributed) dataset.
- The SBULeeLab GitHub repo mirroring the artifact (`01KNWEGYBDMW0V0CFVJ83A4B9J`) is the alternate source.

## Citation

```
@misc{davis2018redosartifact,
  author       = {James C. Davis and Christy A. Coghlan and Francisco Servant and Dongyoon Lee},
  title        = {{Artifact for "The Impact of Regular Expression Denial of Service (ReDoS) in Practice: An Empirical Study at the Ecosystem Scale"}},
  month        = nov,
  year         = 2018,
  publisher    = {Zenodo},
  version      = {1.0.0},
  doi          = {10.5281/zenodo.1294301},
  url          = {https://doi.org/10.5281/zenodo.1294301}
}
```

## Relevance to APEX G-46

1. **The dataset is a benchmark corpus.** APEX's static ReDoS detector should be evaluated against the Davis et al. labelled set — 500K regexes with ground truth (ensemble + dynamic validation). Any new static detector should report precision and recall on this corpus.
2. **The anti-pattern taxonomy is a starting ruleset.** APEX's cheapest tier of ReDoS detection can be "do any of the five Davis anti-patterns match?" This is O(regex size) and catches 80%+ of the real-world cases.
3. **The ensemble-plus-validation design is the recommended architecture.** APEX should not pick one ReDoS detector; it should run multiple in parallel and flag on agreement, with a dynamic "does it actually slow down on this input" confirmation step.
4. **Cross-package reuse insights.** The 339 distinct super-linear regexes appearing in ≥10 packages is the argument for a **shared vulnerability database** keyed by regex literal (or a normalised form of it). If a regex in the current codebase hashes to a known-bad entry, APEX can flag with high confidence.
5. **The 3.5% figure is the marketing number.** When making the case for the ReDoS detector to users, "3.5% of regexes in the wild are vulnerable" is the statistic to cite.

## References

- Zenodo record — [zenodo.org/records/1294301](https://zenodo.org/records/1294301)
- Paper — Davis, Coghlan, Servant, Lee — ESEC/FSE 2018 — `01KNWEGYBDMW0V0CFVJ83A4B9J`
- GitHub mirror — [github.com/SBULeeLab/EcosystemREDOS-FSE18](https://github.com/SBULeeLab/EcosystemREDOS-FSE18)
- vuln-regex-detector (successor tool) — `01KNWGA5GMWKV6AKP04D964G5H`
