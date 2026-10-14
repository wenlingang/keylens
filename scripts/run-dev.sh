#!/usr/bin/env bash
# Builds Keylens, bundles it into a signed dev .app and launches it. Output goes to target/dev-apps/keylens.log.
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$root"
cargo build -p keylens-app
pkill -f "dev-apps/Keylens.app/Contents/MacOS/Keylens" 2>/dev/null || true
app=$(scripts/dev-bundle.sh target/debug/keylens Keylens dev.keylens.dev.app | tail -1)
open -n "$app" --stdout "$root/target/dev-apps/keylens.log" --stderr "$root/target/dev-apps/keylens.log"
echo "Started: ${app} (log: target/dev-apps/keylens.log)"
