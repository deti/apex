# APEX Run — Bug-Finding Agent Loop

Multi-round loop that uses coverage gaps to guide **bug hunting**. Coverage is the map, bugs are the treasure.

## Usage
```
/apex-run [target] [lang] [rounds] [coverage-target]
```
Examples:
- `/apex-run` — run APEX agent loop on current directory
- `/apex-run /tmp/my-project python 5 0.95`
- `/apex-run /tmp/my-c-project c 3 1.0`

## Instructions

Parse `$ARGUMENTS`: target path, language, rounds, coverage target.
Defaults: target=`.`, lang=`rust`, rounds=`5`, coverage_target=`1.0`.

### Environment

```bash
export LLVM_COV=${LLVM_COV:-/opt/homebrew/opt/llvm/bin/llvm-cov}
export LLVM_PROFDATA=${LLVM_PROFDATA:-/opt/homebrew/opt/llvm/bin/llvm-profdata}
```

### What a "Round" Is

A round is NOT "write tests to increase a coverage number." A round is:

1. **Map** the terrain (measure coverage to find unexplored code)
2. **Hunt** for bugs in the unexplored code (parallel agents)
3. **Verify** findings and commit real fixes
4. **Re-map** to see what's still unexplored

The deliverable of each round is a **bug report**, not a coverage delta.

### Agent Loop

For each round (1 to max_rounds):

**Phase 1 — Map.** Measure coverage to identify unexplored code regions.

For Rust projects, run `cargo llvm-cov` directly (APEX's internal runner has sandbox issues):
```bash
cargo llvm-cov --json --output-path /tmp/claude/apex_cov.json 2>&1
```
Parse the JSON to find files with the most uncovered segments. Group by file, rank by uncovered count.

For other languages, run APEX with `--output-format json`:
```bash
cargo run --bin apex --manifest-path $APEX_HOME/Cargo.toml -- \
  run --target <TARGET> --lang <LANG> --output-format json 2>/dev/null
```

**Phase 2 — Hunt.** Dispatch parallel agents (up to 5) targeting the top uncovered files. Each agent is a **bug hunter**, not a coverage chaser.

Each agent prompt MUST include:
- "You are a bug hunter, not a coverage chaser"
- "Write tests that ASSERT SPECIFIC EXPECTED BEHAVIOR"
- "If a test fails, that's a BUG — report it clearly"
- "Name bug-exposing tests with `bug_` prefix"
- The specific file to investigate
- Known struct drift issues (e.g., recently added fields)

Agent categories to dispatch per round:

| Agent | Focus | What it looks for |
|-------|-------|-------------------|
| **Logic bugs** | Parsing code, numeric edge cases | Wrong results, off-by-one, overflow, division by zero |
| **Safety bugs** | `.unwrap()` in non-test code, `[0]` without length check | Panics on user input, index out of bounds |
| **Edge case bugs** | Empty/malformed/unicode input | Crashes on boundary conditions |
| **Correctness bugs** | Business logic, state machines | Violates documented invariants |
| **Concurrency bugs** | Arc/Mutex/async code | Race conditions, deadlocks |

Distribute agent types across top gap files. Don't send 5 agents to write trivial coverage tests.

**Phase 3 — Triage.** When agents complete:
1. Collect all findings into categories: **Crash** (panic), **Wrong Result**, **Silent Data Loss**, **Style**
2. For each real bug found:
   - Merge the test that exposes it
   - Write the fix
   - Verify the fix with the test
3. For tests that pass (no bug found): still merge if they test meaningful behavior, skip if they're just line-covering

**Phase 4 — Re-map.** Run coverage again. Report:
- Bugs found this round (the primary metric)
- Coverage delta (secondary metric)
- Remaining high-risk uncovered areas

**Phase 5 — Decide.** Continue if:
- There are still high-risk uncovered areas (parsing, error handling, user input paths)
- Stop if remaining gaps are only in test code, infra code (firecracker, gRPC), or generated code

### Strategy Selection Guide

| Target type | Primary strategy | Fallback |
|-------------|-----------------|----------|
| Rust workspace | Source-level tests | fuzz (if binary harness exists) |
| Python project | Source-level tests | concolic (for constraint paths) |
| C/Rust binary | fuzz | driller (when fuzz stalls) |
| JavaScript | Source-level tests | — |

### Round Report Format

```
## Round N — Bugs: X found, Y fixed | Coverage: A% → B%

### Bugs Found
🔴 CRASH: `parse_coverage_json` panics on empty input (cvss.rs:142)
🟡 WRONG: `compute_deploy_score` returns 105 when quality_ratio > 1.0 (analysis.rs:340)
🟢 FIXED: Added bounds check, clamped to 0..=100

### Coverage (secondary)
[████████████████████████████████████████████████░░] 94.7%
+312 regions covered | 10,903 remaining

### High-Risk Uncovered (next round targets)
  parsing: rust_cov.rs (LLVM JSON parser), javascript.rs (Istanbul parser)
  user-input: cli/main.rs (argument parsing), config.rs (TOML parsing)
  error-paths: pipeline.rs (detector failures), orchestrator.rs (strategy errors)
```

### Breakpoints

- **Bug found**: Pause to fix it before continuing. Bugs are the deliverable.
- **Stall**: 0 bugs found AND 0% coverage improvement → re-examine strategy
- **Compile failure**: auto-retry once, then pause
- **All high-risk code covered**: declare victory, stop

If the run fails, diagnose the error and suggest a fix.
