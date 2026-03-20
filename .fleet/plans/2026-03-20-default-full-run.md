<!-- status: ACTIVE -->

# Default Full Run — `apex` should run ALL phases by default

**Goal:** When a user runs `apex --target . --lang python`, it should run the full cycle: preflight → install → instrument → coverage → explore → detect → analyze → report. Currently `apex run` and `apex audit` are separate commands. Users should get everything by default.

**Architecture:** Merge `run` and `audit` into a unified pipeline. The CLI gets a new default command that runs all phases. Individual phases available via `apex audit` (detect only) and `apex run --no-detect` (coverage only).

---

## Wave 1: Platform Crew — CLI Unification

### Task 1.1: Add `apex analyze` unified command
**Crew:** platform
**Files:** `crates/apex-cli/src/lib.rs`

The new default command:
1. Preflight check (already wired)
2. Install deps
3. Instrument + coverage baseline
4. Exploration (if coverage < target)
5. Detect (security audit)
6. Compound analyzers
7. Unified report combining coverage + findings + analyzers

If instrumentation fails (missing toolchain), gracefully fall back to audit-only mode with a warning.

### Task 1.2: Make `apex` (no subcommand) run the unified pipeline
**Crew:** platform
**Files:** `crates/apex-cli/src/lib.rs`

When user runs `apex --target . --lang python` without a subcommand, default to the unified pipeline instead of showing help.

### Task 1.3: Unified report format
**Crew:** platform
**Files:** `crates/apex-cli/src/lib.rs`

Combine the `run` output (coverage, uncovered branches) with `audit` output (findings by severity) into a single dashboard:
```
APEX Analysis — myproject (Python)

  Preflight:    build=uv, test=pytest, 24 source files
  Coverage:     97.7% (129/132 branches)
  Findings:     61 (2 high, 48 medium, 1 low)
  Analyzers:    5/5 OK

  Uncovered:
    src/zettel/__init__.py:55  if summarizer is not None
    src/zettel/embedder.py:47  if self._client is None

  Top Findings:
    HIGH  src/store.py:180  SQL injection — cursor.execute with string concat
    HIGH  src/store.py:315  SQL injection — cursor.execute with string concat

  Analyzers:
    OK  service-map (1ms)
    OK  secret-scan (0ms)
    OK  data-flow (0ms)
```

## Wave 2: Runtime Crew — Graceful Fallback

### Task 2.1: Instrument with fallback to audit
**Crew:** runtime
**Files:** `crates/apex-instrument/src/*.rs`, `crates/apex-cli/src/lib.rs`

When `instrument()` fails (missing cargo-llvm-cov, missing go, etc.), instead of erroring:
1. Log warning: "Coverage instrumentation unavailable — running audit only"
2. Skip the exploration phase
3. Continue with detect + analyze
4. Report coverage as "N/A" instead of failing

### Task 2.2: Exclude apex-rpc from coverage builds
**Crew:** runtime
**Files:** `crates/apex-instrument/src/rust_cov.rs`

When running `cargo llvm-cov` on a Rust workspace, exclude crates that can't compile (e.g., apex-rpc needs protoc). Use `--exclude` flag or workspace-level filtering.

## Wave 3: Verification

### Task 3.1: Test unified pipeline on APEX itself
Run `apex --target . --lang rust` and verify:
- Preflight shows build=cargo, test=cargo-test
- Coverage shows 93%+
- Findings shows 1000+
- Analyzers shows 7/7
- Single unified output

### Task 3.2: Test on zettel
Run `apex --target ~/prj/zettel --lang python` and verify full pipeline.
