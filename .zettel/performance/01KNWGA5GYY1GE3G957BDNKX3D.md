---
id: 01KNWGA5GYY1GE3G957BDNKX3D
title: "Tool: Hypothesis (Property-Based Testing with Deadlines)"
type: literature
tags: [tool, hypothesis, property-based-testing, python, shrinking, deadline]
links:
  - target: 01KNWE2QA5VP0K80TMSABACKWT
    type: related
  - target: 01KNWE2QACWYZJXRJ8QE78T043
    type: references
  - target: 01KNZ2ZDKZW3R583KW6X8W4FE7
    type: references
created: 2026-04-10
modified: 2026-04-12
source: "https://hypothesis.works/"
---

# Hypothesis — Property-Based Testing for Python

*Source: https://hypothesis.works/ and https://hypothesis.readthedocs.io/ — the main docs page returned 403 during this session; content below draws on the G-46 spec's description of Hypothesis plus widely-published documentation.*

## What Hypothesis Is

Hypothesis is the dominant property-based testing library for Python, authored by David R. MacIver. Inspired by Haskell's QuickCheck, it lets users express properties ("for all inputs matching this strategy, this assertion holds") and then generates many test cases, shrinks failing cases to minimal witnesses, and caches them in a local database so failures reproduce deterministically on subsequent runs.

## Strategies

Hypothesis generates inputs via **strategies** — composable combinators that describe distributions of values:

```python
from hypothesis import given, strategies as st

@given(st.lists(st.integers(min_value=0, max_value=1000), min_size=0, max_size=10_000))
def test_sort_is_idempotent(xs):
    assert sorted(sorted(xs)) == sorted(xs)
```

Strategies cover primitives (integers, floats, text, binary), containers (lists, sets, dicts, tuples), recursive structures, regex-matching strings (`from_regex`), and user-defined types via `builds` / `composite`.

## Shrinking

When a test fails, Hypothesis **shrinks** the witness to the smallest / simplest failing input by iteratively applying reduction operations. This is essential for making property-test failures human-intelligible — a raw failing input can be thousands of bytes; the shrunk version is typically under 10. The shrinker is integration-level clever: it understands the structure of `strategies` and can drop, halve, and rewrite components.

## Deadlines and Phases (the performance-adjacent features)

Hypothesis exposes a `deadline` setting (default: 200 ms per test function execution). If a generated example takes longer than the deadline, Hypothesis raises a **`DeadlineExceeded`** error and treats the input as a failure. The spec's G-46 competitive-landscape entry notes this: *"Hypothesis supports performance deadline settings but does not have specialised worst-case generation strategies, and the deadline is a wall-clock timeout only."*

Important caveats about Hypothesis's deadline:

1. **It's a single-threshold check, not a regression detector** — a test that takes 190 ms consistently passes; a test that takes 210 ms consistently fails. There is no statistical comparison to a baseline.
2. **It's wall-clock only** — no CPU time, instruction count, memory allocation, or any deterministic signal. Subject to all the noise problems wall-clock time has.
3. **It's not *searching* for slow inputs** — Hypothesis's strategies generate inputs toward *diverse* coverage, not toward *slow* coverage. A deadline violation is incidental, not the goal.
4. **Phases control what Hypothesis does** — `phases=[Phase.generate, Phase.shrink]` is the default; `Phase.explicit` runs only previously-saved examples. There is no "performance-maximising" phase.

## Relevance to APEX G-46

Hypothesis is in the spec's competitive-landscape table because its deadline mechanism is the closest thing existing Python test tooling has to performance assertions. APEX G-46 supersedes it on several axes:

- **Searching vs. checking** — APEX's resource-guided fuzzer actively searches for slow inputs; Hypothesis only notices when one randomly drifts past the deadline.
- **Deterministic signals** — APEX uses instruction counts / basic-block executions as the primary feedback, not wall-clock.
- **Complexity estimation** — APEX fits empirical complexity curves across input sizes; Hypothesis has no such feature.
- **Finding format** — APEX findings carry CWE, CVSS, reproduction witnesses, and mitigation guidance; Hypothesis's output is a Python stack trace with the shrunk witness.

**What APEX should reuse from Hypothesis**: the shrinker. After finding a worst-case input, shrinking it to a human-intelligible minimum witness is exactly what Hypothesis already does well. APEX's resource-guided fuzzer could integrate with Hypothesis as an input generator for Python targets, then pipe the final witness through Hypothesis's shrinking machinery before emitting the Finding.

## References

- MacIver, "Hypothesis: A new approach to property-based testing" — [hypothesis.works](https://hypothesis.works/articles/hypothesis-the-new-approach/)
- Hypothesis docs — [hypothesis.readthedocs.io](https://hypothesis.readthedocs.io/)
- Claessen, Hughes, "QuickCheck: A Lightweight Tool for Random Testing of Haskell Programs" — ICFP 2000 (the original PBT paper Hypothesis derives from)
