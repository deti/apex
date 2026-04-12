---
id: 01KNZ301FVY7EPHSBBT9VZKVQT
title: "Russ Cox: Using Uninitialized Memory for Fun and Profit (Sparse Sets)"
type: reference
tags: [reference, data-structure, sparse-set, algorithms, performance, compilers, russ-cox]
links:
  - target: 01KNWE2Q9YZBAR140ZX5P36TQ5
    type: references
  - target: 01KNWE2QABV7943DKAXTARJHXA
    type: related
  - target: 01KNWE2QA3FA96G8JKN733K0XP
    type: related
  - target: 01KNYZ7YKH344XCTAFQAHQNYHG
    type: related
  - target: 01KNZ3XK3T426EQQRP19CW2G82
    type: related
  - target: 01KNZ2ZDMSA9FAT4B6C0SXEY33
    type: related
created: 2026-04-12
modified: 2026-04-12
source: "https://research.swtch.com/sparse"
author: "Russ Cox"
---

# Russ Cox: Using Uninitialized Memory for Fun and Profit (Sparse Sets)

**URL:** https://research.swtch.com/sparse
**Author:** Russ Cox
**Title:** "Using Uninitialized Memory for Fun and Profit"

## Why this is in a performance-test vault

Sparse sets are a textbook example of a data structure whose *worst-case* and *average-case* performance profiles differ meaningfully from the "obvious" alternative (a bit vector), and understanding those differences is exactly the reasoning pattern APEX G-46 needs to reproduce mechanically. In particular, the sparse set is one of the few widely-deployed data structures that achieves `O(1)` clear, which matters enormously in workloads where a small set is repeatedly populated and cleared (work queues, compiler register allocation, graph traversal mark buffers).

Cox's article is also the canonical reference developers cite when they need to explain *why* this trick is safe despite reading from apparently-uninitialized memory.

## The data structure

A sparse set that holds a subset of `{0, 1, ..., N-1}` consists of two arrays of size `N`:

- `dense[0..n-1]` stores the set elements in arbitrary insertion order, where `n` is the current size.
- `sparse[i]` stores an index into `dense`, specifically the position where element `i` was inserted (if it is a member).

The only invariant is: element `i` is in the set if and only if both:

1. `sparse[i] < n`, and
2. `dense[sparse[i]] == i`.

That double-check is the clever part. The second condition validates the first: even if `sparse[i]` contains arbitrary garbage left over from previous use, the set check still gives the right answer, because the garbage value either (a) points outside the valid range of `dense`, or (b) points inside `dense` to a *different* element, both of which fail the `dense[sparse[i]] == i` test.

### Operations

| Operation | Implementation | Cost |
|---|---|---|
| `contains(i)` | `sparse[i] < n && dense[sparse[i]] == i` | `O(1)` |
| `add(i)` | if not contained: `dense[n] = i; sparse[i] = n; n += 1` | `O(1)` |
| `remove(i)` | swap `dense[sparse[i]]` with `dense[n-1]`, patch `sparse[...]`, `n -= 1` | `O(1)` |
| `clear()` | `n = 0` | `O(1)` |
| iterate in insertion order | walk `dense[0..n-1]` | `O(n)` |

## The performance contrast with bit vectors

For a set over a universe of size `N`, the two obvious representations are:

| Operation | Bit vector (N bits) | Sparse set (2N indices) |
|---|---|---|
| `contains` | `O(1)` | `O(1)` |
| `add` | `O(1)` | `O(1)` |
| `remove` | `O(1)` | `O(1)` |
| `clear` | `O(N/word_size)` | `O(1)` |
| iterate members | `O(N)` | `O(n)` |

The bit vector wins in two ways (smaller memory, and iteration over members happens to scan a tight loop that vectorizes well). The sparse set wins in two other ways (`O(1)` clear and `O(n)` iteration over the *current* members even when `n << N`).

The `clear` advantage is a *worst-case* guarantee that matters in any workload where the set is cleared frequently and is usually smaller than the universe. Register allocation inner loops clear their "live" set once per basic block and the basic block has `O(1)` variables on average — the bit-vector clear cost is `O(N_registers / 64)` per block, the sparse-set clear cost is `O(1)`, and over millions of basic blocks this compounds into a meaningful performance difference.

## Where it actually matters

The article's canonical application is **compiler register allocation**. Real compilers (SSA graph colorers in LLVM, GCC's local allocator) use sparse sets exactly because they need to repeatedly mark a small subset of the register universe, check membership, iterate members in insertion order, and clear the set — all in inner loops. Bit vectors work but lose to sparse sets on real workloads because of the clear cost.

Other common uses:

- **Work queues for graph traversal.** Mark a small set of vertices as "in the queue" within a large graph. Adding, removing, and iterating are all `O(1)` per element.
- **Set-cover algorithms and SAT solvers.** Preprocessing phases that need to repeatedly mark and unmark candidates.
- **Interned-string and symbol-table data structures** where frequent insertion/deletion is required alongside fast iteration.

## Trade-offs

- **Memory cost is `2 * N * sizeof(index)`** — roughly 8× a bit vector for 32-bit indices. This is acceptable when `N` is small (a few thousand) but prohibitive at billion-scale universes.
- **Iteration order is insertion order, not sorted order.** For algorithms that need sorted iteration the sparse set needs to be sorted after the fact, losing the `O(n)` guarantee.
- **Reliance on uninitialized memory.** Valgrind and MemorySanitizer flag the read of `sparse[i]` as a use of uninitialized memory unless the set is marked with special annotations. On security-sensitive builds this can be a source of false positives.

## Relevance to APEX G-46

Three takeaways for APEX's performance-test generation:

1. **Data-structure choice drives worst-case performance.** Many real-world "slow input" bugs are not algorithmic vulnerabilities per se — they are the right algorithm running on the wrong data structure for the workload. An APEX G-46 report that explains "this workload spends 40% of its time in `bit_vector::clear`" is directly actionable only if the report author understands that replacing the bit vector with a sparse set is a thing you can do. The reverse is also true: APEX should be able to warn when a hot path uses a sparse set for a workload that rarely clears, because in that case the sparse set is worse.

2. **Clear-cost is a real, measurable, worst-case quantity.** Unlike wall-clock time (noisy), memory allocation (bursty), or instructions retired (microarchitecturally dependent), the "cost of clear" is a deterministic function of the universe size and data structure choice. It is trivially model-checkable: a static analyzer can recognize the `memset` in a `clear` routine and associate it with its cost model.

3. **Amortized vs. worst-case distinctions matter.** Many "amortized O(1)" claims in the standard library (e.g. `std::vector::push_back`) have worst-case `O(n)` behavior on a specific input shape. A G-46 worst-case finder should know to target those *amortized* data structures with adversarial sequences (e.g., hash-map resize cascades, vector reallocation chains — the classical Schlemiel-the-Painter example already in the vault as `01KNWE2QABV7943DKAXTARJHXA`). The sparse set's `O(1)` clear is a *deamortized* bound, which makes it interesting by contrast: there is no adversarial sequence that degrades it.

## History

The trick appears in Preston Briggs and Linda Torczon's 1993 "An Efficient Representation for Sparse Sets" (ACM Letters on Programming Languages and Systems). Cox's article makes the technique accessible to a general audience and ties it to the broader question of when "uninitialized memory is actually fine." Regehr's blog and various LLVM internals documents cite both.
