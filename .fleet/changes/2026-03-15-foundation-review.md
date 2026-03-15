---
date: 2026-03-15
crew: foundation
severity: mixed
acknowledged_by: []
---

## Foundation Crew Review — apex-core, apex-coverage, apex-mir

### Wrong-result

1. **`apex-coverage/src/semantic.rs:25`** — Regex recompiled on every `extract_signals` call; should use `OnceLock`.
2. **`apex-coverage/src/mutation.rs:71`** — `metamorphic_adequacy([])` returns 1.0/1.0 — "no mutants" masquerades as perfect score.
3. **`apex-core/src/agent_report.rs:258`** — `bang_for_buck` divides by global branch total, not per-file total — contradicts doc comment.
4. **`apex-core/src/agent_report.rs:262`** — `file_cov` has the same wrong denominator — per-file coverage percentages are meaningless.
5. **`apex-mir/src/extract.rs` flush paths** — Truncated-function flush always inserts `Terminator::Return` even when the actual terminator is known.
6. **`apex-coverage/src/mutation.rs:119`** — `MutationRunner::run_mutant` is a line-coverage proxy; hardcoded `detection_margin: 0.8` inflates all `MetamorphicScore` results.

### Silent-corruption

7. **`apex-mir/src/extract.rs:43-45`** — Brace counting counts `{`/`}` in string literals and comments, causing early function close.

### Style

8. **`apex-core/src/agent_report.rs:244`** — Signed-to-unsigned cast at `branch.line == 0` wraps to `u32::MAX`, silently dropping context lines.
9. **`apex-coverage/src/oracle.rs:94-101`** — `Mutex` held across `DashMap` entry operation — fragile lock ordering.
10. **`apex-core/src/git.rs:85`** — Unnecessary `secs` alias in `epoch_to_date` (named "secs" but actually days).
