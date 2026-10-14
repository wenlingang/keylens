#!/usr/bin/env bash
# Tests verify-package.sh against hand-built good and broken bundles.
# Needs the local "Keylens Development" certificate (scripts/dev-cert.sh); skips otherwise.
set -euo pipefail
root="$(cd "$(dirname "$0")/../.." && pwd)"
verify="$root/scripts/verify-package.sh"
dev="Keylens Development"
if ! security find-identity -v -p codesigning | grep -q "\"$dev\""; then
  echo "skip - verify-package tests (no \"$dev\" certificate)"; exit 0
fi

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
echo 'int main(void){return 0;}' > "$work/main.c"

# make_case <name> <archs: "arm64 x86_64"> <sign: dev|adhoc> <bundle-id> <version>
make_case() {
  local name=$1 archs=$2 sign=$3 id=$4 ver=$5
  local app="$work/$name/Keylens.app" flags=()
  for a in $archs; do flags+=(-arch "$a"); done
  mkdir -p "$app/Contents/MacOS"
  cc "${flags[@]}" -o "$app/Contents/MacOS/Keylens" "$work/main.c"
  cat > "$app/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
  <key>CFBundleIdentifier</key><string>$id</string>
  <key>CFBundleExecutable</key><string>Keylens</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>$ver</string>
</dict></plist>
PLIST
  if [[ $sign == dev ]]; then codesign --force --sign "$dev" "$app" 2>/dev/null
  else codesign --force --sign - "$app" 2>/dev/null; fi
  mkdir -p "$work/$name/stage"
  cp -R "$app" "$work/$name/stage/"
  ln -s /Applications "$work/$name/stage/Applications"
  hdiutil create -quiet -volname Keylens -srcfolder "$work/$name/stage" -ov -format UDZO "$work/$name/K.dmg"
}

ID=io.github.wenlingang.keylens
# expect <name> <identity> <version> <pass|keyword>
expect() {
  local name=$1 identity=$2 ver=$3 want=$4 out
  if out=$("$verify" "$work/$name/Keylens.app" "$work/$name/K.dmg" "$identity" "$ver" 2>&1); then
    [[ $want == pass ]] && { echo "ok - $name passes"; return; }
    echo "FAIL - $name should fail with '$want'"; exit 1
  fi
  [[ $want != pass && $out == *"$want"* ]] && { echo "ok - $name fails: $want"; return; }
  echo "FAIL - $name: expected '$want', got: $out"; exit 1
}

make_case good      "arm64 x86_64" dev   $ID            1.2.3
make_case arm-only  "arm64"        dev   $ID            1.2.3
make_case adhoc     "arm64 x86_64" adhoc $ID            1.2.3
make_case wrong-id  "arm64 x86_64" dev   dev.other.app  1.2.3

expect good     "$dev"            1.2.3 pass
expect arm-only "$dev"            1.2.3 "arm64 and x86_64"
expect adhoc    "$dev"            1.2.3 "not signed by"
expect good     "Keylens Release" 1.2.3 "not signed by"
expect wrong-id "$dev"            1.2.3 "identifier"
expect good     "$dev"            9.9.9 "version"

# A dmg without the Applications link must fail.
rm "$work/good/stage/Applications"
hdiutil create -quiet -volname Keylens -srcfolder "$work/good/stage" -ov -format UDZO "$work/good/K.dmg"
expect good "$dev" 1.2.3 "dmg"
