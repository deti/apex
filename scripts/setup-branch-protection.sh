#!/usr/bin/env bash
set -euo pipefail

# Sets up branch protection rules for PR-driven development.
# Requires: gh CLI authenticated with admin access.
#
# What this enables:
#   - Direct pushes to main are blocked
#   - All changes go through pull requests
#   - CI must pass before merge
#   - PR branch must be up to date with main

REPO=$(gh repo view --json nameWithOwner -q .nameWithOwner)
echo "Setting up branch protection for $REPO/main"

gh api repos/"$REPO"/branches/main/protection \
  --method PUT \
  --input - <<'EOF'
{
  "required_status_checks": {
    "strict": true,
    "contexts": ["Check", "Test", "Clippy", "Format", "Changelog"]
  },
  "enforce_admins": false,
  "required_pull_request_reviews": null,
  "restrictions": null,
  "allow_force_pushes": false,
  "allow_deletions": false
}
EOF

echo ""
echo "Branch protection enabled on main:"
echo "  - Required checks: Check, Test, Clippy, Format, Changelog"
echo "  - Branch must be up to date before merge"
echo "  - Force pushes blocked"
echo "  - Direct pushes blocked (changes require PRs)"
echo "  - Admin override allowed (enforce_admins: false)"
echo ""
echo "Workflow:"
echo "  1. Create branch:  git worktree add .worktrees/my-feature -b feat/my-feature"
echo "  2. Work in worktree, commit"
echo "  3. Push + PR:      git push -u origin feat/my-feature && gh pr create"
echo "  4. CI passes → merge PR"
echo "  5. Clean up:       git worktree remove .worktrees/my-feature"
