#!/usr/bin/env bash
# 一次性：在登录钥匙串里创建名为「Keylens Development」的自签名代码签名证书。
# 用它签名的开发版 .app 重新编译后不会丢失辅助功能权限。
set -euo pipefail

NAME="Keylens Development"
if security find-identity -v -p codesigning | grep -q "\"${NAME}\""; then
  echo "✅ 证书「${NAME}」已存在且可用。"
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
echo "接下来系统会弹出密码框，用于把证书设为「代码签名」受信任。"
security add-trusted-cert -r trustRoot -p codeSign -k "$keychain" "$tmp/cert.pem"

if security find-identity -v -p codesigning | grep -q "\"${NAME}\""; then
  echo "✅ 证书「${NAME}」已创建。"
else
  echo "❌ 证书创建后仍不可用，请在「钥匙串访问」里检查「${NAME}」的信任设置。" >&2
  exit 1
fi
