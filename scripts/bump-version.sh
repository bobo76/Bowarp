#!/usr/bin/env bash
# Raise the workspace version in Cargo.toml and refresh Cargo.lock.
#
#   scripts/bump-version.sh [patch|minor|major]   (default: patch)
#
# patch: 0.1.0 -> 0.1.1, minor: 0.1.3 -> 0.2.0, major: 0.4.2 -> 1.0.0. Prints the new version.
# Refuses a Cargo.toml with uncommitted changes and a version that isn't X.Y.Z.
set -euo pipefail

part="${1:-patch}"
case "$part" in
  patch | minor | major) ;;
  *)
    echo "usage: $0 [patch|minor|major]" >&2
    exit 2
    ;;
esac

cd "$(dirname "$0")/.."

if ! git diff --quiet HEAD -- Cargo.toml; then
  echo "Cargo.toml has uncommitted changes; commit or stash them first" >&2
  exit 1
fi

# The `version = "..."` line of the [workspace.package] section.
current=$(awk '
  /^\[/ { in_package = ($0 ~ /^\[workspace\.package\]\r?$/) }
  in_package && /^version[[:space:]]*=/ {
    sub(/^version[[:space:]]*=[[:space:]]*"/, ""); sub(/".*/, ""); print; exit
  }
' Cargo.toml)

if [[ ! "$current" =~ ^([0-9]+)\.([0-9]+)\.([0-9]+)$ ]]; then
  echo "workspace version '$current' in Cargo.toml is not X.Y.Z" >&2
  exit 1
fi
major=${BASH_REMATCH[1]} minor=${BASH_REMATCH[2]} patch=${BASH_REMATCH[3]}

case "$part" in
  patch) patch=$((patch + 1)) ;;
  minor) minor=$((minor + 1)) patch=0 ;;
  major) major=$((major + 1)) minor=0 patch=0 ;;
esac
next="$major.$minor.$patch"

awk -v next_version="$next" '
  /^\[/ { in_package = ($0 ~ /^\[workspace\.package\]\r?$/) }
  # Keep a CRLF line ending (a Windows checkout).
  in_package && !done && /^version[[:space:]]*=/ {
    eol = /\r$/ ? "\r" : ""
    print "version = \"" next_version "\"" eol; done = 1; next
  }
  { print }
' Cargo.toml > Cargo.toml.tmp
mv Cargo.toml.tmp Cargo.toml

cargo update --workspace --quiet

echo "$next"
