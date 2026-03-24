#!/usr/bin/env bash
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
AGENTS_SRC="$REPO_ROOT/agents"
COMMANDS_SRC="$REPO_ROOT/commands"
AGENTS_DEST="$REPO_ROOT/.claude/agents"
COMMANDS_DEST="$REPO_ROOT/.claude/commands"

# Legacy paths (still supported for backwards compat)
LEGACY_AGENTS_SRC="$REPO_ROOT/agents/agents"
LEGACY_COMMANDS_SRC="$REPO_ROOT/agents/commands"

mkdir -p "$AGENTS_DEST" "$COMMANDS_DEST"

# Detect APEX_HOME
if [ -z "${APEX_HOME:-}" ]; then
    APEX_HOME="$REPO_ROOT"
fi

echo "APEX_HOME=$APEX_HOME"
echo ""

installed=0

# Install agents (skip README.md, install.sh, and subdirectories)
for f in "$AGENTS_SRC"/apex-*.md; do
    [ -f "$f" ] || continue
    name="$(basename "$f")"
    cp "$f" "$AGENTS_DEST/$name"
    echo "  agent:   $name"
    ((installed++))
done

# Install commands
for f in "$COMMANDS_SRC"/*.md; do
    [ -f "$f" ] || continue
    name="$(basename "$f")"
    cp "$f" "$COMMANDS_DEST/$name"
    echo "  command: $name"
    ((installed++))
done

echo ""
echo "Installed $installed agents/commands into .claude/"
echo ""
echo "Agents (auto-triggered by Claude Code):"
for f in "$AGENTS_SRC"/apex-*.md; do
    [ -f "$f" ] || continue
    name="$(basename "$f" .md)"
    echo "  $name"
done
echo ""
echo "Slash commands:"
for f in "$COMMANDS_SRC"/*.md; do
    [ -f "$f" ] || continue
    name="$(basename "$f" .md)"
    echo "  /$name"
done
echo ""
echo "Setup:"
echo "  export APEX_HOME=$APEX_HOME"
echo "  export LLVM_COV=/opt/homebrew/opt/llvm/bin/llvm-cov      # if using Homebrew LLVM"
echo "  export LLVM_PROFDATA=/opt/homebrew/opt/llvm/bin/llvm-profdata"
echo ""
echo "Then try: /apex"
echo ""
echo "Tip: Install as a plugin instead for automatic discovery:"
echo "  claude plugin install github:sahajamoth/apex"
