# SSH Tunnel Panel Native

[English README](./README.md)

SSH Tunnel Panel Native 是一个轻量的 Windows 原生 OpenSSH 端口转发管理器。它延续了 [SSH Tunnel Panel](https://github.com/tyx3211/ssh-tunnel-panel) 紧凑、类似 VS Code Ports 的工作流，但不再依赖 Electron。

这个项目最初是为了让 Zed、Codex、终端优先和 agent-first 的远程开发更舒服：端口转发独立于编辑器，又不需要反复手写和管理 SSH 命令。本项目不隶属于 Zed Industries。

## 安装

从 [GitHub Releases](https://github.com/tyx3211/ssh-tunnel-panel-native/releases/latest) 下载最新版 Windows 安装包，然后运行 `SSH Tunnel Panel Setup 0.2.0.exe`。

要求与说明：

- Windows 10 或 Windows 11 x64。
- Windows OpenSSH，也就是 `ssh.exe`，需要能从 `PATH` 找到。
- 推荐使用密钥登录或 `ssh-agent`；应用刻意禁用了交互式密码提示。
- 目前尚未进行代码签名，Windows SmartScreen 可能给出警告。
- 安装包采用当前用户安装，不需要管理员权限。

## 功能

- 从 `~/.ssh/config` 自动发现具体主机别名，并支持 `Include` 文件。
- 在一个面板中管理本地转发 `ssh -L` 和远程转发 `ssh -R`。
- 支持单条启动和一键启动全部；应用启动时不会自动连接。
- 显示启动中、运行中、已停止和失败状态，并保留每条转发的日志。
- `-L` 本地端口被占用时，自动寻找最近的更大可用端口，并在界面中显示实际端口。
- `-R` 保持指定的远程监听端口，远程绑定失败时明确报错。
- 关闭主窗口后继续驻留 Windows 托盘。
- 从托盘选择“退出”时，停止应用管理的全部 SSH 进程。
- 使用 Windows Job Object 管理进程树，即使应用异常终止，也不会遗留受管 SSH 子进程。

## 日常使用

1. 先在终端确认 `ssh <主机别名>` 能正常连接。
2. 在左侧选择主机并新建转发配置。
3. 本地监听选择 `-L`，远程监听选择 `-R`。
4. 启动单条转发，或者点击“一键启动”。
5. 关闭窗口后，应用和已有连接会继续留在托盘。
6. 需要结束应用及全部连接时，从托盘选择“退出”。

应用刻意不自动启动已保存的转发。启动失败会显示在对应栏目和日志中，不会无限卡在“启动中”。

## 持久化

配置使用 JSON 保存到：

```text
%APPDATA%\ssh-tunnel-panel\state.json
```

首次启动时，原生版本能够迁移旧的 `%APPDATA%\zed-tunnel-panel\state.json`。持久化的转发数据结构与 [Electron 版本](https://github.com/tyx3211/ssh-tunnel-panel) 保持兼容，因此切换实现不需要转换数据库。

## 原生版与 Electron 版

本仓库是 Windows 原生 Rust/GPUI 实现。原来的 [Electron/React 实现](https://github.com/tyx3211/ssh-tunnel-panel) 仍作为 Web 技术栈版本和持久化兼容参考保留。

如果更看重较低的空闲开销、更小的安装包以及原生 Windows 生命周期，可以选择本原生版本。两个版本都管理普通的 OpenSSH 进程，核心工作流一致。

## 技术设计

- Rust 2024，并固定稳定版工具链。
- 使用 [GPUI](https://github.com/zed-industries/zed/tree/main/crates/gpui) 和 GPUI Component 构建原生界面。
- 使用 `tray-icon` 提供 Windows 原生通知区域菜单。
- 使用 `thiserror` 建模错误，Serde JSON 配合原子替换完成持久化。
- 使用 Windows Job Object 管理 SSH 进程树。
- 使用一层严格隔离、可审计的 Win32 窗口可见性适配实现标准“关闭到托盘”；原因是 GPUI 0.2.2 尚未公开 Windows 单窗口 hide/show API。
- UI 和进程监督完全事件驱动，空闲时没有周期性 UI 轮询。
- 日志使用共享不可变快照，渲染时不会重复复制完整日志历史。
- Release 使用优化等级 3、fat LTO、单 codegen unit、静态链接 MSVC 运行库、符号剥离和 `panic = "abort"`。

状态变化时，GPUI 仍可按正常能力全速调度渲染；没有变化时，稳定连接阻塞等待进程事件，不依赖定时唤醒。GPUI 0.2.2 的 Windows 后端仍自行管理可见窗口的 vsync 调度；面板隐藏到托盘后不会继续呈现窗口帧，托盘恢复时才重新显示。

## 开发

仓库固定了 Rust 工具链，并为 Windows SDK 提供统一脚本：

```powershell
.\scripts\cargo.ps1 fmt --all --check
.\scripts\cargo.ps1 clippy-strict --all-targets --all-features
.\scripts\cargo.ps1 test --all-targets --all-features
.\scripts\cargo.ps1 run
```

生成优化后的 NSIS 安装包：

```powershell
npm install --global @crabnebula/packager@0.11.2
.\scripts\package.ps1
```

Release 的极限优化会明显增加构建时间；日常开发应使用增量 Debug 构建。

## 安全边界

SSH Tunnel Panel 不保存密码或私钥，也不自行实现 SSH。认证、主机密钥验证、跳板机和连接策略均由 Windows OpenSSH 与你的 SSH config 负责。转发命令使用 batch mode，认证失败会直接报告，不会弹出隐藏的交互式密码输入。

## 许可证

[MIT](./LICENSE)
