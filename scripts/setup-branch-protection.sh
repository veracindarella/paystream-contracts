#!/usr/bin/env bash
# Applies branch protection rules to `main` and `develop` via the GitHub API.
# Requirements: gh CLI authenticated with repo admin rights.
# Usage: ./scripts/setup-branch-protection.sh [OWNER/REPO]
#
# Required status checks:
# - build: CI build must pass before merging
#
# Protection rules:
# - Strict status checks: required contexts must pass on the target branch
# - PR reviews: requires 1 approving review, dismisses stale reviews
# - No force pushes or deletions allowed
# - Admin enforcement: disabled (admins can bypass)
set -euo pipefail

REPO="${1:-veracindarella/paystream-contracts}"

# Branch protection configuration
# Required status checks: "build" (CI workflow must pass)
PROTECTION_CONFIG='{
  "required_status_checks": {
    "strict": true,
    "contexts": ["build"]
  },
  "enforce_admins": false,
  "required_pull_request_reviews": {
    "required_approving_review_count": 1,
    "dismiss_stale_reviews": true
  },
  "restrictions": null,
  "allow_force_pushes": false,
  "allow_deletions": false
}'

# Function to apply protection to a single branch
# Idempotent: checks if protection already exists before applying
apply_protection() {
  local branch=$1
  echo "Checking branch protection for ${branch} on ${REPO}..."

  # Check if protection already exists
  if gh api --silent "/repos/${REPO}/branches/${branch}/protection" 2>/dev/null; then
    echo "Branch ${branch} already has protection rules. Updating..."
  else
    echo "No existing protection on ${branch}. Applying new rules..."
  fi

  # Apply/update protection
  gh api \
    --method PUT \
    -H "Accept: application/vnd.github+json" \
    "/repos/${REPO}/branches/${branch}/protection" \
    --input - <<< "$PROTECTION_CONFIG"

  echo "Branch protection applied successfully to ${branch}."
}

# Apply protection to both main and develop branches
apply_protection "main"
apply_protection "develop"

echo "All branch protection rules configured successfully."
