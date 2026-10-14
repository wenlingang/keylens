#!/usr/bin/env bash
# Tests release-cert.sh in a temporary directory (never touches the login keychain).
set -euo pipefail
root="$(cd "$(dirname "$0")/../.." && pwd)"
out="$(mktemp -d)/cert"
trap 'rm -rf "$(dirname "$out")"' EXIT

"$root/scripts/release-cert.sh" "$out" >/dev/null
for f in keylens-release.p12 keylens-release.p12.base64 password.txt keylens-release.cer.pem; do
  [[ -s "$out/$f" ]] || { echo "FAIL - missing $f"; exit 1; }
done
echo "ok - writes p12, base64, password and certificate"

subject=$(/usr/bin/openssl x509 -in "$out/keylens-release.cer.pem" -noout -subject)
[[ $subject == *"Keylens Release"* ]] && echo "ok - CN is Keylens Release" \
  || { echo "FAIL - subject: $subject"; exit 1; }
end=$(/usr/bin/openssl x509 -in "$out/keylens-release.cer.pem" -noout -enddate | cut -d= -f2)
(( $(LC_ALL=C date -j -f "%b %e %T %Y %Z" "$end" +%Y) >= $(date +%Y) + 9 )) \
  && echo "ok - valid for about 10 years" || { echo "FAIL - expires $end"; exit 1; }

before=$(shasum "$out/keylens-release.p12")
if "$root/scripts/release-cert.sh" "$out" 2>/dev/null; then
  echo "FAIL - second run should refuse"; exit 1
fi
[[ $(shasum "$out/keylens-release.p12") == "$before" ]] \
  && echo "ok - refuses to overwrite an existing certificate" || { echo "FAIL - p12 changed"; exit 1; }
