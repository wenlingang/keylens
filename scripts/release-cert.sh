#!/usr/bin/env bash
# One-time: creates the long-lived self-signed "Keylens Release" code-signing certificate used by CI.
# It is NOT imported into your keychain. Store the .p12 and password in GitHub secrets and back them up:
# if this certificate is lost or replaced, every user must re-grant Accessibility permission.
# Usage: scripts/release-cert.sh [output-dir]   (default: ~/Desktop/keylens-release-cert)
set -euo pipefail

NAME="Keylens Release"
out="${1:-$HOME/Desktop/keylens-release-cert}"
p12="$out/keylens-release.p12"
# LibreSSL writes PKCS#12 that `security import` accepts; OpenSSL 3 defaults do not.
openssl=/usr/bin/openssl

if [[ -e $p12 ]]; then
  echo "❌ $p12 already exists; refusing to overwrite the release certificate." >&2
  exit 1
fi
mkdir -p "$out"
chmod 700 "$out"
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

cat > "$tmp/cfg" <<EOF
[req]
distinguished_name = dn
x509_extensions = ext
prompt = no
[dn]
CN = ${NAME}
[ext]
basicConstraints = critical,CA:false
keyUsage = critical,digitalSignature
extendedKeyUsage = critical,codeSigning
EOF
"$openssl" req -x509 -newkey rsa:2048 -nodes -days 3650 \
  -keyout "$tmp/key.pem" -out "$out/keylens-release.cer.pem" -config "$tmp/cfg" 2>/dev/null
password=$("$openssl" rand -hex 24)
"$openssl" pkcs12 -export -inkey "$tmp/key.pem" -in "$out/keylens-release.cer.pem" \
  -name "$NAME" -out "$p12" -passout "pass:$password"
printf '%s' "$password" > "$out/password.txt"
base64 -i "$p12" > "$out/keylens-release.p12.base64"
chmod 600 "$out"/*

# Self-check: import into a throwaway keychain, exactly as CI will.
kc="$tmp/check.keychain-db"
security create-keychain -p check "$kc"
security import "$p12" -k "$kc" -P "$password" -T /usr/bin/codesign >/dev/null
security find-identity -p codesigning "$kc" | grep -q "\"$NAME\"" \
  || { echo "❌ Generated .p12 could not be imported as a signing identity." >&2; security delete-keychain "$kc"; exit 1; }
security delete-keychain "$kc"

cat <<EOF
✅ Created "$NAME" in $out

Next steps:
  1. Back up keylens-release.p12 and password.txt in your password manager NOW.
     Losing this certificate forces every user to re-grant Accessibility permission.
  2. Store them as GitHub secrets:
       gh secret set RELEASE_CERT_P12 < "$out/keylens-release.p12.base64"
       gh secret set RELEASE_CERT_PASSWORD < "$out/password.txt"
  3. Delete $out once both are done.
EOF
