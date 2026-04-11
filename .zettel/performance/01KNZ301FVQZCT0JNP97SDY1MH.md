---
id: 01KNZ301FVQZCT0JNP97SDY1MH
title: "Snyk: ReDoS and Catastrophic Backtracking — Practitioner Explainer"
type: reference
tags: [redos, explainer, snyk, catastrophic-backtracking, momentjs, javascript, mitigations]
links:
  - target: 01KNWE2QA1E37T7GVKSM6EGXXR
    type: references
  - target: 01KNWGA5F7VA68B8ZPB6XR0RTE
    type: extends
  - target: 01KNYZ7YKF7DHTFHJ50C7AE403
    type: related
  - target: 01KNZ301FV584G004E2SRNW97Z
    type: related
  - target: 01KNZ301FV9DXAXN39MPPAG9JV
    type: related
created: 2026-04-12
modified: 2026-04-12
source: "https://snyk.io/blog/redos-and-catastrophic-backtracking/"
author: "Snyk"
---

# Snyk: ReDoS and Catastrophic Backtracking — Practitioner Explainer

**Post:** https://snyk.io/blog/redos-and-catastrophic-backtracking/
**Publisher:** Snyk

## What this post is

A practitioner-oriented explainer on ReDoS covering three things:

1. The theoretical mechanism by which catastrophic backtracking produces exponential or super-linear execution times.
2. A worked case study on Moment.js, one of the most-cited real-world ReDoS CVEs.
3. A menu of defensive techniques with concrete examples per regex flavor.

## Theory: how regex engines produce ReDoS

Most production regex engines (PCRE, JavaScript's built-in engine, Python's `re`, Ruby, Java, .NET before the recent rewrite) are **backtracking NFA simulators**. They try each alternative at each branch point in turn; when the current attempt fails later, they unwind to the most recent choice point and try the next alternative. For most regexes this is cheap because the number of choice points is small.

For regexes containing nested or adjacent quantifiers, however, the number of *ways* the engine can partition a given substring across the ambiguous regions grows exponentially in substring length. Snyk summarizes: "there is an exponential relationship between the length of the string and the number of paths the engine has to evaluate."

When the overall match ultimately fails (because of a trailing character the engine cannot consume), the engine has to enumerate every one of those paths before concluding the match is impossible. That enumeration is what the user experiences as a stalled process.

## Worked example: `/A(B|C+)+D/` on `ACCCCCX`

The post contrasts the engine's behavior on:

- **Valid input** `"ACCCCCCCCCCCCCCCCCCCCCCCCCCCCD"`: matches in about 52 ms.
- **Invalid input** `"ACCCCCCCCCCCCCCCCCCCCCCCCCCCCX"`: takes about 1.8 seconds — roughly 35× slower.

The pattern `A(B|C+)+D` has two nested ambiguities: the inner `+` on `C` and the outer `+` on the group. A run of Cs can be partitioned across the outer repetition in many ways — 2^n in the worst case. When the trailing `X` does not satisfy the `D` anchor, the engine must try every partition before concluding failure. Each additional `C` doubles the running time.

This is the simplest demonstration of the pattern; every real-world ReDoS CVE (semver, word-wrap, uri-js, uap-core, Moment.js) is an application of the same mechanism to a grammar where the ambiguity is harder to spot.

## Case study: Moment.js CVE

Moment.js versions before 2.15.2 contained a date-format regex that triggered catastrophic backtracking:

```
/D[oD]?(\[[^\[\]]*\]|\s+)+MMMM?/
```

The problematic subpattern `(\[[^\[\]]*\]|\s+)+` has two alternation branches:

- `\[[^\[\]]*\]` — a square-bracketed literal region.
- `\s+` — a whitespace run.

At the point of a whitespace run, the `\s+` inside the alternation plus the outer `+` on the whole group means each character in the whitespace run can be matched either as "one big `\s+`" or as "many small `\s+` groups." For a run of `n` whitespace characters there are `2^(n-1)` partitions.

Reported impact: a 40-character input with 31 spaces blocked the JavaScript event loop for roughly 20 seconds.

### The fix

Removing *one* `+` operator collapsed the outer repetition:

```
/D[oD]?(\[[^\[\]]*\]|\s)+MMMM?/
```

Now the whitespace alternative matches only one whitespace character at a time, and the outer `+` iterates linearly. Running time on the same pathological input dropped to roughly 135 ms.

## Mitigation recipes

The post enumerates five concrete mitigations in decreasing order of preference:

1. **Identify problematic operators.** Look for adjacent or nested `+` / `*` quantifiers that compete to match the same characters. Most real ReDoS bugs are variants of this shape.

2. **Remove redundant quantifiers.** Often a `+` can be deleted or replaced with a single character — as in the Moment.js fix.

3. **Replace open-ended quantifiers with bounded ranges.** Rather than `(\s+)+`, write `(\s+){0,10}`. The upper bound converts exponential backtracking into bounded work.

4. **Use atomic groups where available.** Ruby, PCRE, and some Perl dialects support `(?>...)`, which commits the engine to a match inside the group without allowing later backtracking. JavaScript does not support atomic groups, but the same effect can be simulated with a lookahead plus backreference:

   ```js
   // Emulated atomic group in JavaScript
   /A(?=(B|C+))\1+D/
   ```

   Testing on `"ACCCCCCCCCCCCCCCCCCCCCCCCCCCCCCX"` drops evaluation time from 1.8 seconds to 94 ms.

5. **Move to a non-backtracking regex engine.** Engines based on Thompson's NFA construction (RE2, Rust's `regex` crate, Go's `regexp`) run in linear time on every pattern but do not support backreferences or lookaround. For APEX's purposes, swapping the regex engine is the strongest mitigation whenever the target language allows it.

## Additional practical advice

- Monitor regex patterns in code review for nested quantifiers.
- Test dependencies for known ReDoS CVEs via automated tooling (Snyk, Dependabot, OWASP Dependency-Check).
- Use interactive debuggers like regex101.com to trace how the engine explores a suspicious pattern on adversarial input.
- **Prefer clarity to cleverness.** A series of simple regexes chained by procedural code is safer and often faster than one "magic" mega-regex.
- **Validate input length** before applying a regex to untrusted data. Even a known-vulnerable regex is harmless if the input is capped at 64 bytes.
- JavaScript environments face elevated risk because the single-threaded event loop means a stalled regex halts *all* processing, not just the current request.

## Relevance to APEX G-46

This explainer is the right reference to link from any developer-facing APEX report that flags a ReDoS finding. Its value is in the mitigation menu: APEX can cite this post's five-item list as the remediation guidance attached to any CWE-1333 finding, and the Moment.js worked example provides a concrete narrative for engineers who have never seen a ReDoS bug before.
