---
date: 2026-03-15
crew: platform
severity: mixed
acknowledged_by: []
---

## Platform Crew Review — apex-cli, apex-reach

### Crash

1. **`apex-cli/src/main.rs:13`** — `unwrap_or_default()` on `current_dir()` substitutes empty path on permission errors.
2. **`apex-cli/src/lib.rs:3338,3426,3796`** — Three subcommands call `std::process::exit(1)`, bypassing Tokio shutdown.

### Wrong-result

3. **`apex-cli/src/lib.rs:1028`** — Integer division before multiplication makes `deadline_secs` zero for `process_timeout_ms < 1000`.
4. **`apex-reach/src/extractors/mod.rs:32`** — Swift and CSharp extractors implemented but never dispatched.
5. **`apex-cli/src/lib.rs:3089`** — `run_reach` builds call graph from parent dir of target, not repo root.
6. **`apex-cli/src/lib.rs:3011`** — `apex features` without `--lang` omits Go, Cpp, Swift, CSharp from table.
7. **`apex-reach/src/extractors/python.rs`** — `_private` functions in `__init__.py` marked as `PublicApi`.
8. **`apex-reach/src/extractors/python.rs`** — Every function marked `Main` if file contains `if __name__` anywhere.
9. **`apex-reach/src/graph.rs`** — `fn_at` path lookup has no normalization; relative vs absolute paths fail to match.

### Style

10. **`apex-reach/src/graph.rs`** — `CallGraph::node()` is O(n) linear scan in BFS hot path.
