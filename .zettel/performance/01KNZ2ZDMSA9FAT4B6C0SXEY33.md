---
id: 01KNZ2ZDMSA9FAT4B6C0SXEY33
title: "Russ Cox: Glob Matching Can Be Simple And Fast"
type: literature
tags: [russcox, glob, backtracking, linear-time, shell, ftp-cves]
links:
  - target: 01KNYZ7YKH344XCTAFQAHQNYHG
    type: extends
  - target: 01KNZ2ZDMNY2HD6E56EBXDHMRE
    type: related
  - target: 01KNZ2ZDMQETAMQ5HSCZMW4R2G
    type: related
  - target: 01KNZ301FVY7EPHSBBT9VZKVQT
    type: related
  - target: 01KNZ3XK3T426EQQRP19CW2G82
    type: related
created: 2026-04-12
modified: 2026-04-12
source: "https://research.swtch.com/glob"
---

# Russ Cox — "Glob Matching Can Be Simple And Fast"

*Source: https://research.swtch.com/glob — fetched 2026-04-12.*
*Posted: March 2017. Follow-up to the 2007 "Regular Expression Matching Can Be Simple And Fast" article.*

## The one-sentence thesis

Almost every shell, FTP server, and filesystem tool you've used implements `*` / glob pattern matching with a backtracking algorithm that goes **exponential** on adversarial input — and this has caused real CVEs — even though a single-pass greedy algorithm solves the same problem in linear time.

## The demonstration

Cox benchmarks glob matching on the pattern `(a*)ⁿb` (n copies of `a*` followed by `b`) against an input of 100 `a` characters. The expected behaviour: no match, because there's no `b` in the input. The actual behaviour in many implementations is astonishing:

> "For n = 8, `ls` takes 7.19 minutes while `ls | grep` runs in 1.56 milliseconds, making it 276,538X faster."

That is not a typo. Bash's glob expansion takes over seven minutes on 100 characters of input because it backtracks on every star position. `grep` with the equivalent regex finishes in under two milliseconds because Go-like regex engines (and `grep` specifically when GNU's DFA engine kicks in) don't backtrack.

The culprit: when bash encounters the n-th `*`, it tries every possible length for the star to consume, then recurses. Total work: `O(nᵉ)` where `e` is the number of stars.

## Linear-time alternative #1: greedy single-pass

The core observation: **greedy matching without backtracking suffices for glob patterns because glob has no alternation or character classes that force a choice**. When you see `*`, match it greedily against the input, record "this position is my last fallback", and continue. If the rest of the pattern fails, pop back to the fallback and try once more starting one character later.

Cox gives a concise Go implementation (~30 lines):

```go
func Match(pattern, name string) (matched bool) {
    px, nx := 0, 0
    var nextPx, nextNx int
    for px < len(pattern) || nx < len(name) {
        if px < len(pattern) {
            c := pattern[px]
            switch c {
            default:  // ordinary char
                if nx < len(name) && name[nx] == c {
                    px++; nx++; continue
                }
            case '?':
                if nx < len(name) {
                    px++; nx++; continue
                }
            case '*':
                nextPx = px; nextNx = nx + 1
                px++; continue  // zero-length match attempt
            }
        }
        // backtrack to the most recent *
        if 0 < nextNx && nextNx <= len(name) {
            px = nextPx; nx = nextNx; continue
        }
        return false
    }
    return true
}
```

Crucially, **there is only one backtrack point at any time** — the most recent `*`. The algorithm never revisits an earlier `*`; earlier ones are locked in. This bounds the total work to `O(|pattern| × |name|)`.

## Linear-time alternative #2: translate to regex

Convert the glob pattern to an equivalent regex and hand it to a linear-time regex engine. `*` becomes `.*`, `?` becomes `.`, `[abc]` becomes `[abc]`. Any RE2-family engine then matches in `O(n)`. Plan 9's `ftpd` used this approach; it's a one-line adapter on top of an existing regex library.

## The CVE lineage

The exponential-glob bug is not theoretical — it has produced a series of real CVEs:

- **CVE-2001-1501** — proftpd glob DoS.
- **CVE-2010-2632** — wu-ftpd glob DoS.
- **CVE-2015-5917** — WebDAV server glob DoS.
- **CVE-2018-???** — various filesystem tools using `fnmatch(3)` with backtracking.

Each is the same underlying bug: an FTP/HTTP/WebDAV server accepts a user-provided glob pattern (for wildcards in `LIST` or similar) and passes it to the libc `fnmatch(3)` function. `fnmatch` is implemented with backtracking in glibc, BSD libc, and musl. An attacker sends `(*)ⁿb` and the server burns CPU for minutes.

## Adoption of linear-time glob

As of the article's 2017 writing:

- **Go `filepath.Match`** — linear time (uses Cox's own algorithm).
- **glibc `glob(3)`** — linear time (greedy; was fixed after early CVEs).
- **glibc `fnmatch(3)`** — STILL backtracking. Still exploitable.
- **OpenSSH `match.c`** — linear time.
- **Bash `[[ "$s" == pat ]]`** — still backtracking (bash uses `fnmatch`).
- **Python `fnmatch`** — translates to regex and uses `re`, so inherits `re`'s backtracking (Python `re` is a backtracker). Exponential.
- **POSIX fnmatch spec** — does not require linear time.

So in 2026, eight years after the article was published, bash and Python `fnmatch` are still exponential on adversarial patterns. The fix is known and trivial; the deployment lag is the usual inertia of low-priority infrastructure.

## The broader lesson

Cox's point in both his 2007 regex article and this 2017 glob article is the same: **for the constrained subset of languages that regex and glob represent, linear-time algorithms have been known since 1968 and are no harder to implement than the backtracking versions**. The continued use of backtracking is not a performance / correctness tradeoff — it is a historical accident that has metastasised into security debt.

For glob specifically, there is literally no language feature that requires backtracking. Every benefit of a backtracker is available without it. Yet bash, most shells, and most libcs still use the backtracker because it's what was there.

## Relevance to APEX G-46

1. **Detector rule: calls to `fnmatch(3)` with user-controlled patterns.** High-severity Finding, CWE-1333-adjacent (arguably CWE-407). The fix is to use a linear-time alternative or cap the pattern length.
2. **Detector rule: `glob.glob()` in Python with untrusted pattern.** Same story.
3. **Detector rule: `shopt -s extglob` with user-controlled patterns in shell scripts.** Extglob extends bash's already-exponential matcher with more features; still backtracking.
4. **Corpus.** APEX's performance fuzzer corpus should include the `(a*)ⁿb` family for glob-adjacent functions. Any pattern matcher that takes more than milliseconds on `n=10` has the bug.
5. **Cross-language test generation.** Cox's 30-line Go algorithm is a drop-in replacement for every backtracker. When APEX flags a glob-related bug, the remediation can include the linear-time algorithm as a copy-pasteable fix.
6. **The article generalises the regex story to glob.** Any note citing Cox's 2007 regex article should also point at this one to cover the glob subset.

## References

- Russ Cox — "Glob Matching Can Be Simple And Fast" — [research.swtch.com/glob](https://research.swtch.com/glob)
- Russ Cox — "Regular Expression Matching Can Be Simple And Fast" — 2007 — `01KNYZ7YKH344XCTAFQAHQNYHG`
- Russ Cox — "Regular Expression Matching: The Virtual Machine Approach" — 2009
- CVE-2010-2632 — wu-ftpd — [nvd.nist.gov/vuln/detail/CVE-2010-2632](https://nvd.nist.gov/vuln/detail/CVE-2010-2632)
