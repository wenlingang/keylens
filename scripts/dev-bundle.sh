#!/usr/bin/env bash
# Bundles an executable into a signed .app, for development and debugging that needs Accessibility permission.
# Usage: scripts/dev-bundle.sh <executable> <app-name> <bundle-id>
# Output: target/dev-apps/<app-name>.app
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
