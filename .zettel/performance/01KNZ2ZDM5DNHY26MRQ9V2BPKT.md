---
id: 01KNZ2ZDM5DNHY26MRQ9V2BPKT
title: "MITRE/CISA 2024 CWE Top 25 Most Dangerous Software Weaknesses"
type: literature
tags: [mitre, cisa, cwe-top25, 2024, cwe-400, cwe-1333, rankings]
links:
  - target: 01KNWE2Q9YZBAR140ZX5P36TQ5
    type: references
  - target: 01KNWGA5F4W852RG6C5FJCP204
    type: references
  - target: 01KNWGA5F7VA68B8ZPB6XR0RTE
    type: references
created: 2026-04-12
modified: 2026-04-12
source: "https://cwe.mitre.org/top25/archive/2024/2024_top25_list.html"
---

# 2024 CWE Top 25 Most Dangerous Software Weaknesses

*Source: https://cwe.mitre.org/top25/archive/2024/2024_top25_list.html — fetched 2026-04-12.*
*Published by MITRE and CISA, November 20, 2024.*

## What this list is

Each year, MITRE and CISA rank the top 25 software weakness classes by a combined severity-and-frequency score, computed from the year's CVE records. The 2024 list is derived from **31,770 CVE records** published (roughly) in 2023–2024 that map to a CWE.

The score is not just "how many CVEs were filed under this CWE" — each CVE is weighted by its CVSS base score, so a rare but devastating weakness outranks a common but low-impact one. Formally:

> `score(CWE) = (frequency / max_frequency) × (avg_CVSS_severity / max_severity) × 100`

## The 2024 top 25 (with scores)

| Rank | CWE | Name | Score | Movement |
|---:|---:|---|---:|:---:|
| 1 | CWE-79 | Cross-site Scripting (XSS) | 56.92 | +1 |
| 2 | CWE-787 | Out-of-bounds Write | 45.20 | -1 |
| 3 | CWE-89 | SQL Injection | 35.88 | 0 |
| 4 | CWE-352 | Cross-Site Request Forgery | 19.57 | +5 |
| 5 | CWE-22 | Path Traversal | 12.74 | +3 |
| 6 | CWE-125 | Out-of-bounds Read | 11.42 | +1 |
| 7 | CWE-78 | OS Command Injection | 11.30 | -2 |
| 8 | CWE-416 | Use After Free | 10.19 | -4 |
| 9 | CWE-862 | Missing Authorization | 10.11 | +2 |
| 10 | CWE-434 | Unrestricted File Upload | 10.03 | 0 |
| ... | ... | ... | ... | ... |
| **24** | **CWE-400** | **Uncontrolled Resource Consumption** | **3.23** | **+13** |
| 25 | — | (rank 25 varies in published drafts) | — | — |

**CWE-1333** (Inefficient Regular Expression Complexity / ReDoS) does **not** appear in the 2024 Top 25. It is in the **CWE Weaknesses on the Cusp** list — the 25-50 band — having risen from obscurity post-Cloudflare-2019.

## The CWE-400 surge is the headline for G-46

CWE-400 moved **13 places** — from rank 37 in 2023 to rank 24 in 2024. That is the largest upward movement in the 2024 list. This matters for APEX because:

1. The upward movement is the explicit market signal for why gap G-46 is a priority — users are seeing resource-consumption bugs in the wild at a rate that finally crossed into top 25 visibility.
2. The CWE-400 score of 3.23 understates the impact because CVSS consistently under-weights availability. The CWSS (severity-metric alternative) would score these higher.
3. ReDoS is still sub-threshold, but the trend (Cloudflare 2019, Stack Exchange 2016, npm incidents) suggests CWE-1333 will cross into the list within 2-3 annual cycles.

## Methodology notes

- **Data source**: CVE records from the National Vulnerability Database (NVD) published in a rolling 2-year window.
- **CWE mapping**: CVEs are tagged with their closest CWE. If a CVE is tagged with a child CWE (e.g., CWE-1333), it rolls up to the parent (CWE-400) for the list but retains its specific CWE for analytics.
- **Score normalisation**: all scores are scaled to 0-100 so the top rank = 100.
- **Changes vs 2023**: the 2024 methodology is unchanged from 2023; rankings changes reflect real-world CVE patterns, not methodology drift.

## CWEs in G-46's scope and where they sit

| CWE | 2024 rank | Note |
|---|---:|---|
| CWE-400 — Uncontrolled Resource Consumption | 24 | In top 25; the umbrella for G-46 findings |
| CWE-1333 — Inefficient Regex Complexity | "cusp" (26-50 band) | The ReDoS specialisation |
| CWE-407 — Inefficient Algorithmic Complexity | not ranked | Older, less frequently tagged |
| CWE-834 — Excessive Iteration | not ranked | Parent of most quadratic-loop CVEs |
| CWE-789 — Memory Allocation with Excessive Size | not ranked | XML bomb / zip bomb parent |

All five are in G-46's remit. APEX Findings for performance issues should tag the most specific CWE and additionally roll up to CWE-400 for top-25 alignment in summary reports.

## Relevance to APEX G-46

1. **Severity calibration.** When APEX emits a Finding for a DoS / resource-exhaustion bug, its severity should default to "medium" but escalate to "high" if the target is user-facing (CWE-400 rank 24 severity proxy).
2. **Report framing.** Executive summaries should reference the 2024 Top 25 rank movement as the business-risk justification for spending engineer time on G-46 findings.
3. **Cross-CWE rollups.** APEX's Finding output schema already includes a `cwe` field — but the CI summary view should also show the **rollup** to CWE-400 so operators can track their exposure against the top-25 list without manually merging sub-CWEs.
4. **Trendline coverage.** The cusp-to-top-25 pipeline gives APEX a forward-looking target list: CWE-1333 (likely to enter soon), CWE-770 (Missing Limitation on Resources), CWE-920 (Improper Restriction of Power Consumption), CWE-1284 (Improper Validation of Specified Quantity in Input). Adding coverage for these is betting on where the list goes next.

## References

- MITRE / CISA — "2024 CWE Top 25 Most Dangerous Software Weaknesses" — [cwe.mitre.org](https://cwe.mitre.org/top25/archive/2024/2024_top25_list.html)
- Methodology page — [cwe.mitre.org/top25/archive/2024/2024_methodology.html](https://cwe.mitre.org/top25/archive/2024/2024_methodology.html)
- CWE-400 definition — `01KNWGA5F4W852RG6C5FJCP204`
- CWE-1333 definition — `01KNWGA5F7VA68B8ZPB6XR0RTE`
