# SSH Tunnel Panel Native

[简体中文](./README.zh-CN.md)

SSH Tunnel Panel Native is a lightweight, Windows-native manager for persistent OpenSSH port forwards. It provides the compact, VS Code-like ports workflow of [SSH Tunnel Panel](https://github.com/tyx3211/ssh-tunnel-panel) without Electron.

The project started from a simple need: keep Zed, Codex, terminal-first, and agent-first remote workflows comfortable without tying tunnel management to an editor. It is not affiliated with Zed Industries.

## Install

Download the latest Windows installer from [GitHub Releases](https://github.com/tyx3211/ssh-tunnel-panel-native/releases/latest), then run `SSH Tunnel Panel Setup 0.2.0.exe`.

Requirements and notes:

- Windows 10 or Windows 11 on x64.
- Windows OpenSSH (`ssh.exe`) must be available on `PATH`.
- Key-based authentication or `ssh-agent` is recommended. Interactive password prompts are intentionally disabled.
- Windows SmartScreen may warn because the installer is not code-signed yet.
- The installer is per-user and does not require administrator privileges.

## Features

- Discovers concrete host aliases from `~/.ssh/config`, including `Include` files.
- Keeps local (`ssh -L`) and remote (`ssh -R`) forwarding definitions in one panel.
- Starts one tunnel or all saved tunnels on demand; nothing auto-connects at launch.
- Reports starting, running, stopped, and failed states with per-tunnel logs.
- Finds the nearest higher local port for `-L` when the requested port is occupied, and shows the actual selected port.
- Preserves the requested remote port for `-R` and reports remote bind failures.
- Stays available in the Windows tray when the main window is closed.
- Enforces one application instance per Windows login session and restores the existing window on a repeated launch.
- Stops every managed SSH process when **Exit** is selected from the tray.
- Uses Windows Job Objects so abnormal application termination also tears down managed SSH process trees.

## Daily Use

1. Make sure the target host works with `ssh <host-alias>` in a terminal.
2. Select the host in the sidebar and create a forwarding definition.
3. Choose `-L` for a local listening port or `-R` for a remote listening port.
4. Start an individual tunnel or use **Start all**.
5. Close the window to keep the app and active tunnels in the tray.
6. Use tray **Exit** when you want the app and all managed tunnels to stop.

The application deliberately does not auto-start saved tunnels. A failed startup becomes visible in the tunnel row and log instead of remaining indefinitely in a starting state.

## Persistence

Configuration is stored as JSON at:

```text
%APPDATA%\ssh-tunnel-panel\state.json
```

On first launch, the native app can migrate the former `%APPDATA%\zed-tunnel-panel\state.json` location. Its persisted tunnel schema remains compatible with the [Electron implementation](https://github.com/tyx3211/ssh-tunnel-panel), so switching implementations does not require a database conversion.

## Native And Electron Versions

This repository contains the Windows-native Rust/GPUI implementation. The original [Electron/React implementation](https://github.com/tyx3211/ssh-tunnel-panel) remains available as the cross-platform web-stack reference and persistence compatibility baseline.

Choose this native version when low idle overhead, a smaller installer, and native Windows lifecycle behavior matter most. Both projects manage ordinary OpenSSH processes and use the same core workflow.

## Technical Design

- Rust 2024 with a pinned stable toolchain.
- [GPUI](https://github.com/zed-industries/zed/tree/main/crates/gpui) and GPUI Component for the native interface.
- `tray-icon` for the native Windows notification-area menu.
- A Windows named mutex prevents duplicate application instances before UI or SSH initialization. A named auto-reset event forwards repeated launches to the first process, which restores and activates its window on the GPUI thread.
- `thiserror` for typed errors and Serde JSON with atomic file replacement for persistence.
- Windows Job Objects for SSH process-tree ownership.
- A narrowly audited Win32 window-visibility adapter implements standard close-to-tray behavior because GPUI 0.2.2 does not expose single-window hide/show on Windows.
- Windows-specific process coordination and window visibility are isolated behind a conditionally compiled platform module so future macOS and Linux backends do not leak native APIs into the GPUI application layer.
- Event-driven UI and process supervision: there is no periodic UI polling while idle.
- Shared immutable log snapshots avoid copying complete log histories during each render.
- Release builds use optimization level 3, fat LTO, one codegen unit, a statically linked MSVC runtime, stripped symbols, and aborting panics.

GPUI remains free to schedule rendering work at full speed when state changes, while idle tunnels block on process events instead of waking on a timer. GPUI 0.2.2's Windows backend still owns its visible-window vsync scheduling; hiding the panel removes it from presentation until the tray restores it.

## Development

The Windows SDK and Rust toolchain are pinned by the repository. Common commands:

```powershell
.\scripts\cargo.ps1 fmt --all --check
.\scripts\cargo.ps1 clippy-strict --all-targets --all-features
.\scripts\cargo.ps1 test --all-targets --all-features
.\scripts\cargo.ps1 run
```

Create the optimized NSIS installer with:

```powershell
npm install --global @crabnebula/packager@0.11.2
.\scripts\package.ps1
```

Release optimization is intentionally expensive. Normal development should use incremental debug builds.

## Security Scope

SSH Tunnel Panel does not store passwords or private keys and does not implement SSH itself. Authentication, host-key verification, proxy jumps, and connection policy remain under Windows OpenSSH and your SSH config. Tunnel commands run in batch mode so authentication failures are reported instead of opening hidden interactive prompts.

## License

[MIT](./LICENSE)
