# APEX Run — Agent Loop

Multi-round coverage improvement loop. Measures gaps, writes tests, invokes strategies, re-measures.

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

### Fleet Preflight (before first dispatch)

Before dispatching parallel agents, probe the runtime environment:
```bash
./scripts/fleet-preflight.sh <agent_count>
```
Parse the JSON output. Use `dispatch.max_parallel` for concurrent agent count and
`dispatch.wave_sizes` for wave-based dispatch. Include `agent_rules` in each agent prompt:
- `batch_fixes_before_testing: true` — fix ALL bugs, then test once
- `max_cargo_test_invocations: 3` — test → fix failures → test → clippy (not per-fix)

If the script is unavailable, default to `max(1, cpu_cores / 4)` parallel agents.

### Agent Loop

For each round (1 to max_rounds):

**Step 1 — Measure.** Run APEX with `--strategy agent --output-format json`:
```bash
cargo run --bin apex --manifest-path $APEX_HOME/Cargo.toml -- \
  run --target <TARGET> --lang <LANG> --strategy agent \
  --output-format json 2>/dev/null
```
Capture JSON output. Parse `summary`, `gaps`, and `blocked` arrays.

**Step 2 — Analyze.** Sort gaps by `bang_for_buck` descending. For each gap, select strategy.

**Step 3 — Act.**
- For source-level tests: read the `source_context` and `branch_condition` from JSON, write test files.
- For fuzz/driller/concolic: run the appropriate APEX command:
  ```bash
  cargo run --bin apex --manifest-path $APEX_HOME/Cargo.toml -- \
    run --target <TARGET> --lang <LANG> --strategy <fuzz|driller|concolic> \
    --rounds 1 --output /tmp/apex-output 2>&1
  ```

**Step 4 — Re-measure.** Run Step 1 again. Compare.

**Step 5 — Report.** Print the round report with progress bar.

**Step 6 — Breakpoints.** Check for stall, regression, compile failure, strategy failure.

**Step 7 — Terminate** when coverage target reached, max rounds hit, or user stops.

### Strategy Selection Guide

| Target type | Primary strategy | Fallback |
|-------------|-----------------|----------|
| Rust workspace | Source-level tests | fuzz (if binary harness exists) |
| Python project | Source-level tests | concolic (for constraint paths) |
| C/Rust binary | fuzz | driller (when fuzz stalls) |
| JavaScript | Source-level tests | — |

If the run fails, diagnose the error and suggest a fix.
