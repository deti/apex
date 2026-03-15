---
date: 2026-03-15
crew: runtime
severity: mixed
acknowledged_by: []
---

## Runtime Crew Review — apex-lang, apex-instrument, apex-sandbox, apex-index

### Crash

1. **`apex-sandbox/src/sancov_rt.rs:33`** — `stop` pointer not null-checked; null `stop` with non-null `start` causes UB.

### Wrong-result

2. **`apex-sandbox/src/shm.rs:84`** — `name_str()` returns `""` on non-UTF-8; child gets empty SHM name, coverage lost.
3. **`apex-instrument/src/v8_coverage.rs:122`** — Parent range never added to its own group; branch direction 0 misattributed.
4. **`apex-instrument/src/source_map.rs:60`** — Branches silently dropped when source map has no token.
5. **`apex-instrument/src/source_map.rs:29`** — `branch.line == 0` and `== 1` both saturate to line 0, causing collision.
6. **`apex-index/src/analysis.rs:88`** — `FlakyTest::divergent_runs` always set to `total_runs`; every flaky test appears maximally flaky.
7. **`apex-sandbox/src/firecracker.rs:518`** — `snapshot()` generates three different SnapshotIds; `restore()` always uses prepare-time snapshot.
8. **`apex-instrument/src/v8_coverage.rs:107`** — Stale offset beyond source length maps to last line, producing duplicate BranchIds.
9. **`apex-sandbox/src/process.rs:187`** — Crash range `128..=159` includes exit code 128 (not a signal death).

### Style

10. **`apex-sandbox/src/shm.rs:88`** — No explicit memory ordering for `read()` after child exit.
