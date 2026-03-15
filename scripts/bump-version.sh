#!/usr/bin/env bash
set -euo pipefail

# Usage: ./scripts/bump-version.sh <new-version>
# Example: ./scripts/bump-version.sh 0.2.0
#
# Updates version in all distribution manifests, stamps CHANGELOG.md,
# commits, tags, and prints next steps.

if [ $# -ne 1 ]; then
  echo "Usage: $0 <new-version>"
  echo "Example: $0 0.2.0"
  exit 1
fi

NEW_VERSION="$1"

# Validate semver format
if ! echo "$NEW_VERSION" | grep -qE '^[0-9]+\.[0-9]+\.[0-9]+$'; then
  echo "Error: version must be semver (e.g., 0.2.0)"
  exit 1
fi

OLD_VERSION=$(grep '^version = ' Cargo.toml | head -1 | sed 's/.*"\(.*\)".*/\1/')
echo "Bumping $OLD_VERSION → $NEW_VERSION"

# 1. Cargo.toml (workspace version)
sed -i.bak "s/^version = \"$OLD_VERSION\"/version = \"$NEW_VERSION\"/" Cargo.toml
rm -f Cargo.toml.bak

# 2. npm/package.json
sed -i.bak "s/\"version\": \"$OLD_VERSION\"/\"version\": \"$NEW_VERSION\"/" npm/package.json
rm -f npm/package.json.bak

# 3. python/pyproject.toml
sed -i.bak "s/^version = \"$OLD_VERSION\"/version = \"$NEW_VERSION\"/" python/pyproject.toml
rm -f python/pyproject.toml.bak

# 4. python/apex_cli/__init__.py
sed -i.bak "s/__version__ = \"$OLD_VERSION\"/__version__ = \"$NEW_VERSION\"/" python/apex_cli/__init__.py
rm -f python/apex_cli/__init__.py.bak

# 5. HomebrewFormula/apex.rb
sed -i.bak "s/version \"$OLD_VERSION\"/version \"$NEW_VERSION\"/" HomebrewFormula/apex.rb
rm -f HomebrewFormula/apex.rb.bak

# 6. Stamp CHANGELOG.md — replace [Unreleased] with version + date
TODAY=$(date +%Y-%m-%d)
sed -i.bak "s/^## \[Unreleased\]/## [Unreleased]\n\n## [$NEW_VERSION] — $TODAY/" CHANGELOG.md
rm -f CHANGELOG.md.bak

# Verify all versions match
echo ""
echo "Version locations:"
echo "  Cargo.toml:              $(grep '^version = ' Cargo.toml | head -1)"
echo "  npm/package.json:        $(grep '"version"' npm/package.json | head -1 | xargs)"
echo "  python/pyproject.toml:   $(grep '^version = ' python/pyproject.toml | head -1)"
echo "  python/__init__.py:      $(grep '__version__' python/apex_cli/__init__.py)"
echo "  HomebrewFormula/apex.rb:  $(grep 'version "' HomebrewFormula/apex.rb | head -1 | xargs)"
echo "  CHANGELOG.md:            $(grep "## \[$NEW_VERSION\]" CHANGELOG.md)"

echo ""
echo "Next steps:"
echo "  1. Review changes:  git diff"
echo "  2. Commit:          git add -A && git commit -m 'chore: bump version to $NEW_VERSION'"
echo "  3. Create PR:       gh pr create --title 'Release v$NEW_VERSION'"
echo "  4. After merge:     git tag v$NEW_VERSION && git push --tags"
echo "  5. CI builds release binaries automatically"
echo "  6. Update Homebrew sha256 after release assets are uploaded"
