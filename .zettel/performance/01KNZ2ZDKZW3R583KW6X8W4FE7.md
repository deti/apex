---
id: 01KNZ2ZDKZW3R583KW6X8W4FE7
title: "Hypothesis: What Is It (hypothesis.works)"
type: literature
tags: [hypothesis, property-based-testing, quickcheck, shrinking, python]
links:
  - target: 01KNWGA5GYY1GE3G957BDNKX3D
    type: extends
  - target: 01KNWE2QA8H1GKHCVNHYS5QW1F
    type: references
created: 2026-04-12
modified: 2026-04-12
source: "https://hypothesis.works/articles/what-is-hypothesis/"
---

# Hypothesis — "What Is Hypothesis" article

*Source: https://hypothesis.works/articles/what-is-hypothesis/ — fetched 2026-04-12. Written by David R. MacIver, creator of the library.*

## One-line summary

Hypothesis is a Python library for **property-based testing**: you describe the shape of the data a function should accept, declare the invariants that should hold for all such data, and Hypothesis searches for counter-examples that break the invariants — then automatically shrinks them to the smallest reproducer.

## Core concepts

### Strategies

The fundamental primitive in Hypothesis is a **strategy**: a generator description specifying how to draw random values of some type. Strategies compose:

```python
from hypothesis import strategies as st
sorted_lists = st.lists(st.integers(), min_size=1).map(sorted)
```

The standard library ships strategies for primitives (`integers`, `floats`, `text`, `binary`, `booleans`), containers (`lists`, `tuples`, `dictionaries`, `sets`, `frozensets`), and combinators (`one_of`, `just`, `builds`, `composite`). Crucially strategies are **not** just type-driven random generators; they are annotated with metadata that the shrinker uses.

### The `@given` decorator

```python
from hypothesis import given
from hypothesis import strategies as st

@given(st.lists(st.integers()))
def test_reverse_twice_is_identity(xs):
    assert list(reversed(list(reversed(xs)))) == xs
```

`@given` integrates with `pytest`, `unittest`, and `nose`, so Hypothesis never takes over test discovery — it augments what you already have.

### Repeatable randomness

When Hypothesis finds a failing example, it saves it to a local database (`.hypothesis/` directory). On the next run, the saved failures are replayed *first*, before any new random exploration. This turns random testing into **repeatable random testing** — a regression that Hypothesis caught once will keep being caught on every run, even without the bug being fixed.

## Shrinking — the killer feature

When a property fails on `[17, -3, 42, 0, -999, 1000000]`, Hypothesis doesn't show you that list. It tries to reduce it: `[0]`, `[0, 0]`, `[17]`, `[]`, `[-999]`, `[-1]`, etc., keeping the smallest one that still triggers the failure. What you finally see is something like `[0]` or `[-1, 0]` — the minimal input that still breaks the invariant.

The shrinking algorithm is **integrated into the generator** rather than running as a post-processing step. Each strategy not only produces values but also knows how to simplify them. This lets shrinking exploit structural knowledge: a failing `lists(text())` shrinks toward shorter lists of shorter strings with lower Unicode codepoints, all in a single search.

MacIver's key insight versus classical QuickCheck: QuickCheck shrinks by re-running the generator with a smaller size parameter, which can miss the failure; Hypothesis shrinks by manipulating the **internal representation** (a sequence of draws from the entropy pool), so every shrink candidate is still a valid input from the same strategy.

## Comparison to QuickCheck

| Feature | Haskell QuickCheck | Hypothesis |
|---|---|---|
| Generators | Typeclass per type | First-class strategy objects |
| Shrinker | Separate function per type | Integrated into the generator |
| Database | No | Yes (saves failures) |
| Targeted search | Recent additions | `target()` for numeric properties |
| Deadline / timeout | No | `@settings(deadline=100)` — stops slow executions |
| Stateful testing | Separate library | Built-in rule-based state machine |

The biggest practical difference is the **database of failing examples** — Hypothesis is designed around the assumption that you will run tests over and over, and that previous failures are the best starting point for finding new ones.

## Relevance to APEX G-46

1. **`deadline` as a soft SLO.** Hypothesis's `@settings(deadline=100)` flags any example that takes more than 100 ms as a test failure. This is the minimum viable **in-process performance assertion**, and it's the pattern APEX's SLO mode should match: run the generator, capture time-per-input, fail when the declared budget is exceeded.

2. **Targeted search as a foundation for performance fuzzing.** `hypothesis.target(time_taken, label="execution time")` tells Hypothesis to prefer examples that make a given number *larger*. This is the same idea as SlowFuzz's "maximise resource consumption" feedback — Hypothesis actually ships a usable version of it today. APEX can bootstrap its performance-fuzz mode by generating a Hypothesis strategy for the target function and wrapping the body in a `target(elapsed)` call.

3. **Integrated shrinking is the right model for perf findings.** When APEX finds a 5-second worst-case input, the user wants the **minimal** worst-case input, not the raw fuzzer output. Hypothesis's shrinker template is the reference: manipulate the draw sequence rather than the generated object.

4. **The database is the missing piece.** APEX should persist worst-case inputs across runs. A regression where the slowdown vanishes but re-appears two commits later is exactly the case Hypothesis's database solves.

## References

- MacIver — "Hypothesis: a new approach to property-based testing" — [hypothesis.works](https://hypothesis.works/articles/what-is-hypothesis/)
- Hypothesis docs — [hypothesis.readthedocs.io](https://hypothesis.readthedocs.io/)
- Claessen, Hughes — "QuickCheck: a lightweight tool for random testing of Haskell programs" — ICFP 2000
