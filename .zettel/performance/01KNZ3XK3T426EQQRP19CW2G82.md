---
id: 01KNZ3XK3T426EQQRP19CW2G82
title: "Russ Cox: Regex Matching — The Virtual Machine Approach"
type: literature
tags: [russcox, regex, nfa, vm, pike, bytecode, thompson, linear-time]
links:
  - target: 01KNYZ7YKH344XCTAFQAHQNYHG
    type: extends
  - target: 01KNZ2ZDMQETAMQ5HSCZMW4R2G
    type: related
  - target: 01KNZ2ZDMNY2HD6E56EBXDHMRE
    type: related
  - target: 01KNZ301FVY7EPHSBBT9VZKVQT
    type: related
  - target: 01KNZ2ZDMSA9FAT4B6C0SXEY33
    type: related
created: 2026-04-12
modified: 2026-04-12
source: "https://swtch.com/~rsc/regexp/regexp2.html"
---

# Russ Cox — "Regular Expression Matching: The Virtual Machine Approach"

*Source: https://swtch.com/~rsc/regexp/regexp2.html — fetched 2026-04-12.*
*Published: December 2009. The second article in Cox's four-part regex matching series. Together with the 2007 "Simple and Fast" article, it is the canonical reference for modern regex engine design.*

## What this article adds to the 2007 piece

The 2007 article (`01KNYZ7YKH344XCTAFQAHQNYHG`) explains **why** linear-time matching is possible — the Thompson NFA simulation. This 2009 article explains **how** to implement it efficiently in a real engine, introducing the **regex-bytecode virtual machine** approach:

- Compile the regex to a small bytecode.
- Execute the bytecode on a virtual machine with either backtracking (`O(2ⁿ)` worst case) or thread-parallel (`O(n·m)` guaranteed) semantics.
- The same bytecode can run both ways, so you pick your semantics without recompilation.

This is the architecture of every modern linear-time regex engine: Go `regexp`, Rust `regex`, RE2's NFA engine, Perl 6's default matcher, and (more recently) Python's PEP 594 proposed RE replacement.

## The bytecode instruction set

Four instructions suffice for the core of the language:

| Opcode | Meaning |
|---|---|
| `char c` | If the current input character is `c`, advance the input pointer and the program counter. Otherwise, this thread dies. |
| `match` | Declare a successful match. |
| `jmp L` | Unconditional jump to label `L`. |
| `split L1, L2` | Fork execution into two threads: one at `L1`, one at `L2`. Used for `|`, `*`, `+`, `?`. |

That's the whole ISA for the core matching language. Captures add one more:

| Opcode | Meaning |
|---|---|
| `save k` | Store the current input position into slot `k` of the active thread's capture array. Used to record start/end positions of capture groups. |

## Compilation

Compilation is recursive on the regex's abstract syntax tree. Each sub-regex `e` compiles into a sequence of instructions that, when executed, matches `e`. The paper shows each case:

- **Single character `c`**: `char c`.
- **Concatenation `e₁e₂`**: code for `e₁`, code for `e₂`, in sequence.
- **Alternation `e₁|e₂`**:
  ```
      split L1, L2
  L1: code for e₁
      jmp L3
  L2: code for e₂
  L3:
  ```
- **`e*` (zero or more)**:
  ```
  L1: split L2, L3
  L2: code for e
      jmp L1
  L3:
  ```
- **`e?` (zero or one)**: `split L1, L2 / L1: code for e / L2:`.
- **`e+` (one or more)**: `L1: code for e / split L1, L2 / L2:`.
- **Capture group `(e)`**: `save 2k / code for e / save 2k+1`.

A regex of size `s` compiles to `O(s)` instructions. This is the same property as the raw Thompson NFA (the bytecode *is* the NFA, in a form convenient for execution).

## Two execution engines, same bytecode

### 1. Backtracking VM

A recursive interpreter: start with program counter at 0 and input pointer at 0. At each instruction:

- `char c`: match or fail.
- `jmp L`: set PC to L.
- `split L1, L2`: recursively try `L1`; if that fails, try `L2`.
- `match`: return success.

This is essentially what Perl, Python `re`, Java `Pattern`, and JavaScript do. It's simple to implement, supports backreferences trivially (via save slots), but has the exponential pathological case that is the whole ReDoS story.

### 2. Thread-parallel VM (Pike VM)

A thread is a `(pc, save-array)` pair — nothing else. The VM maintains two lists: `currlist` (threads active at the current input position) and `nextlist` (threads to activate at the next input position). At each input character:

1. For each thread in `currlist`, execute non-consuming instructions (`jmp`, `split`, `save`) immediately, pushing resulting threads into the appropriate list.
2. For `char c` threads, if the current input character matches, move the thread to `nextlist`; else discard it.
3. When `currlist` is exhausted, advance input pointer and swap lists.
4. If any thread reaches `match`, record the match.

**Crucial de-duplication rule**: if two threads arrive at the same `pc` at the same input position, the one arriving *first* dominates. Drop the second one. This is the optimisation that bounds the total thread count to `O(m)` instead of `O(2ⁿ)` — since each of the `m` instructions can be the PC of at most one active thread per position, and the number of positions is `n`, total work is `O(m·n)`.

Subtle point: when capture groups matter, "first arrival" needs ordering: in Perl-compatible semantics ("leftmost-first"), earlier-spawned threads dominate. In POSIX semantics ("leftmost-longest"), the longer-matching thread dominates. The Pike VM handles both with a small extension to the de-duplication rule.

## Submatch extraction

The `save` instructions update the active thread's capture array with the current input position. When a thread reaches `match`, its capture array is the result. Multiple threads with different captures can coexist during matching; the winning thread's captures are reported.

Key insight: **captures don't influence future matching**. Two threads at the same PC but with different capture arrays will execute identically going forward. So for de-duplication, only the PC matters — the earlier arrival wins and its captures are used. This is why the thread-parallel VM can still bound thread count to `O(m)` even with captures, which was a common misunderstanding before this article.

## Why this matters architecturally

The article's main point is that **the same bytecode can be interpreted two ways**. An implementation can ship both engines, pick the thread-parallel VM by default for safety, and fall back to the backtracker only when the regex uses backreferences (which the thread-parallel VM can't handle in general). This is the Rust `regex` crate's architecture, and it is the reason Rust `regex` is simultaneously full-featured and linear-time safe.

Go's `regexp` takes the opposite approach — it ships *only* the thread-parallel VM and rejects any regex that would need backtracking at compile time. More restrictive, but simpler and impossible to accidentally misuse.

## Hybrid strategies

Modern engines go further than two options:

- **DFA conversion on demand** — RE2 generates a DFA from the bytecode lazily, falling back to the NFA when the DFA cache overflows. DFAs are faster than the Pike VM per character (single state lookup vs thread list management) but can blow up in memory.
- **Machine-code generation** — some engines (Hyperscan, PCRE2 JIT) compile the bytecode to native machine code for further speedup, preserving the thread-parallel semantics.
- **SIMD vectorisation** — Hyperscan uses SIMD to match multiple characters at once against the thread list.

All of these build on the VM abstraction Cox describes here.

## Relevance to APEX G-46

1. **The bytecode VM model is the right mental model for regex analysis.** APEX's static ReDoS detector needs to reason about the instruction sequence, not the surface syntax. Whether a regex will backtrack depends on the `split` instructions and their successors — and on whether the engine is a backtracker or a Pike VM. A regex that's dangerous in PCRE is safe in Go `regexp`, and vice versa for backrefs.
2. **Detector rule: regex passed to a backtracking engine on untrusted input.** Even if the static analyser can't prove the regex is safe, switching the engine to a Pike VM makes the Finding moot. APEX's remediation advice should prefer engine switching over regex rewriting when possible.
3. **Capture-group counting heuristic.** Regexes with many `save` instructions and many `split` instructions interacting are the high-risk cases for slow matching *even in* the Pike VM — the `O(m·n)` coefficient `m` can be large, and capture-heavy regexes have high `m`. APEX can surface this as a separate severity from the catastrophic-backtracking cases.
4. **Teaching tool.** The two-engines-one-bytecode framing is also the clearest way to explain to a non-specialist user *why* APEX recommends switching from Python `re` to `google-re2`. Same regex, same result, different VM, totally different worst-case behaviour.

## References

- Cox — "Regular Expression Matching: The Virtual Machine Approach" — [swtch.com/~rsc/regexp/regexp2.html](https://swtch.com/~rsc/regexp/regexp2.html)
- Cox — "Regular Expression Matching Can Be Simple And Fast" — 2007 — `01KNYZ7YKH344XCTAFQAHQNYHG`
- Thompson's construction note — `01KNZ2ZDMQETAMQ5HSCZMW4R2G`
- Pike — The rc(1) regex engine and sam(1) editor — Bell Labs / Plan 9
- RE2 — `01KNZ2ZDMNY2HD6E56EBXDHMRE`
