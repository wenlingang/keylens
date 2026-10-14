# Keylens

**English** · [简体中文](README.zh-CN.md)

Long-press ⌘ to see every keyboard shortcut of the current app — search them, or run them right away. Open source, cross-platform, with community-maintained shortcut data. Built with [gpui-kit](https://github.com/longbridge/gpui-kit).

> **Status:** v0.1, macOS (Apple Silicon) only for now. Windows and Linux are on the roadmap.

## Features

- **Long-press ⌘** to open a panel listing all menu shortcuts of the frontmost app, grouped by menu in columns. The panel stays open after you release ⌘.
- **Search** by title, menu path or keys. `⌘⇧`, `shift` and `cmd+shift+p` all work, regardless of modifier order.
- **Run directly**: click to select, double-click or press Return to run. Menu items are pressed via Accessibility; community shortcuts are sent as synthesized key strokes.
- **Community shortcut data** fills in shortcuts that menus don't expose (e.g. VS Code, Chrome, Slack), marked with ◇.
- **Keyboard-first**: ↑↓←→ to move; Esc clears the search first, then closes the panel.
- **Localized UI**: Simplified Chinese, English, Traditional Chinese and Japanese, following the system language by default.
- **Dark / light** appearance follows the system.

## Build and run (development)

Requires a Rust toolchain that supports edition 2024. So far it has only been verified on macOS 26 (Apple Silicon).

```bash
# One-time: create a local code-signing certificate (asks for your login password)
scripts/dev-cert.sh

# Build, bundle and launch
scripts/run-dev.sh
```

On first launch, add `target/dev-apps/Keylens.app` under System Settings › Privacy & Security › Accessibility and turn it on. It takes effect immediately without a restart, and later rebuilds keep the permission.

## Configuration

`~/.config/keylens/config.toml`:

```toml
# Override the system language: zh-CN, zh-TW, zh-HK, en, ja
language = "ja"
```

Your own shortcut data goes in `~/.config/keylens/apps/*.toml`, using the same format as the built-in data; it overrides built-in entries with the same shortcut for the same app. The "⌘" menu bar icon has an item that opens this folder.

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
