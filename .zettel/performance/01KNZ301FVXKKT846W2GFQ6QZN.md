---
id: 01KNZ301FVXKKT846W2GFQ6QZN
title: "Davis et al.: Using Selective Memoization to Defeat ReDoS"
type: literature
tags: [paper, redos, cwe-1333, memoization, defense, regex-engine, davis]
links:
  - target: 01KNWE2QA1E37T7GVKSM6EGXXR
    type: references
  - target: 01KNWGA5F7VA68B8ZPB6XR0RTE
    type: extends
  - target: 01KNWEGYBDMW0V0CFVJ83A4B9J
    type: related
  - target: 01KNYZ7YKH344XCTAFQAHQNYHG
    type: related
  - target: 01KNWGA5G80W4ESMANJM0M2XAV
    type: related
  - target: 01KNZ301FVEJEFXWZQRNCB36SS
    type: related
created: 2026-04-12
modified: 2026-04-12
source: "https://davisjam.github.io/publications/"
paper: "Using Selective Memoization to Defeat Regular Expression Denial of Service (ReDoS)"
venue: IEEE S&P 2021
authors: [James C. Davis, Francisco Servant, Dongyoon Lee]
year: 2021
---

# Davis et al.: Using Selective Memoization to Defeat ReDoS

**Paper:** "Using Selective Memoization to Defeat Regular Expression Denial of Service (ReDoS)"
**Venue:** IEEE Symposium on Security and Privacy (S&P) 2021
**Authors:** James C. Davis (Purdue), Francisco Servant (Virginia Tech), Dongyoon Lee (Stony Brook)
**Author page:** https://davisjam.github.io/publications/

## Positioning

This paper is the second major pillar of James C. Davis's multi-year research program on ReDoS. His ESEC/FSE 2018 paper (already in the vault, note `01KNWEGYBDMW0V0CFVJ83A4B9J`) is an *empirical* study demonstrating the scale of the ReDoS problem across software ecosystems. The S&P 2021 paper is the *mitigation* response: a concrete engine-level fix that defeats catastrophic and super-linear backtracking without rewriting existing regex patterns or asking developers to switch to a non-backtracking engine like RE2.

The background literature offers developers three defensive options that each have serious limitations:

1. **Rewrite every vulnerable regex.** Labor-intensive, error-prone, and requires knowing which regexes are vulnerable in the first place (hence the need for detectors like Regexploit).
2. **Switch to a non-backtracking engine** (RE2, Rust's `regex` crate, Go's `regexp`). Free of ReDoS by construction but does not support backreferences, lookaround, or some Unicode features that real-world regexes depend on.
3. **Add a per-match timeout.** Catches symptoms, doesn't fix the underlying algorithmic issue, and is not supported by many engines.

Davis's contribution is a fourth option: **keep the existing backtracking engine and its full semantics (backreferences, lookaround, and all), but eliminate the super-linear blow-up by memoizing intermediate match states**. Because the classical NFA simulation of a regex visits `O(n * m)` distinct (position, NFA-state) pairs at most, and memoization eliminates re-visits, the resulting engine runs in worst-case linear time in input length for every regex — *even regexes with backreferences, within the constraints the paper analyzes*.

## Theory: why memoization bounds backtracking

A backtracking regex engine's running time blow-up comes entirely from re-visiting the same (position, sub-match state) pair many times via different branches of the search tree. For a pattern like `(a*)*b`, the engine may reach position 3 with the outer `*` having iterated once, twice, three times, or zero times — all four are distinct sub-match states that diverge on later characters. If you memoize the result of "can we match the rest from state `s` at position `p`?", the second time the engine reaches the same `(s, p)`, it immediately reuses the cached answer.

The observation is Thompson's classical NFA simulation in a different guise: the state space of a regex over an input of length `n` has at most `O(n * |Q|)` points, where `|Q|` is the number of NFA states. Pure backtracking explores this space inefficiently; memoization bounds the exploration.

## The engineering challenge: making memoization *selective*

The obvious objection to "just memoize everything" is that memoizing every position-state pair for every match would explode memory. For a 1 MB input and a regex with 100 NFA states, naïve memoization stores 100M entries — gigabytes.

Davis's "selective memoization" contribution is identifying which states and positions *actually need* memoization. The key observations:

1. **Most (state, position) pairs are only visited once.** The super-linear blow-up comes from a small number of "ambiguous" states that get revisited many times. If you detect those states statically (by NFA analysis) and only memoize *them*, memory overhead is bounded while the correctness and speedup guarantees still hold.
2. **Ambiguous states can be identified by NFA analysis.** The paper describes an algorithm for finding the set of states that would be revisited in the worst case, reusing ideas from the ReDoS-detection literature (including Davis's own earlier tooling) — the same analysis used to *find* vulnerable regexes can be used to *cure* them.
3. **Per-pattern overhead amortizes across matches.** Memoization structures can be set up once per regex and reused across every match. Compilation cost goes up slightly; per-match cost on adversarial inputs drops from super-linear to linear.

## Engine integration

The paper demonstrates the technique by retrofitting it into:

- A Java regex engine (Java's `java.util.regex`).
- A production Node.js regex engine.
- A Python regex engine.

Each retrofit adds only moderate complexity to the engine and breaks no existing tests. For each, the paper shows that previously-vulnerable patterns (drawn from real CVEs like the ones for Moment.js, semver, and word-wrap) now execute in linear time on their pathological inputs, while average-case performance on non-pathological inputs is essentially unchanged.

## Results

Headline claim: across a large set of real-world regexes (drawn from the same ecosystem-scale corpus as the ESEC/FSE 2018 empirical study), selective memoization eliminates ReDoS vulnerabilities without meaningful average-case slowdown and without breaking any regex feature. The overhead for non-adversarial inputs is in the single-digit percent range; for adversarial inputs it bounds the cost at `O(n)` instead of `O(n^2)` or `O(2^n)`.

## Relevance to APEX G-46

This paper is the right *defensive* citation for any G-46 finding of class CWE-1333. When APEX flags a vulnerable regex, the report should offer the developer three remediation tiers:

1. **Rewrite the regex.** Link to the Snyk explainer and Regexploit's PoC output.
2. **Migrate the engine.** Link to RE2/Rust-regex and explain the feature trade-off.
3. **Patch the engine with selective memoization.** Link to this paper, especially for codebases that depend on backreferences or lookaround and cannot move to RE2.

Beyond remediation guidance, selective memoization is also a *plausible engine-level defense* that APEX could ship as a wrapper library for Python, JavaScript, and Java targets. If APEX's scope expands from "generate a performance test" to "emit a hardened library patch," this is the technique to build on.

## Related Davis work worth cross-referencing

From the same author and research program:

- **ESEC/FSE 2018 (already in vault):** the empirical ecosystem study that motivates the S&P 2021 paper.
- **ICSE 2022 (Barlas, Du, Davis): "Exploiting Input Sanitization for Regex Denial of Service."** New ReDoS exploitation technique through sanitization layers.
- **S&P 2023 (Hassan et al.): "Improving Developers' Understanding of ReDoS Tools."** Usability study of static ReDoS detectors — useful for designing APEX's report UX.
- **ICSE 2019 (Davis et al.): "Regexes are Hard."** Best Paper. Developer cognition study on regex programming.
- **Dagstuhl 2024 talk "ReDoS: Past, Present, and Future"** — a summary of the whole program and the road map to systemic fixes.

## Broader Davis research program on regex

Davis has published a cumulative body of work covering:

- Empirical study of ReDoS in practice (ESEC/FSE 2018).
- Cross-language regex portability and generalizability (ESEC/FSE 2019, ASE 2019).
- Developer cognition (ASE 2019 best paper).
- Engine-level defense via selective memoization (S&P 2021).
- Sanitization-assisted exploitation (ICSE 2022).
- Tool usability (S&P 2023).
- Regex engine testing methodology (ICSE JAWs 2026).
- Backreference-induced ReDoS (arXiv 2026, forthcoming).

Any comprehensive G-46 design for regex-heavy targets should survey this corpus; the two papers currently in the APEX vault (ESEC/FSE 2018 and this S&P 2021 note) cover the most load-bearing results.
