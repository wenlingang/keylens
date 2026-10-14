#!/usr/bin/env bash
# Prints the keylens-app package version (the single source of the release version).
set -euo pipefail
cd "$(dirname "$0")/.."
# `cargo pkgid` prints e.g. "path+file:///…/crates/keylens-app#0.1.0" (or "…#keylens-app@0.1.0").
cargo pkgid -p keylens-app | sed 's/.*[#@]//'
