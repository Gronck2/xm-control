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
