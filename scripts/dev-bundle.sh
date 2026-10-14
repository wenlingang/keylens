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
  echo "⚠️  Certificate \"Keylens Development\" not found; falling back to ad-hoc signing. You will need to re-grant permission after each rebuild." >&2
  echo "   Run scripts/dev-cert.sh to avoid this." >&2
  codesign --force --sign - "$app"
fi
echo "$app"
