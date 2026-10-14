#!/usr/bin/env bash
# Tests for app-version.sh and check-version.sh.
set -euo pipefail
cd "$(dirname "$0")/../.."

version=$(scripts/app-version.sh)
expected=$(sed -n 's/^version = "\(.*\)"$/\1/p' crates/keylens-app/Cargo.toml | head -1)
[[ $version == "$expected" && $version =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] \
  && echo "ok - app-version prints the package version" \
  || { echo "FAIL - app-version printed '$version', expected '$expected'"; exit 1; }

scripts/check-version.sh "v$version" >/dev/null \
  && echo "ok - matching tag passes" || { echo "FAIL - matching tag"; exit 1; }

for bad in "v9.9.9" "$version" "v$version-rc1"; do
  if scripts/check-version.sh "$bad" 2>/dev/null; then
    echo "FAIL - tag '$bad' should be rejected"; exit 1
  fi
  echo "ok - rejects '$bad'"
done
