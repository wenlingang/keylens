# Contributing

## Development

    cargo test              # run all tests, including validation of the built-in data
    cargo clippy --all-targets -- -D warnings
    cargo fmt

## Running programs that need Accessibility permission locally

Reading other apps' menus and listening for global key presses both require the Accessibility permission. macOS identifies programs by code signature, so an ad-hoc signed program loses the permission every time it is rebuilt.

1. Run `scripts/dev-cert.sh` once to create a local development certificate (a password dialog will appear).
2. Package the executable into a `.app` with `scripts/dev-bundle.sh <executable> <app-name> <bundle-id>`.
3. Add the `.app` under "System Settings › Privacy & Security › Accessibility" and turn it on. You only need to do this once.
4. Run it with `open -W -n <app> --stdout <file> --args …`; the output is written to the file.

## Contributing shortcut data

Data lives in `data/apps/`, one TOML file per app, licensed CC0.

1. Copy an existing file (for example `data/apps/slack.toml`) as a template. Name the file after the app's short English name.
2. Fill in `[app.match]`:
   - macOS: the bundle ID, which you can look up with `osascript -e 'id of app "App Name"'`;
   - Windows: the process name, such as `Code.exe`;
   - Linux: the `.desktop` file name (without the extension) or the window's WM_CLASS.
3. Give each shortcut a `title` (in English) and `keys`. Providing `i18n.zh-Hans` as well is recommended.
4. **Write each `title` in your own words; do not copy and paste from vendor documentation.**
5. Run `cargo test -p keylens-core` to confirm the data passes validation.

### Key notation

- Modifiers: `Mod` (⌘ on macOS, Ctrl on Windows/Linux), `Cmd`, `Ctrl`, `Alt`/`Option`, `Shift`, `Super`/`Win`, `Fn`
- Main keys: `A`–`Z`, `0`–`9`, `F1`–`F24`, `Up`, `Down`, `Left`, `Right`, `Enter`, `Tab`, `Space`, `Backspace`, `Delete`, `Escape`, `Home`, `End`, `PageUp`, `PageDown`, and `` ` - = [ ] \ ; ' , . / ``
- Join keys with `+`: `Mod+Shift+P`
- Write key sequences as an array: `["Mod+K", "Mod+S"]`
- When platforms differ: `{ default = "Alt+Up", macos = "Option+Up" }`

If your editor has the Even Better TOML extension, the `#:schema ../schema.json` line at the top of each file provides completion and validation.

## Releasing

Releases are built by `.github/workflows/release.yml` and signed with the self-signed "Keylens Release"
certificate stored in the `RELEASE_CERT_P12` / `RELEASE_CERT_PASSWORD` secrets. Never replace that
certificate: users would have to re-grant Accessibility permission. The one-time setup is `scripts/release-cert.sh`
(it writes to `~/keylens-release-cert/`, outside any synced folder).

`scripts/release-cert.sha1` pins that certificate's SHA-1. Never change it: packaging with "Keylens Release"
requires it, and CI fails if the app is signed by any other certificate.

1. Bump `version` in `crates/keylens-app/Cargo.toml`, run `cargo check` to update `Cargo.lock`, and commit.
2. Optional dry run: Actions › Release › Run workflow, then download the `keylens-dmg` artifact.
3. `git tag -a vX.Y.Z -m "<what changed>" && git push origin vX.Y.Z`. CI checks the tag matches the
   Cargo version, runs the tests, packages, verifies and publishes the release. The tag message is shown
   in the in-app update dialog.

Every release also carries `latest.json` (`scripts/update-manifest.sh`). Installed copies fetch it from
`releases/latest/download/latest.json`, so the newest non-prerelease release is what they update to.
To see the update dialog and download progress from a dev build, start it with
`KEYLENS_PRETEND_VERSION=0.0.1 scripts/run-dev.sh` and use "Check for Updates…" (the install then
fails on purpose: the release is not signed with the development certificate).
To try an install by hand: `cargo run -p keylens-macos --example install_update -- <dmg> <Keylens.app> <version>`.

To package locally with your development certificate: `scripts/package.sh --identity "Keylens Development"`.
