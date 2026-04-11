---
id: 01KNZ301FV9DXAXN39MPPAG9JV
title: "Tool: Regexploit (Doyensec NFA-Based ReDoS Detector)"
type: reference
tags: [tool, redos, static-analysis, doyensec, nfa, regex, cwe-1333]
links:
  - target: 01KNWE2QA1E37T7GVKSM6EGXXR
    type: references
  - target: 01KNWGA5F7VA68B8ZPB6XR0RTE
    type: extends
  - target: 01KNWGA5GMWKV6AKP04D964G5H
    type: related
  - target: 01KNZ301FV584G004E2SRNW97Z
    type: related
  - target: 01KNZ301FVAC85VSD6QSXHBTBN
    type: related
  - target: 01KNZ301FVRCH0P2GZS4CNGJ23
    type: related
created: 2026-04-12
modified: 2026-04-12
source: "https://github.com/doyensec/regexploit"
author: "Ben Caller / Doyensec LLC"
license: "Apache-2.0"
---

# Tool: Regexploit (Doyensec NFA-Based ReDoS Detector)

**Repository:** https://github.com/doyensec/regexploit
**Author:** Ben Caller, Doyensec LLC
**License:** Apache-2.0

## What it is

Regexploit is a static analyzer that inspects regular expressions and reports which ones are vulnerable to catastrophic or super-linear backtracking. Unlike pure pattern-matching approaches (grep for `(.*)+`), Regexploit actually parses the regex into an abstract syntax, builds an NFA-like model of its ambiguity, and reasons about whether two subexpressions can both consume the same substring. It then concretely generates a proof-of-concept attack string.

The tool was released in 2020 alongside the accompanying Doyensec blog post and has since been used to find ReDoS bugs across more than 15 major open source projects, including CPython, ua-parser, Pillow, and Pygments, producing multiple CVE assignments.

## Core detection model

Regexploit characterizes the worst-case behavior of a regex by the *polynomial degree* (or exponential) of its backtracking explosion. The metric reported to the user is a "complexity" score:

- **Exponential (infinity)** — nested quantifiers like `(a+)+` that give 2^n behavior.
- **Cubic (3)** — three overlapping repeaters that give O(n^3) behavior. Doyensec explains this threshold: "cubic complexity here means that if the vulnerable part of the string is doubled in length, the execution time should be about 8 times longer (2^3)." Regexploit treats cubic as the practical floor for exploitation — linear and quadratic patterns often do not produce user-visible stalls within the timeout.
- **Quadratic (2)** — two overlapping repeaters, O(n^2). Reported but de-prioritized.

For each flagged regex, Regexploit emits both the complexity rating and a concrete malicious input of the form `prefix + payload * N + bad_char`. Doubling `N` doubles the input size and (at cubic) eights the running time; the user can plug the result into a stopwatch harness and observe the stall.

## Detection algorithm (at a high level)

Regexploit's analyzer operates in three passes:

1. **Parse.** Convert the regex source into a structured AST. This is language-specific; Regexploit handles Python, JavaScript, TypeScript, and .NET regex dialects by borrowing the corresponding host parser (Python's `sre_parse` for Python source; a JavaScript-compatible AST for JS/TS; a .NET-syntax parser for C#).
2. **Find overlapping repeaters.** Walk the AST and look for pairs of Kleene-star / Kleene-plus operators whose languages intersect. "Intersect" here is approximated by character-class containment and prefix unification rather than full language intersection; the approximation is sound-for-vulnerability-absence on the patterns the tool targets.
3. **Check for a required tail.** A backtracking-prone pair only produces a stall if the engine eventually fails and backtracks. The tool looks for a mandatory non-matching suffix after the ambiguous region. Without this suffix, the engine will match greedily and never backtrack, and the regex is safe. Regexploit encodes this condition explicitly — and it is exactly the refinement described in the Semgrep blog post on Dlint, which was found to flag many false positives without it.

Once all three conditions are met, the tool synthesizes the attack string by:

1. Picking a character that both repeaters match (or a small alphabet satisfying both).
2. Generating `N` copies of that character.
3. Appending the first character not in the required tail's alphabet.

## Example output

For the pattern `v\w*_\w*_\w*$` Regexploit reports:

- Complexity: 3 stars (cubic).
- Malicious input: `'v' + '_' * 3456 + '!'`.

This is a classic "three overlapping `\w*` groups separated by fixed characters." Because `_` is in `\w`, each underscore can be matched either by the adjacent literal or by the surrounding `\w*`, and there are `O(n^3)` ways to partition the run. The final `!` is not in `\w` and violates the `$` anchor, forcing the engine to enumerate every partition before concluding the match fails.

## Supported input formats

- **Python source** — AST-based extraction of every `re.compile(...)` and related patterns without executing the code.
- **JavaScript / TypeScript** — ESLint parser integration.
- **C# / .NET** — direct parser support.
- **JSON / YAML** — configuration file scans (useful for uap-core-style regex databases).
- **Installed Python modules** — dynamic analysis via `import` for patterns constructed at runtime.

## Notable discoveries

Doyensec reports that Regexploit has found ReDoS bugs in 15+ major projects including CPython (standard library regex vulnerabilities), ua-parser (echoing CVE-2020-5243), Pillow (image metadata parsing), and Pygments (syntax highlighter lexer rules). The tool has been used as an auditor for large codebases during penetration tests and for scheduled scans of open-source dependencies.

## Relevance to APEX G-46

Regexploit is the right first-line static ReDoS detector for APEX G-46 pipelines targeting Python, JavaScript, and .NET codebases. It complements the other tools in this vault:

- **vs. Davis et al.'s `vuln-regex-detector`** (already in the vault): Davis's tool is an ensemble that runs multiple detectors (rxxr2, safe-regex, weideman) and votes. Regexploit is a single-detector tool with tighter false positive control and is simpler to integrate in CI.
- **vs. Semgrep's metavariable-analysis `redos` mode** (already in the vault indirectly via the Dlint/r2c blog): Semgrep is pattern-based and catches fewer but produces fewer false positives after the `required-tail` refinement. Regexploit's three-condition logic is the more conservative filter.
- **vs. the dynamic route (SlowFuzz, PerfFuzz, Singularity)**: Regexploit is static and therefore fast — it can scan an entire dependency tree in seconds — but it can only find patterns of the shape it is designed to detect. The dynamic tools find surprises the static tool misses.

A sensible G-46 pipeline uses Regexploit (or one of its peers) as a cheap first pass, then dynamic pattern fuzzing for the regexes that survived static screening. The concrete PoC strings Regexploit emits are also valuable corpus seeds for the dynamic phase.
