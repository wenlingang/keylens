# Keylens

长按 ⌘，看清当前应用的所有快捷键：可以搜索，也可以直接执行。开源、跨平台，快捷键数据由社区维护。基于 [gpui-kit](https://github.com/longbridge/gpui-kit)。

> 开发中。v0.1 目标平台是 macOS，Windows 和 Linux 在路线图上。

## 目录结构

- `crates/keylens-core/`：平台无关的核心逻辑（快捷键模型、合并、搜索、触发判断、多语言、社区数据）
- `crates/keylens-platform/`：平台接口
- `data/apps/`：社区快捷键数据（CC0），欢迎补充，见 [CONTRIBUTING.md](CONTRIBUTING.md)
- `docs/`：设计文档、实施计划、实验结论、会话记录
- `spikes/`：一次性技术验证代码，不属于产品

## 开发

    cargo test --workspace

## 授权

代码使用 Apache-2.0，`data/` 目录下的数据使用 CC0 1.0。
