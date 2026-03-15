# APEX Integration

This project uses APEX for coverage analysis and security detection.

## Available Tools (via MCP)

APEX is configured as an MCP server. Use these tools:

- **apex_run** — Run coverage analysis. Pass `target` (project path) and `lang` (python/rust/javascript/etc.).
- **apex_detect** — Security analysis. Returns findings with CWE IDs and severity.
- **apex_reach** — Find entry points reaching a file:line location.
- **apex_ratchet** — CI gate. Returns PASS/FAIL based on coverage threshold.
- **apex_deploy_score** — Deployment confidence score 0-100.

## Workflow

1. After modifying code, run `apex_run` to identify uncovered branches
2. Write tests targeting the gaps
3. Run `apex_detect` to check for security issues before committing
4. Use `apex_ratchet` in CI to enforce coverage standards
