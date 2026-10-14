#!/usr/bin/env bash
# Fails unless the given git tag is exactly "v<keylens-app version>".
# Usage: scripts/check-version.sh <tag>
set -euo pipefail
tag=${1:?usage: check-version.sh <tag>}
version=$("$(dirname "$0")/app-version.sh")
if [[ $tag != "v$version" ]]; then
  echo "❌ Tag \"$tag\" does not match crates/keylens-app/Cargo.toml version \"$version\" (expected \"v$version\")." >&2
  exit 1
fi
echo "✅ Tag $tag matches version $version."
