# Keylens

[中文](#中文) · [English](#english)

---

## 中文

长按 ⌘，看清当前应用的所有快捷键：可以搜索，也可以直接执行。开源、跨平台，快捷键数据由社区维护。基于 [gpui-kit](https://github.com/longbridge/gpui-kit) 构建。

> **状态：** v0.1，目前支持 macOS（Apple Silicon）。Windows 和 Linux 在路线图上。

### 功能

- **长按 ⌘ 呼出**：浮层显示前台应用的全部菜单快捷键，按菜单分列；松开 ⌘ 后浮层保持显示。
- **搜索**：按标题、菜单路径或按键搜索，支持 `⌘⇧`、`shift`、`cmd+shift+p` 等写法，不区分修饰键顺序。
- **直接执行**：单击选中，双击或回车执行。菜单项直接点按，社区数据里的快捷键通过模拟按键执行。
- **社区快捷键数据**：补充菜单里读不到的快捷键（例如 VS Code、Chrome、Slack），用 ◇ 标记。
- **键盘操作**：↑↓←→ 移动，Esc 先清空搜索、再关闭浮层。
- **多语言界面**：简体中文、English、繁體中文、日本語，默认跟随系统语言。
- **深色 / 浅色**：跟随系统外观。

### 安装与运行（开发版）

需要 Rust（支持 edition 2024 的版本）。目前只在 macOS 26（Apple Silicon）上验证过。

```bash
# 一次性：创建本地开发签名证书（会弹出系统密码框）
scripts/dev-cert.sh

# 编译、打包并启动
scripts/run-dev.sh
```

第一次启动后，在「系统设置 › 隐私与安全性 › 辅助功能」里添加 `target/dev-apps/Keylens.app` 并打开开关。授权后自动生效，不需要重启；之后重新编译也不需要再次授权。

### 配置

`~/.config/keylens/config.toml`：

```toml
# 覆盖系统语言：zh-CN、zh-TW、zh-HK、en、ja
language = "ja"
```

自己补充的快捷键数据放在 `~/.config/keylens/apps/*.toml`，格式与内置数据相同，会覆盖内置数据中同一应用的同一快捷键。菜单栏「⌘」图标的菜单里可以直接打开这个目录。

### 目录结构

- `crates/keylens-core/`：平台无关的核心逻辑（快捷键模型、合并、搜索、触发判断、多语言、社区数据）
- `crates/keylens-platform/`：平台接口
- `crates/keylens-macos/`：macOS 实现（辅助功能 API、按键监听、浮层窗口、菜单栏图标）
- `crates/keylens-app/`：gpui-kit 界面与控制逻辑
- `data/apps/`：社区快捷键数据（CC0）
- `docs/`：设计文档、实施计划、实验结论、验证记录
- `spikes/`：一次性技术验证代码，不属于产品

### 参与贡献

欢迎补充快捷键数据和改进代码，见 [CONTRIBUTING.md](CONTRIBUTING.md)。

```bash
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

### 授权

代码使用 Apache-2.0，`data/` 目录下的数据使用 CC0 1.0。

---

## English

Long-press ⌘ to see every keyboard shortcut of the current app — search them, or run them right away. Open source, cross-platform, with community-maintained shortcut data. Built with [gpui-kit](https://github.com/longbridge/gpui-kit).

> **Status:** v0.1, macOS (Apple Silicon) only for now. Windows and Linux are on the roadmap.

### Features

- **Long-press ⌘** to open a panel listing all menu shortcuts of the frontmost app, grouped by menu in columns. The panel stays open after you release ⌘.
- **Search** by title, menu path or keys. `⌘⇧`, `shift` and `cmd+shift+p` all work, regardless of modifier order.
- **Run directly**: click to select, double-click or press Return to run. Menu items are pressed via Accessibility; community shortcuts are sent as synthesized key strokes.
- **Community shortcut data** fills in shortcuts that menus don't expose (e.g. VS Code, Chrome, Slack), marked with ◇.
- **Keyboard-first**: ↑↓←→ to move; Esc clears the search first, then closes the panel.
- **Localized UI**: Simplified Chinese, English, Traditional Chinese and Japanese, following the system language by default.
- **Dark / light** appearance follows the system.

### Build and run (development)

Requires a Rust toolchain that supports edition 2024. So far it has only been verified on macOS 26 (Apple Silicon).

```bash
# One-time: create a local code-signing certificate (asks for your login password)
scripts/dev-cert.sh

# Build, bundle and launch
scripts/run-dev.sh
```

On first launch, add `target/dev-apps/Keylens.app` under System Settings › Privacy & Security › Accessibility and turn it on. It takes effect immediately without a restart, and later rebuilds keep the permission.

### Configuration

`~/.config/keylens/config.toml`:

```toml
# Override the system language: zh-CN, zh-TW, zh-HK, en, ja
language = "ja"
```

Your own shortcut data goes in `~/.config/keylens/apps/*.toml`, using the same format as the built-in data; it overrides built-in entries with the same shortcut for the same app. The "⌘" menu bar icon has an item that opens this folder.

### Project layout

- `crates/keylens-core/`: platform-independent core (shortcut model, merging, search, trigger detection, localization, community data)
- `crates/keylens-platform/`: platform interface
- `crates/keylens-macos/`: macOS implementation (Accessibility API, key listener, panel window, menu bar icon)
- `crates/keylens-app/`: gpui-kit UI and controller
- `data/apps/`: community shortcut data (CC0)
- `docs/`: design spec, implementation plans, spike findings, verification records
- `spikes/`: throwaway technical experiments, not part of the product

### Contributing

Shortcut data and code contributions are welcome — see [CONTRIBUTING.md](CONTRIBUTING.md).

```bash
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

### License

Code is licensed under Apache-2.0; the data in `data/` is dedicated to the public domain under CC0 1.0.
