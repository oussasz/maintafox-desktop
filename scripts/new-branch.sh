#!/usr/bin/env bash
set -euo pipefail

TYPE="${1:-}"
SLUG="${2:-}"

if [[ -z "$TYPE" || -z "$SLUG" ]]; then
  echo "Usage: ./scripts/new-branch.sh <type> <slug>"
  echo "  type: feature | fix | hotfix | chore"
  echo "  slug: lowercase-kebab-case"
  exit 1
fi

if [[ ! "$TYPE" =~ ^(feature|fix|hotfix|chore)$ ]]; then
  echo "Error: type must be one of: feature, fix, hotfix, chore"
  exit 1
fi

if [[ ! "$SLUG" =~ ^[a-z0-9][a-z0-9\-]*[a-z0-9]$ ]]; then
  echo "Error: slug must be lowercase kebab-case. Received: '$SLUG'"
  exit 1
fi

BRANCH_NAME="$TYPE/$SLUG"

if [[ "$TYPE" == "hotfix" ]]; then
  echo "Switching to main and pulling latest..."
  git checkout main && git pull origin main
else
  echo "Switching to develop and pulling latest..."
  git checkout develop && git pull origin develop
fi

echo "Creating branch: $BRANCH_NAME"
git checkout -b "$BRANCH_NAME"

echo ""
echo "Branch created: $BRANCH_NAME"
