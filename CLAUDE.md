# APEX — Autonomous Path EXploration

## Project

Rust workspace (`resolver = "2"`). Binary: `apex`.
Crates: apex-core, apex-coverage, apex-instrument, apex-lang, apex-sandbox, apex-agent, apex-synth, apex-symbolic, apex-concolic, apex-fuzz, apex-rpc, apex-cli, apex-detect, apex-cpg, apex-index.

Heavy deps (z3, libafl, pyo3) are behind optional feature flags — not compiled by default.

## Session Naming

When starting work on a plan or feature, update `.claude/session-name` with a short identifier:
```
echo "apex:research-phase1/4" > .claude/session-name
```
This sets the terminal tab title and status bar label so parallel sessions are distinguishable.

**Format:** `[!]apex:<plan-slug>` with optional phase count
- `apex:research-phase1/4` — working on research phase 1 of 4
- `apex:js-ts-support` — JS/TS language support work
- `apex:gap-closure` — competitive gap closure

**Prefix `!` when the session needs user input:**
```
echo "!apex:research-phase1/4" > .claude/session-name
```
- `!` prefix → yellow status bar + `(!)` in terminal tab title
- No prefix → green status bar, running autonomously

**Color coding:**
- Green: session running normally
- Yellow + `!`: session blocked, needs user input
- Dim: token/context counters (secondary info)

## Build & Test

```bash
cargo test --workspace                    # all tests (~3000+)
cargo test -p apex-detect                 # single crate
cargo clippy --workspace -- -D warnings   # lint
cargo fmt --check                         # format check
```

## Code Style

- Follow existing patterns. No unnecessary abstractions.
- All detectors implement `async fn analyze(&self, ctx: &AnalysisContext) -> Result<Vec<Finding>>`.
- Security patterns use `SecurityPattern` structs with `cwe`, `user_input_indicators`, `sanitization_indicators`.
- Tests go in `#[cfg(test)] mod tests` inside each file, not separate test files.
- Use `#[tokio::test]` for async detector tests.

## Plans & Specs

- Plans: `docs/superpowers/plans/YYYY-MM-DD-<topic>.md`
- Specs: `docs/superpowers/specs/YYYY-MM-DD-<topic>-design.md`
- Internal planning: `/plans/` (gitignored)

## Git Workflow

- Use worktrees for feature branches: `.worktrees/<name>/`
- Never switch branches in the main checkout
- After merging worktree branches, check for struct drift (fields added on main while worktree was active)
