---
date: 2026-03-14
crew: security-detect
affected_partners: [foundation, platform]
severity: minor
acknowledged_by: []
---

## Clippy violation in license_scan.rs blocks clean CI builds

`crates/apex-detect/src/detectors/license_scan.rs:276` has a `clippy::collapsible_str_replace` violation. Fix: replace `.replace('(', "").replace(')', "")` with `.replace(['(', ')'], "")`.

Additionally, `crates/apex-cpg/src/taint_triage.rs:51` uses `partial_cmp().unwrap()` which could panic on NaN scores.

### Full findings from security-detect review

| Severity | File:Line | Issue |
|----------|-----------|-------|
| High | `license_scan.rs:276` | Clippy error blocks `-D warnings` |
| Medium | `taint_triage.rs:51` | `partial_cmp().unwrap()` panics on NaN |
| Low | `sbom.rs:286,316` | `as_object_mut().unwrap()` on SBOM entries |
| Low | `flag_hygiene.rs:40` | Dead field `max_age_days` never read |

981 tests pass, 0 failures.
