#!/usr/bin/env bash
# Tests update-manifest.sh output.
set -euo pipefail
cd "$(dirname "$0")/../.."
sha=e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855
notes=$(mktemp)
trap 'rm -f "$notes"' EXIT
printf 'Fixes "quotes"\nand lines\n' > "$notes"
json=$(scripts/update-manifest.sh 1.2.3 "$sha" "$notes")
check() {
  [[ $(jq -r "$1" <<<"$json") == "$2" ]] && echo "ok - $1" || { echo "FAIL - $1: $(jq -r "$1" <<<"$json")"; exit 1; }
}
check .version 1.2.3
check .dmg Keylens-1.2.3.dmg
check .sha256 "$sha"
check .notes $'Fixes "quotes"\nand lines'
[[ $(scripts/update-manifest.sh 1.2.3 "$sha" | jq -r .notes) == "" ]] && echo "ok - notes optional" \
  || { echo "FAIL - notes optional"; exit 1; }
