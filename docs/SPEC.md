# SSH Tunnel Panel Native Rewrite Specification

## Goal

Build an independent native Windows implementation in Rust and GPUI while preserving the existing Electron product workflow and persisted tunnel definitions. The Electron project remains available as the compatibility reference during the rewrite.

The application remains a focused SSH port-forwarding panel. Cross-platform support is explicitly out of scope for this rewrite.

## Required Behavior

- Discover concrete host aliases from the user's OpenSSH config, including aliases loaded through `Include` directives.
- Show discovered hosts in a left sidebar and allow filtering tunnels by host.
- Create, edit, delete, and persist local (`-L`) and remote (`-R`) tunnel definitions.
- Never start saved tunnels automatically when the application launches.
- Start one tunnel or all tunnels on demand.
- Stop one tunnel or all tunnels on demand.
- Show `stopped`, `starting`, `running`, and `failed` states per tunnel.
- Keep the last 300 timestamped log lines per tunnel for the current application session.
- Use batch authentication and report SSH startup failures without opening a console window.
- Treat a tunnel as running only after SSH reports forwarding readiness. Fail and terminate a tunnel that does not become ready within the startup timeout.
- For `-L`, select the nearest higher available local port when the requested port is occupied and show both requested and actual ports in the UI.
- For `-R`, preserve the requested remote listening port and rely on `ExitOnForwardFailure` for conflict reporting.
- Closing the main window removes it from the desktop but leaves managed tunnels and the tray process running.
- Clicking the tray icon or `Show panel` recreates or activates the main window.
- Tray `Exit` synchronously stops every managed SSH process before quitting.
- An abnormal application exit must also terminate managed SSH process trees through Windows Job Objects.

## Persistence Compatibility

- Continue using JSON rather than introducing a database.
- Use `%APPDATA%\ssh-tunnel-panel\state.json` as the canonical path.
- If the canonical file is absent, migrate `%APPDATA%\zed-tunnel-panel\state.json`.
- Preserve the existing camelCase JSON schema and existing IDs/timestamps.
- Ignore individually malformed host or tunnel entries instead of rejecting the entire file.
- Back up malformed JSON before starting with an empty state.
- Commit writes atomically in the destination directory.

## Architecture

- `model`: serializable domain types and runtime state.
- `store`: persistence location, legacy migration, tolerant parsing, and atomic writes.
- `ssh_config`: OpenSSH config discovery only; OpenSSH remains authoritative for connection behavior.
- `ports`: local bind validation and nearest-port selection.
- `tunnel`: SSH argument construction and worker-thread process supervision.
- `app_model`: foreground GPUI entity that owns persisted state, runtime state, and process handles.
- `ui`: GPUI views and form entities. Render methods do not perform blocking work.
- `tray`: Windows tray menu and event routing.

Worker threads may send owned events to the foreground model. They must not hold or update GPUI entities directly.

## Error Policy

- Use typed `thiserror` enums at module boundaries.
- Preserve useful causes and operation context in user-facing errors.
- Do not expose secrets or environment contents in logs.
- Do not mark process creation as tunnel readiness.

## UI Contract

- Preserve the current compact dark operational layout: integrated title bar, host sidebar, tunnel list, editor panel, and logs.
- Use GPUI Component controls for text input, buttons, scroll behavior, theme, and Windows title-bar hit testing.
- Use familiar icons for commands where the bundled icon set provides them.
- Keep stable panel widths and minimum window dimensions so dynamic status and error text do not resize the overall layout.
- Surface local-port substitution near both the tunnel row and selected editor details.

## Acceptance Gates

- Persistence parsing and migration tests pass.
- SSH config parsing and wildcard exclusion tests pass.
- Port selection and SSH argument tests pass.
- Runtime transition tests cover startup success, startup timeout, explicit stop, and unexpected exit where practical.
- `cargo fmt`, Clippy with warnings denied, all tests, and a release build pass.
- The release executable launches on Windows, renders a nonblank main window, creates a visible tray icon, and exits without leaving managed SSH processes.
- The release profile uses fat LTO, one codegen unit, aborting panics, and stripped symbols.
