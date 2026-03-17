# Real-World Validation — Triage

## All Bugs Found

| # | Severity | Category | File | Bug |
|---|----------|----------|------|-----|
| 1 | High | False Positive | `security_pattern.rs` | CWE-134 `printf("literal")` flagged — no literal-format-string check (1,584 FPs in Linux/CPython) |
| 2 | High | False Positive | `secret_scan.rs` | Entropy threshold 4.5 too low — flags code identifiers as secrets (838 FPs across all repos) |
| 3 | High | Wrong Scope | `config.rs` | `audit` includes code-quality detectors (`panic`, `mixed-bool-ops`) — 5,789 non-security findings in security report |
| 4 | High | False Positive | `security_pattern.rs` | CWE-94 `new Function()` in compilers — no context for legitimate code generation (760 FPs in TypeScript) |
| 5 | Medium | False Positive | `path_normalize.rs` | Path-handling tools flagged for path traversal — no suppression for compiler/toolchain repos (2,475 FPs) |
| 6 | Medium | Duplicate | `pipeline.rs` | Same file:line reported by both `hardcoded-secret` and `secret-scan` — 115 duplicate locations |
| 7 | Medium | Missing Coverage | `security_pattern.rs` | C# security patterns produce 0 findings on real .NET code — patterns don't match real-world usage |
| 8 | Medium | Missing Coverage | `security_pattern.rs` | Kotlin security patterns produce 0 findings on real Kotlin code |
| 9 | Medium | False Positive | `security_pattern.rs` | Ruby `class_eval`/`instance_eval` flagged as injection — normal Rails metaprogramming (163 FPs) |
| 10 | Medium | False Positive | `security_pattern.rs` | Python stdlib `eval`/`exec`/`pickle` flagged — legitimate stdlib implementation, not user code |
| 11 | Medium | Performance | `lib.rs` | TypeScript scan takes 22min (1,314s) — `build_source_cache` or `walkdir` scaling issue |
| 12 | Medium | Performance | `lib.rs` | Kubernetes scan takes 10min (607s) for `pkg/` subdirectory only |
| 13 | Medium | Wrong Severity | `security_pattern.rs` | `sprintf()` sanitization indicator `"` always matches (every line has quotes) — severity never downgrades correctly |
| 14 | Low | Silent Data | `secret_scan.rs` | Encoding tables (cp437, mac_farsi) have high entropy by design — character maps flagged as secrets (254 FPs in CPython) |
| 15 | Low | Silent Data | `secret_scan.rs` | JSON source generator output flagged — generated code has high-entropy type metadata (61 FPs in .NET) |
| 16 | Low | Missing Feature | `config.rs` | `lint` command hardcodes `DetectConfig::default()` — no `--detectors` flag to customize |
| 17 | Low | False Positive | `security_pattern.rs` | `fatalError()`/`try!` flagged as CWE-705 in server binaries — only valid for library code (46 FPs in Vapor) |
| 18 | Low | Duplication | `util.rs` | `FALSE_POSITIVE_VALUES` was duplicated between `hardcoded_secret.rs` and `secret_scan.rs` (missing `"test"` in one copy) |
| 19 | Low | Missing Feature | `pipeline.rs` | No detector category/tag system — cannot filter by security vs code-quality programmatically |
| 20 | Low | Missing Coverage | `dependency_audit.rs` | Dependency audit only works for npm/pip — 0 findings for Rust/Java/Go/C#/Swift/Ruby/Kotlin |

Total: 20 bugs — 4 high, 9 medium, 7 low

## Fix Status

| # | Status | Notes |
|---|--------|-------|
| 18 | FIXED | `FALSE_POSITIVE_VALUES` deduplicated to `util.rs` (this session) |
| 1-17, 19-20 | OPEN | Planned in `docs/plans/real-world-validation.md` Phase 5 |

## Validation Stats

- **Repos tested:** 10 (Linux, CPython, TypeScript, ripgrep, Spring Boot, Kubernetes, .NET, Vapor, Rails, ktor)
- **Languages covered:** C, Python, JS/TS, Rust, Java, Go, C#, Swift, Ruby, Kotlin
- **Total findings:** 12,770
- **True positives:** ~2,847 (23%)
- **False positives:** ~9,923 (77%)
- **Crashes:** 0
- **Panics:** 0
