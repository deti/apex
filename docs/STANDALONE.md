# APEX Standalone Installation

APEX works as a standalone CLI binary without Claude Code.
Use this for CI/CD pipelines, scripts, and non-Claude environments.

## Install the Binary

Pick one method:

```bash
# Recommended — macOS and Linux
curl -sSL https://raw.githubusercontent.com/sahajamoth/apex/main/install.sh | sh

# Homebrew
brew install sahajamoth/tap/apex

# npm (runs via npx)
npx @apex-coverage/cli run --target . --lang python

# pip
pipx install apex-coverage

# Nix
nix run github:sahajamoth/apex

# Cargo (from source)
cargo install --git https://github.com/sahajamoth/apex
```

## Verify

```bash
apex doctor    # Check all prerequisites
apex --version # v0.5.0
```

## Initialize

```bash
cd your-project
apex init      # Auto-detect language, venv, toolchain → generates apex.toml
```

## Commands

```bash
# Core
apex run --target . --lang python       # Coverage gap report + test generation
apex audit --target . --lang python     # Security audit (63 detectors, 40+ CWEs)
apex ratchet --target . --min-cov 0.8   # CI gate — fail if coverage drops
apex deploy-score --target .            # Deploy readiness (0-100)
apex doctor                             # Check dependencies

# Intelligence
apex index --target . --lang python     # Per-test branch index
apex test-optimize --target .           # Minimal test subset
apex dead-code --target .               # Unreachable code
apex complexity --target .              # Function complexity hotspots
apex hotpaths --target .                # Most-executed code paths
apex risk --target . --changed-files src/auth.py

# Security
apex attack-surface --target . --lang python
apex verify-boundaries --target . --lang python
apex secret-scan --target . --lang python
apex data-flow --target . --lang python

# Diff & CI
apex diff --target . --base main
apex regression-check --target . --base main
```

## GitHub Actions

```yaml
# .github/workflows/apex.yml
name: APEX Coverage Gate
on: [push, pull_request]
jobs:
  apex:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - name: Install APEX
        run: curl -sSL https://raw.githubusercontent.com/sahajamoth/apex/main/install.sh | sh
      - name: Coverage Gate
        run: apex ratchet --target . --lang python --min-cov 0.8
```

## MCP Server (for Cursor, Windsurf, etc.)

```bash
apex integrate --editor cursor     # Write .cursor/mcp.json
apex integrate --editor windsurf   # Write ~/.codeium/windsurf/mcp_config.json
apex integrate --dry-run           # Preview config
```

## Build from Source

```bash
git clone https://github.com/sahajamoth/apex.git && cd apex
cargo build --release

# Optional features
cargo build --release --features "treesitter"               # tree-sitter CPG
cargo build --release --features "apex-symbolic/z3-solver"   # Z3 solver
cargo build --release --features "apex-fuzz/libafl-backend"  # LibAFL fuzzer
```

## Configuration

See [apex.reference.toml](../apex.reference.toml) for all 80+ options.
