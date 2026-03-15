# Fleet Review Fixes — 78 Issues Across 6 Crews

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Fix all actionable findings from the 2026-03-15 fleet-wide crew review.

**Architecture:** 6 parallel worktree tasks, one per crew. Each crew owns non-overlapping crate paths, enabling safe concurrent execution. Style-only findings are deferred unless trivial.

**Tech Stack:** Rust, cargo test, clippy

**Overlap note:** Round 1 plan (`2026-03-14-bug-hunt-round1-fixes.md`) already covers 12 findings. Those are marked `[R1]` below and should be skipped if already merged.

---

## Task 1: Foundation Crew — apex-core, apex-coverage, apex-mir (10 issues, 6 actionable)

**Crates:** `apex-core`, `apex-coverage`, `apex-mir`

| # | File:Line | Severity | Issue | Fix |
|---|-----------|----------|-------|-----|
| F1 | `apex-coverage/src/semantic.rs:25` | wrong-result | Regex recompiled every call | Wrap in `static LazyLock<Regex>` |
| F2 | `apex-coverage/src/mutation.rs:71` | wrong-result | `metamorphic_adequacy([])` returns 1.0 | Return 0.0 for empty input (no mutants = no confidence) |
| F3 | `apex-core/src/agent_report.rs:258` | wrong-result | `bang_for_buck` uses global branch total | Use per-file `total_branches` from the file's own profile |
| F4 | `apex-core/src/agent_report.rs:262` | wrong-result | `file_cov` same wrong denominator | Same fix as F3 — use per-file total |
| F5 | `apex-mir/src/extract.rs:43-45` | silent-corruption | Brace counting in strings/comments | Track `in_string`/`in_comment` state; skip braces inside |
| F6 | `apex-coverage/src/mutation.rs:119` | wrong-result | Hardcoded `detection_margin: 0.8` inflates scores | Make configurable via `MutationConfig` or document the assumption |
| F7 | `apex-core/src/agent_report.rs:244` | style | `line == 0` wraps to `u32::MAX` | Guard: `if branch.line == 0 { continue; }` |
| F8 | `apex-mir/src/extract.rs` [R1] | wrong-result | `extract_fn_name` includes generics | Already in round 1 plan Task 4 |
| F9 | `apex-coverage/src/oracle.rs` [R1] | style | Mutex poison panic | Already in round 1 plan Task 5 |
| F10 | `apex-core/src/git.rs:85` | style | Misleading `secs` alias | Rename to `days` — trivial |

**Verify:** `cargo test -p apex-core && cargo test -p apex-coverage && cargo test -p apex-mir`

---

## Task 2: Exploration Crew — apex-fuzz, apex-symbolic, apex-concolic (12 issues, 9 actionable)

**Crates:** `apex-fuzz`, `apex-symbolic`, `apex-concolic`

| # | File:Line | Severity | Issue | Fix |
|---|-----------|----------|-------|-----|
| E1 | `apex-fuzz/src/de_scheduler.rs:11` | crash | `DeScheduler::new(0)` → INFINITY; empty `select()` panics | Guard: if `strategies.is_empty()`, return `None` from `select()` |
| E2 | `apex-fuzz/src/scheduler.rs:62` | crash | `MOptScheduler::mutate()` indexes `stats[0]` on empty | Guard: early return if `stats.is_empty()` |
| E3 | `apex-fuzz/src/thompson.rs:38` | crash | `select()` returns 0 on empty scheduler | Return `None` (change signature to `Option<usize>`) |
| E4 | `apex-concolic/src/search.rs:182-183` | crash | `InterleavedSearch::select()` panics on empty; `rounds_remaining` underflows | Guard empty + use `saturating_sub` |
| E5 | `apex-fuzz/src/scheduler.rs:84` | wrong-result | `report_hit` increments before yield ratio | Compute ratio before incrementing `coverage_hits` |
| E6 | `apex-fuzz/src/corpus.rs:78` | wrong-result | `sample_pair()` never increments `fuzz_count` | Add `entry.fuzz_count += 1` after sampling |
| E7 | `apex-symbolic/src/cache.rs:66` | wrong-result | `set_logic()` doesn't flush cache | Call `self.cache.clear()` in `set_logic()` |
| E8 | `apex-symbolic/src/gradient.rs:174` | wrong-result | `GradientSolver` only considers last constraint | Iterate all constraints; accumulate gradient |
| E9 | `apex-concolic/src/js_conditions.rs:382` | wrong-result | `find_word_outside_parens` ignores escaped quotes | Track `escaped` state in char iterator |
| E10 | `apex-concolic/src/python.rs:342` [R1] | wrong-result | `n + 1` integer overflow | Already in round 1 plan Task 1 |
| E11 | `apex-concolic/src/python.rs:340` [R1] | silent-corruption | `parse().unwrap_or(0)` | Already in round 1 plan Task 1 |
| E12 | `apex-concolic/src/python.rs:434` [R1] | silent-corruption | Unescaped vars in Python stubs | Already in round 1 plan Task 1 |

**Verify:** `cargo test -p apex-fuzz && cargo test -p apex-symbolic && cargo test -p apex-concolic`

---

## Task 3: Security Crew — apex-detect, apex-cpg (12 issues, 10 actionable)

**Crates:** `apex-detect`, `apex-cpg`

| # | File:Line | Severity | Issue | Fix |
|---|-----------|----------|-------|-----|
| S1 | `detectors/ssrf.rs:76` | wrong-result | SSRF categorized as `SecuritySmell` | Change to `FindingCategory::Injection` |
| S2 | `detectors/session_security.rs:176-205` | wrong-result | `!(A && B)` logic bug; false positives | Fix to `!A && !B`; handle camelCase (`httpOnly`, `sameSite`) |
| S3 | `api_diff.rs:249,384,442` [R1] | wrong-result | Circular `$ref` stack overflow | Already in round 1 plan Task 2, but crew found 3 call sites vs 1 — verify all are patched |
| S4 | `sarif.rs:196` | wrong-result | Raw filesystem path in SARIF URI | Strip to relative path; prefix `file:///` per SARIF 2.1.0 |
| S5 | `detectors/command_injection.rs:32-37` | wrong-result | Flags `os.system("literal")` | Skip when argument is a string literal (no interpolation) |
| S6 | `detectors/sql_injection.rs:44-52` | wrong-result | Safe-pattern guard is single-line only | Make regex `(?s)` (DOTALL) to span continuation lines |
| S7 | `taint.rs:25-32` | wrong-result | Missing Flask/Django sources; `TaintRuleSet` unused | Add Flask (`request.args`, `request.form`) and Django (`request.GET`, `request.POST`) sources; wire `TaintRuleSet` into `find_taint_flows` |
| S8 | `taint.rs` vs `taint_rules.rs` | wrong-result | Two divergent hardcoded source lists | Merge into single source of truth in `taint_rules.rs`; have `taint.rs` consume it |
| S9 | `api_coverage.rs:130-131` | wrong-result | Regex compiled in nested loop | `static LazyLock<Regex>` |
| S10 | `detectors/hardcoded_secret.rs:87` | wrong-result | `"test"` in FALSE_POSITIVE_VALUES too broad | Use word-boundary match: `\btest\b` or check full token, not substring |
| S11 | `detectors/crypto_failure.rs:23,42` | wrong-result | Missing uppercase hash names in SAFE_PATTERNS | Add `SHA256`, `SHA384`, `SHA512` (case-insensitive match) |
| S12 | `cvss.rs:297` | silent-corruption | Negative f64 → u64 saturates to 0 | Clamp to `0.0_f64.max(value)` before cast |

**Verify:** `cargo test -p apex-detect`

---

## Task 4: Runtime Crew — apex-lang, apex-instrument, apex-sandbox, apex-index (10 issues, 8 actionable)

**Crates:** `apex-lang`, `apex-instrument`, `apex-sandbox`, `apex-index`

| # | File:Line | Severity | Issue | Fix |
|---|-----------|----------|-------|-----|
| R1 | `apex-sandbox/src/sancov_rt.rs:33` | crash | Null `stop` pointer not checked | Add `if stop.is_null() { return; }` before dereference |
| R2 | `apex-sandbox/src/shm.rs:84` | wrong-result | `name_str()` returns `""` on non-UTF-8 | Return `Err` or use `to_string_lossy()` with a `warn!` |
| R3 | `apex-instrument/src/v8_coverage.rs:122` | wrong-result | Parent range missing from its own group | Add parent range as direction 0 entry in the group |
| R4 | `apex-instrument/src/source_map.rs:60` | wrong-result | Branches dropped when source map has no token | Log at `debug!` level; optionally keep with original positions |
| R5 | `apex-instrument/src/source_map.rs:29` | wrong-result | `line == 0` and `== 1` both saturate to 0 | Map `line == 0` to line 1 (1-indexed); `saturating_sub(1)` only when > 0 |
| R6 | `apex-index/src/analysis.rs:88` | wrong-result | `divergent_runs` always equals `total_runs` | Compute actual divergence count from test result variance |
| R7 | `apex-sandbox/src/firecracker.rs:518` | wrong-result | `snapshot()` generates 3 different IDs | Generate one `SnapshotId` at start; use it for all 3 operations |
| R8 | `apex-instrument/src/v8_coverage.rs:107` | wrong-result | Stale offset maps to last line | Skip offsets beyond `source.len()` with `warn!` |
| R9 | `apex-sandbox/src/process.rs:187` | wrong-result | Crash range includes exit code 128 | Change to `129..=159` (128 is `SIGHUP` death only when `128 + signal`) |
| R10 | `apex-sandbox/src/shm.rs:88` | style | No explicit memory ordering | Add `Ordering::Acquire` on reads after child exit — deferred |

**Note:** apex-index bugs (branch_key, build_profiles, extract_functions) are in round 1 plan Task 3.

**Verify:** `cargo test -p apex-instrument && cargo test -p apex-sandbox && cargo test -p apex-index`

---

## Task 5: Intelligence Crew — apex-agent, apex-synth, apex-rpc (15 issues, 12 actionable)

**Crates:** `apex-agent`, `apex-synth`, `apex-rpc`

| # | File:Line | Severity | Issue | Fix |
|---|-----------|----------|-------|-----|
| I1 | `orchestrator.rs:117-152` | wrong-result | Sandbox errors increment stall counter | Track `sandbox_error_count` separately; only stall-count when ≥1 sandbox success with no new coverage |
| I2 | `orchestrator.rs:112-113` | silent-corruption | Strategy errors silently dropped | `filter_map(\|r\| r.map_err(\|e\| warn!("strategy error: {e}")).ok())` |
| I3 | `orchestrator.rs:120-126` | silent-corruption | Sandbox errors silently dropped | Same pattern as I2 |
| I4 | `rotation.rs:31` | crash | `rotate()` div-by-zero on empty list | Guard: `if self.strategies.is_empty() { return; }` |
| I5 | `bandit.rs:53` | crash | `partial_cmp().unwrap()` panics on NaN | `.unwrap_or(std::cmp::Ordering::Equal)` |
| I6 | `driller.rs:88` | wrong-result | Solver errors invisible | Count errors; emit `warn!` per failure |
| I7 | `monitor.rs:46-49` | wrong-result | `growth_rate()` divides by N not N-1 | `(self.window.len() - 1).max(1)` |
| I8 | `budget.rs:34,44` | wrong-result | Div-by-zero if `num_strategies == 0` | Guard in `new()`: `assert!(num_strategies > 0)` or return default allocation |
| I9 | `prompt_registry.rs:74-77` | wrong-result | HashMap iteration → non-deterministic substitution | Sort keys before iterating, or single-pass regex replacement |
| I10 | `eliminate.rs:20,55` | wrong-result | Byte-length for indentation (wrong for non-ASCII) | Use `line.chars().take_while(\|c\| c.is_whitespace()).count()` |
| I11 | `error_classify.rs:26` | style | Implicit operator precedence | Add explicit parentheses — trivial |
| I12 | `coordinator.rs:103` | wrong-result | Client `seed_id` replaced with fresh ID | Parse `result.seed_id` as `SeedId` or hash-derive it |
| I13 | `worker.rs:131-133` | wrong-result | `(0, 0.0)` ambiguous for "skipped" vs "no coverage" | Return `enum PullOutcome { AllSkipped, Submitted(u64, f64) }` |
| I14 | `source.rs:42` | style | Fragile `lines[0]` without assertion | Add `debug_assert!(!lines.is_empty())` — trivial |
| I15 | `few_shot.rs:28-29` | style | O(n) eviction via `Vec::remove(0)` | Switch to `VecDeque` — deferred unless high throughput |

**Verify:** `cargo test -p apex-agent && cargo test -p apex-synth && cargo test -p apex-rpc`

---

## Task 6: Platform Crew — apex-cli, apex-reach (10 issues, 8 actionable)

**Crates:** `apex-cli`, `apex-reach`

| # | File:Line | Severity | Issue | Fix |
|---|-----------|----------|-------|-----|
| P1 | `lib.rs:1028` | wrong-result | `deadline_secs` integer division truncates to 0 | `(process_timeout_ms + 999) / 1000` (ceiling division) |
| P2 | `extractors/mod.rs:32` | wrong-result | Swift/CSharp extractors never dispatched | Add `Language::Swift \| Language::CSharp` arms to dispatch match |
| P3 | `lib.rs:3089` | wrong-result | `run_reach` uses parent dir, not repo root | Use `git rev-parse --show-toplevel` or walk up to find `.git` |
| P4 | `lib.rs:3011` | wrong-result | `apex features` misses Go/Cpp/Swift/CSharp | Add all `Language` variants to the features table |
| P5 | `extractors/python.rs` | wrong-result | `_private` in `__init__.py` marked PublicApi | Skip functions starting with `_` (except `__init__`, `__main__`) |
| P6 | `extractors/python.rs` | wrong-result | Every function marked `Main` if `__name__` anywhere | Only mark `Main` for functions defined after the `if __name__` guard |
| P7 | `graph.rs` | wrong-result | `fn_at` path lookup fails on relative vs absolute | Normalize paths via `Path::canonicalize()` or strip common prefix |
| P8 | `main.rs:13` | crash | `unwrap_or_default()` gives empty path | Use `env::current_dir().context("cannot determine working directory")?` |
| P9 | `lib.rs:3338,3426,3796` | crash | `process::exit(1)` bypasses Tokio shutdown | Replace with `return Err(...)` propagation |
| P10 | `graph.rs` | style | `CallGraph::node()` is O(n) in BFS hot path | Use `HashMap<String, NodeIndex>` lookup — deferred unless perf issue |

**Verify:** `cargo test -p apex-cli && cargo test -p apex-reach`

---

## Priority Order Within Each Task

Each agent should fix bugs in this order:
1. **crash** — panics, div-by-zero, null deref
2. **wrong-result** — incorrect output that affects users
3. **silent-corruption** — data loss without signal
4. **style** — only if trivial (1-line fix)

Skip `[R1]`-marked items if round 1 plan is already merged.

## Execution

Dispatch 6 parallel worktree agents, one per task. Each agent:
1. Creates a feature branch `fix/crew-<name>-review`
2. For each bug: write failing test (`bug_` prefix), implement fix, verify
3. Run `cargo test` for owned crates
4. Run `cargo clippy` for owned crates
5. Commit all fixes as one logical commit

After all 6 complete:
```bash
# Merge all branches
for crew in foundation exploration security runtime intelligence platform; do
  git merge fix/crew-${crew}-review
done

# Full verification
cargo test --workspace
cargo clippy --workspace -- -D warnings
cargo test --workspace -- bug_
```

## Summary

| Crew | Total | Crash | Wrong | Corrupt | Style | Overlap w/R1 | Net New |
|------|-------|-------|-------|---------|-------|--------------|---------|
| Foundation | 10 | 0 | 6 | 1 | 3 | 2 | 8 |
| Exploration | 12 | 4 | 5 | 2 | 0 | 3 | 9 |
| Security | 12 | 0 | 10 | 1 | 0 | 1 | 11 |
| Runtime | 10 | 1 | 8 | 0 | 1 | 0 | 10 |
| Intelligence | 15 | 2 | 9 | 2 | 3 | 0 | 15 |
| Platform | 10 | 2 | 7 | 0 | 1 | 0 | 10 |
| **Total** | **69** | **9** | **45** | **6** | **8** | **6** | **63** |

**63 net new fixes** across 6 parallel agents. Estimated: ~30 minutes wall-clock with parallel dispatch.
