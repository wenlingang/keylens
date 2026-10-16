#!/usr/bin/env bash
# Prints the fixed part of the GitHub Release notes (install steps, checksum). Generated changes are appended by `gh`.
# Usage: scripts/release-notes.sh <version> <sha256>
set -euo pipefail
version=${1:?usage: release-notes.sh <version> <sha256>}
sha=${2:?}
cat <<EOT
## Install

1. Download \`Keylens-$version.dmg\` and drag **Keylens** into Applications. Do not run it from inside the dmg.
2. Keylens is not notarized by Apple, so macOS blocks the first launch. Either:
   - open System Settings › Privacy & Security and click **Open Anyway** at the bottom, or
   - before opening the dmg, run \`xattr -d com.apple.quarantine ~/Downloads/Keylens-$version.dmg\` in Terminal
     (recent macOS refuses to change apps already in Applications).
3. Grant Accessibility permission when prompted. Later upgrades keep the permission, and Keylens
   installs them itself (menu bar › Check for Updates…).
   Upgrading from 0.1.0 (different certificate): remove Keylens from the Accessibility list first, or run
   \`tccutil reset Accessibility io.github.wenlingang.keylens\`, then grant it again.

简体中文安装说明：[README.zh-CN.md](https://github.com/wenlingang/keylens/blob/master/README.zh-CN.md#安装)

Universal build (Apple Silicon + Intel). The x86_64 (Intel) build has not been tested on real hardware.

SHA-256 (\`Keylens-$version.dmg\`): \`$sha\`
EOT
