---
id: 01KNZ7829AP3CN4KCMV1RE7WYC
title: Collberg & Proebsting 2016 — Repeatability in Computer Systems Research (CACM)
type: literature
tags: [collberg, proebsting, repeatability, reproducibility, cacm-2016, artifact-evaluation, systems-research]
links:
  - target: 01KNZ6FPQ32R61BEN1K1WNZGPX
    type: related
  - target: 01KNZ6FPS6S89303Q1F11G6M1A
    type: related
  - target: 01KNWE2QACWYZJXRJ8QE78T043
    type: related
  - target: 01KNZ4VB6JR9DSJA90V0WAW1TF
    type: related
  - target: 01KNWE2Q9YZBAR140ZX5P36TQ5
    type: related
created: 2026-04-11T21:30:26.986162+00:00
modified: 2026-04-11T21:30:26.986174+00:00
---

*Source: Christian Collberg, Todd A. Proebsting — "Repeatability in Computer Systems Research" — Communications of the ACM, Vol. 59, No. 3, March 2016, pp. 62-69. ACM DOI 10.1145/2812803.*

The paper that shamed computer systems research into caring about repeatability. Collberg and Proebsting tried to run the code from **601 published papers** and measured how hard it was. The results are the most-cited evidence for the reproducibility crisis in systems research and the primary argument for the artifact evaluation processes now standard at top systems conferences.

## The experiment

The authors systematically attempted to **build and execute the code** from 601 computer systems papers published at top venues (OSDI, SOSP, ASPLOS, PLDI, POPL, and similar). For each paper they:
1. Tried to obtain the source code (from the paper's supplementary material, project websites, or by emailing the authors).
2. Tried to build it within 30 minutes of engineering effort.
3. Recorded at what stage the attempt failed.

They did **not** try to reproduce the numerical results of the papers — only to run the code at all. "Repeatability" in their framing is the weaker bar: can a reasonably-motivated reader run your artifact?

## The results

From the paper's abstract and expanded findings:

> "Their assistants could obtain and build the source code within 30 minutes in 32.3% of the studied cases; 15.9% of the artifacts required more than 30 minutes, and 5.7% required additional but still reasonable efforts."

Collating the numbers:
- **~32%** of papers had code that could be built in 30 minutes.
- **~16%** required more than 30 minutes but were eventually buildable.
- **~6%** required "reasonable extra effort" to get running.
- The remaining **~46%** could not be built at all — the code was not available, or was available but did not build.

In other words: **roughly half of top systems papers do not ship a reproducible artifact**. A reader cannot run the authors' code even to do the most basic check on the paper's claims.

## The definitions that matter

The paper draws a careful distinction:

> "Research is repeatable if researchers can re-run the original experiment using the same method in the same environment and obtain the same results. Unlike repeatability, reproducibility does not necessarily require access to the original research artifacts, but rather is the independent confirmation of a scientific hypothesis done post-publication using different properties from different experiments."

This distinction has become standard in the ACM's artifact-evaluation taxonomy:
- **Repeatability**: same team, same artifact, same environment → same result.
- **Reproducibility**: different team, same or different artifact, possibly different environment → same result.
- **Replicability**: different team, different artifact, same scientific claim.

Collberg & Proebsting measured **repeatability** and found a huge gap.

## Why this matters for CI perf gating

The paper's findings are a direct challenge for anyone publishing a CI perf methodology:
1. **Your "novel" benchmarking methodology must be runnable by others** if it is going to be adopted. Most aren't.
2. **Perf claims in papers are unverifiable** by reviewers who can't reproduce the experimental setup. Reviewers accept numerical claims on faith.
3. **The CI perf tooling ecosystem is fragmented** precisely because existing papers cannot be built on top of — if you can't run a predecessor's code, you reimplement.
4. **Public benchmark datasets are rare.** The Besbes et al. 2025 Perfherder dataset (see dedicated note) is noteworthy because it is one of the few real industrial perf datasets publicly available.

## Recommendations from the paper

Collberg & Proebsting propose:
- **Funding agencies should reward repeatability.** NSF and similar should make shareable artifacts part of grant outcomes.
- **Conferences should require an artifact track** with a badge system (repeatable / reproducible / reusable). This has happened at most top venues since 2016 — SIGPLAN, SIGOPS, USENIX, and ACM SIGSOFT all now run artifact evaluation committees.
- **Authors should commit to sharing** at submission time, not at publication time.

The artifact evaluation movement can be directly traced to this paper's influence.

## Impact on the field (2016–2026)

Since publication:
- **PLDI, OOPSLA, OSDI, SOSP, ASPLOS, ISCA** all run artifact evaluation committees.
- **ACM has a three-tier badging system**: Artifact Available, Artifact Evaluated (Functional / Reusable), Results Replicated / Reproduced.
- **Reproducibility-oriented tools**: Popper (Jimenez et al. 2017), CodeOcean, WholeTale, Guix, Nix flakes, Docker for research artifacts.
- **PerfDocs and similar benchmark repositories** explicitly citing Collberg & Proebsting as motivation.

Despite these, subsequent studies (notably by Krishnamurthi, Patterson and Mytkowicz, and the Popper team) show that **repeatability has improved but not by as much as one would hope**. Artifacts still commonly fail to build after a few years because of bit-rot in dependencies.

## Adversarial commentary

- **The 30-minute cutoff is arbitrary.** A researcher willing to spend a day can often get things working. The paper's methodology biases towards strict, reviewer-like effort levels. But this bias is intentional: if *reviewers* don't have time, the bar matters.
- **Success at building ≠ success at reproducing numbers.** Even if you build the artifact, your numbers may differ. The paper does not address this stronger form of reproducibility.
- **Perf research specifically is worse than the average.** Systems papers as a group are bad; perf papers add the extra challenge of reproducing *numerical* results on *different* hardware, which amplifies Mytkowicz-style layout noise.
- **The paper doesn't propose a remedy, just names the problem.** Subsequent artifact-evaluation processes are the remedy.

## Connections

- Mytkowicz et al. 2009 — even if you *can* reproduce the build, layout noise means you may not reproduce the numbers.
- Stabilizer (Curtsinger & Berger 2013) — provides one answer for reproducibility under layout noise.
- ACM Artifact Review and Badging — operational response to Collberg & Proebsting.
- The Besbes et al. 2025 Perfherder dataset — counterexample: a public, labelled, reusable perf dataset.

## Reference

Collberg, C., Proebsting, T. A. (2016). *Repeatability in Computer Systems Research*. Communications of the ACM, 59(3): 62-69. DOI 10.1145/2812803. cacm.acm.org/research/repeatability-in-computer-systems-research/
