<!-- status: ACTIVE -->

# Fail-Safe & Lock-Free Detectors — 10 New Detectors

Date: 2026-03-19
Research: `docs/superpowers/specs/2026-03-19-concurrency-detection-research.md`

## Detector Inventory

### P1 — High Impact (4 detectors)

| # | Name | Pattern | CWE | Languages |
|---|------|---------|-----|-----------|
| 1 | `mutex-across-await` | `Mutex::lock()` guard alive past `.await` | 833 | Rust |
| 2 | `open-without-with` | `open()` not in `with` context manager | 775 | Python |
| 3 | `unbounded-queue` | `VecDeque`/`mpsc::channel()` without capacity | 400 | Rust, Python, Go |
| 4 | `ffi-panic` | `panic!/unwrap/expect` inside `extern "C" fn` | 248 | Rust |

### P2 — Medium Impact (3 detectors)

| # | Name | Pattern | CWE | Languages |
|---|------|---------|-----|-----------|
| 5 | `relaxed-atomics` | `Ordering::Relaxed` on shared concurrent state | 362 | Rust |
| 6 | `missing-async-timeout` | Async I/O without timeout wrapper | 400 | Rust, Python, JS |
| 7 | `zombie-subprocess` | `Command::output()` in timeout without kill | 772 | Rust, Python |

### P3 — Lower Impact (3 detectors)

| # | Name | Pattern | CWE | Languages |
|---|------|---------|-----|-----------|
| 8 | `missing-shutdown-handler` | `#[tokio::main]` without signal handling | 772 | Rust |
| 9 | `connection-in-loop` | DB connect() inside loop/handler | 400 | Python, JS, Java |
| 10 | `poisoned-mutex-recovery` | `unwrap_or_else(e.into_inner())` on Mutex | 362 | Rust |

## Bugs Found in APEX Itself (12)

| Severity | File | Bug |
|----------|------|-----|
| WRONG 92% | oracle.rs:106 | Auto-covered branches invisible to merge_bitmap |
| LEAK 97% | main.rs | No SIGTERM/SIGINT → SHM + zombie leaks |
| LEAK 95% | python.py:217 | Subprocess not killed on timeout → zombies |
| LEAK 95% | coordinator.rs:33 | Unbounded seed queue → OOM |
| CRASH 88% | sancov_rt.rs:54 | Null deref in unsafe extern C |
| WRONG 85% | orchestrator.rs:210 | Poisoned mutex silently recovered |
| WRONG 85% | driller.rs:75 | std::sync::Mutex held across blocking solver |
| WRONG 85% | python.py:244 | coverage json step has no timeout |
| WRONG 85% | sancov_rt.rs:77 | Relaxed atomics on ARM data race |
| WRONG 82% | oracle.rs:116 | Relaxed ordering → stale coverage on ARM |
| WRONG 80% | coordinator.rs:108 | SeedId per batch not per seed |
| LEAK 88% | coordinator.rs:184 | gRPC handle no cancellation |

## Implementation Plan

### Wave 1 — P1 detectors (parallel)
- [ ] 1. mutex-across-await (security-detect crew)
- [ ] 2. open-without-with (security-detect crew)
- [ ] 3. unbounded-queue (security-detect crew)
- [ ] 4. ffi-panic (security-detect crew)

### Wave 2 — P2 detectors
- [ ] 5. relaxed-atomics
- [ ] 6. missing-async-timeout
- [ ] 7. zombie-subprocess

### Wave 3 — P3 detectors
- [ ] 8-10. remaining three

### Wave 4 — Fix APEX bugs found by own detectors (dogfood)
- [ ] Fix the 12 bugs listed above using the new detectors to validate
