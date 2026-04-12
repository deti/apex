---
id: 01KNZ301FVEJEFXWZQRNCB36SS
title: "Semgrep + Dlint: Improving ReDoS Detection via Large-Scale Scanning"
type: reference
tags: [tool, redos, static-analysis, semgrep, dlint, r2c, python, empirical, cve-2020-6817]
links:
  - target: 01KNWE2QA1E37T7GVKSM6EGXXR
    type: references
  - target: 01KNWGA5F7VA68B8ZPB6XR0RTE
    type: extends
  - target: 01KNWGA5GMWKV6AKP04D964G5H
    type: related
  - target: 01KNZ301FV9DXAXN39MPPAG9JV
    type: related
  - target: 01KNWEGYBDMW0V0CFVJ83A4B9J
    type: related
  - target: 01KNZ2ZDMZM2PTFEAZ18TAJ3V0
    type: related
  - target: 01KNZ301FVXKKT846W2GFQ6QZN
    type: related
created: 2026-04-12
modified: 2026-04-12
source: "https://semgrep.dev/blog/2020/improving-redos-detection-with-dlint-and-r2c/"
author: "Semgrep (r2c) engineering team"
year: 2020
---

# Semgrep + Dlint: Improving ReDoS Detection via Large-Scale Scanning

**Post:** https://semgrep.dev/blog/2020/improving-redos-detection-with-dlint-and-r2c/
**Publisher:** Semgrep / r2c
**Year:** 2020

## Context

This blog post describes how the Semgrep (then r2c) team used their distributed analysis platform to improve Dlint's ReDoS detector, and in the process discovered CVE-2020-6817 in Mozilla's Bleach HTML sanitization library. It is a compact case study in the "detect → triage at scale → improve the detector → re-scan" loop that is exactly what an APEX G-46 static-analysis pipeline should look like.

## Methodology

The r2c platform runs arbitrary static analyzers against thousands of open-source repositories in parallel, collects findings, and presents them for triage. For a ReDoS-detector improvement project, the loop looked like:

1. Run Dlint's existing ReDoS checker against ~10,000 public Python repositories on the platform.
2. Triage the findings into three buckets:
   - **False positives** — regexes flagged as catastrophic but actually safe.
   - **True vulnerabilities** — authentic ReDoS bugs worth reporting.
   - **Tool deficiencies** — cases where Dlint missed a real ReDoS or misclassified a safe pattern. These drive detector improvements.
3. Use the aggregate false-positive rate across the platform as the objective function for improvements.
4. Modify Dlint and re-scan to measure the improvement concretely.

This closed loop is what distinguishes large-scale static analysis from ad-hoc grep-based detection: every change can be quantitatively measured on a realistic corpus.

## Key detector improvement: require a non-matching tail

The team observed that Dlint was producing many false positives for regex patterns that contained nested quantifiers but *no* required character after them. Their example contrast:

- `(a+)+b` followed by a non-`a` non-`b` character **does** trigger catastrophic backtracking. The trailing `b` forces the engine to test every partition of the `a` run.
- `(a+)+` with no trailing required character **does not** trigger catastrophic backtracking. The engine greedily matches the whole `a` run on the first try and never has to backtrack.

The difference is whether the engine is ever forced to re-partition. Without a mandatory tail, the greedy match succeeds and no backtracking occurs.

Adding this "nested quantifier must be followed by a non-optional character that can cause match failure" constraint to Dlint eliminated **22.8% of false positives (28 of 123 cases)** in a single change. The same refinement is implemented in Doyensec's Regexploit (under the name "required tail") and in the academic literature going back to Chapman & Stolee's empirical ReDoS work.

## Case study: CVE-2020-6817 (Mozilla Bleach)

Dlint's improved detector flagged a real ReDoS in the `sanitize_css` function of the Bleach HTML sanitization library. Bleach is Mozilla's canonical "safe HTML" sanitizer used by millions of applications that accept user-submitted rich text.

### Vulnerable pattern

The vulnerable regex lives in Bleach's CSS sanitizer. It contains overlapping alternation inside a quantifier, with `\w` appearing in multiple alternation branches. Because `\w` matches every alphanumeric character, two branches of the alternation both consume the same characters, and the engine must try every partition of the input when the overall match ultimately fails.

### Exploitation

The exploit path requires three conditions:

1. An application must call `bleach.clean()` with `styles=[...]` explicitly populated. The default Bleach configuration disables style attributes entirely, so the bug does not affect default users.
2. A user-supplied HTML fragment must reach `bleach.clean()` with a `style` attribute containing repeating word-character pairs — e.g., `style="b-b-b-b-..."` — followed by a character outside the regex's allowed set (the blog post uses `^`).
3. When Bleach passes the `style` value through the CSS sanitizer regex, the regex engine enters the catastrophic backtracking path.

The resulting CPU stall is long enough to serve as a denial-of-service on any web application running Bleach with style attributes enabled.

### Mitigation

The Mozilla security team acknowledged the issue quickly and patched it, assigning CVE-2020-6817. The fix rewrites the CSS parser regex to avoid overlapping alternation.

## Additional findings

Using the same approach, r2c reported ReDoS issues in:

- **Splunk SDK for Python** — internal regex patterns vulnerable to backtracking.
- **Python GSSAPI requests library** — PR #22 fixes a vulnerable pattern in authentication header parsing.

## Relevance to APEX G-46

Two lessons transfer directly to APEX:

1. **Empirical calibration matters as much as the algorithm.** A ReDoS detector's algorithm is only the starting point; running it on a realistic corpus and measuring false-positive rate identifies real gaps (like the missing required-tail check) that pure unit testing will not. An APEX static pipeline should follow the same loop: ship a detector, run it on a large corpus, triage by hand, refine.

2. **Static and dynamic detection complement each other.** The r2c team found CVE-2020-6817 via a pure static scan. Coverage-guided dynamic fuzzing would have needed a harness that reaches `bleach.clean()` with a specific configuration — much higher setup cost. On the other hand, dynamic fuzzing finds polymorphic bugs that static rules cannot encode. A complete APEX pipeline needs both.

## Semgrep's current ReDoS detection

Present-day Semgrep offers ReDoS detection as a metavariable analyzer (`metavariable-analysis: redos`). Its rules target two canonical catastrophic-backtracking patterns:

1. **Nested quantifiers:** patterns like `(a+)+` where a quantified group sits inside another quantifier. The inner `+` and outer `+` produce exponentially many match paths.
2. **Mutually inclusive alternation:** patterns like `([a-z]|a)+` where alternation branches have overlapping character classes. Each character in the input can be assigned to either branch, producing exponentially many match paths.

Both patterns are flagged only when followed by a required character that can trigger failure — the refinement this blog post describes.

## Citations

- Semgrep ReDoS detection post (this blog): https://semgrep.dev/blog/2020/improving-redos-detection-with-dlint-and-r2c/
- CVE-2020-6817 (Mozilla Bleach): NVD entry
- Semgrep metavariable-analysis docs: https://semgrep.dev/docs/writing-rules/metavariable-analysis
