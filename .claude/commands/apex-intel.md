# APEX Intel — Full SDLC Intelligence Report

Run all intelligence analyses and present a comprehensive report.

## Usage
```
/apex-intel [target]
```
Examples:
- `/apex-intel` — analyze current directory
- `/apex-intel /tmp/my-project`

## Instructions

Parse `$ARGUMENTS`: target path. Default: `.`.

### Prerequisite Check

Verify `.apex/index.json` exists at the target:
```bash
ls <TARGET>/.apex/index.json 2>&1
```

If missing, tell the user to run `/apex-index` first and stop.

### Run Intelligence Commands

Run these in sequence (all use `--output-format json` for parsing):

```bash
APEX=/Users/ad/prj/bcov/Cargo.toml

# 1. Test optimization
cargo run --bin apex --manifest-path $APEX -- test-optimize --target <TARGET> --output-format json 2>/dev/null

# 2. Dead code
cargo run --bin apex --manifest-path $APEX -- dead-code --target <TARGET> --output-format json 2>/dev/null

# 3. Complexity
cargo run --bin apex --manifest-path $APEX -- complexity --target <TARGET> --output-format json 2>/dev/null

# 4. Hot paths
cargo run --bin apex --manifest-path $APEX -- hotpaths --target <TARGET> --top 10 --output-format json 2>/dev/null

# 5. Contracts
cargo run --bin apex --manifest-path $APEX -- contracts --target <TARGET> --output-format json 2>/dev/null

# 6. Deploy score
cargo run --bin apex --manifest-path $APEX -- deploy-score --target <TARGET> --output-format json 2>/dev/null
```

### If there are changed files (auto-detect)

```bash
cd <TARGET> && git diff --name-only HEAD~1 2>/dev/null
```

If changed files exist, also run:
```bash
# 7. Risk assessment
cargo run --bin apex --manifest-path $APEX -- risk --target <TARGET> --changed-files <FILES> --output-format json 2>/dev/null

# 8. Test prioritization
cargo run --bin apex --manifest-path $APEX -- test-prioritize --target <TARGET> --changed-files <FILES> 2>/dev/null
```

### Present the Report

Format as a structured intelligence report:

```
## APEX Intelligence Report

### Test Suite Health
- Minimal covering set: N / M tests (X.Xx speedup)
- Under-tested functions: N (< 50% branch exercise ratio)
- Discovered invariants: N contracts

### Code Quality
- Dead branches: N across M files
- Hot paths: top branch accounts for X% of execution
- Complexity: N functions fully exercised, M partially tested

### Deployment Readiness
- Deploy score: NN/100 — RECOMMENDATION
  Coverage: X/30 | Quality: X/25 | Detectors: X/25 | Stability: X/20

### Risk (if changed files detected)
- Risk level: LOW/MEDIUM/HIGH/CRITICAL
- Changed branch coverage: X%
- Affected tests: N

### Recommendations
1. [Most impactful action]
2. [Second most impactful]
3. [Third]
```

Prioritize recommendations by impact: security findings first, then coverage gaps in changed code, then test optimization.
