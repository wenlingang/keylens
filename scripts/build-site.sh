#!/usr/bin/env bash
# Assemble the homepage (site/) with its screenshots and icon into one folder.
# Usage: scripts/build-site.sh [out-dir]   (default: target/site)
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
out="${1:-$root/target/site}"

rm -rf "$out"
mkdir -p "$out/screenshots"
cp "$root"/site/* "$out/"
cp "$root"/docs/screenshots/en-*.png "$out/screenshots/"
cp "$root/assets/AppIcon.png" "$out/"
echo "site built in $out"
