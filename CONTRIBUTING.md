# 参与贡献

## 开发

    cargo test              # 运行全部测试，包括内置数据校验
    cargo clippy --all-targets -- -D warnings
    cargo fmt

## 本地运行需要辅助功能权限的程序

读取其他应用的菜单、监听全局按键都需要「辅助功能」权限。macOS 按代码签名识别程序，ad-hoc 签名的程序每次重新编译后权限都会失效。

1. 一次性运行 `scripts/dev-cert.sh`，创建本地开发证书（会弹出密码框）。
2. 用 `scripts/dev-bundle.sh <可执行文件> <应用名> <bundle-id>` 打包成 `.app`。
3. 在「系统设置 › 隐私与安全性 › 辅助功能」里添加这个 `.app` 并打开开关，只需要一次。
4. 用 `open -W -n <app> --stdout <文件> --args …` 运行，输出会写进文件。

## 贡献快捷键数据

数据放在 `data/apps/`，每个应用一个 TOML 文件，协议是 CC0。

1. 复制一个现有文件（比如 `data/apps/slack.toml`）作为模板，文件名用应用的简短英文名。
2. 填写 `[app.match]`：
   - macOS：bundle ID，可以用 `osascript -e 'id of app "应用名"'` 查询；
   - Windows：进程名，如 `Code.exe`；
   - Linux：`.desktop` 文件名（不带后缀）或窗口的 WM_CLASS。
3. 每个快捷键写 `title`（英文）和 `keys`，建议同时提供 `i18n.zh-Hans`。
4. **`title` 请用自己的话写，不要从厂商文档复制粘贴。**
5. 运行 `cargo test -p keylens-core` 确认数据通过校验。

### 按键写法

- 修饰键：`Mod`（macOS 上是 ⌘，Windows/Linux 上是 Ctrl）、`Cmd`、`Ctrl`、`Alt`/`Option`、`Shift`、`Super`/`Win`、`Fn`
- 主键：`A`–`Z`、`0`–`9`、`F1`–`F24`、`Up`、`Down`、`Left`、`Right`、`Enter`、`Tab`、`Space`、`Backspace`、`Delete`、`Escape`、`Home`、`End`、`PageUp`、`PageDown`，以及 `` ` - = [ ] \ ; ' , . / ``
- 用 `+` 连接：`Mod+Shift+P`
- 连续按键写成数组：`["Mod+K", "Mod+S"]`
- 各平台不同时：`{ default = "Alt+Up", macos = "Option+Up" }`

编辑器装了 Even Better TOML 插件时，文件开头的 `#:schema ../schema.json` 会提供补全和校验。
