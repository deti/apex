<!-- status: IN_PROGRESS -->

# Self-Analysis Action Plan: 1,545 -> ~200 Actionable Findings

**Goal:** Execute all 6 action items from `docs/apex-v031-self-analysis.md` to reduce actionable findings from 1,545 to approximately 200.

**Date:** 2026-03-20
**Estimated total impact:** -1,459 findings

## File Map

| Crew | Files |
|------|-------|
| security-detect | `crates/apex-detect/src/detectors/path_normalize.rs` |
| security-detect | `crates/apex-detect/src/detectors/panic_pattern.rs` |
| security-detect | `crates/apex-detect/src/config.rs` |
| security-detect | `crates/apex-detect/src/detectors/security_pattern.rs` |
| security-detect | `crates/apex-detect/src/detectors/multi_command_injection.rs` |
| security-detect | `crates/apex-detect/src/detectors/missing_async_timeout.rs` |
| runtime | `crates/apex-sandbox/src/firecracker.rs` |
| runtime | grep for `Command::output().await` inside `timeout()` across workspace |
| platform | `crates/apex-cli/src/lib.rs` (and other async handlers with std::fs) |

---

## Wave 1 — P0 Noise Reduction (parallel, no dependencies)

### Task 1.1 — security-detect crew
**Threat-model suppression for `path-normalize` detector**
**Files:** `crates/apex-detect/src/detectors/path_normalize.rs`
**Impact:** -670 findings

Use `multi_path_traversal.rs` as the template. That detector already has:
- `is_trusted_input_model()` — checks `ctx.threat_model.model_type` for CliTool/ConsoleTool/CiPipeline
- `has_web_handler_context()` — Rust fallback heuristic when no threat model is set
- Severity downgrade to `Low` + `noisy: true` for trusted-input projects

Apply the same pattern to `path_normalize.rs`:

- [ ] Add `use apex_core::config::ThreatModelType;` import
- [ ] Add `is_trusted_input_model()` function (copy from multi_path_traversal.rs)
- [ ] Add `has_web_handler_context()` function (copy from multi_path_traversal.rs)
- [ ] In `analyze()`, compute `project_is_trusted_input` once before the file loop
- [ ] For function-level findings (line 562): when `file_noisy`, set `severity: Low, noisy: true`
- [ ] For expression-level findings in `find_expression_sinks()`: pass noisy flag through, when file is noisy set `severity: Low, noisy: true`
- [ ] Write tests:
  - `rust_cli_tool_threat_model_is_noisy` — CliTool findings are Low+noisy
  - `rust_web_service_threat_model_is_high` — WebService findings stay Medium
  - `rust_no_threat_model_no_web_context_is_noisy` — Rust heuristic fallback
  - `rust_no_threat_model_with_web_context_is_high` — web handler stays High
  - `python_cli_tool_is_noisy` — non-Rust language with CLI model
- [ ] Run `cargo nextest run -p apex-detect --test path_normalize` — confirm pass
- [ ] Commit: "fix(detect): threat-model suppression for path-normalize (-670 findings)"

### Task 1.2 — security-detect crew
**Tag `panic-pattern` noisy for CLI/console tools + add to NOISY_DETECTORS**
**Files:** `crates/apex-detect/src/detectors/panic_pattern.rs`, `crates/apex-detect/src/config.rs`
**Impact:** -339 findings

`unwrap()` in a CLI binary is acceptable practice. For CLI/ConsoleTool threat models, panic findings should be noisy.

- [ ] Add `use apex_core::config::ThreatModelType;` import to panic_pattern.rs
- [ ] Add `is_trusted_input_model()` helper (same as Task 1.1)
- [ ] In `analyze()`, compute `project_is_cli_tool` once before the file loop
- [ ] When `project_is_cli_tool` is true, set all findings to `noisy: true` (keep existing severity logic — Low for unwrap/expect, Medium for panic!/todo!)
- [ ] In `config.rs`, add `"panic-pattern"` to `NOISY_DETECTORS` array
- [ ] Update `empty_toml_gives_defaults` test if NOISY_DETECTORS count changed
- [ ] Write tests:
  - `cli_tool_findings_are_noisy` — CliTool unwrap/panic findings have noisy=true
  - `web_service_findings_are_not_noisy` — WebService findings stay noisy=false
  - `no_threat_model_findings_are_not_noisy` — default behavior preserved
  - `console_tool_findings_are_noisy` — ConsoleTool also suppressed
- [ ] Run `cargo nextest run -p apex-detect` — confirm pass
- [ ] Commit: "fix(detect): tag panic-pattern noisy for CLI tools (-339 findings)"

---

## Wave 2 — P1 Concurrency Fixes (parallel, depends on Wave 1)

### Task 2.1 — runtime crew
**Fix mutex-across-await in firecracker.rs + zombie-subprocess findings**
**Files:** `crates/apex-sandbox/src/firecracker.rs`, plus all files with `Command::output().await` inside `timeout()`
**Impact:** -15 findings

Mutex-across-await (2 locations in firecracker.rs):
- Lines ~422 and ~446/534: `self.snapshot.lock()` held across `.await`
- Fix: extract data from lock before `.await`, or restructure to drop guard before async point

Zombie-subprocess (13 locations):
- Search workspace for `Command::` spawn/output inside timeout blocks without `kill_on_drop(true)`
- Add `kill_on_drop(true)` to each `Command::spawn()` that is inside a timeout block

- [ ] Find all mutex-across-await sites: `grep -rn "\.lock()" crates/apex-sandbox/src/firecracker.rs`
- [ ] Fix location 1 (~line 422): restructure to drop MutexGuard before .await
- [ ] Fix location 2 (~line 446/534): same pattern
- [ ] Search for zombie-subprocess sites: `grep -rn "Command::" crates/ | grep -v test | grep -v "#"`
- [ ] For each site inside a timeout block, add `.kill_on_drop(true)` after `.spawn()`
- [ ] Run `cargo nextest run -p apex-sandbox` — confirm pass
- [ ] Run `cargo nextest run --workspace` — confirm no regressions
- [ ] Commit: "fix(sandbox): eliminate mutex-across-await + zombie-subprocess findings"

### Task 2.2 — security-detect crew
**Triage relaxed-atomics (33) + tune missing-async-timeout detector (44)**
**Files:** `crates/apex-detect/src/detectors/relaxed_atomics.rs`, `crates/apex-detect/src/detectors/missing_async_timeout.rs`
**Impact:** -77 findings

Relaxed-atomics triage:
- Many `Relaxed` orderings are on counters that are only read for approximate monitoring — these are correct
- Add suppression for `AtomicU64`/`AtomicUsize` used as counters (heuristic: variable name contains "count", "total", "metric", "stat")

Missing-async-timeout false positives:
- The detector flags `HashMap::get()`, `Vec::push()`, and other non-I/O methods that happen to be called in async context
- Add allowlist of known non-blocking operations to suppress false positives
- Specifically exclude: `HashMap::get`, `HashMap::insert`, `Vec::push`, `Vec::pop`, `BTreeMap`, `HashSet`, in-memory operations

- [ ] In `relaxed_atomics.rs`: add counter-name heuristic to suppress monitoring counters
- [ ] Write test: `relaxed_ordering_on_counter_is_suppressed`
- [ ] Write test: `relaxed_ordering_on_shared_state_is_flagged`
- [ ] In `missing_async_timeout.rs`: add allowlist for non-I/O operations
- [ ] Write test: `hashmap_get_in_async_not_flagged`
- [ ] Write test: `real_io_in_async_without_timeout_flagged`
- [ ] Run `cargo nextest run -p apex-detect` — confirm pass
- [ ] Commit: "fix(detect): reduce relaxed-atomics + async-timeout false positives (-77 findings)"

---

## Wave 3 — P2/P3 Code Quality + Security Review (parallel, depends on Wave 2)

### Task 3.1 — platform crew
**Migrate blocking std::fs to tokio::fs in async CLI handlers**
**Files:** `crates/apex-cli/src/lib.rs` and other async handlers
**Impact:** -288 findings

Only change `std::fs` calls that are inside `async fn` bodies. Keep `std::fs` in sync functions.

- [ ] Search: `grep -rn "std::fs\|fs::read_to_string\|fs::write\|fs::read\b\|File::open\|File::create" crates/apex-cli/src/`
- [ ] Identify which calls are inside async functions (check for `async fn` scope)
- [ ] Replace `std::fs::read_to_string` with `tokio::fs::read_to_string` (add `.await`)
- [ ] Replace `std::fs::write` with `tokio::fs::write` (add `.await`)
- [ ] Replace `std::fs::read` with `tokio::fs::read` (add `.await`)
- [ ] Replace `File::open` + sync read with `tokio::fs::read_to_string` where appropriate
- [ ] Ensure `tokio` dep has `fs` feature in `crates/apex-cli/Cargo.toml`
- [ ] Do NOT change calls in sync helper functions or non-async code paths
- [ ] Run `cargo nextest run -p apex-cli` — confirm pass
- [ ] Run `cargo check --workspace` — confirm no type errors
- [ ] Commit: "refactor(cli): migrate std::fs to tokio::fs in async handlers (-288 findings)"

### Task 3.2 — security-detect crew
**Threat-model suppression for security-pattern + multi-command-injection detectors**
**Files:** `crates/apex-detect/src/detectors/security_pattern.rs`, `crates/apex-detect/src/detectors/multi_command_injection.rs`
**Impact:** ~-70 findings

APEX is a CLI tool that uses `Command::new` by design. For CliTool/ConsoleTool threat models, `Command::new` findings from the security-pattern detector should be suppressed.

`security_pattern.rs` already imports `should_suppress` from `threat_model.rs`. Check if it already uses threat-model suppression and if not, wire it in:

- [ ] Read `security_pattern.rs` analyze() method — check current threat-model handling
- [ ] If not already suppressed: for Rust `Command::new` and `std::process::Command` patterns, when `should_suppress()` returns `Some(true)`, mark finding `noisy: true` + severity `Low`
- [ ] In `multi_command_injection.rs`: add same threat-model check
- [ ] Write test: `cli_tool_command_new_is_noisy`
- [ ] Write test: `web_service_command_new_is_high`
- [ ] Write test: `cli_tool_subprocess_call_is_noisy` (Python pattern)
- [ ] Run `cargo nextest run -p apex-detect` — confirm pass
- [ ] Commit: "fix(detect): threat-model suppression for command execution detectors (-70 findings)"

---

## Wave 4 — Finalization (sequential, depends on all waves)

### Task 4.1 — platform crew
**Final verification + CHANGELOG**
**Files:** `CHANGELOG.md`

- [ ] Run full workspace build: `cargo check --workspace`
- [ ] Run full workspace tests: `cargo nextest run --workspace`
- [ ] Run clippy: `cargo clippy --workspace -- -D warnings`
- [ ] Run fmt check: `cargo fmt --check`
- [ ] Update `CHANGELOG.md` under `[Unreleased]`:
  ```
  ### Fixed
  - Threat-model suppression for path-normalize detector (-670 findings)
  - panic-pattern tagged noisy for CLI/console tools (-339 findings)
  - mutex-across-await eliminated in firecracker.rs sandbox
  - zombie-subprocess fixed with kill_on_drop(true)
  - Reduced false positives in relaxed-atomics and missing-async-timeout detectors
  - Migrated blocking std::fs to tokio::fs in async CLI handlers (-288 findings)
  - Threat-model suppression for command execution detectors (-70 findings)
  ```
- [ ] Commit: "docs: update CHANGELOG for self-analysis action items"

---

## Crew Assignment Summary

| Wave | Task | Crew | Files | Impact |
|------|------|------|-------|--------|
| 1 | 1.1 | security-detect | path_normalize.rs | -670 |
| 1 | 1.2 | security-detect | panic_pattern.rs, config.rs | -339 |
| 2 | 2.1 | runtime | firecracker.rs, Command sites | -15 |
| 2 | 2.2 | security-detect | relaxed_atomics.rs, missing_async_timeout.rs | -77 |
| 3 | 3.1 | platform | apex-cli/src/lib.rs | -288 |
| 3 | 3.2 | security-detect | security_pattern.rs, multi_command_injection.rs | -70 |
| 4 | 4.1 | platform | CHANGELOG.md | 0 |
| | | **Total** | | **-1,459** |

## Dependency Graph

```
Wave 1: [1.1, 1.2]  (parallel — both security-detect, but different files)
    |
    v
Wave 2: [2.1, 2.2]  (parallel — runtime + security-detect)
    |
    v
Wave 3: [3.1, 3.2]  (parallel — platform + security-detect)
    |
    v
Wave 4: [4.1]       (sequential — final verification)
```

## Template Reference

`crates/apex-detect/src/detectors/multi_path_traversal.rs` lines 217-224 (`is_trusted_input_model`) and lines 232-250 (`has_web_handler_context`) are the canonical patterns for threat-model-aware suppression. All Wave 1/2/3 detector changes should follow this pattern.

## Risk Notes

- **Task 3.1 (tokio::fs migration)** is the highest-risk change — it touches the CLI entry point and requires careful identification of async vs sync boundaries. A wrong migration (changing sync fn to use tokio::fs) will cause compilation errors.
- **Task 2.1 (firecracker mutex)** requires careful restructuring — the lock guard must be dropped before any `.await` point, which may require extracting data into local variables.
- **Tasks 1.1, 1.2, 3.2** are low-risk — they add suppression logic that only changes finding metadata (severity/noisy flag), not program behavior.
