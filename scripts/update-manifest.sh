#!/usr/bin/env bash
# Prints latest.json, the in-app updater's manifest, attached to every GitHub Release. The app reads
# it from releases/latest/download/latest.json (see crates/keylens-app/src/update.rs).
# Usage: scripts/update-manifest.sh <version> <sha256> <size-in-bytes> [<notes-file>]
set -euo pipefail
version=${1:?usage: update-manifest.sh <version> <sha256> <size-in-bytes> [<notes-file>]}
sha=${2:?}
size=${3:?}
notes_file=${4:-}
notes=""
if [[ -n $notes_file ]]; then
  notes=$(<"$notes_file")
fi
jq -n --arg version "$version" --arg dmg "Keylens-$version.dmg" --arg sha256 "$sha" \
  --argjson size "$size" --arg notes "$notes" \
  '{version: $version, dmg: $dmg, sha256: $sha256, size: $size, notes: $notes}'
