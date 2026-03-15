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
- **Plan registry:** `/plans/STATUS.md` — single source of truth for all plan statuses
- **Status convention:** first line of each plan file: `<!-- status: DONE|ACTIVE|PARKED|DEPRECATED|FUTURE -->`
  - `DONE` — all tasks implemented
  - `ACTIVE` — currently being worked on
  - `PARKED` — partially done, not actively worked on
  - `DEPRECATED` — superseded or no longer relevant
  - `FUTURE` — roadmap item, not started
- Before creating a new plan, check STATUS.md. Update it when plan status changes.

## Distribution

Binary: 5MB static release (`lto = true`, `codegen-units = 1`). No runtime deps beyond libc.

| Channel | Location | Install |
|---------|----------|---------|
| GitHub Releases | `.github/workflows/release.yml` | Tag `v*` triggers cross-build for 4 targets |
| curl installer | `install.sh` | `curl -sSL .../install.sh \| sh` |
| Homebrew | `HomebrewFormula/apex.rb` | `brew install allexdav2/tap/apex` |
| npm | `npm/` | `npx @apex-coverage/cli run` |
| pip | `python/` | `pipx install apex-coverage` |
| Nix | `flake.nix` | `nix run github:allexdav2/apex` |
| cargo | source | `cargo install --git https://github.com/allexdav2/apex` |

**Version bumping:** `./scripts/bump-version.sh 0.2.0` — updates all 5 locations atomically and stamps CHANGELOG.md.

**Release process:**
1. `./scripts/bump-version.sh <version>`
2. Create PR, get CI green, merge
3. `git tag v<version> && git push --tags` → CI builds binaries
4. Update Homebrew sha256 after release assets upload
5. `npm publish` / `twine upload`

## Agent Fleet Pattern

When dispatching parallel worktree agents, use fleet officer / crew terminology:
- Main session = **fleet officer** (orchestrates, dispatches, merges)
- Worktree agents = **crew** (each handles one independent task in isolation)

### Pre-Dispatch Checklist
Before dispatching crew agents:
1. Run `git status` — check for uncommitted changes
2. If changes exist in files crew will touch:
   a. Create a WIP commit: `git commit -am "WIP: pre-dispatch snapshot"`
   b. Note the WIP commit hash for later cleanup
3. Dispatch crew agents (they now have full state)
4. After all crew complete and changes are merged:
   a. `git reset HEAD~1` to undo the WIP commit (keeps changes staged)

### Merging Crew Changes
NEVER copy files wholesale from worktrees. Always use diffs:
1. Generate patch: `git -C <worktree> diff HEAD -- crates/ > /tmp/crew-name.patch`
2. Apply to main: `git apply /tmp/crew-name.patch`
3. If patch conflicts, read the diff and apply changes manually to specific lines

This prevents overwriting uncommitted main changes that crew agents couldn't see.

## Git Workflow

- **PR-driven**: all changes go through pull requests. Direct pushes to main are blocked.
- **CI required**: Check, Test, Clippy, Format, Changelog must pass before merge.
- **Changelog required**: every PR must update CHANGELOG.md under `[Unreleased]`.
- Use worktrees for feature branches: `.worktrees/<name>/`
- Never switch branches in the main checkout
- After merging worktree branches, check for struct drift (fields added on main while worktree was active)

```bash
# Typical PR workflow
git worktree add .worktrees/my-feature -b feat/my-feature
cd .worktrees/my-feature
# ... work, commit ...
git push -u origin feat/my-feature
gh pr create --title "feat: my feature"
# After merge:
cd ../..
git worktree remove .worktrees/my-feature
```
