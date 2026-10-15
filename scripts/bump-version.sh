#!/usr/bin/env bash
set -euo pipefail

# Usage: ./scripts/bump-version.sh [major|minor|patch] or ./scripts/bump-version.sh <version>
#        ./scripts/bump-version.sh --check   (exit non-zero unless all version files agree)
# Examples:
#   ./scripts/bump-version.sh patch   -> 0.1.14 => 0.1.15
#   ./scripts/bump-version.sh minor   -> 0.1.14 => 0.2.0
#   ./scripts/bump-version.sh major   -> 0.1.14 => 1.0.0
#   ./scripts/bump-version.sh 2.0.0   -> sets version to 2.0.0

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
# The version files live at the repository root, one level above scripts/
ROOT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"

# Files containing the version
PACKAGE_JSON="$ROOT_DIR/package.json"
CARGO_TOML="$ROOT_DIR/src-tauri/Cargo.toml"
TAURI_CONF="$ROOT_DIR/src-tauri/tauri.conf.json"

# Each file's own current version. They are read separately because files can drift apart;
# searching every file for package.json's version would silently skip a file that has.
json_version() { grep -o '"version": "[^"]*"' "$1" | head -1 | cut -d'"' -f4; }
cargo_version() { sed -n '/^\[package\]/,/^\[/{s/^version *= *"\(.*\)"/\1/p;}' "$1" | head -1; }

PACKAGE_VERSION=$(json_version "$PACKAGE_JSON")
CARGO_VERSION=$(cargo_version "$CARGO_TOML")
TAURI_VERSION=$(json_version "$TAURI_CONF")

# ./scripts/bump-version.sh --check: fail unless all three files agree (used by CI).
if [[ "${1:-}" == "--check" ]]; then
  if [[ -n "$PACKAGE_VERSION" && "$PACKAGE_VERSION" == "$CARGO_VERSION" && "$PACKAGE_VERSION" == "$TAURI_VERSION" ]]; then
    echo "Versions in sync: $PACKAGE_VERSION"
    exit 0
  fi
  echo "Version files disagree:" >&2
  echo "  package.json              $PACKAGE_VERSION" >&2
  echo "  src-tauri/Cargo.toml      $CARGO_VERSION" >&2
  echo "  src-tauri/tauri.conf.json $TAURI_VERSION" >&2
  echo "Run ./scripts/bump-version.sh <version> to set them all." >&2
  exit 1
fi

CURRENT="$PACKAGE_VERSION"

if [[ -z "$CURRENT" ]]; then
  echo "Error: Could not read current version from package.json"
  exit 1
fi

echo "Current version: $CURRENT"
if [[ "$CURRENT" != "$CARGO_VERSION" || "$CURRENT" != "$TAURI_VERSION" ]]; then
  echo "Warning: the version files disagree (Cargo.toml: $CARGO_VERSION, tauri.conf.json: $TAURI_VERSION); all three will be set to the new version."
fi

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

# Update each file from its own current version
sed -i.bak "s/\"version\": \"$PACKAGE_VERSION\"/\"version\": \"$NEW\"/" "$PACKAGE_JSON" && rm -f "$PACKAGE_JSON.bak"

# Cargo.toml: only the version in [package], not dependency versions. (The range form works
# in both GNU and BSD sed; the GNU-only "0,/re/" address silently does nothing on macOS.)
sed -i.bak "/^\[package\]/,/^\[/ s/^version = \"$CARGO_VERSION\"/version = \"$NEW\"/" "$CARGO_TOML" && rm -f "$CARGO_TOML.bak"

sed -i.bak "s/\"version\": \"$TAURI_VERSION\"/\"version\": \"$NEW\"/" "$TAURI_CONF" && rm -f "$TAURI_CONF.bak"

# Cargo.lock records the crate's own version, so refresh it now instead of leaving a stray
# lockfile change for the next build (offline: only the workspace crate's entry changes)
LOCK_UPDATED=0
if command -v cargo >/dev/null 2>&1 && [[ -f "$ROOT_DIR/src-tauri/Cargo.lock" ]]; then
  if (cd "$ROOT_DIR/src-tauri" && cargo update --workspace --offline >/dev/null 2>&1); then
    LOCK_UPDATED=1
  else
    echo "Warning: could not refresh src-tauri/Cargo.lock; run 'cargo update --workspace' in src-tauri before committing." >&2
  fi
fi

# Confirm every file took the new version, so a missed edit is an error, not a silent drift
for FILE_VERSION in "$(json_version "$PACKAGE_JSON")" "$(cargo_version "$CARGO_TOML")" "$(json_version "$TAURI_CONF")"; do
  if [[ "$FILE_VERSION" != "$NEW" ]]; then
    echo "Error: a version file was not updated to $NEW. Check package.json, src-tauri/Cargo.toml, and src-tauri/tauri.conf.json." >&2
    exit 1
  fi
done

echo "Updated version in:"
echo "  - package.json"
echo "  - src-tauri/Cargo.toml"
echo "  - src-tauri/tauri.conf.json"
if [[ "$LOCK_UPDATED" == "1" ]]; then echo "  - src-tauri/Cargo.lock"; fi

# Prompt for git tag
read -rp "Create git commit and tag v$NEW? [y/N] " REPLY
if [[ "$REPLY" =~ ^[Yy]$ ]]; then
  git add "$PACKAGE_JSON" "$CARGO_TOML" "$TAURI_CONF"
  if [[ "$LOCK_UPDATED" == "1" ]]; then git add "$ROOT_DIR/src-tauri/Cargo.lock"; fi
  git commit -m "Bump version to $NEW"
  git tag "v$NEW"
  echo "Created commit and tag v$NEW"
  echo "Run 'git push && git push --tags' to trigger a release."
fi
