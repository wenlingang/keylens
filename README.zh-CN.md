# Keylens

[English](README.md) · **简体中文**

长按 ⌘，看清当前应用的所有快捷键：可以搜索，也可以直接执行。开源、跨平台，快捷键数据由社区维护。基于 [gpui-kit](https://github.com/longbridge/gpui-kit) 构建。

> **状态：** v0.1，目前仅支持 macOS（Universal 版本，Apple Silicon + Intel；Intel 版本未经真机验证）。Windows 和 Linux 在路线图上。

## 功能

- **长按 ⌘ 呼出**：浮层显示前台应用的全部菜单快捷键，按菜单分列；松开 ⌘ 后浮层保持显示。
- **搜索**：按标题、菜单路径或按键搜索，支持 `⌘⇧`、`shift`、`cmd+shift+p` 等写法，不区分修饰键顺序。
- **直接执行**：单击选中，双击或回车执行。菜单项直接点按，社区数据里的快捷键通过模拟按键执行。
- **社区快捷键数据**：补充菜单里读不到的快捷键（例如 VS Code、Chrome、Slack），用 ◇ 标记。
- **键盘操作**：↑↓←→ 移动，Esc 先清空搜索、再关闭浮层。
- **多语言界面**：简体中文、English、繁體中文、日本語，默认跟随系统语言。
- **深色 / 浅色**：跟随系统外观。

## 安装

1. 从 [Releases](https://github.com/wenlingang/keylens/releases) 下载最新的 `Keylens-X.Y.Z.dmg`，把 **Keylens** 拖进「应用程序」。请不要直接在 dmg 里运行。
2. Keylens 未经 Apple 公证，第一次打开会被拦下。任选一种方式放行：
   - 打开「系统设置 › 隐私与安全性」，在底部点「仍要打开」；
   - 或在终端执行：`xattr -dr com.apple.quarantine /Applications/Keylens.app`
3. 按引导在「系统设置 › 隐私与安全性 › 辅助功能」中打开 Keylens 的开关，授权后自动生效。之后升级覆盖安装，不需要重新授权。

支持 macOS 11 及以上，Universal 版本（Apple Silicon + Intel）；目前只在 macOS 26（Apple Silicon）上实测过，Intel 版本未经真机验证。

## 从源码构建

需要 Rust（支持 edition 2024 的版本）。目前只在 macOS 26（Apple Silicon）上验证过。

```bash
# 一次性：创建本地开发签名证书（会弹出系统密码框）
scripts/dev-cert.sh

# 编译、打包并启动
scripts/run-dev.sh
```

第一次启动后，在「系统设置 › 隐私与安全性 › 辅助功能」里添加 `target/dev-apps/Keylens.app` 并打开开关。授权后自动生效，不需要重启；之后重新编译也不需要再次授权。

## 配置

`~/.config/keylens/config.toml`：

```toml
# 覆盖系统语言：zh-CN、zh-TW、zh-HK、en、ja
language = "ja"
```

自己补充的快捷键数据放在 `~/.config/keylens/apps/*.toml`，格式与内置数据相同，会覆盖内置数据中同一应用的同一快捷键。菜单栏「⌘」图标的菜单里可以直接打开这个目录。

## 目录结构

- `crates/keylens-core/`：平台无关的核心逻辑（快捷键模型、合并、搜索、触发判断、多语言、社区数据）
- `crates/keylens-platform/`：平台接口
- `crates/keylens-macos/`：macOS 实现（辅助功能 API、按键监听、浮层窗口、菜单栏图标）
- `crates/keylens-app/`：gpui-kit 界面与控制逻辑
- `data/apps/`：社区快捷键数据（CC0）
- `docs/`：设计文档、实施计划、实验结论、验证记录
- `spikes/`：一次性技术验证代码，不属于产品

## 参与贡献

欢迎补充快捷键数据和改进代码，见 [CONTRIBUTING.md](CONTRIBUTING.md)。

```bash
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

## 授权

代码使用 Apache-2.0，`data/` 目录下的数据使用 CC0 1.0。
