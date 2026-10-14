#!/usr/bin/env bash
# 把一个可执行文件打包成 .app 并签名，供需要辅助功能权限的开发调试使用。
# 用法：scripts/dev-bundle.sh <可执行文件> <应用名> <bundle-id>
# 输出：target/dev-apps/<应用名>.app
set -euo pipefail

exe=$1
name=$2
bundle_id=$3
root="$(cd "$(dirname "$0")/.." && pwd)"
app="$root/target/dev-apps/$name.app"

mkdir -p "$app/Contents/MacOS"
cp "$exe" "$app/Contents/MacOS/$name"
cat > "$app/Contents/Info.plist" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
  <key>CFBundleIdentifier</key><string>$bundle_id</string>
  <key>CFBundleName</key><string>$name</string>
  <key>CFBundleExecutable</key><string>$name</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleVersion</key><string>1</string>
  <key>LSUIElement</key><true/>
</dict></plist>
EOF

if security find-identity -v -p codesigning | grep -q '"Keylens Development"'; then
  codesign --force --sign "Keylens Development" "$app"
else
  echo "⚠️  没有找到「Keylens Development」证书，改用 ad-hoc 签名；重新编译后需要重新授权。" >&2
  echo "   运行 scripts/dev-cert.sh 可以避免这个问题。" >&2
  codesign --force --sign - "$app"
fi
echo "$app"
