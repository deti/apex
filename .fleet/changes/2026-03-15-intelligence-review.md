---
date: 2026-03-15
crew: intelligence
severity: mixed
acknowledged_by: []
---

# Intelligence Crew Review — 2026-03-15

Crates reviewed: `apex-agent`, `apex-synth`, `apex-rpc`.

---

## Findings

### 1. `apex-agent/src/orchestrator.rs:152` — wrong-result

**Stall counter is not reset when sandbox runs produce zero new seeds (strategies fail silently)**

In the main loop, when all strategies return empty suggestions, `stall_count` is incremented
(`stall_count += 1`). That is correct. But when strategies do return seeds and all sandbox runs
are filtered out by `filter_map(|r| r.ok())` (i.e. every sandbox run returns `Err`), the code
falls into the `else` branch and then sets `stall_count = if new_coverage { 0 } else { stall_count + 1 }`.
This means a run where the sandbox silently fails every seed (all `Err`) increments `stall_count`
correctly — however the `stall_threshold` check at line 155 uses the same counter, so a wave of
transient sandbox errors (e.g. I/O failures) can prematurely terminate exploration as if it were
truly stalled.

More concretely: if the sandbox is momentarily unavailable for 10 iterations, the loop exits with
a "stalled" warning even though no actual coverage opportunity was tested. There is no distinction
between "strategies produce seeds but sandbox errors" and "genuine stall".

**Suggested fix:** Track sandbox error counts separately and only start stall counting when at
least one sandbox execution actually ran successfully with no new coverage. Alternatively, skip
stall counting when `results.is_empty()` due to all sandbox errors rather than genuinely no seeds.

---

### 2. `apex-agent/src/orchestrator.rs:112–113` — silent-corruption

**Strategy errors are silently swallowed, masking bugs in strategy implementations**

```rust
.filter_map(|r| r.ok())
```

When a strategy's `suggest_inputs` returns `Err`, the error is silently dropped. For the fuzzer
this is acceptable, but for the driller strategy (which propagates mutex-poison errors) or LLM
strategies, silent discard hides real failures that should be surfaced as warnings.

**Suggested fix:** Replace `filter_map(|r| r.ok())` with a map that logs at `warn!` level before
discarding: `filter_map(|r| r.map_err(|e| warn!("strategy error: {e}")).ok())`.

---

### 3. `apex-agent/src/orchestrator.rs:120–126` — silent-corruption

**Sandbox errors are silently swallowed (same pattern as #2)**

```rust
.filter_map(|r| r.ok())
```

Sandbox `Err` returns are discarded without logging. A sandbox implementation bug or persistent
I/O failure would be invisible in the logs.

**Suggested fix:** Same as #2 — log before discarding.

---

### 4. `apex-agent/src/rotation.rs:31` — crash

**`RotationPolicy::rotate()` panics (via integer overflow) if `strategies` is empty**

```rust
pub fn rotate(&mut self) {
    self.current_index = (self.current_index + 1) % self.strategies.len();
}
```

If `strategies` is empty, `self.strategies.len()` is 0 and `% 0` causes a panic (division by
zero). The companion `current()` method also panics for the same reason via direct index access
(`&self.strategies[self.current_index]`).

The test suite only exercises the single-strategy wrap case but never an empty strategy list.
An empty `RotationPolicy` can be constructed via `RotationPolicy::new(vec![])` and then
`rotate()` or `current()` will panic.

**Suggested fix:** Guard with an early return in both `rotate()` and `current()`, or add a
constructor invariant that rejects empty strategy lists.

---

### 5. `apex-agent/src/bandit.rs:52–53` — crash

**`StrategyBandit::select()` panics on an empty arms list via `.unwrap()`**

```rust
self.arms
    .iter()
    .max_by(|a, b| { ... })
    .map(|s| s.as_str())
    .unwrap_or("")          // <- this returns "" — ok
```

Actually `unwrap_or("")` is safe (returns `""` for empty), but the `partial_cmp` inside `max_by`
calls `.unwrap()` on the comparison result:

```rust
sa.partial_cmp(&sb).unwrap()
```

If either `sa` or `sb` is `NaN` (which `Beta::new` can produce for degenerate alpha/beta
values), `partial_cmp` returns `None` and `.unwrap()` panics. `Beta::new(a, b)` can produce a
valid distribution for `a, b > 0` but the fallback path in `sample_arm` uses
`Beta::new(a, b).ok().map(|d| d.sample(rng)).unwrap_or(0.0)` which returns `0.0` for bad params
— yet the `reward` function accumulates `value` without clamping, so a caller passing
`reward("strategy", f64::NAN)` poisons `alpha`, making future `Beta::new(NaN, 1.0)` fail and
return `0.0`, which is then compared with `partial_cmp` — that is safe. However if `Beta`
sampling itself returns `NaN` (possible with some RNG states and distribution shapes), the
`partial_cmp(...).unwrap()` panics.

**Suggested fix:** Replace `.unwrap()` with `.unwrap_or(std::cmp::Ordering::Equal)`.

---

### 6. `apex-agent/src/source.rs:42` — crash

**`extract_source_contexts` panics on a branch with `line == 0`**

```rust
let min_line = lines[0].saturating_sub(WINDOW).max(1);
```

`lines` is already sorted and `lines[0]` is the minimum line number. If a `BranchId` has
`line == 0` (which is structurally possible — `BranchId::new` accepts `u32` with no lower-bound
check), then `lines[0] == 0`, `saturating_sub(15) == 0`, `.max(1) == 1`, which is safe for
`min_line`. However the slice expression on line 44:

```rust
let slice = source_lines[(min_line - 1) as usize..max_line as usize].to_vec();
```

`min_line - 1` with `min_line == 1` gives `0`, which is valid. So this path is actually safe.

However, if `lines` is empty after grouping (which cannot happen since `by_file` is only
populated from the iterator), this would panic. The real risk is at the level of `lines[0]`
direct indexing — if the Vec were ever empty it would panic. The `by_file` construction
guarantees non-empty vecs, so this is currently safe but fragile. There is no assertion or
comment documenting this invariant.

**Severity downgraded to style** — add a comment or assert that `lines` is non-empty when
indexing.

---

### 7. `apex-agent/src/driller.rs:80–93` — wrong-result

**Solver errors inside the frontier loop are silently ignored, producing incomplete results without any signal**

```rust
if let Ok(Some(seed)) = solver.solve(&prefix, true) {
    inputs.push(InputSeed::new(seed.data.to_vec(), SeedOrigin::Symbolic));
}
```

When `solver.solve` returns `Err`, the error is silently swallowed via the `if let` pattern.
This is intentional (as the test `suggest_inputs_solver_error_skipped` documents), but it
means the orchestrator has no way to know whether the Driller produced 0 seeds because all
branches are already covered, or because the solver consistently failed. Persistent solver
failure (e.g. Z3 OOM) looks identical to "no solvable branches".

**Suggested fix:** Count solver errors and return them as a metric, or emit a `warn!` per failure
so operators can detect a broken solver configuration.

---

### 8. `apex-synth/src/prompt_registry.rs:74–77` — wrong-result

**Template variable substitution iterates over a `HashMap` in arbitrary order, so variable expansion is non-deterministic when a value contains the pattern of another variable**

```rust
for (key, value) in vars {
    result = result.replace(&format!("{{{{ {key} }}}}"), value);
}
```

If variable `"file"` has value `"{{ lines }}"`, expanding `file` first would introduce a new
`{{ lines }}` placeholder that the subsequent `lines` substitution would expand. Iteration over
`HashMap` is arbitrarily ordered, so behavior is non-deterministic and depends on hash
randomization. In practice this is unlikely to be triggered by the default templates, but any
user-supplied variable value containing a `{{ … }}` pattern can cause incorrect prompt
generation.

**Suggested fix:** Use a single-pass regex replacement (e.g. with the `aho-corasick` crate) or
sort the variable map before substitution to make the order deterministic, and document that
variable values must not contain `{{ }}` patterns.

---

### 9. `apex-synth/src/eliminate.rs:20` — wrong-result

**`eliminate_irrelevant` computes indentation using `line.len() - line.trim_start().len()`, which counts bytes not characters, producing wrong results for non-ASCII source**

```rust
let cur_indent = line.len() - line.trim_start().len();
```

For source files containing multi-byte UTF-8 characters in leading whitespace (e.g. a file with
U+2003 EM SPACE used as indentation), `len()` counts bytes but `trim_start()` operates on
Unicode scalar values. A 3-byte UTF-8 space character would contribute 3 to `line.len()` but
only 1 to the difference in character count, causing the indent comparison to be wrong and
potentially including or excluding lines incorrectly.

The same issue exists in `extract_function_body` at line 55.

**Suggested fix:** Use `line.chars().take_while(|c| c.is_whitespace()).count()` for indent
measurement, or restrict to ASCII-only whitespace with a comment.

---

### 10. `apex-synth/src/error_classify.rs:26` — wrong-result

**Operator precedence bug in `classify_test_error` — `&&` binds tighter than `||`**

```rust
} else if lower.contains("assertionerror") || lower.contains("assert") && lower.contains("fail")
```

Due to Rust's operator precedence (`&&` before `||`), this parses as:

```rust
lower.contains("assertionerror") || (lower.contains("assert") && lower.contains("fail"))
```

This is actually the intended logic (classify as `Assertion` if "assertionerror" appears OR if
both "assert" and "fail" appear). However it is a common source of confusion and subtle bugs if
the conditions are later modified. The lack of explicit parentheses is a readability and
maintenance hazard.

**Severity: style** — but worth explicit parenthesisation for clarity:
```rust
lower.contains("assertionerror") || (lower.contains("assert") && lower.contains("fail"))
```

---

### 11. `apex-rpc/src/coordinator.rs:103` — wrong-result

**`submit_results` creates a new `SeedId::new()` per result and discards the client-supplied `seed_id` string from the proto**

```rust
for result in &batch.results {
    let seed_id = SeedId::new();   // fresh ID, ignores result.seed_id
    for pb_branch in &result.new_branches {
        let branch = proto_to_core_branch(pb_branch);
        if self.oracle.mark_covered(&branch, seed_id) {
```

The `ResultBatch` proto carries `seed_id: String` (set by the worker) for attribution and
deduplication, but the coordinator silently replaces it with a fresh `SeedId`. This means the
coverage oracle associates coverage with random IDs, not the actual seed that triggered it.
Downstream bug reporting (`BugLedger`) and any seed-level attribution in reports will be
incorrect.

**Suggested fix:** Parse `result.seed_id` as a `SeedId` (or derive one from it via a hash) and
use it in `mark_covered`, so attribution flows correctly from worker to coordinator.

---

### 12. `apex-rpc/src/worker.rs:131–133` — wrong-result

**`pull_once` returns `(0, 0.0)` when `results.is_empty()` without calling `submit_results`, but this is indistinguishable from a successful submit that produced no coverage**

```rust
if results.is_empty() {
    return Ok((0, 0.0));
}
```

When all seeds are skipped by the `execute` callback (returning `None`), `pull_once` returns
`(0, 0.0)`. The caller cannot distinguish this "all skipped" case from a true submit that
covered 0 branches. If the caller uses the coverage percentage to decide whether to continue
pulling, it may stall because it sees perpetual `0%` growth even though there are seeds in the
queue.

**Suggested fix:** Return a typed result (`enum PullOutcome { AllSkipped, Submitted(u64, f64) }`)
or document clearly that `(0, 0.0)` can mean either "nothing to process" or "processed but no new coverage".

---

### 13. `apex-agent/src/monitor.rs` — wrong-result (off-by-one in growth_rate formula)

**`growth_rate()` divides by `window.len()` rather than `window.len() - 1`**

```rust
(newest as f64 - oldest as f64) / self.window.len() as f64
```

A classic rate formula over N samples should divide by `N - 1` (the number of intervals) not
`N` (the number of samples). With a window of 2 samples at values 10 and 30, the rate is
reported as `(30-10)/2 = 10.0` when the actual per-iteration growth is `20.0`. The test at
line 186 asserts `10.0`, confirming the formula is consistent but the semantic meaning is
incorrect: it underestimates growth rate by a factor proportional to window size.

This causes `MonitorAction` to escalate to `SwitchStrategy` / `AgentCycle` / `Stop` more
aggressively than intended (the stall thresholds are computed relative to `window_size`, not
growth rate, so this doesn't affect the action directly — but any caller using `growth_rate()`
for decisions gets a wrong value).

**Severity: wrong-result if `growth_rate()` is used as a signal externally.** Internally the
`action()` method does not use `growth_rate()`, so the escalation logic itself is unaffected.

**Suggested fix:** Divide by `(self.window.len() - 1).max(1)`.

---

### 14. `apex-agent/src/budget.rs:34` — wrong-result

**`set_minimum_share` clamp upper bound is computed from `num_strategies` at call time, but `num_strategies` could be 0**

```rust
pub fn set_minimum_share(&mut self, share: f64) {
    self.minimum_share = share.clamp(0.0, 1.0 / self.num_strategies as f64);
}
```

If `num_strategies == 0`, this computes `1.0 / 0u64 as f64 = f64::INFINITY`, and
`share.clamp(0.0, f64::INFINITY)` is `share` (no upper bound). A `BudgetAllocator::new(budget, 0)`
followed by `allocate()` will then divide by zero at line 44:

```rust
let per = self.total_budget / n as u64;   // n=0 → panic
```

**Suggested fix:** Guard against `num_strategies == 0` in `new()` or `allocate()`.

---

### 15. `apex-synth/src/few_shot.rs:28–29` — style (O(n) eviction)

**`FewShotBank::add_example` uses `Vec::remove(0)` to evict oldest entry, which is O(n)**

```rust
if self.examples.len() >= self.capacity {
    self.examples.remove(0);
}
```

For a bank with capacity in the hundreds, this shifts every element on each eviction. For the
current small capacities this is acceptable, but at capacity 100+ with high synthesis throughput
it becomes a hot path.

**Suggested fix:** Use `VecDeque` with `pop_front()` / `push_back()` for O(1) eviction.

---

## Summary Table

| # | File | Line | Severity | Issue |
|---|------|------|----------|-------|
| 1 | `apex-agent/src/orchestrator.rs` | 117–152 | wrong-result | Sandbox errors increment stall counter, triggering premature exploration stop |
| 2 | `apex-agent/src/orchestrator.rs` | 112–113 | silent-corruption | Strategy errors silently dropped without logging |
| 3 | `apex-agent/src/orchestrator.rs` | 120–126 | silent-corruption | Sandbox errors silently dropped without logging |
| 4 | `apex-agent/src/rotation.rs` | 31 | crash | `rotate()` panics with division by zero on empty strategy list |
| 5 | `apex-agent/src/bandit.rs` | 53 | crash | `partial_cmp(...).unwrap()` panics if Beta distribution samples NaN |
| 6 | `apex-agent/src/source.rs` | 42 | style | Fragile direct index on `lines[0]`; non-empty invariant is implicit |
| 7 | `apex-agent/src/driller.rs` | 88 | wrong-result | Solver errors silently swallowed; persistent solver failure is invisible |
| 8 | `apex-synth/src/prompt_registry.rs` | 74–77 | wrong-result | HashMap iteration order makes variable substitution non-deterministic |
| 9 | `apex-synth/src/eliminate.rs` | 20, 55 | wrong-result | Byte-length used for indentation measurement; wrong for non-ASCII source |
| 10 | `apex-synth/src/error_classify.rs` | 26 | style | Implicit operator precedence; should use explicit parentheses |
| 11 | `apex-rpc/src/coordinator.rs` | 103 | wrong-result | Client `seed_id` replaced with fresh ID; breaks attribution |
| 12 | `apex-rpc/src/worker.rs` | 131–133 | wrong-result | `pull_once` returns `(0, 0.0)` ambiguously for both "all skipped" and "no coverage" |
| 13 | `apex-agent/src/monitor.rs` | 46–49 | wrong-result | `growth_rate()` divides by N instead of N-1; underestimates rate |
| 14 | `apex-agent/src/budget.rs` | 34, 44 | wrong-result | Division by zero if `num_strategies == 0` |
| 15 | `apex-synth/src/few_shot.rs` | 28–29 | style | O(n) eviction with `Vec::remove(0)`; should use `VecDeque` |
