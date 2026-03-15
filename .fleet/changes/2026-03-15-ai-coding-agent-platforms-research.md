# AI Coding Agent Platforms: Agent Definition Formats Deep Research

**Date:** 2026-03-15
**Scope:** Eight major AI coding agent platforms, their agent definition formats, tool systems, permissions, multi-agent support, and distribution mechanisms.

---

## Executive Summary

The AI coding agent ecosystem has matured rapidly through 2025-2026. A clear pattern has emerged: **Markdown files with YAML frontmatter** has become the de facto standard for agent definitions, adopted by Claude Code, GitHub Copilot, OpenAI Codex, Cursor, and Windsurf independently. The key differentiators are now in tool restriction granularity, multi-agent orchestration, and permission models.

---

## 1. Claude Code (Anthropic)

### Popularity
- **GitHub Stars:** ~71.5K (anthropics/claude-code)
- **VS Code Extension:** 5.2M installs
- **Commit Share:** ~4% of all public GitHub commits (~135K/day)
- **Monthly Active Users:** Part of Claude's 18.9M MAU ecosystem

### Agent Definition Format

Agents are Markdown files with YAML frontmatter stored in `.claude/agents/`. The filename is the agent identifier.

**Location priority (highest to lowest):**

| Priority | Location | Scope |
|----------|----------|-------|
| 1 | `--agents` CLI flag (JSON) | Current session only |
| 2 | `.claude/agents/` | Current project |
| 3 | `~/.claude/agents/` | All projects (user-level) |
| 4 | Plugin `agents/` directory | Where plugin is enabled |

**Complete YAML Frontmatter Schema:**

```yaml
---
name: my-agent              # Required. Lowercase letters and hyphens.
description: When to use    # Required. Guides automatic delegation.
tools: Read, Grep, Glob     # Optional. Allowlist. Inherits all if omitted.
disallowedTools: Write, Edit # Optional. Denylist, removed from inherited set.
model: sonnet               # Optional. sonnet|opus|haiku|full-model-id|inherit
permissionMode: default     # Optional. default|acceptEdits|dontAsk|bypassPermissions|plan
maxTurns: 50                # Optional. Max agentic turns before stop.
skills:                     # Optional. Skills injected at startup (full content).
  - api-conventions
  - error-handling
mcpServers:                 # Optional. MCP servers scoped to this subagent.
  - playwright:
      type: stdio
      command: npx
      args: ["-y", "@playwright/mcp@latest"]
  - github                  # String reference to already-configured server.
hooks:                      # Optional. Lifecycle hooks scoped to subagent.
  PreToolUse:
    - matcher: "Bash"
      hooks:
        - type: command
          command: "./scripts/validate.sh"
  PostToolUse:
    - matcher: "Edit|Write"
      hooks:
        - type: command
          command: "./scripts/lint.sh"
memory: user                # Optional. user|project|local. Persistent cross-session memory.
background: true            # Optional. Run as background task. Default: false.
isolation: worktree         # Optional. Run in temporary git worktree.
---

System prompt goes here in Markdown. This is the only prompt the subagent
receives (plus basic environment details like working directory).
```

**CLI-defined agents (JSON, session-only):**

```bash
claude --agents '{
  "code-reviewer": {
    "description": "Expert code reviewer",
    "prompt": "You are a senior code reviewer...",
    "tools": ["Read", "Grep", "Glob", "Bash"],
    "model": "sonnet"
  }
}'
```

### Tool/Capability System
- Built-in tools: Read, Write, Edit, Bash, Grep, Glob, Agent (spawn subagents), and MCP tools.
- Agent spawning restriction: `Agent(worker, researcher)` syntax limits which subagents can be spawned.
- Skills can be preloaded into subagent context at startup.

### Permissions
- Five permission modes: `default`, `acceptEdits`, `dontAsk`, `bypassPermissions`, `plan`.
- `PreToolUse` hooks enable conditional validation (e.g., blocking SQL writes).
- `permissions.deny` array in settings to block specific subagents: `Agent(Explore)`.

### Multi-Agent Orchestration
- **Subagents:** Run within a single session, isolated context windows.
- **Agent Teams:** Coordinate across separate sessions (different feature).
- Built-in subagents: Explore (haiku, read-only), Plan (inherited model, read-only), General-purpose (all tools).
- Subagents cannot spawn other subagents (no nesting).
- Background tasks run concurrently; foreground blocks main conversation.
- Resume support: subagents retain full conversation history via agent ID.

### Distribution
- Plugins system: agents distributed via plugin packages.
- Version control: project agents in `.claude/agents/` checked into git.
- User agents: `~/.claude/agents/` for personal cross-project use.

---

## 2. OpenAI Codex CLI

### Popularity
- **GitHub Stars:** ~62K (openai/codex)
- **Mac App:** 1M+ downloads in first week (Feb 2026)
- **Users:** Tripled since start of 2026
- **Pricing:** Included with ChatGPT Plus/Pro/Business/Enterprise

### Agent Definition Format

Codex uses **two separate systems**: `AGENTS.md` for project instructions and **Skills** for reusable agent capabilities.

**AGENTS.md Discovery Hierarchy:**

1. Global scope: `~/.codex/AGENTS.override.md` then `~/.codex/AGENTS.md`
2. Project scope: walks from git root to CWD, checking `AGENTS.override.md` then `AGENTS.md` in each directory
3. Files concatenate root-downward; closer directories override earlier guidance
4. Max combined size: 32 KiB (configurable via `project_doc_max_bytes`)

**AGENTS.md format:** Plain Markdown, no frontmatter. Contains instructions, not structured agent definitions.

```markdown
# Project Guidelines

Always run `npm test` after modifying JavaScript files.
Use TypeScript strict mode for all new files.
Follow the error handling patterns in src/utils/errors.ts.
```

**Skills Directory Structure:**

```
my-skill/
  SKILL.md                   # Required
  scripts/                   # Optional
  references/                # Optional
  assets/                    # Optional
  agents/
    openai.yaml              # Optional: UI/behavior metadata
```

**SKILL.md Frontmatter:**

```yaml
---
name: skill-name
description: When this skill should and should not trigger.
---

Instructions for Codex to follow when this skill is active.
```

**agents/openai.yaml (Optional Extended Config):**

```yaml
interface:
  display_name: "User-facing name"
  short_description: "Brief description"
  icon_small: "./assets/small-logo.svg"
  icon_large: "./assets/large-logo.png"
  brand_color: "#3B82F6"
  default_prompt: "Optional surrounding prompt"

policy:
  allow_implicit_invocation: false

dependencies:
  tools:
    - type: "mcp"
      value: "toolName"
      description: "Tool description"
      transport: "streamable_http"
      url: "https://example.com"
```

**Skill Location Priority:**

| Scope | Path |
|-------|------|
| REPO (CWD) | `.agents/skills` |
| REPO (nested) | `../.agents/skills` |
| REPO (root) | `$REPO_ROOT/.agents/skills` |
| USER | `$HOME/.agents/skills` |
| ADMIN | `/etc/codex/skills` |
| SYSTEM | Built-in |

**Main Config (`~/.codex/config.toml`):**

```toml
model = "o4-mini"
approval_policy = "on-request"       # untrusted|on-request|never
sandbox_mode = "workspace-write"     # workspace-write|danger-full-access

[profiles.fast]
model = "gpt-4.1-mini"

[shell_environment_policy]
inherit = "core"

[[skills.config]]
path = "/path/to/skill/SKILL.md"
enabled = false
```

### Tool/Capability System
- Skills define tool dependencies via `agents/openai.yaml`.
- MCP server integration for external tools.
- Explicit vs. implicit invocation control per skill.

### Permissions
- `approval_policy`: `untrusted` (ask for everything), `on-request` (ask for risky ops), `never` (auto-approve).
- `sandbox_mode`: `workspace-write` (contained) or `danger-full-access`.
- Granular reject rules in approval policy.

### Multi-Agent Orchestration
- Multi-agent workflows supported via `[agents]` section in `config.toml`.
- Skills can be parallelized.
- Named profiles for different agent configurations.

### Distribution
- Skills are directory-based, shareable via git.
- System-level skills at `/etc/codex/skills`.
- No formal plugin/marketplace system yet.

---

## 3. Cline (VS Code Extension)

### Popularity
- **GitHub Stars:** ~59K (cline/cline)
- **VS Code Installs:** 5M+
- **Contributors:** 4,704% YoY growth
- **Originally named:** Claude Dev (renamed to Cline)

### Agent Definition Format

Cline uses `.clinerules` for project instructions and VS Code settings for API/model configuration.

**`.clinerules` formats:**

Option A -- Single file at project root:
```
# .clinerules (plain text or Markdown)
Always use TypeScript strict mode.
Run tests with `npm test` before committing.
Follow error handling patterns in src/utils/.
```

Option B -- Directory with multiple rule files:
```
.clinerules/
  coding-standards.md
  testing-rules.md
  mcp-routing.md
```

**MCP Rules Configuration (in `.clinerules`):**

```yaml
mcpRules:
  categories:
    webInteraction:
      servers: ["puppeteer", "playwright"]
      triggers: ["scrape", "website", "browser"]
      description: "Web scraping and interaction"
    mediaAndDesign:
      servers: ["dalle", "figma"]
      triggers: ["generate image", "design"]
      description: "Media generation"
  defaultBehavior:
    priorityOrder: ["webInteraction", "mediaAndDesign"]
    fallbackBehavior: "ask user"
```

**Custom Instructions:** Set globally via VS Code settings panel (`cline.customInstructions`), overrideable per-project with `.clinerules`.

### Tool/Capability System
- File creation/editing, command execution, browser control.
- MCP server integration with keyword-based auto-routing.
- Agent Client Protocol (ACP) support for cross-editor compatibility.
- Cline SDK for programmatic access.

### Permissions
- **Human-in-the-loop by default:** Every file edit, command execution, and browser action requires explicit user approval.
- Auto-approve settings available per tool type in VS Code settings.
- No formal permission mode system like Claude Code.

### Multi-Agent Orchestration
- No native multi-agent support.
- Plan-then-act mode: presents approach before execution.
- Cline CLI 2.0 adds `--acp` flag for Agent Client Protocol integration.

### Distribution
- VS Code Marketplace extension.
- `.clinerules` checked into repos for team sharing.
- No plugin/agent marketplace.

---

## 4. OpenClaw

### Popularity
- **GitHub Stars:** ~250K+ (openclaw/openclaw) -- most-starred non-aggregator software project on GitHub
- **Achieved in:** ~4 months (surpassed React)
- **Funding:** Active open-source community

### Overview

OpenClaw is a **personal AI assistant** (not primarily a coding agent) that runs locally and connects to 20+ messaging platforms (WhatsApp, Telegram, Slack, Discord, Signal, iMessage, IRC, Teams, etc.). It includes coding capabilities but is broader in scope.

**Relationship to OpenHands:** OpenClaw and OpenHands appear in the same GitHub organization. OpenHands (68.6K stars, $18.8M Series A) is the dedicated **software development agent platform**. OpenClaw is the broader personal assistant. They share some infrastructure but serve different purposes.

### Agent Definition Format

OpenClaw uses YAML-based configuration:

```yaml
# Gateway configuration
gateway:
  port: 8080
  auth: ...

# Channel configuration
channels:
  telegram:
    token: ...
  slack:
    token: ...

# Agent defaults
agents:
  defaults:
    workspace: /path/to/workspace
```

**Skills system:** OpenClaw uses a skills directory structure similar to Codex:
```
skills/
  coding-agent/
    SKILL.md
```

**AGENTS.md:** OpenClaw also reads `AGENTS.md` at repo root for project-level instructions.

### OpenHands Agent SDK (Related)

The dedicated coding platform uses a Python SDK:

```python
from openhands import Agent, LLM, Conversation
from openhands.tools import TerminalTool, FileEditorTool, TaskTrackerTool

llm = LLM(model="claude-sonnet-4-20250514", api_key="...")
agent = Agent(tools=[TerminalTool(), FileEditorTool(), TaskTrackerTool()])
conversation = Conversation(agent=agent)
conversation.run("Fix the failing tests")
```

### Tool/Capability System
- Browser control (CDP-managed Chrome/Chromium)
- Voice integration (macOS/iOS/Android)
- Live Canvas (A2UI visual workspace)
- Device nodes for mobile with permission-aware tool execution
- Multi-agent routing across isolated workspaces

### Permissions
- Per-agent isolation via workspaces and routing rules
- Permission-aware tool execution on device nodes

### Multi-Agent Orchestration
- Multi-agent routing across isolated workspaces
- Session-based architecture
- Per-agent isolation

### Distribution
- Docker/container-based deployment
- pip installable
- Self-hosted

---

## 5. Cursor

### Popularity
- **Users:** 1M+ daily active users (Dec 2025)
- **Paying Subscribers:** 360K+
- **ARR:** Surpassed $1B in 2025
- **Valuation:** $29.3B (Nov 2025, Series D)
- **Not open source** (closed-source IDE, fork of VS Code)

### Agent Definition Format

Cursor uses **Rules** stored in `.cursor/rules/` as `.mdc` (Markdown with Config) files with YAML frontmatter.

**Rule Types:**

| Type | Frontmatter | Behavior |
|------|-------------|----------|
| Always Apply | `alwaysApply: true` | Included in every chat session |
| Apply Intelligently | `alwaysApply: false`, has `description` | Agent decides relevance |
| Apply to Specific Files | Has `globs` | Applied when matching files are in context |
| Apply Manually | No `alwaysApply`, no `globs` | Invoked via `@rule-name` |

**MDC File Format (`.cursor/rules/*.mdc`):**

```yaml
---
description: "TypeScript coding standards for API endpoints"
alwaysApply: false
globs: ["src/api/**/*.ts", "src/routes/**/*.ts"]
---

Use strict TypeScript with no `any` types.
All API handlers must return typed response objects.
Error handling must use the AppError class from src/utils/errors.ts.
Always include request validation using zod schemas.
```

**Frontmatter Schema:**

| Field | Type | Description |
|-------|------|-------------|
| `description` | string | Explains purpose; used by agent for intelligent application |
| `alwaysApply` | boolean | If true, included in every session |
| `globs` | string[] | File patterns for automatic activation |

**Alternative: AGENTS.md**
Plain Markdown at project root or subdirectories. No frontmatter. Supports nested hierarchy with most-specific-directory precedence.

**Rule Precedence:** Team Rules > Project Rules > User Rules

**Scope Locations:**
- Project: `.cursor/rules/*.mdc` (checked into version control)
- User: Cursor Settings UI (global to machine)
- Team: Team dashboard (shared across org)

### Tool/Capability System
- Rules are passive context injection, not active tool definitions.
- Cursor's agent mode handles tool execution internally.
- Skills (dynamic capabilities) complement Rules (static context).
- No user-defined tool restrictions per rule.

### Permissions
- No granular permission model per rule.
- Agent mode has global accept/reject for file changes.
- No equivalent to Claude Code's `permissionMode` per agent.

### Multi-Agent Orchestration
- No multi-agent support. Single agent with context-switching rules.
- Background agents planned but not yet shipped.

### Distribution
- `.cursor/rules/` directory checked into git for team sharing.
- No plugin/marketplace for rules.
- Community site: cursorrules.org for sharing rule templates.

---

## 6. Windsurf (Codeium)

### Popularity
- **Users:** 800K+ active developers
- **ARR:** $82M at time of acquisition
- **Enterprise Customers:** 350+
- **Acquisition:** Bought by Cognition AI for ~$250M (Dec 2025)
- **Ranking:** #1 in LogRocket AI Dev Tool Power Rankings (Feb 2026)
- **Not open source** (closed-source IDE)

### Agent Definition Format

Windsurf uses **Rules** (`.windsurf/rules/*.md`) and **Workflows** (`.windsurf/workflows/*.md`) with YAML frontmatter.

**Rules Format:**

```yaml
---
trigger: model_decision
---

# TypeScript Standards

- Use strict mode for all TypeScript files
- Prefer interfaces over type aliases for object shapes
- All async functions must have proper error handling
```

**Trigger Types:**

| Trigger Value | Behavior |
|---------------|----------|
| `always_on` | Full content in system prompt on every message |
| `model_decision` | Model sees description; loads full content when relevant |
| `glob` | Activates when Cascade reads/edits matching files |
| `manual` | Requires `@rule-name` mention |

**Glob-based Rule Example:**

```yaml
---
trigger: glob
globs: "**/*.test.ts"
---

All test files must use vitest.
Use `describe`/`it` blocks, not `test()`.
Mock external services with msw.
```

**Frontmatter Schema:**

| Field | Type | Description |
|-------|------|-------------|
| `trigger` | string | Activation mode: `always_on`, `model_decision`, `glob`, `manual` |
| `globs` | string | File patterns (required when trigger is `glob`) |

**File Locations:**

| Scope | Path | Limit |
|-------|------|-------|
| Workspace | `.windsurf/rules/*.md` | 12,000 chars/file |
| Global | `~/.codeium/windsurf/memories/global_rules.md` | 6,000 chars |
| System (macOS) | `/Library/Application Support/Windsurf/rules/*.md` | Enterprise |
| System (Linux) | `/etc/windsurf/rules/*.md` | Enterprise |

**Total combined limit:** 12,000 characters (global + workspace).

**Workflows** (`.windsurf/workflows/*.md`):
- Manual-only invocation via `/workflow-name`.
- 12,000 character max per file.
- Can nest (call other workflows).
- Stored in `.windsurf/workflows/` (workspace) or `~/.codeium/windsurf/global_workflows/` (global).

**Cross-compatibility:** Windsurf also reads root-level `AGENTS.md` files (always-on).

### Tool/Capability System
- Cascade operates in Write Mode, Chat Mode, and Turbo Mode.
- Plan Mode for detailed implementation plans before coding.
- Agent Skills support (as of Jan 2026).
- No user-defined tool restrictions per rule.

### Permissions
- No granular per-agent permission model.
- `.codeiumignore` for file exclusions.
- Enterprise ignore rules at `~/.codeium/.codeiumignore`.

### Multi-Agent Orchestration
- No multi-agent orchestration.
- Single Cascade agent with mode switching.

### Distribution
- `.windsurf/rules/` checked into git.
- Enterprise system-level rules for org-wide policies.
- No plugin/marketplace for rules.

---

## 7. aider

### Popularity
- **GitHub Stars:** ~39K+ (Aider-AI/aider)
- **Open Source:** Yes (Apache 2.0)
- **Philosophy:** Model-agnostic, git-native (auto-commits every change)

### Agent Definition Format

Aider does **not** define "agents" as discrete entities. Instead it uses **conventions files** and **YAML configuration**.

**CONVENTIONS.md:**
Plain Markdown loaded as read-only context:

```markdown
# Coding Conventions

- Use httpx instead of requests for HTTP calls
- All functions must have type hints
- Use pytest for testing, not unittest
- Prefer dataclasses over plain dicts for structured data
```

**Loading methods:**
```bash
aider --read CONVENTIONS.md
```

Or in `.aider.conf.yml`:
```yaml
read: CONVENTIONS.md
# Or multiple:
read: [CONVENTIONS.md, STYLE_GUIDE.md]
```

**`.aider.conf.yml` Configuration (searched in: home dir, git root, CWD):**

```yaml
model: claude-sonnet-4-20250514
auto-commits: true
lint-cmd: ruff check
test-cmd: pytest
edit-format: diff
read: CONVENTIONS.md
yes-always: false
vim: false
chat-language: english
```

**`.aider.model.settings.yml` (model-specific overrides):**

```yaml
- name: claude-sonnet-4-20250514
  edit_format: diff
  weak_model_name: claude-haiku-4-20250414
  use_repo_map: true
  send_undo_reply: true
  examples_as_sys_msg: true
```

**Chat Modes:** `code`, `architect`, `ask`, `help` -- switchable at runtime.

### Tool/Capability System
- File editing (whole file or diff-based)
- Shell command suggestions (opt-in)
- Linting integration
- Test running
- Git operations (auto-commit, diff analysis)
- Tree-sitter for repo mapping
- No MCP support

### Permissions
- Commands require user confirmation by default.
- `--yes-always` to auto-approve everything.
- Shell commands are suggested, not auto-executed.
- No granular per-tool permission system.

### Multi-Agent Orchestration
- **Architect mode:** Two-model system where a "big" model plans and a "small" model implements.
- No true multi-agent parallelism.
- No subagent spawning.

### Distribution
- pip/pipx installable.
- `.aider.conf.yml` checked into repos.
- Community conventions repo on GitHub for sharing templates.

---

## 8. GitHub Copilot (Coding Agent + CLI)

### Popularity
- **Users:** ~15M developers
- **Market Position:** Most widely adopted AI coding tool
- **Pricing:** Included with Copilot Individual/Business/Enterprise plans
- **Coding Agent:** GA since September 2025

### Agent Definition Format

Custom agents are Markdown files with YAML frontmatter in `.github/agents/`.

**Complete Frontmatter Schema:**

```yaml
---
name: "API Developer"                    # Optional. Display name.
description: "Builds REST API endpoints" # Required. Purpose and capabilities.
target: vscode                           # Optional. vscode|github-copilot|both (default)
tools:                                   # Optional. List or "*" for all.
  - code_editing
  - terminal
  - file_search
model: claude-sonnet-4-20250514          # Optional. Specific model.
disable-model-invocation: false          # Optional. Prevent auto-delegation.
user-invocable: true                     # Optional. Allow manual selection.
mcp-servers:                             # Optional. Additional MCP servers.
  my-server:
    type: stdio
    command: npx
    args: ["-y", "my-mcp-server"]
    tools: ["tool1", "tool2"]
    env:
      API_KEY: "${secrets.API_KEY}"
metadata:                                # Optional. Annotation name/value pairs.
  team: backend
  version: "1.2"
---

You are an API development specialist. When building endpoints:
1. Follow RESTful conventions
2. Include input validation
3. Add comprehensive error handling
4. Write integration tests

(Max 30,000 characters for this section)
```

**Filename constraints:** Only `.`, `-`, `_`, `a-z`, `A-Z`, `0-9` allowed.

**Related Files:**
- `.github/copilot-instructions.md` -- repository-wide custom instructions (always active).
- `AGENTS.md` at repo root -- supported since August 2025 for broader instructions.

### Tool/Capability System
- Built-in tools: code editing, terminal, file search, browser.
- MCP server integration per agent via `mcp-servers` field.
- Specialized built-in agents: Explore (codebase analysis), Task (command execution).
- Skills system for specialized task enhancement.
- Hooks for shell commands at key execution points.

### Permissions
- `disable-model-invocation`: prevent Copilot from auto-selecting an agent.
- `user-invocable`: control whether users can manually select the agent.
- Content exclusions not respected by coding agent (documented limitation).
- No per-tool permission granularity.

### Multi-Agent Orchestration
- Copilot can delegate to specialized agents automatically.
- Multiple agents can run in parallel.
- CLI agents: Explore and Task run as sub-agents.
- No formal subagent-spawning control like Claude Code's `Agent(type)` syntax.

### Distribution
- `.github/agents/` checked into repos.
- `.github/copilot-instructions.md` for repo-wide context.
- GitHub Marketplace for Copilot extensions (separate from agent files).
- Enterprise: org-level policies and instructions.

---

## Cross-Platform Comparison Matrix

| Feature | Claude Code | Codex CLI | Cline | Cursor | Windsurf | aider | Copilot |
|---------|-------------|-----------|-------|--------|----------|-------|---------|
| **Agent File Format** | `.md` + YAML FM | `SKILL.md` + YAML FM | `.clinerules` (plain) | `.mdc` + YAML FM | `.md` + YAML FM | `.yml` config | `.md` + YAML FM |
| **Agent Directory** | `.claude/agents/` | `.agents/skills/` | `.clinerules/` | `.cursor/rules/` | `.windsurf/rules/` | N/A | `.github/agents/` |
| **Global Instructions** | `CLAUDE.md` | `AGENTS.md` | VS Code settings | User Rules (UI) | `global_rules.md` | `CONVENTIONS.md` | `copilot-instructions.md` |
| **Tool Restrictions** | Per-agent allowlist/denylist | Per-skill dependencies | MCP keyword routing | None per rule | None per rule | None | Per-agent tool list |
| **Permission Modes** | 5 modes per agent | 3 approval policies | Human-in-the-loop | Global only | None | `--yes-always` | 2 boolean flags |
| **Multi-Agent** | Subagents + Agent Teams | Skills parallelism | None | None | None | Architect mode | Auto-delegation |
| **Subagent Nesting** | No (1 level) | No | No | No | No | No | No |
| **Model Override** | Per agent | Per profile | Per conversation | No | No | Per session | Per agent |
| **Background Tasks** | Yes (Ctrl+B) | No | No | No | No | No | Yes |
| **Persistent Memory** | user/project/local scopes | No | Memory bank (community) | No | Cascade Memories | No | No |
| **Hooks/Lifecycle** | PreToolUse, PostToolUse, Stop, SubagentStart, SubagentStop | Notify events | None | None | None | None | Custom hooks |
| **MCP Support** | Per-agent inline or reference | Per-skill | Keyword-routed | Global | No | No | Per-agent |
| **Max Content** | Unlimited | 32 KiB combined | Unlimited | Unlimited | 12K chars total | Unlimited | 30K chars/agent |
| **Git Worktree Isolation** | `isolation: worktree` | No | No | No | No | No | No |

## Emerging Standards

### AGENTS.md as Universal Format
Multiple tools now read `AGENTS.md` at repo root: **Codex CLI**, **Cursor**, **Windsurf**, **GitHub Copilot**. This is becoming the cross-platform lowest-common-denominator format -- plain Markdown, no frontmatter, always active.

### Agent Client Protocol (ACP)
Cline CLI 2.0 introduced ACP, standardizing how coding agents and editors communicate (similar to LSP for language servers). Supports JetBrains, Zed, Neovim, Emacs, and any ACP-compliant editor.

### MCP (Model Context Protocol)
Anthropic's MCP has become the standard for tool integration across Claude Code, Codex, Cline, and GitHub Copilot. Cursor and Windsurf have partial support. aider does not support MCP.

---

## Popularity Rankings (March 2026)

| Rank | Tool | GitHub Stars | Users/Installs | Open Source |
|------|------|-------------|----------------|-------------|
| 1 | OpenClaw | ~250K | N/A | Yes |
| 2 | aider | ~39K (repo) | Millions (pip) | Yes |
| 3 | Claude Code | ~71.5K | 5.2M VS Code installs | Partial (CLI open, API closed) |
| 4 | OpenHands | ~68.6K | N/A | Yes |
| 5 | Codex CLI | ~62K | 1M+ Mac app downloads | Yes |
| 6 | Cline | ~59K | 5M+ VS Code installs | Yes |
| 7 | GitHub Copilot | N/A (closed) | 15M developers | No |
| 8 | Cursor | N/A (closed) | 1M+ DAU | No |
| 9 | Windsurf | N/A (closed) | 800K+ active | No |

**Note:** Star counts are volatile and may reflect different dynamics (OpenClaw's rapid rise has been questioned for authenticity). Usage metrics (DAU, commits/day, installs) are more reliable indicators of real adoption.

---

## Strategic Observations

1. **Claude Code has the most sophisticated agent system** by a significant margin: per-agent tool restrictions, five permission modes, lifecycle hooks, persistent memory, git worktree isolation, background tasks, and subagent spawning control. No other platform comes close in granularity.

2. **Codex CLI's Skills system** is the closest competitor to Claude Code's agents, with structured directory layouts, optional extended YAML config, and MCP tool dependencies per skill.

3. **GitHub Copilot's format** mirrors Claude Code's approach (Markdown + YAML frontmatter in a dotfile directory) but with fewer fields and no permission/hook/memory features.

4. **Cursor and Windsurf** treat rules as passive context injection, not active agent definitions. They lack tool restrictions, permission modes, and multi-agent orchestration.

5. **aider remains model-agnostic** and deliberately simple. It avoids the "agent definition" paradigm entirely, preferring conventions files and YAML config.

6. **Cline's human-in-the-loop default** is its differentiator: every action requires approval. This trades speed for safety but limits autonomous operation.

7. **AGENTS.md is the emerging universal format** for cross-tool compatibility, but it only supports the lowest common denominator (plain text instructions, always active).

8. **No platform supports true subagent nesting.** All limit delegation to one level. Multi-agent orchestration remains session-level, not compositional.
