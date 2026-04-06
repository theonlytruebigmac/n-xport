#!/usr/bin/env bash
set -euo pipefail

# Usage: ./bump-version.sh [major|minor|patch] or ./bump-version.sh <version>
# Examples:
#   ./bump-version.sh patch   -> 0.1.14 => 0.1.15
#   ./bump-version.sh minor   -> 0.1.14 => 0.2.0
#   ./bump-version.sh major   -> 0.1.14 => 1.0.0
#   ./bump-version.sh 2.0.0   -> sets version to 2.0.0

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"

# Files containing the version
PACKAGE_JSON="$SCRIPT_DIR/package.json"
CARGO_TOML="$SCRIPT_DIR/src-tauri/Cargo.toml"
TAURI_CONF="$SCRIPT_DIR/src-tauri/tauri.conf.json"

# Read current version from package.json
CURRENT=$(grep -o '"version": "[^"]*"' "$PACKAGE_JSON" | head -1 | cut -d'"' -f4)

if [[ -z "$CURRENT" ]]; then
  echo "Error: Could not read current version from package.json"
  exit 1
fi

echo "Current version: $CURRENT"

BUMP="${1:-patch}"

IFS='.' read -r MAJOR MINOR PATCH <<< "$CURRENT"

case "$BUMP" in
  major)
    NEW="$((MAJOR + 1)).0.0"
    ;;
  minor)
    NEW="$MAJOR.$((MINOR + 1)).0"
    ;;
  patch)
    NEW="$MAJOR.$MINOR.$((PATCH + 1))"
    ;;
  *)
    # Treat as explicit version
    if [[ "$BUMP" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
      NEW="$BUMP"
    else
      echo "Error: Invalid argument '$BUMP'. Use major, minor, patch, or a version like 1.2.3"
      exit 1
    fi
    ;;
esac

echo "New version:     $NEW"

# Update package.json
sed -i.bak "s/\"version\": \"$CURRENT\"/\"version\": \"$NEW\"/" "$PACKAGE_JSON" && rm -f "$PACKAGE_JSON.bak"

# Update Cargo.toml (only the package version, not dependency versions)
sed -i.bak "0,/^version = \"$CURRENT\"/s//version = \"$NEW\"/" "$CARGO_TOML" && rm -f "$CARGO_TOML.bak"

# Update tauri.conf.json
sed -i.bak "s/\"version\": \"$CURRENT\"/\"version\": \"$NEW\"/" "$TAURI_CONF" && rm -f "$TAURI_CONF.bak"

echo "Updated version in:"
echo "  - package.json"
echo "  - src-tauri/Cargo.toml"
echo "  - src-tauri/tauri.conf.json"

# Prompt for git tag
read -rp "Create git commit and tag v$NEW? [y/N] " REPLY
if [[ "$REPLY" =~ ^[Yy]$ ]]; then
  git add "$PACKAGE_JSON" "$CARGO_TOML" "$TAURI_CONF"
  git commit -m "Bump version to $NEW"
  git tag "v$NEW"
  echo "Created commit and tag v$NEW"
  echo "Run 'git push && git push --tags' to trigger a release."
fi
