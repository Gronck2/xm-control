# XM Control (crate `xm-control`)

A Windows window app in Rust that fully controls Sony WH-1000XM6 headphones from a PC (no phone):
it talks Sony's "MDR" protocol over Bluetooth Classic RFCOMM. Windows only.
The UI stack is not decided yet (Win32 + Direct2D like the maintainer's other projects, or egui/eframe); see docs/adr once recorded.

Talk to the person in their language (the maintainer speaks Russian). Code, comments, commit messages and docs are in English.

**You cannot see or hear the headphones.** Anything that sends commands to a device has to be checked by the person: ask them what the headphones did.
Never send firmware update / DFU, factory reset or pairing-deletion (unpair) commands to a device.

The protocol reference is mos9527/SonyHeadphonesClient (MIT, supports the XM6). Study it and write the Rust yourself;
do not copy its code verbatim. Credit it in README/NOTICE.

The separate project `..\BtPresence` (pause by RSSI) is not part of this repository; don't touch it.

## Build and check

- Toolchain: stable Rust, `x86_64-pc-windows-gnu` locally (CI uses MSVC on windows-latest). `cargo` is in `%USERPROFILE%\.cargo\bin` and may not be on PATH;
  the GNU linker (mingw) is in `%LOCALAPPDATA%\Microsoft\WinGet\Packages\BrechtSanders.WinLibs.POSIX.UCRT_Microsoft.Winget.Source_8wekyb3d8bbwe\mingw64\bin`.
- Before committing, run what CI runs (`.github/workflows/ci.yml`):
  `cargo fmt --check`, `cargo clippy --release --all-targets -- -D warnings`, `cargo test --release`.
  `rustfmt.toml` sets `max_width = 160`.
- Unit tests live in `#[cfg(test)] mod tests` at the bottom of each module and cover hardware-free logic (framing, parsing, encoding).
- Don't bump `version` in `Cargo.toml` or push tags: releases are made by the maintainer.

## Agent pipeline

Non-trivial work goes through the `dev-pipeline` plugin (`/dev-pipeline:run-task`); project specifics are the Pipeline contract.
Do not add same-named `.claude/agents/` copies unless a deliberate override is needed.

## Pipeline contract

- **Base branch:** main
- **PR target:** main
- **Forge:** gh (GitHub `Gronck2/xm-control`; `gh` is in `C:\Program Files\GitHub CLI`)
- **Branch naming:** task/NNNN-slug; docs-only work on docs/<slug>
- **Docs layout:** tasks docs/tasks/, plans docs/plan/ (shared NNNN numbering), ADRs docs/adr/ (immutable),
  living docs: docs/arch/overview.md. Docs language: English.
- **Test paths:** inline `#[cfg(test)] mod tests` in each module under `src/`; `tests/` for integration tests
- **Gate commands:**
  - lint: `cargo clippy --release --all-targets -- -D warnings`
  - format: `cargo fmt --check`
  - tests: `cargo test --release`
- **Security-shaped paths:** `src/main.rs` (entry point), the Bluetooth/RFCOMM transport and any code that builds or sends
  device commands (the command denylist: firmware update / DFU, factory reset, unpair must never be sendable), settings file handling.
- **Merge/release policy:** PRs into main are opened without asking; only the owner merges, by squash, and only with green CI.
  Never propose merging a PR whose CI is red or pending. Direct pushes to main, tags, releases, version bumps and force-push only on the owner's explicit request.
- **Extra rules:** the agent cannot see or hear the headphones: every behaviour that sends commands to a device is verified by the owner
  (ask what the headphones did). Never send DFU / firmware update, factory reset or pairing-deletion commands. Don't touch `..\BtPresence`.
  Protocol knowledge from mos9527/SonyHeadphonesClient (MIT) is rewritten, not copied.
