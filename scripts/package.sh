#!/usr/bin/env bash
# Builds a universal, signed Keylens.app and wraps it in a .dmg for release.
# Usage: scripts/package.sh [--identity <name>]   (default identity: "Keylens Release")
# Env:   BUILD_NUMBER  CFBundleVersion (default 0; CI passes github.run_number)
# Output: target/release-dist/{Keylens.app, Keylens-<version>.dmg, Keylens-<version>.dmg.sha256}
set -euo pipefail

identity="Keylens Release"
while [[ $# -gt 0 ]]; do
  case $1 in
    --identity) identity=${2:?--identity needs a value}; shift 2 ;;
    *) echo "Unknown argument: $1" >&2; exit 2 ;;
  esac
done

root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$root"
bundle_id="io.github.wenlingang.keylens"
min_macos="11.0"
build="${BUILD_NUMBER:-0}"
dist="$root/target/release-dist"
app="$dist/Keylens.app"

# Never fall back to ad-hoc signing: users would lose Accessibility permission on every upgrade.
if ! security find-identity -p codesigning | grep -q "\"$identity\""; then
  echo "❌ Signing identity \"$identity\" not found in the keychain search list." >&2
  exit 1
fi

version="$(scripts/app-version.sh)"
dmg="$dist/Keylens-$version.dmg"

export MACOSX_DEPLOYMENT_TARGET="$min_macos"
for target in aarch64-apple-darwin x86_64-apple-darwin; do
  cargo build --release --locked -p keylens-app --target "$target"
done

rm -rf "$dist"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
lipo -create -output "$app/Contents/MacOS/Keylens" \
  target/aarch64-apple-darwin/release/keylens target/x86_64-apple-darwin/release/keylens

iconset="$dist/AppIcon.iconset"
mkdir -p "$iconset"
for size in 16 32 128 256 512; do
  sips -z "$size" "$size" assets/AppIcon.png --out "$iconset/icon_${size}x${size}.png" >/dev/null
  sips -z $((size * 2)) $((size * 2)) assets/AppIcon.png --out "$iconset/icon_${size}x${size}@2x.png" >/dev/null
done
iconutil -c icns "$iconset" -o "$app/Contents/Resources/AppIcon.icns"
rm -rf "$iconset"

cat > "$app/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
  <key>CFBundleIdentifier</key><string>$bundle_id</string>
  <key>CFBundleName</key><string>Keylens</string>
  <key>CFBundleDisplayName</key><string>Keylens</string>
  <key>CFBundleExecutable</key><string>Keylens</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>$version</string>
  <key>CFBundleVersion</key><string>$build</string>
  <key>CFBundleIconFile</key><string>AppIcon</string>
  <key>LSMinimumSystemVersion</key><string>$min_macos</string>
  <key>LSUIElement</key><true/>
  <key>NSHighResolutionCapable</key><true/>
</dict></plist>
PLIST

codesign --force --timestamp=none --sign "$identity" "$app"

staging="$dist/dmg"
mkdir -p "$staging"
cp -R "$app" "$staging/"
ln -s /Applications "$staging/Applications"
hdiutil create -quiet -volname Keylens -srcfolder "$staging" -ov -format UDZO "$dmg"
rm -rf "$staging"
(cd "$dist" && shasum -a 256 "$(basename "$dmg")" > "$(basename "$dmg").sha256")

scripts/verify-package.sh "$app" "$dmg" "$identity" "$version"
echo "$dmg"
