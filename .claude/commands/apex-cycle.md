---
name: apex
description: "APEX unified analysis cycle — discover, index, hunt, detect, analyze, report"
arguments:
  - name: phase
    description: "Phase to run: all (default), hunt, detect, analyze, intel, deploy, configure"
    default: "all"
  - name: target
    description: "Path to target repository"
    default: "."
  - name: lang
    description: "Language: rust, python, js, java"
    default: ""
---

# APEX — Unified Analysis Cycle

You are the APEX orchestrator. You run a structured analysis cycle against a codebase,
producing a unified report. Each phase builds on the previous one's output.

## Phase Model

```
DISCOVER → INDEX → HUNT → DETECT → ANALYZE → INTEL → REPORT
   0         1       2       3         4        5        6
```

Running `/apex` runs all phases. Running `/apex hunt` runs phases 0-2 only.
Running `/apex detect` runs phases 0,1,3 only. Etc.

## Phase 0: DISCOVER (always runs first)

1. **Detect language** from files if not specified:
   - `Cargo.toml` → rust
   - `pyproject.toml` / `setup.py` / `requirements.txt` → python
   - `package.json` → js
   - `pom.xml` / `build.gradle` → java
   - Read `apex.toml` if it exists — `lang` field overrides detection

2. **Check prerequisites:**
   ```bash
   cargo run --bin apex -- doctor 2>&1
   ```

3. **Discover artifacts** (what analyzers will be applicable):
   Scan for: Dockerfiles, .tf files, .env files, OpenAPI specs, SQL migrations,
   JSX/TSX files, i18n files, runbooks, SLO configs, Cargo.toml/package.json

   Report what was found:
   ```
   Discovered: Cargo.toml, 3 Dockerfiles, 2 .env files, openapi.json, 5 migrations
   → 14 analyzers applicable
   ```

4. **Load threat model** from `apex.toml [threat_model]` section if present.
   If not present and phase=all, suggest running `/apex-configure` first (but don't block).

## Phase 1: INDEX (builds the branch index)

Skip if `.apex/index.json` exists and is fresh (source hash matches).

For Rust:
```bash
cargo run --bin apex -- index --target $TARGET --lang $LANG
```

Report: `Index: 234 tests, 1,847 branches, 72.3% covered`

## Phase 2: HUNT (bug-finding rounds)

This is the core APEX value prop. Coverage is the map, bugs are the treasure.

**Only runs when phase is `all` or `hunt`.**

### 2a. Map — Build the targeting package

Get current coverage with line-level precision:
```bash
cargo llvm-cov --json 2>/dev/null > /tmp/apex_cov.json
```

Parse the JSON to extract **per-file uncovered regions** — not just percentages,
but the exact line ranges:
```
src/parser.rs:142-180  — parse_vlq(): negative VLQ values, overflow handling
src/engine.rs:42-67    — process(): empty input path, error recovery
src/auth.rs:89-112     — validate_token(): expired token, malformed JWT
```

For each file with uncovered regions, read the source around those lines
(±10 lines of context) so hunters get the actual code.

### 2b. Enrich — Cross-reference with APEX intelligence

Before dispatching hunters, layer on everything APEX already knows:

1. **Security findings** (from Phase 3 if already run, or run detectors first):
   - Which uncovered regions have security patterns nearby?
   - Example: "lines 89-112 in auth.rs are uncovered AND detector flagged
     `validate_token()` for CWE-287 (improper authentication)"

2. **Complexity hotspots** (from index if available):
   - Which uncovered regions are in high-complexity functions?
   - Example: "parse_vlq() has cyclomatic complexity 14 — high risk of edge case bugs"

3. **Taint flows** (from CPG if Python):
   - Which uncovered regions are on taint paths?
   - Example: "user input flows through `process()` to `db.execute()` — lines 42-67
     are the unsanitized middle segment"

4. **Hot paths** (from index if available):
   - Which uncovered code is on frequently-executed paths?
   - High-frequency + uncovered = highest risk

### 2c. Dispatch hunters — with precision targeting

Each hunter agent receives a **targeting package**, not just a file name:

```
TARGET: src/auth.rs
UNCOVERED REGIONS:
  Lines 89-112: validate_token()
    Code: [actual source lines pasted here]
    Context: Called from handle_request() at line 34
    Security: CWE-287 flagged — improper authentication check
    Complexity: 8 (moderate)
    Taint: user input reaches this via request.headers["Authorization"]

  Lines 118-135: refresh_session()
    Code: [actual source lines pasted here]
    Context: Called from middleware at line 12
    Security: No findings
    Complexity: 4 (low)

CATEGORY FOCUS: safety bugs
INSTRUCTIONS: Write tests targeting these specific uncovered regions.
  Think adversarially about what bugs hide in the uncovered lines.
  Name tests bug_validate_token_*, bug_refresh_session_*.
  Run: cargo test -p auth
```

**What the targeting package contains:**
- Exact uncovered line ranges (from llvm-cov JSON)
- Actual source code for those lines (±10 lines context)
- Call context (who calls this function)
- Security findings that intersect the region (detector output)
- Complexity score (from APEX complexity analysis)
- Taint flows through the region (from CPG, if available)
- Hot path status (frequently executed?)

**What it does NOT contain:**
- The entire file (hunters read what they need, but get pointed to the right spot)
- Raw JSON dumps (pre-parsed into actionable format)
- Other files' data (each hunter gets one file's targeting package)

### 2d. Hunt rounds

For each round (max 5):

1. **Map** — Parse llvm-cov JSON into targeting packages (2a)
2. **Enrich** — Cross-reference with detectors, complexity, taint (2b)
3. **Dispatch** — Send targeting packages to parallel hunter agents (2c)
4. **Collect** — Gather tests written, bugs found
5. **Triage** findings:
   - Crash/panic = immediate fix
   - Wrong result = high priority
   - Silent data loss = medium priority
   - Style/quality = note for later
6. **Fix** — Merge tests that expose real bugs. Write fixes for crashes.
7. **Re-map** — Measure coverage again. Report:
   ```
   Round 1: 2 bugs found, coverage 72.3% → 78.1% (+5.8%)
     CRASH validate_token() panics on malformed JWT — src/auth.rs:95
     WRONG parse_vlq() returns wrong value for negative input — src/parser.rs:156
   ```

**Continue or stop:**
- Stop if: 0 bugs found AND < 2% improvement (stall)
- Stop if: coverage target reached
- Stop if: max rounds reached

**Breakpoint:** If a Crash is found, pause and ask user before continuing.

### Strategy escalation for hard gaps:
- Easy gaps (missing branch) → targeted unit test with exact lines
- Medium gaps (error path) → edge-case test with adversarial input
- Hard gaps (binary decision, no security signal) → suggest `apex run --strategy fuzz`
- Hard gaps (constraint-dependent) → suggest `apex run --strategy driller`
- Gaps with taint flows → prioritize — these are security-relevant untested paths

## Phase 3: DETECT (security pipeline)

Run the full detector pipeline:
```bash
cargo run --bin apex -- audit --target $TARGET --lang $LANG --severity-threshold low --output-format json
```

Parse the JSON output. Present by severity:
```
Security: 0 critical, 2 high, 5 medium, 12 low
  HIGH  src/auth.rs:42 — SQL injection via unsanitized input [CWE-89]
  HIGH  src/api.rs:118 — Command injection in shell call [CWE-78]
```

If ASVS/STRIDE data is in the output, show compliance summary:
```
ASVS L1: 18/21 verified, 2 failed, 1 manual
STRIDE: Spoofing=OK, Tampering=WARN, Repudiation=OK, ...
```

## Phase 4: ANALYZE (compound analysis)

This runs automatically — the compound pipeline discovers artifacts and dispatches.

```bash
# This is now built into `apex run` and `apex audit`, but for standalone:
cargo run --bin apex -- audit --target $TARGET --lang $LANG --output-format json
```

The JSON output now includes `compound_analysis` with analyzer results.

Parse and present:
```
Analyzers (12 applicable, 12 ran):
  service-map        2 HTTP deps, 1 DB connection (15ms)
  dep-graph          47 nodes, 0 cycles (8ms)
  container-scan     3 issues in Dockerfile (12ms)
  config-drift       2 drifted keys between .env.staging/.env.prod (5ms)
  schema-check       1 dangerous migration (DROP COLUMN) (3ms)
  a11y-scan          4 WCAG violations in 12 JSX files (22ms)
  secret-scan        0 secrets found (18ms)
  ...
```

Highlight critical items:
```
WARNING container-scan: Dockerfile runs as root (no USER directive)
WARNING schema-check: migrations/0042.sql has DROP COLUMN (data loss risk)
WARNING config-drift: API_KEY differs between staging and production
```

## Phase 5: INTEL (SDLC intelligence)

Requires index from Phase 1. Run intelligence commands:

```bash
cargo run --bin apex -- test-optimize --target $TARGET --lang $LANG --output-format json
cargo run --bin apex -- dead-code --target $TARGET --lang $LANG --output-format json
cargo run --bin apex -- complexity --target $TARGET --lang $LANG --output-format json
cargo run --bin apex -- hotpaths --target $TARGET --lang $LANG --output-format json
cargo run --bin apex -- deploy-score --target $TARGET --lang $LANG --output-format json
```

If git changes detected (`git diff --name-only HEAD~1`):
```bash
cargo run --bin apex -- risk --target $TARGET --lang $LANG --changed-files $FILES --output-format json
cargo run --bin apex -- test-prioritize --target $TARGET --lang $LANG --changed-files $FILES --output-format json
```

Present:
```
Intelligence:
  Test Suite: 234 tests, 18 redundant (can remove), 3 flaky candidates
  Dead Code: 12 unreachable branches in 4 files
  Hot Paths: src/core/engine.rs:process() — 89% of execution time
  Deploy Score: 74/100
  Risk (3 changed files): MEDIUM — 28 tests affected
```

## Phase 6: REPORT (unified output)

Combine ALL phase outputs into a single dashboard:

```
APEX Analysis Report — myproject

  Coverage:     78.1%  (+5.8% from hunt phase)
  Bugs Found:   3 (2 fixed, 1 noted)
  Security:     2 high, 5 medium, 12 low
  Analyzers:    12/12 OK
  Deploy Score: 74/100 (CAUTION)

  Top Actions:
  1. Fix 2 HIGH security findings (auth.rs, api.rs)
  2. Add USER directive to Dockerfile
  3. Review DROP COLUMN migration before deploy
  4. Resolve API_KEY drift between staging/prod
  5. Remove 18 redundant tests (save 2m CI time)
```

## Phase Mapping for Subcommands

| User types | Phases run |
|-----------|-----------|
| `/apex` | 0 → 1 → 2 → 3 → 4 → 5 → 6 |
| `/apex hunt` | 0 → 1 → 2 → 6 |
| `/apex detect` | 0 → 3 → 4 → 6 |
| `/apex analyze` | 0 → 4 → 6 |
| `/apex intel` | 0 → 1 → 5 → 6 |
| `/apex deploy` | 0 → 1 → 3 → 5 → 6 (deploy verdict) |
| `/apex configure` | Threat model wizard (separate) |

## Important Rules

- **Never skip Phase 0** — discovery is always first
- **Phase 2 (hunt) is the most expensive** — don't run it for `/apex detect` or `/apex intel`
- **All phases share source cache** — scan files once, not per-phase
- **Breakpoints on crashes** — if Phase 2 finds a crash, pause and confirm
- **JSON output** — if `--output-format json` is implied (unlikely in slash command), output full CompoundReport
- **Keep it moving** — don't ask questions mid-cycle. Discover, execute, report.
