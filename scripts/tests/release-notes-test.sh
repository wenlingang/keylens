#!/usr/bin/env bash
# Tests release-notes.sh output.
set -euo pipefail
cd "$(dirname "$0")/../.."
notes=$(scripts/release-notes.sh 1.2.3 abc123def)
for want in "Keylens-1.2.3.dmg" "abc123def" "xattr -dr com.apple.quarantine /Applications/Keylens.app" \
            "README.zh-CN.md" "Open Anyway" "x86_64"; do
  [[ $notes == *"$want"* ]] && echo "ok - mentions '$want'" || { echo "FAIL - missing '$want'"; exit 1; }
done
