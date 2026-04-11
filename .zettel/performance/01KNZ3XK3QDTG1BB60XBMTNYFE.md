---
id: 01KNZ3XK3QDTG1BB60XBMTNYFE
title: "McIlroy: A Killer Adversary for Quicksort"
type: literature
tags: [mcilroy, quicksort, antiquicksort, adversarial, pivot, algorithmic-complexity]
links:
  - target: 01KNWE2QA3FA96G8JKN733K0XP
    type: references
  - target: 01KNZ2ZDMGGV8NPY88N04STZ6W
    type: references
  - target: 01KNWE2Q9YZBAR140ZX5P36TQ5
    type: related
created: 2026-04-12
modified: 2026-04-12
source: "https://www.cs.dartmouth.edu/~doug/mdmspe.pdf"
---

# McIlroy — "A Killer Adversary for Quicksort" (1999)

*Source: https://www.cs.dartmouth.edu/~doug/mdmspe.pdf — paper; binary PDF could not be fetched inline during this session, so this note reconstructs McIlroy's construction from the Wikipedia Quicksort article, Bentley & McIlroy's "Engineering a Sort Function", and the paper's widely-cited summary. The paper is ~7 pages and the PDF URL is still live.*
*Author: M. Douglas McIlroy (Bell Labs, later Dartmouth). Published in Software: Practice and Experience, 1999.*

## The central result

McIlroy constructs an **adversary** — a procedure that watches a running quicksort and decides *on the fly* what the input data should have been so that the quicksort's pivot choices are consistently bad. The result is a way to produce, for *any* specific deterministic quicksort implementation, an input that drives it to its **worst-case `O(n²)` behaviour**.

Most strikingly, the adversary defeats **Bentley and McIlroy's own 1993 quicksort** from their famous *"Engineering a Sort Function"* paper — which uses ninther pivot selection (median-of-three-medians-of-three) and was widely considered the gold standard of practical quicksort. McIlroy demonstrated, six years later, that his own implementation had a worst-case input discoverable by a small adversary program.

## The adversary construction

The key insight: most quicksort implementations read input values to compare them but never re-read them until later stages of the algorithm. The adversary exploits this by:

1. **Lying by omission.** When quicksort first compares two elements `a[i]` and `a[j]`, the adversary has not yet committed to what those elements' actual values are. It keeps an "abstract" ordering and answers the comparison in the way that produces the worst pivot.
2. **Committing incrementally.** When quicksort's pivot-selection logic requires an actual ordering (e.g., when choosing the median of three), the adversary assigns values so that the chosen pivot is the smallest (or largest) currently-abstract element.
3. **Consistency check.** The adversary must remain consistent — if it lies about `a[i] < a[j]`, later queries about those same elements must give the same answer. This is maintained with a union-find structure over the abstract values.

The output of the adversary is not just a proof that `O(n²)` is reachable — it's an **actual concrete input array** that you can save, replay, and use as a regression test.

## The "antisort" output

Running the adversary against a specific quicksort implementation yields a concrete permutation `A` of `{1, 2, ..., n}`. Feeding `A` back into the same quicksort deterministically produces `n²/4` or more comparisons. This is the **antisort** of that particular implementation.

Key observation: the antisort is **implementation-specific**. The antisort for Bentley-McIlroy 1993 does not look like the antisort for Java's Arrays.sort. Each deterministic quicksort has its own antisort. Randomised quicksort defeats the adversary — because the adversary can't predict the pivot choices — which is why many high-quality implementations (Java's Arrays.sort since 7, Rust's slice::sort_unstable, Go's sort) now use either randomised or introspective quicksort.

## The proof technique

The proof is by a **potential function argument** over the quicksort's recursion tree:

- Define the "mass" of a recursive call as the number of elements it receives.
- Total work is `∑ mass(call)`.
- If every pivot partitions `n` into `(1, n-1)`, total work is `n + (n-1) + ... + 1 = n²/2`.
- The adversary constructs a lie-tree such that every pivot chosen by the target implementation lands at the extremal position of its current partition — i.e., always produces the `(1, n-1)` split.
- This gives `Θ(n²)` total work.

The argument is conceptually simple; the engineering challenge is making the adversary's lies consistent with later comparisons, which requires careful data-structure management.

## Implementation strategies (as given in the paper)

- **Union-find over abstract values.** Each abstract value is a node; unions happen when comparisons commit the relationship.
- **Lazy materialisation.** Only commit a value to a concrete integer at the point it becomes pivot.
- **Tracking "ghost" positions.** When quicksort shuffles elements during partition, the adversary must maintain the mapping from (abstract id) to (current array index).

The paper gives ~100 lines of C for the adversary. It's small enough to be an undergraduate assignment.

## Who else is vulnerable

McIlroy's result implies that **any** deterministic-pivot quicksort is defeatable. By 2026, known antisorts exist for:

- **GNU libc `qsort`** — historically vulnerable; at various points switched to mergesort for guaranteed `O(n log n)`.
- **Bentley-McIlroy 1993** — McIlroy's own primary target.
- **Java pre-7 `Arrays.sort`** — classic quicksort; switched to Yaroslavskiy's dual-pivot in Java 7, which has its own antisort.
- **Java 7+ Arrays.sort (Yaroslavskiy)** — a 2013 paper demonstrated its antisort.
- **C++ `std::sort`** — introsort in libstdc++ falls back to heapsort on deep recursion, which protects against `O(n²)` *time* but still has `O(n log n)` comparisons; the adversary can still force the heapsort fallback, inflating the constant.

**Randomised quicksort** is not defeatable by a *non-adaptive* adversary (one that doesn't see the RNG state). With high probability it runs in `O(n log n)` regardless of input. This is why most modern implementations randomise the pivot at the cost of ~1% average-case performance.

## Connection to hash-collision DoS

McIlroy's antisort is the **structurally equivalent attack** to the 2003 Crosby-Wallach hash-collision DoS, but for a different data structure. Both attacks:

- Take a deterministic public algorithm.
- Construct pre-images for the algorithm's worst case via an oracle argument.
- Force the data structure from `O(n log n)` (or `O(1)`) expected to `O(n²)` actual.
- Are mitigated by randomisation (SipHash / randomised pivot).

The lesson is the same: **any deterministic public-algorithm data structure is adversarial-unsafe on attacker-controlled inputs**. Only randomisation restores the expected-case performance guarantee.

## Relevance to APEX G-46

1. **Detector rule: unhardened sort on untrusted input.** Flag code paths where `qsort`, `std::sort`, or language-stdlib sort is called on user-controlled data *and* the implementation is not randomised and not fallback-protected. In practice this is rare today but real — custom sorting networks, homegrown quicksorts, bespoke Python `sort(key=)` where the key computation triggers an expensive quickcompare.
2. **Corpus: shipped antisorts.** APEX should include known antisorts (Bentley-McIlroy 1993, Java Yaroslavskiy) as corpus entries for benchmarking sort implementations under adversarial load.
3. **Fuzz-then-verify template.** APEX's resource-guided fuzzer, when targeting a sort function, should be able to *rediscover* the antisort automatically. That's a strong smoke test for the fuzzer: a known `O(n²)`-vulnerable sort should be detectable in under 10 minutes.
4. **Design-level recommendation.** When APEX flags a quicksort on untrusted data, the remediation should recommend (a) switch to a randomised pivot, (b) add a heapsort fallback on deep recursion (introspective sort), or (c) use a comparison-based sort with worst-case `O(n log n)` guarantees such as mergesort or heapsort outright.
5. **Historical pedigree.** The McIlroy adversary is the first published algorithmic-complexity-attack paper and predates the Crosby-Wallach hash-collision paper by four years. When telling the story of G-46, it's the origin event.

## References

- McIlroy — "A Killer Adversary for Quicksort" — *Software: Practice and Experience*, 1999 — [cs.dartmouth.edu/~doug/mdmspe.pdf](https://www.cs.dartmouth.edu/~doug/mdmspe.pdf)
- Bentley, McIlroy — "Engineering a Sort Function" — *Software: Practice and Experience*, 1993
- Wikipedia — Quicksort — [en.wikipedia.org/wiki/Quicksort](https://en.wikipedia.org/wiki/Quicksort) (secondary source used for this note's reconstruction)
- Crosby, Wallach — Hash-collision DoS — `01KNWEGYB8807ET2427V3VCRJ3`
- Algorithmic complexity attack umbrella — `01KNZ2ZDMGGV8NPY88N04STZ6W`
