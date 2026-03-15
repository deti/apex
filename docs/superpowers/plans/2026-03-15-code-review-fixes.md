<!-- status: ACTIVE -->

# Code Review Fixes — Full Codebase Audit

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Fix all 34 findings from the 4-crew code review audit. Organized into 5 parallel waves by crate group.

**Architecture:** Each wave targets a disjoint set of files — fully parallelizable. Fixes are ordered HIGH→MEDIUM→LOW within each wave.

---

## Wave A: apex-cpg (4 findings)

### A1: SSA `find_assignments` ignores method scope [HIGH bug]
**File:** `crates/apex-cpg/src/ssa.rs:320`
- [ ] Filter `find_assignments` to only nodes reachable from `method_node` via AST edges
- [ ] Remove the underscore from `_method_node` parameter
- [ ] Add test: multi-function CPG produces correct per-function SSA

### A2: Taint summary single `found_sanitizer` shared across BFS paths [HIGH security]
**File:** `crates/apex-cpg/src/taint_summary.rs:122`
- [ ] Carry sanitization state per-path in BFS queue: `(NodeId, bool)` tuple
- [ ] Each `TaintFlow` gets its own `sanitized` flag from its path
- [ ] Add test: two paths to return, one sanitized one not — verify both reported correctly

### A3: Query executor `Regex::new().unwrap()` on user input [HIGH bug]
**File:** `crates/apex-cpg/src/query/executor.rs:223`
- [ ] Change cache to `HashMap<String, Option<Regex>>`
- [ ] Return `false` for invalid patterns instead of panicking
- [ ] Add test: malformed regex in query returns empty results, not panic

### A4: Cartesian product unbounded memory [LOW perf]
**File:** `crates/apex-cpg/src/query/executor.rs:136`
- [ ] Add `MAX_ROWS` constant (e.g. 100_000)
- [ ] Break out of product expansion if `rows.len()` exceeds limit
- [ ] Log warning when limit hit

---

## Wave B: apex-detect (4 findings)

### B1: SSRF whole-file sanitization suppresses all findings [HIGH security]
**File:** `crates/apex-detect/src/detectors/ssrf.rs:50`
- [ ] Move sanitization check to per-line context window (5 lines before/after)
- [ ] Only suppress a finding if sanitization appears in the same function scope
- [ ] Add test: file with `urlparse` import + unsanitized SSRF elsewhere still flags

### B2: `crypto_failure` substring match `"DES"` matches "DESCRIBES" [MEDIUM security]
**File:** `crates/apex-detect/src/detectors/crypto_failure.rs:85`
- [ ] Replace `trimmed.contains(pattern)` with regex word-boundary matching `\bDES\b`
- [ ] Apply to all WEAK_CIPHERS and WEAK_HASHES entries
- [ ] Add test: "DESCRIBES" does not trigger, "DES" does

### B3: `broken_access` reports CSRF on GET forms [MEDIUM bug]
**File:** `crates/apex-detect/src/detectors/broken_access.rs:117`
- [ ] Add condition: only flag if `method` value contains `post`/`put`/`delete` (case-insensitive)
- [ ] Add test: `<form method="get">` does NOT trigger, `<form method="post">` does

### B4: `path_traversal` no sanitization check on `Path(var)` [LOW security]
**File:** `crates/apex-detect/src/detectors/path_traversal.rs:41`
- [ ] Add sanitization indicators check (same as SSRF/broken_access patterns)
- [ ] Skip if variable name suggests non-user-input (`self.`, `config.`, `BASE_DIR`)
- [ ] Add test: `Path(user_input)` flags, `Path(self.root)` does not

---

## Wave C: Execution Engine — apex-fuzz, apex-concolic, apex-symbolic, apex-synth (9 findings)

### C1: `MOptScheduler::mutate()` panics on empty [HIGH bug]
**File:** `crates/apex-fuzz/src/scheduler.rs:62`
- [ ] Add early-return guard: `if self.mutators.is_empty() { return input.to_vec(); }`
- [ ] Update the `#[should_panic]` test to assert the new non-panicking behavior

### C2: `InterleavedSearch::select()` panics on empty [HIGH bug]
**File:** `crates/apex-concolic/src/search.rs:182`
- [ ] Add guard: `if self.strategies.is_empty() { return 0; }`

### C3: `report_hit()` yield > 1.0 [HIGH bug]
**File:** `crates/apex-fuzz/src/scheduler.rs:79`
- [ ] Clamp `yield_now` to `1.0`: `let yield_now = (hits as f64 / apps as f64).min(1.0);`
- [ ] Update bug-documenting test assertions

### C4: `report_hit()` with `applications==0` decreases EMA [MEDIUM bug]
**File:** `crates/apex-fuzz/src/scheduler.rs:85`
- [ ] When `applications == 0`, set `yield_now = 1.0` (hit without application = full success)
- [ ] Update bug-documenting test

### C5: `find_word_outside_parens` missing escape tracking [MEDIUM bug]
**File:** `crates/apex-concolic/src/js_conditions.rs:374`
- [ ] Copy backslash-counting logic from `find_operator_outside_parens` (lines 332-344)
- [ ] Add test: `"key\\\\" in obj` correctly finds `in` operator

### C6: `solve_decomposed` empty parts = UNSAT [MEDIUM bug]
**File:** `crates/apex-symbolic/src/path_decomp.rs:68`
- [ ] Add early return: `if parts.is_empty() { return Ok(Some(InputSeed::new(vec![], SeedOrigin::Symbolic))); }`
- [ ] Update bug-documenting test

### C7: `boundary_seeds` generates SyntaxError Python [MEDIUM bug]
**File:** `crates/apex-concolic/src/python.rs:384`
- [ ] Skip seed generation when `assigns` and `call_args` are both empty
- [ ] Add test

### C8: `coverage_scores` unbounded growth [MEDIUM perf]
**File:** `crates/apex-concolic/src/search.rs:122`
- [ ] Replace `Vec<f64>` with `HashMap<usize, f64>` for `coverage_scores`
- [ ] Remove entry in `on_terminate`

### C9: `PropertyInferer` indented methods get NoException [MEDIUM bug]
**File:** `crates/apex-synth/src/property.rs:225`
- [ ] Filter extracted functions through `is_public_function` before generating NoException
- [ ] Update bug-documenting test

---

## Wave D: Infrastructure — apex-instrument, apex-sandbox (7 findings)

### D1: JS instrumentor `format!("--flag=path")` [HIGH security]
**File:** `crates/apex-instrument/src/javascript.rs:71`
- [ ] Split `--reports-dir=path` into two separate Vec elements `["--reports-dir", path]`
- [ ] Apply to all three occurrences (lines 71, 91, 115)

### D2: Go instrumentor path traversal via `../` [HIGH security]
**File:** `crates/apex-instrument/src/go.rs:133`
- [ ] Add `candidate.starts_with(target_root)` guard after `target_root.join(&suffix)`
- [ ] Add test: path with `../` is rejected

### D3: Source map dst-line guard drops valid mappings [HIGH bug]
**File:** `crates/apex-instrument/src/source_map.rs:36`
- [ ] Remove the `get_dst_line() != line_0` guard (rely on `lookup_token` returning None for unmapped)
- [ ] Add test: multi-line mapping range correctly attributed

### D4: `rust_cov.rs` `as u16` silent truncation [HIGH bug]
**File:** `crates/apex-instrument/src/rust_cov.rs:167`
- [ ] Use `.min(u16::MAX as u64) as u16` (clamp, consistent with source_map.rs)
- [ ] Log warning when clamping occurs

### D5: SHM read unsound on ARM [HIGH bug]
**File:** `crates/apex-sandbox/src/shm.rs:91`
- [ ] Add `std::sync::atomic::fence(Ordering::Acquire)` before the `from_raw_parts` read
- [ ] Document the architectural assumption

### D6: Python sandbox bypasses CommandRunner [HIGH security]
**File:** `crates/apex-sandbox/src/python.rs:123`
- [ ] Accept `&dyn CommandRunner` parameter and use it instead of bare `Command::new("python3")`
- [ ] Add test with mock runner

### D7: Hardcoded `/tmp` in rust_cov [MEDIUM bug]
**File:** `crates/apex-instrument/src/rust_cov.rs:250`
- [ ] Replace `/tmp` with `std::env::temp_dir()`

---

## Wave E: Core + CLI + misc (6 findings)

### E1: Deadline integer division truncates to 0 [HIGH bug]
**File:** `crates/apex-cli/src/lib.rs:1032`
- [ ] Change to `cfg.sandbox.process_timeout_ms * fuzz_iters as u64 / 1000`
- [ ] Add test: timeout_ms=500 with iters=10 produces non-zero deadline

### E2: stdin write_all no timeout [HIGH bug]
**File:** `crates/apex-core/src/command.rs:111`
- [ ] Wrap `stdin.write_all` in `tokio::time::timeout(deadline, ...)`
- [ ] Handle timeout as error (kill child, return error)

### E3: RPC port-release TOCTOU race [MEDIUM bug]
**File:** `crates/apex-rpc/src/worker.rs:210`
- [ ] Use `TcpListener::bind("127.0.0.1:0")` and pass the listener to the server
- [ ] If server API doesn't accept listener, use `SO_REUSEADDR`

### E4: `path_shim` shell injection [MEDIUM security]
**File:** `crates/apex-core/src/path_shim.rs:96`
- [ ] Validate `program` is alphanumeric + dash/underscore only
- [ ] Single-quote `log_file` in generated script

### E5: `divergent_runs` always equals `total_runs` [MEDIUM bug]
**File:** `crates/apex-index/src/analysis.rs:85`
- [ ] Compute actual divergent run count from branch_sets
- [ ] Update bug-documenting test

### E6: Python reach — only 1 decorator line checked [LOW bug]
**File:** `crates/apex-reach/src/extractors/python.rs:145`
- [ ] Scan backwards through all contiguous `@` lines above `def`
- [ ] Add test: stacked decorators correctly identify HTTP handler

### E7: Minor cleanups [LOW]
- [ ] `apex-core/src/llm.rs:51` — change `Vec<String>` to `VecDeque<String>`
- [ ] `apex-cli/src/main.rs:14` — replace `unwrap_or_default()` with proper error
- [ ] `apex-lang/src/cpp.rs:204` — remove dead `parse_ctest_summary` or promote to production

---

## Execution

```
Wave A (apex-cpg)          ─┐
Wave B (apex-detect)        ─┤
Wave C (execution engine)   ─┼── all independent, fully parallel
Wave D (infra/sandbox)      ─┤
Wave E (core/cli/misc)      ─┘
```

5 worktree agents, each handles one wave. All waves touch disjoint files.

## Verification

After all waves merge:

```bash
cargo fmt --check
cargo clippy --workspace -- -D warnings
cargo nextest run --workspace
```
