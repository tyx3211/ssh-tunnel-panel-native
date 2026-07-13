## SSH Tunnel Panel Native v0.2.0

The first public release of the native Windows implementation.

### Highlights

- Native Rust/GPUI interface matching the established SSH Tunnel Panel workflow.
- OpenSSH `-L` and `-R` forwarding with host discovery and persistent JSON configuration.
- Per-tunnel lifecycle, logs, startup timeout, and local-port conflict fallback.
- Native Windows tray behavior and deterministic SSH process cleanup through Job Objects.
- Event-driven rendering and process supervision for low idle overhead.
- Per-user NSIS installer for Windows 10/11 x64.

### Install

Download `SSH Tunnel Panel Setup 0.2.0.exe` below. Windows may show a SmartScreen warning because this release is not code-signed.

Windows OpenSSH (`ssh.exe`) and key-based or `ssh-agent` authentication are recommended.
