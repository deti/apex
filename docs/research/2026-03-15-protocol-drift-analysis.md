# Protocol Drift Analysis: Agent Skills, AGENTS.md, OCI Spec, Claude Code Agents, Codex Skills

**Date:** 2026-03-15
**Scope:** Field-level schema evolution and breaking changes across five specification formats

---

## 1. Agent Skills (SKILL.md) — Anthropic

### Timeline

| Date | Event |
|------|-------|
| 2025-09-22 | `anthropics/skills` repo created on GitHub |
| 2025-10-16 | Public announcement of Agent Skills as open standard |
| 2025-12-16 | `agentskills/agentskills` repo created (spec + reference SDK moved here) |
| 2025-12-18 | GitHub Copilot adds Agent Skills support |
| 2025-12-19 | OpenAI Codex CLI adds experimental SKILL.md support |
| 2025-12 | Linux Foundation Agentic AI Foundation (AAIF) formed; Agent Skills contributed |
| 2026-01 | Spring AI, Red Hat, and others adopt the format |
| 2026-03 | 93.8k stars on anthropics/skills; 13.2k stars on agentskills/agentskills |

### SKILL.md Frontmatter Schema (current, no versioned releases)

The spec has **no version number**. The agentskills repo has 63 commits and zero tagged releases.

| Field | Required | Type | Constraints |
|-------|----------|------|-------------|
| `name` | Yes | string | 1-64 chars, `^[a-z0-9]+(-[a-z0-9]+)*$`, must match parent dir name |
| `description` | Yes | string | 1-1024 chars, no angle brackets `<>` |
| `license` | No | string | SPDX identifier recommended |
| `compatibility` | No | string | 1-500 chars, environment requirements |
| `metadata` | No | map(string -> string) | Arbitrary KV pairs for client-specific data |
| `allowed-tools` | No | string (space-delimited) | **Experimental.** Pre-approved tool identifiers |

**Whitelist enforced** — any key outside `{name, description, license, compatibility, metadata, allowed-tools}` fails validation.

### Directory Structure

```
skill-name/
├── SKILL.md          # Required entry point
├── scripts/          # Optional executables
├── references/       # Optional docs (REFERENCE.md, etc.)
├── assets/           # Optional templates, images, data
└── ...               # Any additional files
```

### Schema Drift Notes

- **No version field in the spec itself.** Versioning is handled via `metadata.version` by convention only.
- **No breaking changes observed** — the spec has been additive since launch.
- `allowed-tools` is marked experimental; support varies across implementations.
- The `metadata` field is a deliberate extension point — no key namespacing enforced.
- Progressive disclosure is a design principle, not a schema constraint: frontmatter (~100 tokens) loads at startup; full body loads on activation.

### Platform Adoption

| Platform | Date | Notes |
|----------|------|-------|
| Claude Code | 2025-10 (launch) | Native support, progressive disclosure |
| GitHub Copilot | 2025-12-18 | Via VS Code agent skills |
| OpenAI Codex CLI | 2025-12-19 | Experimental, adds `agents/openai.yaml` sidecar |
| Cursor | 2025-12 | Adopted via AAIF |
| Gemini CLI | 2026-01 | Adopted via AAIF |
| Spring AI | 2026-01 | Java ecosystem integration |
| OpenCode, Amp, Goose, Warp, Zed, RooCode, Aider | Various | Growing ecosystem |

---

## 2. AGENTS.md — OpenAI / AAIF

### Timeline

| Date | Event |
|------|-------|
| 2025-08 | OpenAI releases AGENTS.md with Codex |
| 2025-12 | Contributed to Agentic AI Foundation under Linux Foundation |
| 2025-12 | Adopted by 60k+ open-source projects |
| 2026-03 | Supported by 20+ platforms |

### Format Specification

**AGENTS.md has no schema.** It is deliberately unstructured Markdown.

- No YAML frontmatter required
- No required fields
- No version number
- No validation rules
- No structural constraints beyond "valid Markdown"

### Resolution Rules

- **Hierarchical:** closest AGENTS.md to the edited file wins (monorepo support)
- **Override:** explicit user prompts override AGENTS.md instructions
- **No execution:** agents only run commands if explicitly listed; no auto-execution

### Recommended Content (convention, not enforced)

- Project overview
- Build/test commands
- Code style guidelines
- Architecture documentation
- Security considerations
- Deployment steps

### Structured Frontmatter Proposals

**No formal proposal exists** for adding YAML frontmatter to AGENTS.md. The deliberate design choice is to keep it as plain Markdown, distinguishing it from SKILL.md (which requires frontmatter) and CLAUDE.md (which is also plain Markdown).

### Platform Adoption

| Platform | Notes |
|----------|-------|
| OpenAI Codex (CLI + Cloud) | Native, original creator |
| GitHub Copilot | Native support |
| Claude Code | Reads AGENTS.md files |
| Cursor | Native support |
| Gemini CLI | Native support |
| VS Code | Via Copilot |
| Windsurf, Aider, Zed, Warp, RooCode | Growing ecosystem |
| Jules, Devin, Factory, Amp | Enterprise adoption |

### Drift Assessment

AGENTS.md is **drift-resistant by design** — having no schema means nothing can break. The tradeoff is that there is no machine-readable metadata for tooling to consume.

---

## 3. OCI Specification — v1.0 to v1.1

### Image Spec: Manifest Field Comparison

| Field | v1.0 | v1.1 | Change |
|-------|------|------|--------|
| `schemaVersion` | Required (= 2) | Required (= 2) | Unchanged |
| `mediaType` | Optional | Optional | Unchanged (`application/vnd.oci.image.manifest.v1+json`) |
| `config` | Required (descriptor) | Required (descriptor) | Now allows empty JSON descriptor |
| `layers` | Required (array) | Required (array) | Now allows empty array |
| `annotations` | Optional (map) | Optional (map) | Unchanged |
| `subject` | -- | **New.** Optional descriptor | Links to another manifest (signatures, attestations) |
| `artifactType` | -- | **New.** Optional string | Custom artifact type; **MUST** be set when `config.mediaType` = empty |

### Image Spec: Descriptor Changes

| Field | v1.0 | v1.1 | Change |
|-------|------|------|--------|
| `mediaType` | Required | Required | Unchanged |
| `digest` | Required | Required | Unchanged |
| `size` | Required | Required | Unchanged |
| `urls` | Optional | Optional | Unchanged |
| `annotations` | Optional | Optional | Unchanged |
| `data` | -- | **New.** Optional string | Base64-encoded blob content for small payloads |

### Image Spec: New Media Types in v1.1

| Media Type | Purpose |
|------------|---------|
| `application/vnd.oci.empty.v1+json` | Empty config descriptor (blob = `{}`, size = 2) |
| `application/vnd.oci.image.layer.v1.tar+zstd` | zstd-compressed layer |
| `application/vnd.oci.image.layer.nondistributable.v1.tar+zstd` | zstd ND layer (deprecated) |

### Image Spec: Deprecated in v1.1

- **Non-distributable layer types** — all `nondistributable` media types deprecated. Recommendation: do not create new ones.

### Distribution Spec: API Changes (v1.0 -> v1.1)

| Endpoint/Feature | v1.0 | v1.1 | Change |
|-------------------|------|------|--------|
| `GET /v2/<name>/referrers/<digest>` | -- | **New** | Returns OCI Index of associated manifests |
| `OCI-Subject` response header | -- | **New** | Indicates registry supports referrers API |
| `OCI-Filters-Applied` header | -- | **New** | Indicates server-side filtering |
| `Warning` response header | -- | **New** | Non-error notifications and deprecation signals |
| Referrers Tag Schema fallback | -- | **New** | `sha256-<hex>` tag for v1.0 registries |
| Anonymous blob mount (`from` optional) | -- | **New** | Mount without specifying source repo |
| 413 response code | Implicit | **Explicit** | Registry may return when manifest too large |
| Extensions (leading `_`) | -- | **New** | Custom registry APIs without spec changes |

### What Broke Between Versions

1. **Empty descriptor + artifactType coupling**: If `config.mediaType` = `application/vnd.oci.empty.v1+json`, then `artifactType` MUST be set. Older clients that used `config.mediaType` for artifact typing (e.g., Helm charts) face a semantic conflict — the guidance to always use `artifactType` can break existing artifacts that relied on `config.mediaType` as the type signal.

2. **Referrers Tag Schema race condition**: The fallback tag mechanism uses GET-modify-PUT without conditional requests. Concurrent writers can lose updates — the fallback index ends up missing referrers even though both uploads succeeded. This is a known design flaw in the spec.

3. **Non-distributable layer deprecation**: Tools that generated non-distributable layers now produce deprecated artifacts. Air-gapped scenarios are affected.

4. **Artifact Manifest removal**: The dedicated artifact manifest type was proposed in RCs but **removed from the final v1.1 release** due to portability concerns. Instead, the existing image manifest was extended with `subject` + `artifactType`. This surprised teams that had implemented against RC drafts.

5. **Registry compatibility matrix**: Not all registries implement v1.1 simultaneously. The `subject` field should be "ignored" by v1.0 registries, but in practice some registries reject unknown fields rather than silently dropping them.

### OCI Version Timeline

| Date | Event |
|------|-------|
| 2017-07 | OCI Image Spec v1.0.0 |
| 2017-11 | OCI Distribution Spec v1.0.0 |
| 2021 | OCI Image Spec v1.0.1, v1.0.2 (minor fixes) |
| 2023-01 | v1.1.0-rc1 (introduced artifact manifest type) |
| 2023-07 | v1.1.0-rc3 (artifact manifest removed; subject/artifactType on image manifest) |
| 2024-02 | v1.1.0-rc4 |
| 2024-03-13 | OCI Image Spec v1.1.0 + Distribution Spec v1.1.0 (final release) |

---

## 4. Claude Code Agent (Subagent) Format

### Frontmatter Field Evolution

Reconstructed from changelog entries. Claude Code subagent format has **no spec version number** — it evolves with each CLI release.

| Field | Type | When Added | Notes |
|-------|------|------------|-------|
| `name` | string | Launch (~2025-05) | Required. Lowercase + hyphens |
| `description` | string | Launch | Required. Drives delegation matching |
| `tools` | string (CSV) | Launch | Optional. Inherited if omitted |
| `model` | string | Launch | Optional. `sonnet`/`opus`/`haiku`/`inherit`/full ID |
| `permissionMode` | string | Launch | `default`/`acceptEdits`/`dontAsk`/`bypassPermissions`/`plan` |
| `maxTurns` | number | Launch | Max agentic turns |
| `disallowedTools` | string (CSV) | ~2025 H2 | Denylist counterpart to `tools` |
| `mcpServers` | list | ~2025 H2 | Inline or reference MCP server configs |
| `hooks` | object | ~2025 Q4 | PreToolUse, PostToolUse, Stop events |
| `skills` | list | ~2025 Q4 | Preload skill content into context |
| `memory` | string | v2.1.33 (~2026-02) | `user`/`project`/`local` persistent memory scope |
| `background` | boolean | ~2026-01 | Always run as background task |
| `isolation` | string | ~2026-01 | `worktree` for git worktree isolation |
| `color` | string | Undocumented | `blue`/`purple`/`yellow` — used by `/agents` UI, not in official docs |

### Breaking Changes

| Version | Change | Impact |
|---------|--------|--------|
| v2.1.63 | `Task` tool renamed to `Agent` | `Task(...)` still works as alias |
| v2.1.74 | Full model IDs fixed in frontmatter | Before this, `claude-opus-4-5` was silently ignored |
| v2.1.73 | Bedrock/Vertex model alias routing fixed | `model: opus` was silently downgraded |

### Known Documentation Gaps (per GitHub issue #8501)

- `color` field exists in implementation but is undocumented
- `/agents` command auto-transforms descriptions into multi-line formats with examples — undocumented behavior
- CLI `--agents` accepts JSON; `/agents` command uses Markdown — format mismatch not documented
- AgentDefinition TypeScript SDK type not cross-referenced in CLI docs

### Scope/Location Resolution

| Location | Scope | Priority |
|----------|-------|----------|
| `--agents` CLI flag | Session only | 1 (highest) |
| `.claude/agents/` | Project | 2 |
| `~/.claude/agents/` | User (all projects) | 3 |
| Plugin `agents/` directory | Where plugin enabled | 4 (lowest) |

---

## 5. Codex CLI Skills Format

### Timeline

| Date | Event |
|------|-------|
| 2025-12-19 | Codex CLI adds experimental SKILL.md support (follows Agent Skills spec) |
| 2026-01-14 | GPT-5.2-Codex available |
| 2026-03-05 | CLI 0.110.0: Plugin system loads skills |
| 2026-03-05 | CLI 0.111.0: Sample skill documentation for artifacts |
| 2026-03-11 | CLI 0.114.0: Hooks engine with SessionStart/Stop; option to disable bundled system skills |

### SKILL.md Frontmatter (follows Agent Skills spec)

Same as Section 1 — `name` and `description` required, same constraints.

### Codex-Specific Extension: `agents/openai.yaml`

This is **Codex's sidecar file** extending the base Agent Skills spec. It lives alongside SKILL.md.

```yaml
# agents/openai.yaml
interface:
  display_name: "User-Facing Name"          # Optional
  short_description: "User-facing desc"      # Optional
  icon_small: "assets/icon.svg"              # Optional, SVG
  icon_large: "assets/icon.png"              # Optional, PNG
  brand_color: "#FF5733"                     # Optional, hex
  default_prompt: "Use this skill to..."     # Optional

policy:
  allow_implicit_invocation: true            # Default: true. When false, requires explicit $skill invocation

dependencies:
  tools:                                     # Optional, MCP tool declarations
    - type: "mcp"
      value: "tool-identifier"
      description: "What the tool does"
      transport: "streamable_http"
      url: "https://example.com/mcp"
```

### Skill Discovery Locations (priority order)

| Priority | Path | Scope |
|----------|------|-------|
| 1 | `$CWD/.agents/skills` | Current directory |
| 2 | `$CWD/../.agents/skills` | Parent directory |
| 3 | `$REPO_ROOT/.agents/skills` | Repository root |
| 4 | `$HOME/.agents/skills` | User home |
| 5 | `/etc/codex/skills` | System/admin |
| 6 | Built-in system skills | Bundled |

### Drift from Base Agent Skills Spec

- **Added `agents/openai.yaml` sidecar** — not part of the Agent Skills spec, Codex-specific extension
- **Different discovery paths** — Agent Skills spec does not prescribe discovery; Codex defines its own hierarchy
- **`allow_implicit_invocation` policy** — Codex-specific behavioral control with no equivalent in the base spec
- **MCP tool dependencies** — Codex adds structured tool dependency declarations

---

## Cross-Format Comparison Matrix

| Dimension | Agent Skills (SKILL.md) | AGENTS.md | OCI Spec | Claude Code Agents | Codex Skills |
|-----------|------------------------|-----------|----------|-------------------|-------------|
| **Spec version** | None | None | v1.0, v1.1 | None (CLI version) | None |
| **Governance** | AAIF/Linux Foundation | AAIF/Linux Foundation | OCI/Linux Foundation | Anthropic | OpenAI |
| **Schema type** | YAML frontmatter | Plain Markdown | JSON | YAML frontmatter | YAML frontmatter + YAML sidecar |
| **Required fields** | name, description | None | Per manifest type | name, description | name, description |
| **Extension mechanism** | `metadata` map | N/A (freeform) | annotations, extensions | N/A (added per release) | `agents/openai.yaml` sidecar |
| **Version in schema** | No (metadata convention) | No | schemaVersion=2 | No | No (metadata convention) |
| **Breaking changes** | None observed | N/A | Yes (artifact type coupling, referrers race) | Model ID handling fixed | N/A (too new) |
| **Validation tooling** | skills-ref CLI | None | Container toolchain | None published | None published |
| **Progressive disclosure** | Yes (by design) | No | N/A | Yes (description -> delegation) | Yes (metadata -> full SKILL.md) |

---

## Key Findings

### 1. No Versioning Across Agent Formats
Neither Agent Skills, AGENTS.md, Claude Code agents, nor Codex skills have a formal spec version. This means consumers cannot detect schema changes programmatically. Only OCI uses explicit versioning.

### 2. Extension Points Diverge
- Agent Skills uses `metadata` (flat KV map) — limited but simple
- Codex adds a sidecar YAML file (`agents/openai.yaml`) — richer but platform-specific
- Claude Code adds fields to frontmatter per release — no extension point, just schema growth
- AGENTS.md has no schema to extend

### 3. OCI's Artifact Story Was Painful
The v1.0-to-v1.1 transition took 7 years (2017-2024). The artifact manifest type was proposed, implemented in RCs, then removed in favor of extending the image manifest. Teams building against RCs had to rewrite. The referrers tag schema fallback has a known race condition.

### 4. Claude Code Schema Is Underdocumented
The `color` field, description auto-transformation, and format differences between `--agents` JSON and `/agents` Markdown are not documented. The schema grows with each CLI release without a migration guide.

### 5. Convergence Is Happening at the Foundation Level
Both Agent Skills and AGENTS.md are now under the Agentic AI Foundation (Linux Foundation). The same platforms tend to support both. However, the formats serve different purposes: AGENTS.md is project-level context (unstructured), while Agent Skills are reusable capability modules (structured).
