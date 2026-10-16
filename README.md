# Keylens

**English** · [简体中文](README.zh-CN.md)

Long-press ⌘ to see every keyboard shortcut of the current app — search them, or run them right away. Open source, cross-platform, with community-maintained shortcut data. Built with [gpui-kit](https://github.com/longbridge/gpui-kit).

> **Status:** v0.1, macOS only for now (universal build for Apple Silicon and Intel; the Intel build has not been verified on real hardware). Windows and Linux are on the roadmap.

<p align="center">
  <img src="docs/screenshots/en-hero.png" alt="Keylens panel over Zed, listing its menu shortcuts in three columns" width="800">
</p>

## A look around

**Forgot a shortcut? Just hold ⌘.** The panel reads the menus of whatever app is in front, so there's nothing to set up per app — the picture above is Zed, and VS Code, Notion or Slack open just the same.

**Type keys, not just words.** Searching `⌘⇧` lists everything bound to Command + Shift, in any order, alongside regular title matches. Press Return on a result and Keylens runs it for you.

**Dark mode comes for free.** The panel follows the system appearance, so it fits in at night as well as it does during the day.

<p align="center">
  <img src="docs/screenshots/en-search.png" alt="Searching ⌘⇧ in Zed shows the 17 shortcuts that use Command + Shift" width="49%">
  <img src="docs/screenshots/en-dark.png" alt="The same panel in dark appearance" width="49%">
</p>

## Features

- **Long-press ⌘** to open a panel listing all menu shortcuts of the frontmost app, grouped by menu in columns. The panel stays open after you release ⌘.
- **Search** by title, menu path or keys. `⌘⇧`, `shift` and `cmd+shift+p` all work, regardless of modifier order.
- **Run directly**: click to select, double-click or press Return to run. Menu items are pressed via Accessibility; community shortcuts are sent as synthesized key strokes.
- **Community shortcut data** fills in shortcuts that menus don't expose (e.g. VS Code, Chrome, Slack), marked with ◇.
- **Add your own shortcuts** from the panel's "Edit Shortcuts" or the menu bar: record keys by pressing them; they are saved to your data folder and show up in the panel within 2 seconds.
- **Keyboard-first**: ↑↓←→ to move; Esc clears the search first, then closes the panel.
- **Localized UI**: Simplified Chinese, English, Traditional Chinese and Japanese, following the system language by default.
- **Dark / light** appearance follows the system.

## Install

1. [Download the latest Keylens.dmg](https://github.com/wenlingang/keylens/releases/latest/download/Keylens.dmg) (or pick a version on [Releases](https://github.com/wenlingang/keylens/releases)), open it and drag **Keylens** into Applications. Do not run it directly from inside the dmg.
2. Keylens is not notarized by Apple, so macOS blocks it the first time you open it. Allow it in either way:
   - Open System Settings › Privacy & Security and click **Open Anyway** at the bottom;
   - or, before opening the dmg, run in a terminal: `xattr -d com.apple.quarantine ~/Downloads/Keylens.dmg` (recent macOS refuses to change apps already in Applications)
3. Follow the prompt and turn on Keylens under System Settings › Privacy & Security › Accessibility. It takes effect immediately after you grant it. Upgrading by replacing the app later does not require granting it again.

Keylens checks for new versions once a day (or from **Check for Updates…** in its menu bar menu). When one is out it asks, then downloads, installs and restarts itself; the update is installed only if it is signed by the same certificate, so the Accessibility permission carries over.

Requires macOS 11 or later. Universal build (Apple Silicon + Intel); so far it has only been verified on macOS 26 (Apple Silicon), and the Intel build has not been verified on real hardware.

## Build from source

Requires a Rust toolchain that supports edition 2024. So far it has only been verified on macOS 26 (Apple Silicon).

```bash
# One-time: create a local code-signing certificate (asks for your login password)
scripts/dev-cert.sh

# Build, bundle and launch
scripts/run-dev.sh
```

On first launch, add `target/dev-apps/KeylensDev.app` (menu bar icon `⌘dev`) under System Settings › Privacy & Security › Accessibility and turn it on. It takes effect immediately without a restart, and later rebuilds keep the permission.

## Configuration

Open **Settings…** from the "⌘" menu bar icon, or press ⌘, while the panel is open. You can choose how to open the panel (hold or double-tap), which modifier key to use, how long to hold or how fast to double-tap, whether releasing a held key closes the panel, and the interface language. Changes apply immediately.

Settings are saved to `~/.config/keylens/config.toml` (`$XDG_CONFIG_HOME/keylens` on Linux), which you can also edit by hand. Your comments are kept when the settings window saves, and hand edits take effect within two seconds:

```toml
# Override the system language: zh-CN, zh-TW, zh-HK, en, ja
language = "ja"

[trigger]
mode = "hold"          # hold | double-tap
key = "cmd"            # cmd, option, ctrl or shift (also super, alt, win, control)
hold-ms = 800          # 300–2000
double-tap-ms = 350    # 200–600
hide-on-release = false  # hold only: close the panel when the key is released

[update]
auto-check = true      # check for new versions once a day
# skip-version = "0.2.0"  # written by "Skip This Version" in the update dialog
```

Your own shortcut data goes in `~/.config/keylens/apps/*.toml`, using the same format as the built-in data; it overrides built-in entries with the same shortcut for the same app. The "⌘" menu bar icon has an item that opens this folder. You can also edit them in a window: choose "Edit Shortcuts" in the panel footer.

## Project layout

- `crates/keylens-core/`: platform-independent core (shortcut model, merging, search, trigger detection, localization, community data)
- `crates/keylens-platform/`: platform interface
- `crates/keylens-macos/`: macOS implementation (Accessibility API, key listener, panel window, menu bar icon)
- `crates/keylens-app/`: gpui-kit UI and controller
- `data/apps/`: community shortcut data (CC0)
- `docs/`: design spec, implementation plans, spike findings, verification records
- `spikes/`: throwaway technical experiments, not part of the product

## Contributing

Shortcut data and code contributions are welcome — see [CONTRIBUTING.md](CONTRIBUTING.md).

```bash
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

## License

Code is licensed under Apache-2.0; the data in `data/` is dedicated to the public domain under CC0 1.0.
