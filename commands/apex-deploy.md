# APEX Deploy — Deployment Readiness Check

Check if the codebase is ready to deploy with an aggregate confidence score.

## Usage
```
/apex-deploy [target] [lang]
```
Examples:
- `/apex-deploy` — check current directory
- `/apex-deploy /tmp/my-project python`

## Instructions

Parse `$ARGUMENTS`: target path, language.
Defaults: target=`.`, lang=`python`.

### Prerequisite Check

Verify `.apex/index.json` exists. If missing, build it:
```bash
cargo run --bin apex --manifest-path $APEX_HOME/Cargo.toml -- \
  index --target <TARGET> --lang <LANG> --parallel 4 2>&1
```

### Step 1: Run audit for detector findings

```bash
AUDIT=$(cargo run --bin apex --manifest-path $APEX_HOME/Cargo.toml -- \
  audit --target <TARGET> --lang <LANG> --output-format json 2>/dev/null)
```

Parse to count total findings and critical findings.

### Step 2: Compute deploy score

```bash
cargo run --bin apex --manifest-path $APEX_HOME/Cargo.toml -- \
  deploy-score --target <TARGET> \
  --detector-findings <N> --critical-findings <N> \
  --output-format json 2>/dev/null
```

### Step 3: Check risk of recent changes

```bash
CHANGED=$(cd <TARGET> && git diff --name-only HEAD~1 2>/dev/null | tr '\n' ',')

cargo run --bin apex --manifest-path $APEX_HOME/Cargo.toml -- \
  risk --target <TARGET> --changed-files "$CHANGED" --output-format json 2>/dev/null
```

### Step 4: Regression check (if base branch known)

```bash
cargo run --bin apex --manifest-path $APEX_HOME/Cargo.toml -- \
  regression-check --target <TARGET> --lang <LANG> --base main --output-format json 2>/dev/null
```

### Present Results

```
## Deployment Readiness

### Score: NN/100 — RECOMMENDATION

| Component | Score | Max |
|-----------|-------|-----|
| Coverage | X | 30 |
| Test quality | X | 25 |
| Detectors | X | 25 |
| Stability | X | 20 |

### Risk Assessment: LEVEL
- Changed files: N
- Coverage of changes: X%
- Affected tests: N

### Regression Check: PASS/FAIL
- Behavioral changes: N tests

### Verdict
[Clear GO / CAUTION with specific concerns / BLOCK with required actions]
```

If BLOCK: list the specific actions needed before deploying.
If CAUTION: list concerns and whether they're acceptable risks.
If GO: confirm deployment is safe and note any monitoring recommendations.
