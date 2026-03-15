<!-- status: ACTIVE -->

# Fleet Review — Remaining & Skipped Fixes

> **For agentic workers:** Use superpowers:executing-plans. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Fix the 3 real bugs skipped due to worktree drift, merge to main via PR, and close out the fleet review.

**Architecture:** Single agent (3 fixes in one crate), then merge + PR.

---

## Task 1: Security Crew Drift Fixes (3 bugs)

These files exist on main but were missing from the Security agent's worktree (branched before they were created).

**Crate:** `apex-detect`

### S1: `crates/apex-detect/src/detectors/ssrf.rs:76` — SSRF finding miscategorized

- [ ] Read `ssrf.rs`, find where `FindingCategory` is set
- [ ] Change from `SecuritySmell` to `Injection`
- [ ] Add test `bug_ssrf_categorized_as_injection`

### S9: `crates/apex-detect/src/api_coverage.rs:130-131` — Regex compiled in nested loop

- [ ] Read `api_coverage.rs`, find the regex compilation in the loop
- [ ] Wrap in `static LazyLock<Regex>` (same pattern as `secret_scan.rs`)
- [ ] Add test `bug_api_coverage_regex_not_recompiled`

### S11: `crates/apex-detect/src/detectors/crypto_failure.rs:23,42` — Missing uppercase hash names

- [ ] Read `crypto_failure.rs`, find `SAFE_PATTERNS`
- [ ] Add `SHA256`, `SHA384`, `SHA512` (or make match case-insensitive)
- [ ] Add test `bug_uppercase_hash_names_safe`

### Verify & Commit

- [ ] `cargo test -p apex-detect`
- [ ] `cargo clippy -p apex-detect -- -D warnings`
- [ ] Commit: `fix(security): ssrf category, regex caching, uppercase hashes — drift fixes`

---

## Task 2: Merge to Main via PR

All fleet review fixes currently live on worktree branch `worktree-agent-ab41df82`. Need to get them to main.

- [ ] Verify `cargo test --workspace` passes on the merged worktree
- [ ] Create feature branch `fix/fleet-review-2026-03-15`
- [ ] Push branch
- [ ] Create PR with summary of all 58 fixes
- [ ] Update CHANGELOG.md under `[Unreleased]` → `### Fixed`

---

## Task 3: Fleet Review Process Improvement

Prevent future worktree drift and review hallucinations.

### 3a: Add file-existence validation to crew reviews

- [ ] Add to `.fleet/officers/dispatcher.yaml` review_checklist:
  `All findings reference files that exist (verify with ls/glob before reporting)`

### 3b: Update crew review prompts

- [ ] When dispatching crew review agents, include instruction:
  `Before reporting a finding, verify the file exists with Glob. If it doesn't, skip it.`

### 3c: Document worktree drift mitigation

- [ ] Add to `.fleet/officers/dispatcher.yaml` dispatch_protocol:
  `For long-running worktrees, agents should run 'git merge main' before starting work to pick up files added after branch point`

---

## Dropped Findings (hallucinated)

These were reported by the Security crew but the files don't exist anywhere in the codebase:

| Finding | File | Verdict |
|---------|------|---------|
| S7 | `taint.rs` — Missing Flask/Django sources | **Hallucinated** — no taint analysis module exists |
| S8 | `taint.rs` vs `taint_rules.rs` — Divergent lists | **Hallucinated** — neither file exists |

The word "taint" appears in `sql_injection.rs`, `hagnn.rs` etc. as comments/strings, but there is no dedicated taint analysis module. If taint analysis is desired, it's a new feature, not a bug fix.

---

## Execution Order

1. Task 1 — single agent, 3 quick fixes (~5 min)
2. Task 2 — merge verification + PR
3. Task 3 — process improvements (can be done inline)

## Preflight

```bash
./scripts/fleet-preflight.sh 1
# Expected: max_parallel=1, no waves needed
```
