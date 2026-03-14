---
date: 2026-03-14
crew: exploration
affected_partners: [runtime]
severity: minor
acknowledged_by: []
---

## Mutex poison risk in PythonConcolicStrategy.get_trace()

In `crates/apex-concolic/src/python.rs:143`, `trace_cache.lock().unwrap()` will panic if the mutex is poisoned. If a prior panic occurs while holding this lock (e.g., during `run_tracer`), any subsequent call to `get_trace()` or `suggest_inputs()` will bring down the orchestrator thread.

**Fix:** Use `.lock().map_err()` like `FuzzStrategy` does in `apex-fuzz/src/lib.rs`.

### Full findings from exploration review

| Severity | File:Line | Issue |
|----------|-----------|-------|
| Minor | `python.rs:143` | Mutex `.lock().unwrap()` poison risk |
| Minor | `thompson.rs:46` | `partial_cmp().unwrap()` on NaN-capable f64 |
| Info | `js_conditions.rs:418` | Safe but fragile `best.unwrap()` pattern |
| Info | `lib.rs:50,198` / `solver.rs:9` / `python.rs:69` | Dead code annotations on unwired structs |

229 tests pass, 0 failures. Clippy clean.
