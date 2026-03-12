# APEX Index — Build Per-Test Branch Index

Build the branch index that powers all APEX intelligence commands.

## Usage
```
/apex-index [target] [lang] [parallel]
```
Examples:
- `/apex-index` — index the current directory as Python
- `/apex-index /tmp/my-project python 8`
- `/apex-index . rust 4`

## Instructions

Parse `$ARGUMENTS`: target path, language, parallel workers.
Defaults: target=`.`, lang=`python`, parallel=`4`.

1. Run the index build:
   ```bash
   cargo run --bin apex --manifest-path /Users/ad/prj/bcov/Cargo.toml -- \
     index --target <TARGET> --lang <LANG> --parallel <N> 2>&1
   ```

2. If successful, show a summary:
   - Number of tests indexed
   - Number of branches covered / total
   - Coverage percentage
   - Index file location (`.apex/index.json`)

3. If the index already exists, check staleness:
   ```bash
   python3 -c "
   import json, os
   idx = json.load(open('<TARGET>/.apex/index.json'))
   print(f'Tests: {len(idx.get(\"traces\", []))}')
   print(f'Branches: {idx.get(\"covered_branches\", 0)} / {idx.get(\"total_branches\", 0)}')
   print(f'Created: {idx.get(\"created_at\", \"unknown\")}')
   "
   ```
   Ask user if they want to rebuild.

4. After building, suggest next steps:
   - `apex test-optimize` to find minimal test set
   - `apex dead-code` to find untested branches
   - `apex deploy-score` to check deployment readiness
   - Or use `/apex-intel` for a full intelligence report
