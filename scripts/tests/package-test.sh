#!/usr/bin/env bash
# Tests package.sh: refuses a missing identity; with the dev certificate, produces a verified dmg.
set -euo pipefail
root="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$root"

if out=$(scripts/package.sh --identity "No Such Identity 7f3a" 2>&1); then
  echo "FAIL - missing identity should fail"; exit 1
fi
[[ $out == *'"No Such Identity 7f3a" not found'* ]] \
  && echo "ok - missing identity fails before building" \
  || { echo "FAIL - unexpected output: $out"; exit 1; }

# The release identity requires the committed certificate pin, checked before anything is built.
if [[ ! -e scripts/release-cert.sha1 ]]; then
  if out=$(scripts/package.sh --identity "Keylens Release" 2>&1); then
    echo "FAIL - release identity without a pin file should fail"; exit 1
  fi
  [[ $out == *"release-cert.sha1"* && $out != *Compiling* ]] \
    && echo "ok - release identity without scripts/release-cert.sha1 fails before building" \
    || { echo "FAIL - unexpected output: $out"; exit 1; }
else
  echo "skip - missing-pin check (scripts/release-cert.sha1 exists)"
fi

dev="Keylens Development"
if ! security find-identity -v -p codesigning | grep -q "\"$dev\""; then
  echo "skip - full package build (no \"$dev\" certificate)"; exit 0
fi
version=$(scripts/app-version.sh)
BUILD_NUMBER=42 scripts/package.sh --identity "$dev"
dist=target/release-dist
plist="$dist/Keylens.app/Contents/Info.plist"
check() { [[ $2 == "$3" ]] && echo "ok - $1" || { echo "FAIL - $1: got '$2', want '$3'"; exit 1; }; }
check "dmg exists" "$(ls "$dist/Keylens-$version.dmg")" "$dist/Keylens-$version.dmg"
check "CFBundleVersion" "$(/usr/libexec/PlistBuddy -c 'Print :CFBundleVersion' "$plist")" 42
check "LSMinimumSystemVersion" "$(/usr/libexec/PlistBuddy -c 'Print :LSMinimumSystemVersion' "$plist")" 11.0
check "icon" "$(ls "$dist/Keylens.app/Contents/Resources/AppIcon.icns")" "$dist/Keylens.app/Contents/Resources/AppIcon.icns"
(cd "$dist" && shasum -a 256 -c "Keylens-$version.dmg.sha256" >/dev/null) \
  && echo "ok - sha256 file matches" || { echo "FAIL - sha256"; exit 1; }
