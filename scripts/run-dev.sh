#!/usr/bin/env bash
# 编译 Keylens，打包成签名的开发版 .app 并启动。输出写到 target/dev-apps/keylens.log。
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$root"
cargo build -p keylens-app
pkill -f "dev-apps/Keylens.app/Contents/MacOS/Keylens" 2>/dev/null || true
app=$(scripts/dev-bundle.sh target/debug/keylens Keylens dev.keylens.dev.app | tail -1)
open -n "$app" --stdout "$root/target/dev-apps/keylens.log" --stderr "$root/target/dev-apps/keylens.log"
echo "已启动：${app}（日志：target/dev-apps/keylens.log）"
