# Repository Development Rules

This repository is an independent native Windows Rust application built with GPUI.
The existing Electron implementation is a behavior and persistence compatibility reference; do not modify or remove it from this repository.

## Sources Of Truth

Follow these references in order:

1. `docs/SPEC.md` for product behavior and acceptance criteria.
2. Current GPUI and GPUI Component APIs and examples.
3. The Rust API Guidelines, rustfmt, and Clippy.
4. The installed `rust` skill sourced from `pgdogdev/pgdog`.

## Rust Rules

- Use Rust edition 2024 and the toolchain pinned in `rust-toolchain.toml`.
- Model invalid states with enums and newtypes where this improves correctness.
- Use `thiserror` for typed errors throughout the application. Do not add `anyhow` unless an application boundary genuinely needs heterogeneous contextual errors.
- Prefer safe Windows wrappers such as `win32job`. The only handwritten `unsafe` exception is the
  audited `window_visibility` boundary around Win32 `ShowWindow`, because GPUI 0.2.2 does not expose
  single-window hide/show on Windows. Do not expand that exception.
- Prefer borrowing over cloning, but do not introduce complicated lifetimes to avoid small UI-state clones.
- Avoid `unwrap`. Use `expect` only for a documented invariant that cannot be represented in the type system.
- Keep blocking filesystem and process work outside GPUI render methods.
- Keep GPUI entities on the foreground thread. Send plain owned events from worker threads.
- Keep modules focused and test domain logic independently from the UI.

## Required Gates

Run these before considering a change complete:

```powershell
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets
cargo build --release
```

Use `scripts/cargo.ps1` when the installed Windows SDK is older than 10.0.26100 and GPUI cannot discover `fxc.exe` itself. Daily development uses incremental debug builds. Fat LTO and single-codegen-unit optimization are release-only.
