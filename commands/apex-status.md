# APEX Status

Show current branch coverage for the workspace.

## Usage
```
/apex-status [crate]
```
Examples:
- `/apex-status` — whole workspace
- `/apex-status apex-agent` — single crate

## Instructions

1. Run `cargo llvm-cov --json` with LLVM env vars set:
   ```bash
   LLVM_COV=${LLVM_COV:-/opt/homebrew/opt/llvm/bin/llvm-cov} \
   LLVM_PROFDATA=${LLVM_PROFDATA:-/opt/homebrew/opt/llvm/bin/llvm-profdata} \
   cargo llvm-cov --json --output-path /tmp/apex_status.json 2>&1 | tail -3
   ```

2. Parse `/tmp/apex_status.json` with Python and display a table:
   - Columns: `File`, `Coverage %`, `Hit`, `Total`
   - Sort by coverage % ascending (worst first)
   - If `$ARGUMENTS` is set, filter to that crate only
   - Show workspace total at the bottom

3. Highlight files below 20% coverage in the summary.

4. State the top 3 files that would most improve overall coverage if tested.
