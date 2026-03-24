# APEX — Main Entrypoint

The single starting point for APEX. Checks project health, shows key metrics, and recommends next actions.

## Usage
```
/apex [target] [lang]
```
Examples:
- `/apex` — analyze current directory (auto-detect language)
- `/apex /tmp/my-project python`
- `/apex . rust`

## Instructions

Parse `$ARGUMENTS`: target path, language. Defaults: target=`.`, lang=auto-detect.

### Auto-detect language

If lang not provided, check the target directory:
```bash
# Python: look for pytest/setup.py/pyproject.toml
ls <TARGET>/pytest.ini <TARGET>/setup.py <TARGET>/pyproject.toml <TARGET>/setup.cfg 2>/dev/null
# Rust: look for Cargo.toml
ls <TARGET>/Cargo.toml 2>/dev/null
# JS: look for package.json
ls <TARGET>/package.json 2>/dev/null
# Java: look for pom.xml/build.gradle
ls <TARGET>/pom.xml <TARGET>/build.gradle 2>/dev/null
```

### Phase 1: Prerequisites

Check APEX binary is available:
```bash
cargo run --bin apex --manifest-path $APEX_HOME/Cargo.toml -- doctor 2>&1
```

If doctor fails, show what's missing and how to fix it. Stop here.

### Phase 2: Index Status

Check if `.apex/index.json` exists and is fresh:
```bash
python3 -c "
import json, os, sys
path = os.path.join('$TARGET', '.apex/index.json')
if not os.path.exists(path):
    print('NO_INDEX')
    sys.exit(0)
idx = json.load(open(path))
tests = len(idx.get('traces', []))
covered = idx.get('covered_branches', 0)
total = idx.get('total_branches', 0)
pct = (covered/total*100) if total else 0
created = idx.get('created_at', 'unknown')
print(f'INDEX_OK tests={tests} covered={covered} total={total} pct={pct:.1f} created={created}')
"
```

If no index exists, build one:
```bash
cargo run --bin apex --manifest-path $APEX_HOME/Cargo.toml -- \
  index --target <TARGET> --lang <LANG> --parallel 4 2>&1
```

### Phase 3: Core Metrics

Run these in sequence:

```bash
APEX=$APEX_HOME/Cargo.toml

# Deploy score (the single most important number)
cargo run --bin apex --manifest-path $APEX -- \
  deploy-score --target <TARGET> --output-format json 2>/dev/null

# Test optimization (how bloated is the test suite?)
cargo run --bin apex --manifest-path $APEX -- \
  test-optimize --target <TARGET> --output-format json 2>/dev/null

# Dead code (how much code is untested?)
cargo run --bin apex --manifest-path $APEX -- \
  dead-code --target <TARGET> --output-format json 2>/dev/null

# Hot paths (where does execution concentrate?)
cargo run --bin apex --manifest-path $APEX -- \
  hotpaths --target <TARGET> --top 5 --output-format json 2>/dev/null
```

### Phase 4: Change-Aware Analysis (if git repo)

```bash
# Detect changed files
CHANGED=$(cd <TARGET> && git diff --name-only HEAD~1 2>/dev/null | tr '\n' ',')
```

If changed files exist:
```bash
# Risk of current changes
cargo run --bin apex --manifest-path $APEX -- \
  risk --target <TARGET> --changed-files "$CHANGED" --output-format json 2>/dev/null

# Tests to run for these changes
cargo run --bin apex --manifest-path $APEX -- \
  test-prioritize --target <TARGET> --changed-files "$CHANGED" 2>/dev/null
```

### Phase 5: Security Quick Check

```bash
cargo run --bin apex --manifest-path $APEX -- \
  audit --target <TARGET> --lang <LANG> --output-format json 2>/dev/null
```

### Present the Dashboard

```
## APEX Dashboard

### Project: <target>
Language: <lang> | Tests: N | Branches: N covered / N total | Coverage: X%
Index: fresh (built YYYY-MM-DD) | Last commit: <hash> <message>

### Deploy Score: NN/100 — GO/ACCEPTABLE/CAUTION/BLOCK
  Coverage    ██████████████████████████████ X/30
  Quality     █████████████████████████      X/25
  Detectors   █████████████████████████      X/25
  Stability   ████████████████████           X/20

### Key Findings
- Test suite: N tests, could be reduced to M (X.Xx speedup)
- Dead code: N branches across M files never executed
- Hot path: file:line accounts for X% of execution
- Security: N findings (N critical)

### Current Changes (if any)
- Risk: LEVEL — X% coverage of changed code
- Affected tests: N (run these first: test_a, test_b, test_c)

### Recommended Actions
1. [Highest impact action]
2. [Second]
3. [Third]
```

### Recommended Actions Logic

Prioritize by:
1. **Critical security findings** — always first if any exist
2. **Uncovered branches in changed files** — immediate risk
3. **Flaky tests** — eroding CI trust
4. **Coverage gaps in hot paths** — highest-traffic untested code
5. **Dead code cleanup** — reduce maintenance burden
6. **Test suite optimization** — reduce CI time

### Follow-up Suggestions

End with:
```
Next steps:
  /apex-run          — autonomous coverage improvement loop
  /apex-intel        — deep intelligence report
  /apex-deploy       — full deployment readiness check
  /apex-gaps <file>  — detailed uncovered regions
```
