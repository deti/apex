# APEX Agents for Claude Code

Claude Code agents and slash commands for working with APEX.

## Install as Plugin (recommended)

```bash
claude plugin install github:sahajamoth/apex
```

This registers APEX as a Claude Code plugin. Commands are available as `/apex:apex`, `/apex:apex-run`, etc. Agents are auto-dispatched when Claude detects relevant triggers.

## Install via Script (manual)

```bash
./agents/install.sh
```

This copies agents and commands into `.claude/` so Claude Code picks them up as project-local `/apex`, `/apex-run`, etc.

## Environment

Set `APEX_HOME` to point to the APEX repo checkout:

```bash
export APEX_HOME=/path/to/apex
```

If not set, commands assume APEX is in the current working directory's git root.

For Rust coverage, also set:

```bash
export LLVM_COV=/opt/homebrew/opt/llvm/bin/llvm-cov
export LLVM_PROFDATA=/opt/homebrew/opt/llvm/bin/llvm-profdata
```

## Plugin Structure

When installed as a plugin, Claude Code discovers:

```
apex/
├── .claude-plugin/
│   ├── plugin.json        # Plugin manifest (name, version, metadata)
│   └── marketplace.json   # Marketplace catalog entry
├── commands/              # Slash commands → /apex:<command>
│   ├── apex.md            # /apex:apex — main entrypoint
│   ├── apex-run.md        # /apex:apex-run — full coverage loop
│   ├── apex-index.md      # /apex:apex-index — build branch index
│   ├── apex-intel.md      # /apex:apex-intel — SDLC intelligence
│   ├── apex-deploy.md     # /apex:apex-deploy — deploy readiness
│   ├── apex-status.md     # /apex:apex-status — coverage table
│   ├── apex-gaps.md       # /apex:apex-gaps — uncovered regions
│   ├── apex-generate.md   # /apex:apex-generate — generate tests
│   └── apex-ci.md         # /apex:apex-ci — CI coverage gate
├── agents/                # Auto-dispatched agents
│   ├── apex-coverage-analyst.md
│   ├── apex-test-writer.md
│   ├── apex-runner.md
│   ├── apex-sdlc-analyst.md
│   ├── apex-crew-*.md     # Crew agents for parallel work
│   └── ...
└── integrations/          # MCP server configs for other AI tools
```

## Agents (auto-invoked by Claude)

| Agent | Trigger |
|-------|---------|
| `apex-coverage-analyst` | "what's our coverage?", "which parts are uncovered?" |
| `apex-test-writer` | "write tests for X", "improve coverage in Y" |
| `apex-runner` | "run apex against Z", "run apex on itself" |
| `apex-sdlc-analyst` | "what's our deploy score?", "find flaky tests", "show hot paths" |

## Slash Commands (user-invoked)

| Command | Usage |
|---------|-------|
| `/apex` | **Main entrypoint** — dashboard with deploy score, key findings, recommendations |
| `/apex-run [target] [lang]` | Full autonomous coverage loop |
| `/apex-index [target] [lang]` | Build per-test branch index for intelligence commands |
| `/apex-intel [target]` | Full SDLC intelligence report |
| `/apex-deploy [target] [lang]` | Deployment readiness check |
| `/apex-status [crate]` | Show coverage table |
| `/apex-gaps [crate-or-file]` | List uncovered regions with explanations |
| `/apex-generate <crate-or-file>` | Generate tests for uncovered code |
| `/apex-ci [min-coverage]` | Check CI coverage gate |
