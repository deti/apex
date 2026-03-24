# APEX Gaps

List uncovered code regions and explain what tests would cover them.

## Usage
```
/apex-gaps [crate-or-file]
```
Examples:
- `/apex-gaps apex-coverage` — gaps in apex-coverage crate
- `/apex-gaps crates/apex-fuzz/src/mutators.rs` — gaps in one file
- `/apex-gaps` — top 5 most-impactful gaps across workspace

## Instructions

1. Run `cargo llvm-cov --json` (with LLVM env vars) and save to `/tmp/apex_gaps.json`.

2. Filter segments to: `has_count=true`, `is_region_entry=true`, `is_gap=false`, `count=0`.
   If `$ARGUMENTS` given, filter to matching filename.

3. For each uncovered region, read the surrounding source (±3 lines) and explain:
   - What the code does
   - Why it's likely uncovered (error path? unused feature? race condition?)
   - A concrete test that would cover it

4. Show at most 15 gaps. Prioritize by:
   - Files with most uncovered regions first
   - Error handling paths (contain `Err(`, `unwrap_or`, `?`)
   - Public API surface

5. End with a summary: "Writing N tests could bring coverage from X% to ~Y%".
