# APEX — Autonomous Path EXploration

## Project

Rust workspace (`resolver = "2"`). Binary: `apex`.
Crates: apex-core, apex-coverage, apex-instrument, apex-lang, apex-sandbox, apex-agent, apex-synth, apex-symbolic, apex-concolic, apex-fuzz, apex-rpc, apex-cli, apex-detect, apex-cpg, apex-index.

Heavy deps (z3, libafl, pyo3) are behind optional feature flags — not compiled by default.

## Session Naming

Each Claude Code session gets its own name file at `.claude/sessions/<session_id>.name`. Multiple sessions in the same project don't collide.

**Set the session task** (project name is derived from repo dir automatically):
```bash
# Shows as "bcov:research-phase1/4" in tab — "bcov:" prefix is automatic
echo "research-phase1/4" > .claude/sessions/${PPID}.name
# No name file → tab just shows "bcov"
```

**Attention marker** — touch `.attn` file when blocked on user input:
```bash
touch .claude/sessions/${PPID}.attn    # ● yellow dot appears
rm -f .claude/sessions/${PPID}.attn    # back to green
```

**Format:** just the task slug (project prefix is automatic from repo dir):
- `research-phase1/4` → shows as `bcov:research-phase1/4`
- `js-ts-support` → shows as `bcov:js-ts-support`
- (no file) → shows as `bcov`

**Attention marker** (● yellow dot):
- Separate `.attn` file (not baked into the name)
- Tab title: `● apex:research-phase1/4`
- Status bar: yellow `● apex:research-phase1/4`

**Color coding:**
- Green: session running normally
- Yellow + ●: session needs user input
- Dim: token/context counters

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
