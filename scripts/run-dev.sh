#!/usr/bin/env bash
# Builds Keylens, bundles it into a signed dev .app and launches it. Output goes to target/dev-apps/keylens.log.
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$root"
cargo build -p keylens-app
# Named KeylensDev (menu bar icon "⌘dev", logs in ~/Library/Logs/Keylens-dev) so it never gets
# mixed up with an installed release running at the same time.
pkill -f "dev-apps/KeylensDev.app/Contents/MacOS/KeylensDev" 2>/dev/null || true
app=$(scripts/dev-bundle.sh target/debug/keylens KeylensDev dev.keylens.dev.app | tail -1)
open -n "$app" --stdout "$root/target/dev-apps/keylens.log" --stderr "$root/target/dev-apps/keylens.log"
echo "Started: ${app} (log: target/dev-apps/keylens.log)"
