#!/usr/bin/env bash
# Verifies a packaged Keylens.app and its .dmg; exits non-zero on the first failed check.
# Usage: scripts/verify-package.sh <app> <dmg> <identity> <version>
set -euo pipefail
app=${1:?usage: verify-package.sh <app> <dmg> <identity> <version>}
dmg=${2:?}
identity=${3:?}
version=${4:?}
bundle_id="io.github.wenlingang.keylens"
fail() { echo "❌ $*" >&2; exit 1; }

archs=$(lipo -archs "$app/Contents/MacOS/Keylens")
[[ " $archs " == *" arm64 "* && " $archs " == *" x86_64 "* ]] \
  || fail "Binary must contain arm64 and x86_64, found: $archs"

codesign --verify --strict --deep "$app" 2>/dev/null || fail "codesign --verify failed for $app"
info=$(codesign -dvv "$app" 2>&1)
grep -qx "Authority=$identity" <<<"$info" || fail "App is not signed by \"$identity\""
grep -qx "Identifier=$bundle_id" <<<"$info" || fail "Signing identifier is not $bundle_id"
# Accessibility permission survives upgrades only if the designated requirement pins the certificate.
requirement=$(codesign -d -r- "$app" 2>&1)
# Self-signed certificates appear as "certificate leaf = H…" or "certificate root = H…"; ad-hoc gives only a cdhash.
grep -Eq 'certificate (leaf|root) = H"' <<<"$requirement" \
  || fail "Designated requirement does not pin a certificate leaf: $requirement"

plist_version=$(/usr/libexec/PlistBuddy -c 'Print :CFBundleShortVersionString' "$app/Contents/Info.plist")
[[ $plist_version == "$version" ]] || fail "Info.plist version is $plist_version, expected $version"

mnt=$(mktemp -d)
hdiutil attach -quiet -nobrowse -readonly -mountpoint "$mnt" "$dmg" || fail "Cannot mount dmg $dmg"
trap 'hdiutil detach -quiet "$mnt" 2>/dev/null || true; rmdir "$mnt" 2>/dev/null || true' EXIT
[[ -d "$mnt/Keylens.app" && -L "$mnt/Applications" ]] \
  || fail "dmg must contain Keylens.app and an Applications link"

echo "✅ Package verified: $identity, $archs, version $version"
