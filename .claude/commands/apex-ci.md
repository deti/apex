# APEX CI Gate

Check if current coverage meets the ratchet threshold and show what would fail CI.

## Usage
```
/apex-ci [min-coverage]
```
Examples:
- `/apex-ci` — check against default 100% threshold
- `/apex-ci 0.95` — check against 95% threshold

## Instructions

1. Parse `$ARGUMENTS` for threshold (default `1.0`).

2. Run:
   ```bash
   LLVM_COV=${LLVM_COV:-/opt/homebrew/opt/llvm/bin/llvm-cov} \
   LLVM_PROFDATA=${LLVM_PROFDATA:-/opt/homebrew/opt/llvm/bin/llvm-profdata} \
   cargo run --bin apex --manifest-path $APEX_HOME/Cargo.toml -- \
     ratchet --target . --lang rust \
     --min-coverage <THRESHOLD> 2>&1
   ```

3. Show the result clearly:
   - PASS or FAIL
   - Current coverage % vs required %
   - If failing: which files drag the average down most

4. If failing, suggest the minimum number of tests needed to pass the gate.
