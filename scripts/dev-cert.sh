#!/usr/bin/env bash
# One-time: creates a self-signed code-signing certificate named "Keylens Development" in the login keychain.
# Dev .app builds signed with it keep their Accessibility permission after rebuilding.
set -euo pipefail

NAME="Keylens Development"
if security find-identity -v -p codesigning | grep -q "\"${NAME}\""; then
  echo "✅ Certificate \"${NAME}\" already exists and is usable."
  exit 0
fi

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
openssl req -x509 -newkey rsa:2048 -nodes -days 3650 \
  -keyout "$tmp/key.pem" -out "$tmp/cert.pem" -config "$tmp/cfg" 2>/dev/null
openssl pkcs12 -export -inkey "$tmp/key.pem" -in "$tmp/cert.pem" \
  -out "$tmp/id.p12" -passout pass:keylens 2>/dev/null

keychain="$HOME/Library/Keychains/login.keychain-db"
security import "$tmp/id.p12" -k "$keychain" -P keylens -T /usr/bin/codesign
echo "A password dialog will appear next, to trust the certificate for code signing."
security add-trusted-cert -r trustRoot -p codeSign -k "$keychain" "$tmp/cert.pem"

if security find-identity -v -p codesigning | grep -q "\"${NAME}\""; then
  echo "✅ Certificate \"${NAME}\" created."
else
  echo "❌ Certificate is still unusable after creation; check the trust settings of \"${NAME}\" in Keychain Access." >&2
  exit 1
fi
