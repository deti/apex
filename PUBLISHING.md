# APEX Marketplace Publishing Checklist

## Prerequisites

- [ ] `apex mcp` subcommand working (committed)
- [ ] npm package published: `npm publish` in `npm/`
- [ ] PyPI package published: `twine upload` in `python/`
- [ ] GitHub Release tagged: `git tag v0.1.0 && git push --tags`

## 1. Official MCP Registry (highest priority)

```bash
# Install publisher
brew install mcp-publisher
# or: cargo install mcp-publisher

# Login
mcp-publisher login github

# Validate
mcp-publisher publish --dry-run --server integrations/mcp-registries/server.json

# Publish
mcp-publisher publish --server integrations/mcp-registries/server.json
```

**File:** `integrations/mcp-registries/server.json`
**URL:** https://registry.modelcontextprotocol.io

## 2. Smithery.ai

1. Go to https://smithery.ai/new
2. Sign in with GitHub
3. Select `allexdav2/apex` repo
4. Smithery reads `smithery.yaml` from repo root (copy `integrations/smithery.yaml` to repo root first)
5. Submit

**File:** `integrations/smithery.yaml` (copy to repo root before submitting)

## 3. Glama.ai

Automatic — indexes from GitHub. Ensure:
- [ ] README mentions "MCP server"
- [ ] Keywords include "mcp" in package metadata
- [ ] `apex mcp` documented in README

**URL:** https://glama.ai/mcp/servers (auto-indexed)

## 4. Cline MCP Marketplace

1. Open issue at https://github.com/cline/mcp-marketplace
2. Provide:
   - GitHub repo URL: https://github.com/allexdav2/apex
   - Logo: 400x400 PNG
   - Description: "Code coverage & security analysis for 11 languages"
3. Wait ~2 days for review

## 5. Cursor Directory

1. Go to https://cursor.directory/plugins
2. Submit APEX with:
   - MCP config: `{ "command": "apex", "args": ["mcp"] }`
   - Description
   - GitHub URL

## 6. npm Discoverability

Ensure `npm/package.json` has:
```json
{
  "keywords": ["mcp", "mcp-server", "model-context-protocol", "coverage", "security", "sast"],
  "bin": { "apex": "./bin/apex" }
}
```

## Post-Publishing Verification

```bash
# Test that MCP clients can find APEX:

# Cursor: add to .cursor/mcp.json
{ "mcpServers": { "apex": { "command": "apex", "args": ["mcp"] } } }

# Codex: add to .codex/config.toml
# [mcp_servers.apex]
# type = "stdio"
# command = "apex"
# args = ["mcp"]

# Test manually:
echo '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"test","version":"1"}}}' | apex mcp
```
