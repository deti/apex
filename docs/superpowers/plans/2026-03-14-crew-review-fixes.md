<!-- status: DONE -->
# Crew Review Fixes Implementation Plan

> **For agentic workers:** REQUIRED: Use fleet crew agents for implementation. Each task is tagged with the owning crew. Dispatch crews in parallel where tasks don't share files. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Fix all 14 critical and 18 high-impact important findings from the 6-crew parallel review.

**Architecture:** Tasks grouped by crew ownership. Each crew works only within its owned paths. Tasks within the same crew are sequential; tasks across different crews run in parallel. Three waves: Wave 1 (foundation — depended on by all), Wave 2 (security + exploration + runtime — independent), Wave 3 (intelligence + platform — depend on earlier fixes).

**Tech Stack:** Rust, tokio, DashMap, tonic gRPC, serde, tracing

---

## File Map

| Crew | Files Modified |
|------|---------------|
| **Foundation** | `apex-coverage/src/oracle.rs`, `apex-core/src/command.rs`, `apex-coverage/src/heuristic.rs` |
| **Security** | `apex-cpg/src/taint_rules.rs` |
| **Exploration** | `apex-fuzz/src/cmplog.rs` |
| **Runtime** | `apex-sandbox/src/shim.rs`, `apex-instrument/src/java.rs`, `apex-instrument/src/rust_cov.rs`, `apex-instrument/src/wasm.rs`, `apex-sandbox/src/python.rs`, `apex-index/src/rust.rs` |
| **Intelligence** | `apex-agent/src/classifier.rs`, `apex-agent/src/orchestrator.rs` |
| **Platform** | `apex-cli/src/lib.rs`, `apex-cli/src/doctor.rs` |

---

## Wave 1: Foundation Crew

### Task 1: Fix `merge_bitmap` non-deterministic DashMap iteration

**Crew:** foundation
**Files:**
- Modify: `crates/apex-coverage/src/oracle.rs:126-140`

`DashMap::iter()` has no stable order. Bitmap index-to-branch mapping is non-deterministic → silently wrong coverage data.

- [ ] **Step 1: Write failing test**

```rust
#[test]
fn merge_bitmap_uses_stable_ordering() {
    let oracle = CoverageOracle::new();
    let branches: Vec<_> = (0u32..10).map(|l| make_branch(l, 0)).collect();
    oracle.register_branches(branches.clone());

    // Bitmap covers indices 0, 2, 4
    let mut bitmap = vec![0u8; 10];
    bitmap[0] = 1;
    bitmap[2] = 1;
    bitmap[4] = 1;

    let seed = SeedId::new();
    let delta1 = oracle.merge_bitmap(&bitmap, seed);

    // Do it again — same bitmap must produce same results
    let oracle2 = CoverageOracle::new();
    oracle2.register_branches(branches.clone());
    let delta2 = oracle2.merge_bitmap(&bitmap, SeedId::new());

    // The newly_covered branches must be identical
    let mut ids1: Vec<_> = delta1.newly_covered.iter().map(|b| b.line).collect();
    let mut ids2: Vec<_> = delta2.newly_covered.iter().map(|b| b.line).collect();
    ids1.sort();
    ids2.sort();
    assert_eq!(ids1, ids2, "bitmap mapping must be deterministic");
}
```

- [ ] **Step 2: Add an ordered branch index to CoverageOracle**

Add a field to maintain insertion-order mapping:

```rust
pub struct CoverageOracle {
    branches: DashMap<BranchId, BranchState>,
    branch_order: Mutex<Vec<BranchId>>,  // insertion-ordered index
    // ... existing fields
}
```

Update `register_branches` to also push to `branch_order`:

```rust
pub fn register_branches(&self, ids: impl IntoIterator<Item = BranchId>) {
    let mut order = self.branch_order.lock().unwrap();
    for id in ids {
        self.branches.entry(id.clone()).or_insert_with(|| {
            self.total_count.fetch_add(1, Ordering::Relaxed);
            order.push(id.clone());
            BranchState::Uncovered
        });
    }
}
```

Update `merge_bitmap` to use the ordered index:

```rust
pub fn merge_bitmap(&self, bitmap: &[u8], seed_id: SeedId) -> DeltaCoverage {
    let mut delta = DeltaCoverage::default();
    let order = self.branch_order.lock().unwrap();
    for (idx, &byte) in bitmap.iter().enumerate() {
        if byte > 0 {
            if let Some(branch) = order.get(idx) {
                if self.mark_covered(branch, seed_id) {
                    delta.newly_covered.push(branch.clone());
                }
            }
        }
    }
    delta
}
```

- [ ] **Step 3: Run tests**

Run: `cargo test -p apex-coverage`
Expected: PASS

- [ ] **Step 4: Commit**

```bash
git add crates/apex-coverage/src/oracle.rs
git commit -m "fix: use insertion-ordered index for bitmap-to-branch mapping"
```

---

### Task 2: Kill child process on timeout

**Crew:** foundation
**Files:**
- Modify: `crates/apex-core/src/command.rs:118-122`

Timed-out child processes are orphaned — never killed.

- [ ] **Step 1: Write failing test**

```rust
#[tokio::test]
async fn timeout_kills_child_process() {
    let spec = CommandSpec::new("sleep", std::env::temp_dir())
        .args(["10"])
        .timeout(100); // 100ms timeout
    let runner = RealCommandRunner;
    let result = runner.run_command(&spec).await;
    assert!(matches!(result, Err(ApexError::Timeout(100))));
    // The sleep process should have been killed — verify by checking
    // no zombie "sleep 10" is left (OS-specific, but timeout should at least return)
}
```

- [ ] **Step 2: Fix the timeout handler to kill the child**

```rust
let result = tokio::time::timeout(deadline, child.wait_with_output()).await;

match result {
    Err(_) => {
        // Kill the child process to prevent orphans
        let _ = child.kill().await;
        Err(ApexError::Timeout(spec.timeout_ms))
    }
    // ... rest unchanged
}
```

Note: `child.wait_with_output()` consumes `child`, so we need to restructure. Use `child.kill()` before `wait_with_output`:

```rust
let deadline = std::time::Duration::from_millis(spec.timeout_ms);
let result = tokio::time::timeout(deadline, child.wait_with_output()).await;

match result {
    Err(_) => {
        // Timeout fired — child is already dropped (wait_with_output consumed it),
        // but tokio::process::Child::drop kills the process on Unix.
        // However, to be explicit and cross-platform safe:
        Err(ApexError::Timeout(spec.timeout_ms))
    }
    // ...
}
```

Actually, `tokio::time::timeout` cancels the future, which drops `child.wait_with_output()`. On drop, tokio's `Child` sends SIGKILL on Unix. Verify this behavior with the test. If the drop doesn't kill, restructure to hold `child` separately:

```rust
let mut child = /* spawned */;
let pid = child.id();
let deadline = std::time::Duration::from_millis(spec.timeout_ms);
match tokio::time::timeout(deadline, child.wait_with_output()).await {
    Err(_) => {
        // Future dropped, child should be killed by tokio's Drop impl.
        // Belt-and-suspenders: try explicit kill via PID
        #[cfg(unix)]
        if let Some(pid) = pid {
            let _ = nix::sys::signal::kill(
                nix::unistd::Pid::from_raw(pid as i32),
                nix::sys::signal::Signal::SIGKILL,
            );
        }
        Err(ApexError::Timeout(spec.timeout_ms))
    }
    Ok(Err(e)) => Err(ApexError::Subprocess { exit_code: -1, stderr: format!("wait: {e}") }),
    Ok(Ok(output)) => Ok(CommandOutput { /* ... */ }),
}
```

The simplest correct fix: tokio's `Child` Drop impl already kills the process. Add a test that verifies timeout returns promptly (not after 10s). If the test passes, no additional kill code is needed — just document the reliance on tokio's Drop behavior.

- [ ] **Step 3: Run tests**

Run: `cargo test -p apex-core timeout`
Expected: PASS

- [ ] **Step 4: Commit**

```bash
git add crates/apex-core/src/command.rs
git commit -m "fix: verify child process killed on timeout via tokio Drop"
```

---

### Task 3: Fix `branch_distance` for extreme i64 values

**Crew:** foundation
**Files:**
- Modify: `crates/apex-coverage/src/heuristic.rs:48-54`

- [ ] **Step 1: Write failing test**

```rust
#[test]
fn branch_distance_extreme_values() {
    let score = branch_distance(CmpOp::Lt, i64::MAX, i64::MIN);
    assert!(score >= 0.0 && score <= 1.0, "score was {score}");

    let score2 = branch_distance(CmpOp::Gt, i64::MIN, i64::MAX);
    assert!(score2 >= 0.0 && score2 <= 1.0, "score2 was {score2}");
}
```

- [ ] **Step 2: Clamp the distance to non-negative before normalizing**

```rust
CmpOp::Lt => {
    if a < b {
        1.0
    } else {
        let dist = ((a as f64) - (b as f64) + 1.0).max(0.0);
        1.0 - normalize(dist)
    }
}
```

Apply the same `.max(0.0)` pattern to `Le`, `Gt`, `Ge` arms.

- [ ] **Step 3: Run tests**

Run: `cargo test -p apex-coverage branch_distance`
Expected: PASS

- [ ] **Step 4: Commit**

```bash
git add crates/apex-coverage/src/heuristic.rs
git commit -m "fix: clamp branch_distance to non-negative before normalizing"
```

---

## Wave 2a: Security Crew

### Task 4: Replace substring matching with exact matching in TaintRuleSet

**Crew:** security-detect
**Files:**
- Modify: `crates/apex-cpg/src/taint_rules.rs:117-130`

`name.contains("exec")` matches `executor`, `execute_callback`. `"clean"` matches `cleanup_data()`.

- [ ] **Step 1: Write failing test**

```rust
#[test]
fn is_sink_rejects_substring_match() {
    let rules = TaintRuleSet::python_defaults();
    // "exec" is a sink, but "executor" should NOT be
    assert!(rules.is_sink("exec"));
    assert!(!rules.is_sink("executor"));
    assert!(!rules.is_sink("execute_callback"));
}

#[test]
fn is_sanitizer_rejects_substring_match() {
    let rules = TaintRuleSet::python_defaults();
    assert!(rules.is_sanitizer("shlex.quote"));
    assert!(!rules.is_sanitizer("cleanup_data"));
    assert!(!rules.is_sanitizer("my_escape_plan"));
}
```

- [ ] **Step 2: Change matching to exact or suffix match**

Replace `.contains()` with a match that checks the function name ends with the pattern (for dotted names like `shlex.quote`) or equals it exactly:

```rust
pub fn is_source(&self, name: &str) -> bool {
    self.sources.iter().any(|s| name == s.as_str() || name.ends_with(&format!(".{s}")))
}

pub fn is_sink(&self, name: &str) -> bool {
    self.sinks.iter().any(|s| name == s.as_str() || name.ends_with(&format!(".{s}")))
}

pub fn is_sanitizer(&self, name: &str) -> bool {
    self.sanitizers.iter().any(|s| name == s.as_str() || name.ends_with(&format!(".{s}")))
}
```

This handles both `exec` (exact) and `os.system` (suffix after `.`), while rejecting `executor` (neither exact nor suffix match).

- [ ] **Step 3: Update python_defaults sink list**

Some sinks in `python_defaults()` are bare words like `"execute"` that rely on substring matching. Update them to be more specific: `"cursor.execute"`, `"conn.execute"`, `"cursor.executemany"`.

- [ ] **Step 4: Run tests**

Run: `cargo test -p apex-cpg`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add crates/apex-cpg/src/taint_rules.rs
git commit -m "fix: use exact/suffix matching in taint rules, not substring"
```

---

## Wave 2b: Exploration Crew

### Task 5: Fix CmpLogMutator panic on different-length operands

**Crew:** exploration
**Files:**
- Modify: `crates/apex-fuzz/src/cmplog.rs:140-149`

`copy_from_slice` panics when `needle.len() != replacement.len()`.

- [ ] **Step 1: Write failing test**

```rust
#[test]
fn cmplog_mutator_different_length_operands() {
    let mut mutator = CmpLogMutator::new();
    let table = CmpLogTable::new();
    let branch = BranchId::new(1, 10, 0, 0);
    // Record entry with different-length operands
    table.record(branch.clone(), CmpLogEntry {
        op: CmpOp::Eq,
        a: 0x41414141,      // "AAAA" as bytes
        b: 0x424242,        // "BBB" as bytes (shorter!)
    });
    let input = b"test AAAA data";
    // Should not panic
    let result = mutator.mutate(input, &mut rand::thread_rng());
    assert!(!result.is_empty());
}
```

- [ ] **Step 2: Handle different-length replacements with splice**

Replace the `copy_from_slice` with proper splicing (same approach as `RedQueenMutator`):

```rust
let mut out = input.to_vec();
if needle.len() == replacement.len() {
    out[pos..pos + needle.len()].copy_from_slice(replacement);
} else {
    // Splice: remove needle, insert replacement
    out.splice(pos..pos + needle.len(), replacement.iter().copied());
}
out
```

- [ ] **Step 3: Run tests**

Run: `cargo test -p apex-fuzz cmplog`
Expected: PASS

- [ ] **Step 4: Commit**

```bash
git add crates/apex-fuzz/src/cmplog.rs
git commit -m "fix: handle different-length operands in CmpLogMutator"
```

---

### Task 6: Fix CmpLogTable::entries_for dropping wrapped VecDeque data

**Crew:** exploration
**Files:**
- Modify: `crates/apex-fuzz/src/cmplog.rs:216-221`

`as_slices().0` only returns the first contiguous segment of a VecDeque.

- [ ] **Step 1: Write failing test**

```rust
#[test]
fn entries_for_returns_all_after_wrap() {
    let table = CmpLogTable::new();
    let branch = BranchId::new(1, 10, 0, 0);
    // Fill past the ring buffer limit (256) to force wrap
    for i in 0..300u64 {
        table.record(branch.clone(), CmpLogEntry {
            op: CmpOp::Eq, a: i as i64, b: (i + 1) as i64,
        });
    }
    // After 300 inserts with 256 limit, should have 256 entries
    let entries = table.entries_for(&branch);
    assert_eq!(entries.len(), 256, "should return all 256 entries, not just first slice");
}
```

- [ ] **Step 2: Change return type to Vec to handle non-contiguous data**

```rust
pub fn entries_for(&self, branch: &BranchId) -> Vec<&CmpLogEntry> {
    match self.entries.get(branch) {
        Some(ring) => ring.iter().collect(),
        None => Vec::new(),
    }
}
```

Update callers of `entries_for` to work with `Vec<&CmpLogEntry>` instead of `&[CmpLogEntry]`.

Alternatively, use `make_contiguous()` if mutability is available, or return a `Cow`-like wrapper. The simplest correct fix is returning a collected `Vec`.

- [ ] **Step 3: Run tests**

Run: `cargo test -p apex-fuzz`
Expected: PASS

- [ ] **Step 4: Commit**

```bash
git add crates/apex-fuzz/src/cmplog.rs
git commit -m "fix: entries_for returns all VecDeque entries, not just first slice"
```

---

## Wave 2c: Runtime Crew

### Task 7: Deduplicate fnv1a_hash — use apex_core::hash everywhere

**Crew:** runtime
**Files:**
- Modify: `crates/apex-instrument/src/java.rs` (remove local `fnv1a_hash`, import from apex_core)
- Modify: `crates/apex-instrument/src/rust_cov.rs` (remove local `fnv1a`, import from apex_core)
- Modify: `crates/apex-instrument/src/wasm.rs` (remove local `fnv1a_hash`, import from apex_core)
- Modify: `crates/apex-sandbox/src/python.rs` (remove local `fnv1a_hash`, import from apex_core)
- Modify: `crates/apex-index/src/rust.rs` (remove local `fnv1a_hash`, import from apex_core)

- [ ] **Step 1: Verify apex_core exports fnv1a_hash**

Run: `grep -n "pub fn fnv1a_hash" crates/apex-core/src/hash.rs`
Expected: Shows the canonical implementation.

- [ ] **Step 2: In each file, delete the local copy and add import**

Replace each local `fn fnv1a_hash(...)` with:
```rust
use apex_core::hash::fnv1a_hash;
```

For `rust_cov.rs` which uses the name `fnv1a`, either rename callers or add:
```rust
use apex_core::hash::fnv1a_hash as fnv1a;
```

- [ ] **Step 3: Verify Cargo.toml dependencies**

Each crate's `Cargo.toml` must list `apex-core` as a dependency. Check:
- `apex-instrument/Cargo.toml` — should already have it
- `apex-sandbox/Cargo.toml` — should already have it
- `apex-index/Cargo.toml` — should already have it

- [ ] **Step 4: Run tests**

Run: `cargo test -p apex-instrument -p apex-sandbox -p apex-index`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add crates/apex-instrument/ crates/apex-sandbox/ crates/apex-index/
git commit -m "fix: deduplicate fnv1a_hash — use apex_core::hash everywhere"
```

---

### Task 8: Fix SanCov shim guard index 0 skipped

**Crew:** runtime
**Files:**
- Modify: `crates/apex-sandbox/src/shim.rs:50-53`

C shim skips `*guard == 0`, but `sancov_rt.rs` assigns IDs starting from 0. First edge never counted.

- [ ] **Step 1: Write test documenting the issue**

```rust
#[test]
fn sancov_shim_source_does_not_skip_guard_zero() {
    // The C shim source should count guard index 0
    let source = SANCOV_RT_SOURCE;
    // The old code had `!*guard` which skips index 0
    // The fix should use `*guard >= APEX_MAP_SIZE` only
    assert!(!source.contains("!*guard"), "shim should not skip guard index 0");
}
```

- [ ] **Step 2: Fix the C shim**

Change line 51 from:
```c
if (!__apex_trace_bits || !*guard || *guard >= APEX_MAP_SIZE) return;
```
To:
```c
if (!__apex_trace_bits || *guard >= APEX_MAP_SIZE) return;
```

This matches the Rust `sancov_rt.rs` behavior which counts guard 0.

- [ ] **Step 3: Also fix guard_init to start from 1 if guard 0 is problematic**

Actually, the safer fix is to start guard assignment from 1 instead of 0 in `sancov_rt.rs`, keeping the C shim's `!*guard` check as a sentinel. Check what LLVM's SanCov convention is — LLVM initializes guards to 0, and `!*guard` is the standard "uninitialized guard" check. The fix should be in `sancov_rt.rs` to start from 1:

```rust
static NEXT_ID: AtomicU32 = AtomicU32::new(1); // Start from 1, not 0
```

This way both C and Rust agree: guard 0 = uninitialized sentinel, guard 1+ = real edges.

- [ ] **Step 4: Run tests**

Run: `cargo test -p apex-sandbox sancov`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add crates/apex-sandbox/src/shim.rs crates/apex-sandbox/src/sancov_rt.rs
git commit -m "fix: align C shim and Rust sancov_rt on guard index 0 semantics"
```

---

## Wave 3a: Intelligence Crew

### Task 9: Fix operator precedence bug in BranchClassifier

**Crew:** intelligence
**Files:**
- Modify: `crates/apex-agent/src/classifier.rs:24`

`||` vs `&&` precedence causes any snippet with `[` to be classified as `DataFlow`.

- [ ] **Step 1: Write failing test**

```rust
#[test]
fn classify_list_literal_not_dataflow() {
    let classifier = BranchClassifier;
    // A simple list literal should be Trivial, not DataFlow
    let result = classifier.classify_source("[1, 2, 3]");
    assert_ne!(result, BranchDifficulty::DataFlow);
}

#[test]
fn classify_dict_access_is_dataflow() {
    let classifier = BranchClassifier;
    // dict[key].method > value is DataFlow
    let result = classifier.classify_source("data[key].score > threshold");
    assert_eq!(result, BranchDifficulty::DataFlow);
}
```

- [ ] **Step 2: Fix the precedence**

Change line 24 from:
```rust
if snippet.contains('[') || snippet.contains('.') && snippet.contains('>') {
```
To:
```rust
if (snippet.contains('[') || snippet.contains('.')) && snippet.contains('>') {
```

- [ ] **Step 3: Run tests**

Run: `cargo test -p apex-agent classifier`
Expected: PASS

- [ ] **Step 4: Commit**

```bash
git add crates/apex-agent/src/classifier.rs
git commit -m "fix: operator precedence in BranchClassifier::classify_source"
```

---

### Task 10: Log observe() errors instead of silently dropping

**Crew:** intelligence
**Files:**
- Modify: `crates/apex-agent/src/orchestrator.rs:163`

`let _ = strategy.observe(result).await` silently drops errors.

- [ ] **Step 1: Fix the silent drop**

Change line 163 from:
```rust
let _ = strategy.observe(result).await;
```
To:
```rust
if let Err(e) = strategy.observe(result).await {
    tracing::warn!(error = %e, strategy = %strategy.name(), "Strategy observe failed");
}
```

- [ ] **Step 2: Run tests**

Run: `cargo test -p apex-agent`
Expected: PASS

- [ ] **Step 3: Commit**

```bash
git add crates/apex-agent/src/orchestrator.rs
git commit -m "fix: log strategy observe errors instead of silently dropping"
```

---

## Wave 3b: Platform Crew

### Task 11: Replace std::process::exit with Result returns

**Crew:** platform
**Files:**
- Modify: `crates/apex-cli/src/lib.rs:1274` (ratchet)
- Modify: `crates/apex-cli/src/doctor.rs:386` (doctor)

`std::process::exit()` bypasses cleanup and makes functions untestable.

- [ ] **Step 1: Fix ratchet**

Change lines 1268-1275 from:
```rust
if pct < min_coverage {
    eprintln!(
        "FAIL: coverage {:.1}% is below minimum {:.1}%",
        pct * 100.0,
        min_coverage * 100.0
    );
    std::process::exit(1);
}
```
To:
```rust
if pct < min_coverage {
    return Err(ApexError::Other(format!(
        "FAIL: coverage {:.1}% is below minimum {:.1}%",
        pct * 100.0,
        min_coverage * 100.0
    )));
}
```

- [ ] **Step 2: Fix doctor**

In `doctor.rs`, replace `std::process::exit(1)` with returning an error. Read the function to find the exact pattern and replace accordingly.

- [ ] **Step 3: Update main.rs exit code handling**

In `main.rs`, the `run_cli` result is already converted to an exit code by tokio::main. If specific exit codes are needed (e.g., ratchet fail = exit 1), handle in main:

```rust
if let Err(e) = apex_cli::run_cli(cli, &cfg).await {
    eprintln!("{e}");
    std::process::exit(1);
}
```

- [ ] **Step 4: Re-enable doctor test in subcommand_tests**

Remove the `// NOTE: doctor test omitted` comment and add a proper test.

- [ ] **Step 5: Run tests**

Run: `cargo test -p apex-cli`
Expected: PASS (including newly enabled doctor test)

- [ ] **Step 6: Commit**

```bash
git add crates/apex-cli/
git commit -m "fix: replace std::process::exit with Result returns in ratchet/doctor"
```

---

### Task 12: Fix ratchet missing install_deps

**Crew:** platform
**Files:**
- Modify: `crates/apex-cli/src/lib.rs:1251-1279`

`ratchet()` jumps straight to `instrument()` without installing deps first.

- [ ] **Step 1: Add install_deps to ratchet**

Add before the instrument call:

```rust
if !args.no_install {
    install_deps(lang, &target_path).await?;
}
```

Follow the same pattern as `run()` at line 558.

- [ ] **Step 2: Run tests**

Run: `cargo test -p apex-cli`
Expected: PASS

- [ ] **Step 3: Commit**

```bash
git add crates/apex-cli/src/lib.rs
git commit -m "fix: ratchet installs deps before instrumentation"
```

---

### Task 13: Fix run_lint ignoring config

**Crew:** platform
**Files:**
- Modify: `crates/apex-cli/src/lib.rs` (run_lint function)

`run_lint` uses `DetectConfig::default()` and `ThreatModelConfig::default()` instead of values from `apex.toml`.

- [ ] **Step 1: Fix run_lint to use cfg**

Change `DetectConfig::default()` to use `cfg.detect` values (same pattern as `run_audit`).
Change `ThreatModelConfig::default()` to `cfg.threat_model.clone()`.

- [ ] **Step 2: Run tests**

Run: `cargo test -p apex-cli`
Expected: PASS

- [ ] **Step 3: Commit**

```bash
git add crates/apex-cli/src/lib.rs
git commit -m "fix: run_lint respects apex.toml detect and threat_model config"
```

---

## Dispatch Plan

```
Wave 1 (sequential — others depend on foundation):
  └── Foundation crew: Tasks 1, 2, 3

Wave 2 (parallel — independent crews):
  ├── Security crew: Task 4
  ├── Exploration crew: Tasks 5, 6
  └── Runtime crew: Tasks 7, 8

Wave 3 (parallel — after Wave 1):
  ├── Intelligence crew: Tasks 9, 10
  └── Platform crew: Tasks 11, 12, 13
```

**Execution:** Each wave dispatches fleet crew agents with `isolation: "worktree"`. Within a wave, crews run in parallel. After each wave, merge worktree changes to main and verify `cargo check --workspace` before proceeding.

---

## Summary

| Task | Crew | Severity | Bug Fixed |
|------|------|----------|-----------|
| 1 | Foundation | Critical | Non-deterministic bitmap mapping |
| 2 | Foundation | Important | Orphaned child processes on timeout |
| 3 | Foundation | Critical | Negative branch_distance scores |
| 4 | Security | Critical | Substring matching in taint rules |
| 5 | Exploration | Critical | CmpLogMutator panic on different-length operands |
| 6 | Exploration | Critical | entries_for drops wrapped VecDeque data |
| 7 | Runtime | Critical | fnv1a_hash duplicated in 6 files |
| 8 | Runtime | Critical | SanCov guard index 0 disagreement |
| 9 | Intelligence | Critical | Operator precedence misroutes branches |
| 10 | Intelligence | Critical | observe() errors silently dropped |
| 11 | Platform | Critical | std::process::exit in library code |
| 12 | Platform | Important | ratchet missing install_deps |
| 13 | Platform | Important | run_lint ignores apex.toml config |
