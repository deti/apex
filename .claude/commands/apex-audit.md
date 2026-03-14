# APEX Audit — Full Security Analysis

Run a comprehensive security audit with detectors, STRIDE threat model, ASVS compliance, and SSDF compliance.

## Usage
```
/apex-audit [target] [lang]
```
Examples:
- `/apex-audit /path/to/project python` — full audit of a Python project
- `/apex-audit . rust` — audit current directory as Rust
- `/apex-audit` — auto-detect from apex.toml or prompt

## Instructions

Parse `$ARGUMENTS`: positional `target` and `lang`. If missing, check for `apex.toml` in CWD, or ask user.

### Step 1: Validate target

```bash
ls $TARGET/apex.toml 2>/dev/null
```

If `apex.toml` exists, read it for `lang` and `detect` config. Otherwise use the provided `lang` argument.

### Step 2: Run the full audit

Execute `apex audit` with all parameters enabled:

```bash
cd $TARGET && cargo run --bin apex -- audit \
  --target . \
  --lang $LANG \
  --severity-threshold low \
  --compliance-level L2 \
  --output-format text
```

If `cargo run` is not available (release binary), use:
```bash
apex audit --target $TARGET --lang $LANG --severity-threshold low --compliance-level L2
```

### Step 3: Present results

The output includes four sections. Present each with analysis:

**1. Security Findings** — Group by severity, highlight CRITICAL/HIGH first. For each:
- Explain the vulnerability in context
- Show the affected code (read the file if needed)
- Suggest the specific fix

**2. STRIDE Threat Matrix** — For each HIGH-risk category:
- Explain what the missing mitigation means for this project
- Provide a concrete code example of how to add the mitigation

**3. ASVS Compliance** — Summarize pass/fail counts. For each FAIL:
- Explain the ASVS requirement
- Link it to the finding that caused the failure
- Note which findings need manual review (NotAutomated)

**4. SSDF Compliance** — Show satisfied vs unsatisfied tasks. For unsatisfied:
- Explain what the task requires
- Suggest tooling or process changes to satisfy it

### Step 4: Generate action items

Produce a prioritized list of fixes:

```
## Action Items (by priority)

1. [CRITICAL] Fix SQL injection in auth/login.py:42
2. [HIGH] Add CSRF protection (STRIDE: Tampering)
3. [ASVS FAIL] V5.2.2 — sanitize user input in forms.py
4. [SSDF GAP] PW.7.2 — establish code review process
```

### Step 5: Offer next steps

```
Next steps:
  /apex-run          — coverage-guided bug hunting
  /apex-threat-model — reconfigure threat model
  /apex-deploy       — deployment readiness check
  /apex-ci           — add audit to CI pipeline
```
