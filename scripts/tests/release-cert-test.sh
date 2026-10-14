#!/usr/bin/env bash
# Tests release-cert.sh in a temporary directory (never touches the login keychain).
# KEYLENS_CERT_PIN_FILE redirects the pin file away from the repo's scripts/release-cert.sha1.
set -euo pipefail
root="$(cd "$(dirname "$0")/../.." && pwd)"
base="$(mktemp -d)"
trap 'rm -rf "$base"' EXIT
out="$base/cert"
mkdir -p "$base/pin"
export KEYLENS_CERT_PIN_FILE="$base/pin/release-cert.sha1"
mode() { stat -f %Lp "$1"; }

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

[[ $(mode "$out") == 700 ]] || { echo "FAIL - dir mode $(mode "$out")"; exit 1; }
for f in "$out"/*; do
  [[ $(mode "$f") == 600 ]] || { echo "FAIL - $f mode $(mode "$f")"; exit 1; }
done
echo "ok - output dir is 700 and files are 600"

pin=$(cat "$KEYLENS_CERT_PIN_FILE")
expected=$(/usr/bin/openssl x509 -in "$out/keylens-release.cer.pem" -outform der | shasum -a 1 | cut -d' ' -f1 | tr a-f A-F)
[[ $pin =~ ^[0-9A-F]{40}$ ]] || { echo "FAIL - pin is not 40 uppercase hex chars: '$pin'"; exit 1; }
[[ $pin == "$expected" ]] && echo "ok - pin file holds the certificate SHA-1" \
  || { echo "FAIL - pin $pin, certificate SHA-1 $expected"; exit 1; }

before=$(shasum "$out/keylens-release.p12")
if "$root/scripts/release-cert.sh" "$out" 2>/dev/null; then
  echo "FAIL - second run should refuse"; exit 1
fi
[[ $(shasum "$out/keylens-release.p12") == "$before" ]] \
  && echo "ok - refuses to overwrite an existing certificate" || { echo "FAIL - p12 changed"; exit 1; }

# An existing pin file means a release certificate already exists: refuse before creating anything.
if err=$("$root/scripts/release-cert.sh" "$base/fresh" 2>&1); then
  echo "FAIL - run with an existing pin file should refuse"; exit 1
fi
[[ ! -e "$base/fresh" && $err == *"release-cert.sha1"* && $(cat "$KEYLENS_CERT_PIN_FILE") == "$pin" ]] \
  && echo "ok - refuses when the pin file exists and creates nothing" \
  || { echo "FAIL - pin-file guard: $err"; exit 1; }

# A non-empty existing directory is refused and left exactly as it was.
export KEYLENS_CERT_PIN_FILE="$base/pin/other.sha1"
busy="$base/busy"
mkdir -p "$busy/sub"
echo keep > "$busy/file.txt"
chmod 755 "$busy" "$busy/sub"
chmod 644 "$busy/file.txt"
if err=$("$root/scripts/release-cert.sh" "$busy" 2>&1); then
  echo "FAIL - non-empty dir should be refused"; exit 1
fi
[[ $err == *"exists and is not empty"* && $(mode "$busy") == 755 && $(mode "$busy/sub") == 755 \
   && $(mode "$busy/file.txt") == 644 && $(ls "$busy" | tr '\n' ' ') == "file.txt sub " \
   && ! -e "$KEYLENS_CERT_PIN_FILE" ]] \
  && echo "ok - refuses a non-empty directory and leaves it untouched" \
  || { echo "FAIL - non-empty dir guard: $err"; exit 1; }

# An existing empty directory is accepted and locked down to 700.
empty="$base/empty"
mkdir -m 755 "$empty"
"$root/scripts/release-cert.sh" "$empty" >/dev/null
[[ $(mode "$empty") == 700 && -s "$empty/keylens-release.p12" ]] \
  && echo "ok - accepts an empty directory and sets it to 700" \
  || { echo "FAIL - empty dir mode $(mode "$empty")"; exit 1; }
