---
id: 01KNZ2ZDMQETAMQ5HSCZMW4R2G
title: "Wikipedia: Thompson's Construction (regex → NFA)"
type: literature
tags: [wikipedia, thompson, nfa, regex, automata, algorithm]
links:
  - target: 01KNYZ7YKH344XCTAFQAHQNYHG
    type: references
  - target: 01KNZ2ZDMNY2HD6E56EBXDHMRE
    type: related
  - target: 01KNWE2QA1E37T7GVKSM6EGXXR
    type: related
created: 2026-04-12
modified: 2026-04-12
source: "https://en.wikipedia.org/wiki/Thompson%27s_construction"
---

# Thompson's Construction — Wikipedia

*Source: https://en.wikipedia.org/wiki/Thompson%27s_construction — fetched 2026-04-12.*

## What it is

**Thompson's construction** (also: McNaughton-Yamada-Thompson algorithm) is an algorithm for converting a regular expression into an equivalent non-deterministic finite automaton (NFA). It is the foundational algorithm behind every modern linear-time regex engine — Thompson's 1968 QED editor, the original Unix `grep`, Go's `regexp`, Rust's `regex`, RE2, and Hyperscan all ultimately descend from it.

Named after **Ken Thompson**, who published the construction in the 1968 CACM paper *"Programming Techniques: Regular expression search algorithm"*. An essentially equivalent algorithm was published earlier by **McNaughton and Yamada** (1960); historians give partial credit to both.

## Why it matters

The construction gives two things simultaneously:
1. A mechanical way to build an NFA from *any* regex.
2. An NFA whose size is **linear** in the regex (`O(s)` states for a regex of length `s`), which is the essential property that makes linear-time matching possible — a larger NFA would defeat the whole purpose.

The resulting NFA has distinctive properties:
- Exactly one initial state, with no incoming transitions.
- Exactly one final (accepting) state, with no outgoing transitions.
- At most two transitions leaving any state.
- `2s - c` total states, where `s` is the number of symbols in the regex and `c` is the number of concatenations.

## The five recursive rules

The algorithm recursively breaks a regex into sub-expressions and composes their NFAs. Each rule assumes you already have NFAs for the sub-expressions and wires them up.

### 1. Empty expression (ε)

```
 ┌──ε──>○
(start)      (accept)
```

Two states, one ε-transition between them. The empty language `{ε}`.

### 2. Single character `a`

```
 ┌──a──>○
(start)      (accept)
```

Two states, one `a`-labelled transition. The language `{a}`.

### 3. Concatenation `s·t`

Build NFAs `N(s)` and `N(t)`. Merge the accept state of `N(s)` with the start state of `N(t)`.

```
(s_start)──[N(s)]──>(s_accept = t_start)──[N(t)]──>(t_accept)
```

Total states: `|N(s)| + |N(t)| - 1`.

### 4. Alternation `s | t`

Build `N(s)` and `N(t)`. Add a new start state with ε-transitions to both sub-start states, and a new accept state with ε-transitions from both sub-accept states.

```
           ε                         ε
 (new)────────>(s_start)──N(s)──>(s_accept)────────>(new)
 (start) ε                         ε          (accept)
         └────>(t_start)──N(t)──>(t_accept)──┘
```

Total states: `|N(s)| + |N(t)| + 2`.

### 5. Kleene star `s*`

Build `N(s)`. Add a new start and a new accept. Four ε-transitions:
1. New start → old start (enter the loop)
2. Old accept → new accept (exit the loop)
3. New start → new accept (zero iterations — match empty)
4. Old accept → old start (repeat)

```
                  ε
            ┌──────────────────┐
 (new)──ε──>(s_start)──N(s)──>(s_accept)──ε──>(new)
 (start) ε                                     (accept)
      └──────────────────────────────────────>┘
```

Total states: `|N(s)| + 2`.

## Why the NFA is O(s) in the regex size

Each recursive rule adds a bounded number of new states (0, 0, -1, 2, 2). A regex of length `s` uses at most `s` rule applications, so the total number of states is linear. This is the property that distinguishes Thompson's construction from naive "build a DFA directly" approaches, which can blow up exponentially (`2^s`) in pathological cases.

## Matching with a Thompson NFA

Given a Thompson NFA with `m` states and `≤2m` transitions, matching a string of length `n`:

- **Parallel simulation** — at each input character, compute the set of NFA states reachable via ε-transitions from any currently-active state, then the set reachable via the character transition. At most `m` states are active at any point.
- Per-step cost: `O(m)` to compute the new active set.
- Total cost: `O(m·n)`.

For a regex of length `s`, this is `O(s·n)`. *Linear in input length.* Regardless of the regex, regardless of the input, there is no backtracking and no exponential blowup.

## Contrast with backtracking

Perl/Python/Java (pre-Java 9)/JavaScript all use **backtracking** implementations instead. These generate a parse tree of the regex and match by recursive descent with full backtracking on failure. The backtracker is simpler, supports backreferences and lookaheads, and is *usually* fast. But on pathological inputs it goes exponential — `O(2^n)` on regex-adversarial inputs, which is the entire ReDoS story.

See `01KNYZ7YKH344XCTAFQAHQNYHG` (Russ Cox's article) for the million-times-faster benchmark showing the practical consequence.

## Historical note

Thompson used this construction in his 1968 QED editor, which was the first interactive Unix text editor. The actual implementation in that editor was **self-modifying IBM 7094 machine code** — the regex was compiled into a sequence of JMP instructions representing the NFA, and matching was done by literally jumping through the generated code. This was fast on its contemporary hardware but hostile to modern CPUs (self-modifying code destroys instruction cache). Modern implementations (RE2, Go `regexp`, Rust `regex`) use an interpreter on a compact bytecode instead; see Cox's "Virtual Machine Approach" article for the design.

## Relevance to APEX G-46

1. **Thompson's construction is the algorithmic foundation** of every "safe" regex APEX ships with. When APEX recommends switching from PCRE to RE2/Go regexp/Rust regex, this is the algorithm that makes the recommendation meaningful.
2. **Detector logic.** APEX's static ReDoS checker can convert a regex to a Thompson NFA, then test whether it has states with "ambiguous ε-closure" — a condition that implies potential for backtracking-era explosion in PCRE. This is what the Weideman / Rathnayake detectors do under the hood.
3. **Complexity analysis for arbitrary regexes.** Even without running the regex, the NFA's structure tells you the worst case. If all Kleene stars are on strictly disjoint characters, the regex is safe. If two stars share characters (`(a|a)*`, `(a+)*`), the NFA has ambiguous ε-paths and PCRE-style engines will backtrack.
4. **This note is the canonical "why linear-time regex is possible" pointer.** When explaining G-46's ReDoS detector to users, the pipeline to reference is: Thompson 1968 → Cox 2007 article → RE2 2006 → Rust regex crate 2014 → your codebase 2026.

## References

- Wikipedia — [en.wikipedia.org/wiki/Thompson's_construction](https://en.wikipedia.org/wiki/Thompson%27s_construction)
- Thompson — "Programming Techniques: Regular expression search algorithm" — CACM 1968 — DOI 10.1145/363347.363387
- McNaughton, Yamada — "Regular Expressions and State Graphs for Automata" — IRE Transactions 1960
- Russ Cox — Regex article series — `01KNYZ7YKH344XCTAFQAHQNYHG`, [swtch.com/~rsc/regexp/](https://swtch.com/~rsc/regexp/)
