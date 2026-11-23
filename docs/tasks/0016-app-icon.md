# 0016 Application icon (headphones)

## Goal
Give the app an original headphones icon: embedded in the exe (Explorer, shortcuts, taskbar) and used as the window icon (title bar and taskbar), and used by the installer.

## Why
The exe and the window currently show the generic Windows application icon.

## Scope
- Draw a simple, readable, original headphones icon (headband arc and two ear cups). No Sony logo or third-party art.
- A committed generator script (`assets/make_icon.ps1`, PowerShell + System.Drawing, no extra installs: Python is not available on this machine) that writes `assets/xm-control.ico` with 16, 24, 32, 48, 64 and 256 px images. Commit the script and the `.ico`. A 256 px PNG preview is optional and need not be committed.
- Embed the `.ico` into the exe as a Windows resource with id 1 (`1 ICON "assets/xm-control.ico"`), the way `C:\test\wtf_sync` does it: `embed-resource` build-dependency, resource script and `build.rs`. It must work on `x86_64-pc-windows-gnu` (local) and MSVC (CI).
- Window icon: the UI is plain Win32 (`src/ui/view.rs`, `run()` registers a `WNDCLASSW`). Load the big and small icons from the module resource with `LoadImageW`/`LoadIconW` (id 1; small one at `SM_CXSMICON`) and set `hIcon` in `WNDCLASSW` (or use `WNDCLASSEXW` with `hIconSm`) so both the title bar and the taskbar show it.
- `packaging/installer.iss`: add `SetupIconFile=..\assets\xm-control.ico` in `[Setup]`. Shortcuts use the exe's embedded icon already; add explicit `IconFilename` only if a shortcut shows the wrong icon.
- A test that parses `assets/xm-control.ico` (include_bytes or read at test time): ICONDIR header (reserved 0, type 1, count 6) and directory entries with sizes 16, 24, 32, 48, 64, 256 (width/height byte 0 means 256).

## Out of scope
Tray icon, multiple icon variants (connected/disconnected), dark/light variants, version bump, any device command.

## XM6 status
Not applicable: no device interaction.

## Acceptance
(a) `assets/xm-control.ico` and the generator script are committed; the ICO test passes; fmt, clippy (`-D warnings`) and `cargo test --release` are green locally (GNU) and in CI (MSVC). The built exe has icon resource 1 and still has exactly one manifest (comctl32 v6 look and per-monitor DPI still work).
(b) Owner: look at the exe in Explorer, the running window's title bar and taskbar button, and (after an installer build) the installer and Start-menu shortcut; confirm the icon is recognisable at small sizes.

## Open owner decisions
- Replace the hand-rolled manifest embedding in `build.rs` (windres on GNU, linker args on MSVC) with `embed-resource` for both manifest and icon. Recommendation: yes, one mechanism, same as wtf_sync; adds one build-dependency (`embed-resource`), no runtime dependency. Fallback if the double-manifest problem shows up on GNU: keep the current `build.rs`, add the icon line to `app.rc` (windres handles it on GNU) and use `embed-resource` only for MSVC.
- No ADR needed (build detail, not an architectural decision).
