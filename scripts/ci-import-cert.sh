#!/usr/bin/env bash
# CI only: imports the release certificate from $P12 (base64) / $P12_PASSWORD into a temporary keychain
# at $RUNNER_TEMP/release.keychain-db and trusts it for code signing.
set -euo pipefail
: "${P12:?RELEASE_CERT_P12 secret is not set}" "${P12_PASSWORD:?RELEASE_CERT_PASSWORD secret is not set}"
: "${RUNNER_TEMP:?}"
keychain="$RUNNER_TEMP/release.keychain-db"
kc_pass=$(openssl rand -hex 16)
p12="$RUNNER_TEMP/release.p12"
pem="$RUNNER_TEMP/release.cer.pem"
trap 'rm -f "$p12" "$pem"' EXIT

printf '%s' "$P12" | base64 --decode > "$p12"
security create-keychain -p "$kc_pass" "$keychain"
security set-keychain-settings -lut 21600 "$keychain"
security unlock-keychain -p "$kc_pass" "$keychain"
# -x: the private key is non-extractable from this keychain; codesign can still use it.
security import "$p12" -k "$keychain" -P "$P12_PASSWORD" -T /usr/bin/codesign -x
security set-key-partition-list -S apple-tool:,apple: -s -k "$kc_pass" "$keychain" >/dev/null
# Prepend to the user search list so codesign finds the identity.
security list-keychains -d user -s "$keychain" $(security list-keychains -d user | tr -d '"')

/usr/bin/openssl pkcs12 -in "$p12" -nokeys -passin "pass:$P12_PASSWORD" -out "$pem"
sudo security add-trusted-cert -d -r trustRoot -p codeSign -k /Library/Keychains/System.keychain "$pem"
security find-identity -v -p codesigning "$keychain"
