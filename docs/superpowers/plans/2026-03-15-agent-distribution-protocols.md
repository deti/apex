# Agent Distribution Protocols & Registries — Research

**Date:** 2026-03-15
**Status:** Research complete

---

## 1. A2A Protocol (Google / Linux Foundation)

**What:** Agent-to-Agent protocol for inter-agent communication and discovery. Complementary to MCP (which is agent-to-tool). A2A is agent-to-agent.

**How it works:**
- Each agent publishes an **Agent Card** — a JSON metadata document describing identity, capabilities, skills, endpoint, and auth requirements.
- Discovery: agents find each other via published cards (likely `.well-known/agent.json` pattern, analogous to `.well-known/openid-configuration`).
- Communication is HTTP-based. Supports streaming, push notifications, and cryptographic card signatures.

**Agent Card schema:**
```json
{
  "agentProvider": {
    "name": "APEX",
    "description": "Autonomous path exploration and code coverage analysis",
    "icon": "https://apex.example.com/icon.png"
  },
  "capabilities": {
    "streaming": false,
    "pushNotifications": false,
    "extendedAgentCard": false
  },
  "skills": [
    {
      "name": "analyze-coverage",
      "description": "Run coverage gap analysis on a codebase"
    },
    {
      "name": "ratchet",
      "description": "CI coverage gate — fail if coverage drops"
    }
  ],
  "interfaces": [{ "type": "http" }],
  "securitySchemes": [{ "type": "none" }],
  "signature": null
}
```

**What APEX needs:** An HTTP server mode (or SSE/streamable-http transport) to act as an A2A agent. The current CLI would need a thin server wrapper.

**Registration:** No central registry — agents discover each other via published cards at known URLs. Self-hosted.

---

## 2. agents.md — Open Standard for Agent Instructions

**What:** A Markdown file placed at the root of a repository that tells AI coding agents how to work with the project. Stewarded by the Agentic AI Foundation under the Linux Foundation. 60,000+ repos use it. Supported by GitHub Copilot, Claude, Cursor, and others.

**Format:** Plain Markdown. No required fields. No JSON schema. Just human-readable instructions.

**Where to place:** Repository root as `AGENTS.md`. In monorepos, nested files in subdirectories take precedence.

**Note:** This is NOT an agent registration protocol. It is instructions *for* agents, not *about* agents. APEX already has the equivalent in `CLAUDE.md`. An `AGENTS.md` for the APEX repo would tell coding agents how to contribute to APEX — it would not register APEX as a distributable agent.

**Relevance to APEX:** Low for distribution. We already have `CLAUDE.md`. Could add an `AGENTS.md` that mirrors it for broader agent compatibility.

---

## 3. MCP Registry (Official — modelcontextprotocol.io)

**What:** The official "app store for MCP servers." Centralized discovery, decentralized distribution. Metadata points to packages in npm/PyPI/Docker/OCI — the registry does not host binaries.

**Publishing workflow:**
1. Build the `mcp-publisher` CLI from the registry repo
2. Authenticate via GitHub OAuth, GitHub OIDC (for CI), DNS verification, or HTTP verification
3. Namespace is validated (e.g., `io.github.allexdav2/apex` requires GitHub auth as that user)
4. Server metadata validated against `server.schema.json`
5. Published to PostgreSQL-backed registry

**ServerJSON format (what APEX needs):**
```json
{
  "name": "io.github.allexdav2/apex",
  "title": "APEX Coverage Analyzer",
  "description": "Autonomous path exploration and code coverage gap analysis",
  "version": "0.1.0",
  "repository": {
    "url": "https://github.com/allexdav2/apex"
  },
  "websiteUrl": "https://github.com/allexdav2/apex",
  "packages": [
    {
      "registryType": "npm",
      "identifier": "@apex-coverage/cli",
      "version": "0.1.0",
      "transport": { "type": "stdio" },
      "runtimeHint": "npx"
    },
    {
      "registryType": "pypi",
      "identifier": "apex-coverage",
      "version": "0.1.0",
      "transport": { "type": "stdio" },
      "runtimeHint": "uvx"
    }
  ]
}
```

**Transport types:** `stdio` (local), `streamable-http`, `sse`

**Package registry types:** npm, pypi, nuget, oci, mcpb

**API endpoints:**
- `GET /v0/servers` — list/search
- `GET /v0/servers/{name}` — latest version
- `GET /v0/servers/{name}/versions/{version}` — specific version

**Status:** API freeze at v0.1. Live at `registry.modelcontextprotocol.io`.

---

## 4. Third-Party MCP Registries

### Smithery.ai
- Web-based directory of MCP servers
- Submit via GitHub repo URL
- Auto-detects configuration from repo
- Good for visibility but not a package manager

### Glama
- Another MCP server directory
- Curated list with categories
- Submit via their website

### mcp.run
- Focuses on hosted/remote MCP servers
- WebAssembly-based server execution
- Different model — runs servers for you rather than distributing them

**Recommendation:** Register on the official MCP registry first. Smithery and Glama are secondary visibility channels.

---

## 5. NPM / PyPI as Agent Distribution

APEX already has both channels set up:

### npm (`npm/package.json`)
- Package: `@apex-coverage/cli`
- Binary wrapper downloads platform-specific binary on postinstall
- Users run: `npx @apex-coverage/cli run --target .`
- For MCP: add `"mcp"` to keywords, add MCP transport config

### PyPI (`python/pyproject.toml`)
- Package: `apex-coverage`
- Similar binary wrapper pattern
- Users run: `pipx run apex-coverage run --target .`
- For MCP: add `mcp-server-` prefix convention (many MCP servers use `mcp-server-*` naming)

**What to add for MCP discoverability:**
- npm: Add keywords `["mcp", "mcp-server", "coverage", "code-analysis"]`
- PyPI: Add classifiers and keywords for MCP
- Both: Ensure the binary supports `--mcp` or `mcp` subcommand for stdio transport

---

## 6. Cursor Marketplace

**What:** Plugin marketplace at `cursor.com/marketplace`. MCP-based. Manually reviewed. Must be open source.

**Directory structure:**
```
apex-cursor-plugin/
├── .cursor-plugin/
│   └── plugin.json          # Manifest (only "name" required)
├── rules/                   # .mdc files — AI guidance
├── skills/                  # Specialized agent capabilities
├── agents/                  # Custom agent configurations
├── commands/                # Agent-executable scripts
└── .mcp.json                # MCP server configuration
```

**Manifest (`plugin.json`):**
```json
{
  "name": "apex-coverage",
  "description": "Code coverage gap analysis and test generation guidance",
  "version": "0.1.0",
  "author": { "name": "allexdav2" }
}
```

**Component types:** Rules, Skills, Agents, Commands, MCP Servers, Hooks

**Publishing:** Submit at `cursor.com/marketplace/publish`. Every plugin is manually reviewed before listing. Updates also reviewed. For multi-plugin repos, add `.cursor-plugin/marketplace.json`.

**What APEX needs:** A thin Cursor plugin that wraps the APEX binary as an MCP server, plus rules files for how Cursor's agent should use APEX.

---

## 7. VS Code Marketplace (for Cline/Continue)

**What:** Standard VS Code extension marketplace. Cline and Continue are VS Code extensions that support MCP servers.

**How it works for APEX:**
- APEX does NOT need to be a VS Code extension itself
- Instead, Cline/Continue users configure APEX as an MCP server in their settings
- The MCP server configuration points to the APEX binary

**Configuration (Cline `mcp_settings.json`):**
```json
{
  "mcpServers": {
    "apex": {
      "command": "apex",
      "args": ["mcp"],
      "transport": "stdio"
    }
  }
}
```

**Alternative:** Build a VS Code extension that bundles APEX and provides:
- Extension manifest (`package.json` with VS Code extension fields)
- Activation events
- Commands registered in the command palette
- Publish via `vsce publish` to the VS Code Marketplace

**Recommendation:** Skip building a VS Code extension. Instead, document MCP server configuration for Cline/Continue users. The MCP registry handles discoverability.

---

## Priority Order for APEX

| Priority | Channel | Effort | Reach |
|----------|---------|--------|-------|
| 1 | **MCP Registry** (official) | Medium — need MCP stdio transport | All MCP clients (Claude, Cursor, Cline, Continue, Windsurf) |
| 2 | **Cursor Marketplace** | Low — thin plugin wrapper | Cursor users |
| 3 | **npm/PyPI keywords** | Trivial — metadata update | Package manager search |
| 4 | **Smithery/Glama** | Trivial — web form | Browsing developers |
| 5 | **A2A Agent Card** | High — need HTTP server mode | Multi-agent orchestration |
| 6 | **VS Code Extension** | High — full extension | VS Code users (but MCP covers this) |

**Critical prerequisite:** APEX must implement an MCP stdio transport (`apex mcp` subcommand). This unlocks channels 1-4 simultaneously.

---

## Speakeasy (Bonus)

Speakeasy converts OpenAPI specs into hosted MCP servers. If APEX had an HTTP API with an OpenAPI spec, Speakeasy could auto-generate an MCP server from it. Lower priority — APEX is a CLI tool, not an API service.
