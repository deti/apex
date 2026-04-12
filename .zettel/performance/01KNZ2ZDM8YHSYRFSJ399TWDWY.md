---
id: 01KNZ2ZDM8YHSYRFSJ399TWDWY
title: "Google SRE Workbook: Table of Contents"
type: literature
tags: [sre, google, sre-workbook, slo, toil, overload, reliability]
links:
  - target: 01KNWE2QA5VP0K80TMSABACKWT
    type: references
  - target: 01KNYZ7YKPN5VE39GKTVDE9FB4
    type: related
  - target: 01KNZ2ZDMA4SCFK7QSPAJTPTGP
    type: references
created: 2026-04-12
modified: 2026-04-12
source: "https://sre.google/workbook/table-of-contents/"
---

# Google SRE Workbook — Table of Contents

*Source: https://sre.google/workbook/table-of-contents/ — fetched 2026-04-12.*
*Editors: Betsy Beyer, Niall Richard Murphy, David K. Rensin, Kent Kawahara, Stephen Thorne. O'Reilly, 2018. Free online.*

The Workbook is the practical companion to the 2016 "Site Reliability Engineering" book. Where the first book explains **what** SREs do, the Workbook shows **how**, with case studies from Google, Evernote, The Home Depot, and others. Every chapter builds on a concept first introduced in the SRE Book.

## Full ToC with performance-relevance highlights

### Introductory material
- Foreword I (Stephen Thorne)
- Foreword II (Alex Matey)
- Preface
- **Chapter 1 — How SRE Relates to DevOps** — definitional piece mapping the SRE role onto the DevOps vocabulary. Not directly relevant to G-46 but useful context.

### Part I — Foundations
- **Chapter 2 — Implementing SLOs** ★ — the concrete how-to for defining SLIs and SLOs from scratch; see dedicated note `01KNZ2ZDMA4SCFK7QSPAJTPTGP`.
- **Chapter 3 — SLO Engineering Case Studies** — Evernote, The Home Depot implementing SLOs in real production. Case study format.
- **Chapter 4 — Monitoring** — the "four golden signals" (latency, traffic, errors, saturation) practical guide.
- **Chapter 5 — Alerting on SLOs** ★ — burn-rate alerts, multiwindow multiburnrate, the canonical pattern for high-signal SLO-based paging.
- **Chapter 6 — Eliminating Toil** — what toil is, how to measure it, how to automate it away.
- **Chapter 7 — Simplicity** — design-for-simplicity case studies.

### Part II — Practices
- **Chapter 8 — On-Call** — oncall load, rotation design, handoff.
- **Chapter 9 — Incident Response** — ICS-inspired incident command.
- **Chapter 10 — Postmortem Culture: Learning from Failure** — the blameless postmortem playbook.
- **Chapter 11 — Managing Load** ★ — directly relevant to G-46. Load-shedding, graceful degradation, traffic prioritisation, client retries and backoff. This is the operational counterpart to APEX's "what happens when the system is under attack" Findings.
- **Chapter 12 — Introducing Non-Abstract Large System Design** — NALSD methodology.
- **Chapter 13 — Data Processing Pipelines** — SLOs for batch and streaming pipelines, including freshness SLIs.
- **Chapter 14 — Configuration Design and Best Practices** — config as code.
- **Chapter 15 — Configuration Specifics** — worked examples.
- **Chapter 16 — Canarying Releases** — progressive rollout and automated abort.

### Part III — Processes
- **Chapter 17 — Identifying and Recovering from Overload** ★ — this is the most performance-relevant chapter. Queuing theory in practice, how to recognise overload from metrics, and four recovery patterns: shedding, throttling, graceful degradation, and service restart. The chapter's definition of "overload" dovetails with the resource-exhaustion class that G-46 is designed to catch proactively.
- **Chapter 18 — SRE Engagement Model** — when SRE takes a service on/off.
- **Chapter 19 — SRE: Reaching Beyond Your Walls** — external SRE culture propagation.
- **Chapter 20 — SRE Team Lifecycles** — team formation, maturation, dissolution.
- **Chapter 21 — Organizational Change Management in SRE** — internal change management.

### Appendices
- **Appendix A — Example SLO Document** — a copy-pasteable SLO doc template.
- **Appendix B — Example Error Budget Policy** ★ — the canonical escalation policy for when a service burns its error budget. Template for APEX's baseline-regression policy output.
- **Appendix C — Results of Postmortem Analysis** — aggregate lessons from 18 months of Google postmortems.

## The chapters G-46 references directly

| Chapter | Why it matters for APEX |
|---|---|
| Ch. 2 — Implementing SLOs | The how-to for what G-46's `--slo` flag asserts. |
| Ch. 5 — Alerting on SLOs | Multi-burn-rate is the right model for regression alerts in CI. |
| Ch. 11 — Managing Load | Counterpart: if APEX detects a latent quadratic, ch. 11 is what the operator should do at runtime until it's fixed. |
| Ch. 17 — Identifying and Recovering from Overload | The operations view of what a resource-exhaustion CVE looks like in production. |
| Appendix B — Error Budget Policy | Template for an APEX baseline-regression policy. |

## Relevance to APEX G-46

1. **SLO language alignment.** APEX Findings for performance issues should use the SLI/SLO/SLA vocabulary from Chapter 2 (and its SRE-Book predecessor) rather than inventing new terms. Users who know SRE will understand immediately; everyone else gets pointed at a canonical reference.
2. **Burn-rate thinking for CI.** Rather than "test X is now 2x slower, fail the build", APEX can adopt the Ch. 5 multi-burn-rate approach: a big single-commit regression trips the short window; a slow creep over 30 commits trips the long window.
3. **The `apex perf` command is a pre-production SRE tool.** Everything in the Workbook about runtime operations has a design equivalent at static-analysis / test-generation time. Load-shedding's pre-production analogue is "cap number of items the parser accepts".
4. **Error-budget policy template.** Appendix B is a concrete document template to derive APEX's "when your performance baseline burns down X% of its budget, escalation path is Y" policy output.

## References

- Beyer, Murphy, Rensin, Kawahara, Thorne — "The Site Reliability Workbook" — O'Reilly 2018 — [sre.google/workbook](https://sre.google/workbook/)
- Predecessor: "Site Reliability Engineering" (2016) — `01KNYZ7YKPN5VE39GKTVDE9FB4` (SLO chapter)
- Chapter 2 deep-dive: `01KNZ2ZDMA4SCFK7QSPAJTPTGP`
