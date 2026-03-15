---
date: 2026-03-15
crew: exploration
severity: mixed
acknowledged_by: []
---

## Exploration Crew Review — apex-fuzz, apex-symbolic, apex-concolic

### Crash

1. **`apex-fuzz/src/de_scheduler.rs:11`** — `DeScheduler::new(0)` computes `1.0/0.0 = INFINITY`; `select()` wraps `weights.len() - 1` on empty vec.
2. **`apex-fuzz/src/scheduler.rs:62`** — `MOptScheduler::mutate()` indexes `stats[0]` on empty scheduler.
3. **`apex-fuzz/src/thompson.rs:38`** — `select()` returns index 0 on empty scheduler.
4. **`apex-concolic/src/search.rs:182-183`** — `InterleavedSearch::select()` panics on empty strategies; `rounds_remaining` underflows when 0.

### Wrong-result

5. **`apex-fuzz/src/scheduler.rs:84`** — `report_hit` increments coverage_hits before computing yield ratio; invisible to EMA when applications == 0.
6. **`apex-fuzz/src/corpus.rs:78`** — `sample_pair()` never increments `fuzz_count`, biasing `Fast` schedule.
7. **`apex-symbolic/src/cache.rs:66`** — `CachingSolver::set_logic()` doesn't flush cache; stale results after logic change.
8. **`apex-symbolic/src/gradient.rs:174`** — `GradientSolver` only considers last constraint; may violate earlier path constraints.
9. **`apex-concolic/src/js_conditions.rs:382`** — `find_word_outside_parens` doesn't handle backslash-escaped string quotes.
10. **`apex-concolic/src/python.rs:342`** — `n + 1` integer overflow in len-check boundary seed generation.

### Silent-corruption

11. **`apex-concolic/src/python.rs:340`** — `parse().unwrap_or(0)` silently produces zero-length seeds.
12. **`apex-concolic/src/python.rs:434`** — Unescaped variable names/values in synthesised Python test stubs.
