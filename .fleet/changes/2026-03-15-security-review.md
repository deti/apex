---
date: 2026-03-15
crew: security
severity: mixed
acknowledged_by: []
---

## Security Crew Review — apex-detect, apex-cpg

### Wrong-result

1. **`detectors/ssrf.rs:76`** — SSRF findings categorized as `SecuritySmell` instead of `Injection`.
2. **`detectors/session_security.rs:176-205`** — Logic bug: condition is `\!(A && B)` not `\!A && \!B`. False positives on `secure: false` and camelCase.
3. **`api_diff.rs:249,384,442`** — `resolve_ref` constructs fresh `HashSet` at each call site; self-referencing `$ref` infinite-loops.
4. **`sarif.rs:196`** — `artifactLocation.uri` emits raw filesystem path, violating SARIF 2.1.0.
5. **`detectors/command_injection.rs:32-37`** — Flags every `os.system()` regardless of literal arguments.
6. **`detectors/sql_injection.rs:44-52`** — Safe-pattern guard only recognizes single-line parameterized queries.
7. **`taint.rs:25-32`** — Missing Flask/Django taint sources; `find_taint_flows` never consults `TaintRuleSet`.
8. **`taint.rs` vs `taint_rules.rs`** — Two divergent hardcoded source lists maintained separately.
9. **`api_coverage.rs:130-131`** — Regex compiled on every call in nested loop.
10. **`detectors/hardcoded_secret.rs:87`** — `"test"` in FALSE_POSITIVE_VALUES suppresses `latest_token`, `attestation_key` etc.
11. **`detectors/crypto_failure.rs:23,42`** — Missing uppercase `SHA256`/`SHA384`/`SHA512` in SAFE_PATTERNS.

### Silent-corruption

12. **`cvss.rs:297`** — `roundup` casts negative f64 to `u64`, silently saturating to 0.
