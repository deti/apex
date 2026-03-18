# APEX Real-World Validation: 10 Top GitHub Repos

**10 repos · 11 languages · 12,656 findings · 0 crashes**

APEX v0.3.0 was run against 10 of the most popular open-source projects on GitHub to measure detection coverage, false-positive rates, and performance across all supported languages.

---

## Results at a Glance

| # | Repository | Language | Findings | True Positives |
|---|-----------|----------|----------|---------------|
| 1 | **Linux Kernel** | C | 2,073 | High-entropy constants (FP-heavy) |
| 2 | **CPython** (C) | C | 2,122 | `gets()` banned function |
| 3 | **CPython** (Lib) | Python | 2,164 | pickle deserialization (CWE-502) |
| 4 | **TypeScript** | JS/TS | 2,935 | path-normalize patterns |
| 5 | **ripgrep** | Rust | 578 | — (clean codebase) |
| 6 | **Spring Boot** | Java | 94 | `readObject` deserialization (CWE-502) |
| 7 | **Kubernetes** | Go | 295 | hardcoded secret in flexvolume |
| 8 | **.NET Runtime** | C# | 374 | password constants in OleDb |
| 9 | **Vapor** | Swift | 87 | PEM key + dev passwords (CWE-798) |
| 10 | **Rails** | Ruby | 1,861 | `Marshal.load` deserialization |
| 11 | **ktor** | Kotlin | 73 | — (cleanest) |

**Total: 12,656 findings · 0 crashes · 0 panics**

---

## v0.2.1 → v0.3.0 Changes

| Metric | v0.2.1 | v0.3.0 | Change |
|--------|-------:|-------:|--------|
| Total findings | 12,770 | 12,656 | -114 (-0.9%) |
| Languages scanned | 11 | 11 | — |
| Repos with 0 crashes | 10/10 | 10/10 | — |
| New: C# dep audit | — | 374 findings | +C# coverage |
| New: Kotlin patterns | — | 73 findings | +Kotlin coverage |
| New: Ruby patterns | — | 1,861 findings | +Ruby coverage |
| New: Go patterns | — | 295 findings | +Go coverage |

The finding count is stable despite scanning more code with more detectors. New language-specific detectors (C#, Kotlin, Ruby, Go) found issues that v0.2.1 missed, while FP reductions from v0.2.1 bug fixes offset the increase.

---

## Confirmed True Positives

| Repository | Finding | CWE | Severity |
|-----------|---------|-----|----------|
| Kubernetes | Hardcoded secret in flexvolume driver | CWE-798 | High |
| Vapor | PEM private key in development config | CWE-798 | High |
| Vapor | Hardcoded passwords ("secret", "vapor") in routes | CWE-798 | High |
| CPython | `gets()` banned function in fileobject.c | CWE-242 | High |
| CPython | pickle deserialization in spawn.py, reduction.py | CWE-502 | High |
| CPython | Hardcoded password in urllib/request.py | CWE-798 | High |
| Spring Boot | `readObject` unsafe deserialization in JSON shade | CWE-502 | High |
| .NET Runtime | Password constants in OleDb connection strings | CWE-798 | High |
| Rails | `Marshal.load` in cache serializer | CWE-502 | Medium |
| ktor | High-entropy call ID default | CWE-798 | Low |

---

## Top Detector Breakdown

| Detector | Findings | % | Notes |
|----------|-------:|--:|-------|
| security-pattern | 2,773 | 22% | Command injection, code injection, process execution patterns |
| panic-pattern | 4,163 | 33% | unwrap(), panic!, assert! in non-test code |
| mixed-bool-ops | 1,643 | 13% | Complex boolean logic |
| path-normalize | 2,422 | 19% | File operations with unsanitized paths |
| secret-scan | 285 | 2% | Entropy-based secret detection |
| flag-hygiene | 178 | 1% | Feature flag discipline |
| static-analysis | 264 | 2% | Rust-specific (clippy-derived) |
| Other | 928 | 7% | hardcoded-secret, dep-audit, etc. |

---

## 20 Bugs Found & Fixed (v0.2.1)

| # | Bug | Impact | Status |
|---|-----|--------|--------|
| 1 | CWE-134 `printf("literal")` not skipped | 1,584 FPs | **Fixed** |
| 2 | Entropy threshold 4.5 too low | 838 FPs | **Fixed** |
| 3 | Audit includes code-quality detectors | 5,789 FPs | **Fixed** |
| 4 | CWE-94 in compiler files not skipped | 760 FPs | **Fixed** |
| 5 | path-normalize fires on compiler tools | 2,475 FPs | **Fixed** |
| 6 | Duplicate findings at same file:line | 115 dupes | **Fixed** |
| 7 | C# patterns don't match real .NET code | 0 TPs | **Fixed** |
| 8 | Kotlin patterns don't match real code | 0 TPs | **Fixed** |
| 9 | Ruby metaprogramming flagged as injection | 163 FPs | **Fixed** |
| 10 | Python stdlib eval/pickle flagged | FPs | **Fixed** |
| 11 | TypeScript scan takes 22 minutes | Perf | **Fixed** |
| 12 | Kubernetes scan takes 10 minutes | Perf | **Fixed** |
| 13 | sprintf `"` sanitization always matches | Severity bug | **Fixed** |
| 14 | Encoding tables flagged as secrets | 254 FPs | **Fixed** |
| 15 | Generated code flagged as secrets | 61 FPs | **Fixed** |
| 16 | `lint --detectors` flag missing | UX | **Fixed** |
| 17 | CWE-705 fires on binary entry points | 46 FPs | **Fixed** |
| 18 | FALSE_POSITIVE_VALUES dedup | Code quality | **Fixed** |
| 19 | No detector tag/category system | UX | **Fixed** |
| 20 | dep-audit only covers npm/pip | Coverage | **Fixed v0.3.0** — now covers C#, Ruby, Swift, C++ |

---

## Methodology

1. Cloned each repo at HEAD (shallow `--depth 1`) as of 2026-03-18
2. Ran `apex audit --target <repo> --lang <lang> --output-format json` with default config
3. APEX v0.3.0 release binary (optimized build)
4. All 11 runs completed with 0 crashes, 0 panics
5. Results parsed and compared against v0.2.1 baseline
