# Plan 0016 Application icon

## Approach
Generate the icon from a small PowerShell script using System.Drawing/GDI+ (antialiased vector drawing rendered separately at each size, PNG-compressed entries assembled into a multi-size ICO by the script); Python is not installed here, commit both script and result. Embed it via a resource script compiled by `embed-resource` (the approach of `C:\test\wtf_sync\build.rs`, which writes the `.rc` with absolute asset paths into `OUT_DIR` and calls `embed_resource::compile(..., NONE).manifest_optional()`). Load the icon from the module resource in the Win32 window class.

## Branch
`task/0016-app-icon` from main.

## Steps
1. `assets/make_icon.ps1` (Windows PowerShell 5.1, System.Drawing): draw the headphones as vector paths with antialiasing directly at each size 16, 24, 32, 48, 64, 256 on a transparent canvas (headband arc, two ear cups, simple flat colour with enough contrast for 16 px; thicker strokes at small sizes if 16 px is muddy), encode each as PNG and write the ICO header + directory + PNG payloads by hand (ICONDIR/ICONDIRENTRY, width/height byte 0 for 256). Also write a 256 px PNG preview to a path given as a parameter (not committed). Run it; commit the `.ico`.
2. Resource: rename/extend `app.rc` to contain `1 ICON "assets/xm-control.ico"` and `1 24 "app.manifest"`.
3. `Cargo.toml`: `[build-dependencies] embed-resource = "3"` (same version as wtf_sync: 3.0.12). Do not touch `version`.
4. `build.rs`: replace the manual windres / MSVC link-arg code with an `embed_resource::compile` call (skip for non-Windows targets); keep `rerun-if-changed` for `app.rc`, `app.manifest` and `assets/`. Check that the GNU build ends with a single manifest (inspect the exe's resources, or confirm the comctl32 v6 look and DPI behaviour); if gcc's `default-manifest.o` conflict appears, apply the fallback from the task file.
5. `src/ui/view.rs` `run()`: load the icons with `LoadImageW(instance, MAKEINTRESOURCE(1), IMAGE_ICON, GetSystemMetrics(SM_CXICON/SM_CYICON), ...)` and the small one with `SM_CXSMICON/SM_CYSMICON`; switch to `WNDCLASSEXW`/`RegisterClassExW` with `hIcon` and `hIconSm` (or keep `WNDCLASSW` for the big icon and send `WM_SETICON` ICON_SMALL after creation). Add the needed `windows-sys` features if any are missing (`Win32_UI_WindowsAndMessaging` is already enabled). Failure to load is non-fatal (null icon).
6. Test: `#[cfg(test)] mod tests` in a small module (or in `src/ui/view.rs`) that reads the ICO via `include_bytes!("../../assets/xm-control.ico")`, checks reserved = 0, type = 1, count = 6 and the entry sizes {16, 24, 32, 48, 64, 256}.
7. `packaging/installer.iss`: `SetupIconFile=..\assets\xm-control.ico` in `[Setup]`.
8. Run fmt, clippy (`-D warnings`), `cargo test --release`; ask the owner to check the icon visually (task acceptance b).

## Files likely touched
`assets/make_icon.ps1`, `assets/xm-control.ico` (new), `app.rc`, `build.rs`, `Cargo.toml` (build-dependency only), `Cargo.lock`, `src/ui/view.rs`, `packaging/installer.iss`. No file under `src/main.rs`, the transport or settings handling is touched, so no security-shaped path changes.

## Tradeoffs
`embed-resource` vs the current hand-rolled build.rs: the current code only covers the manifest on MSVC (linker args) and would need a separate `rc.exe` path to embed an icon there; `embed-resource` handles rc.exe and windres in one call, at the cost of one build-dependency. PowerShell/System.Drawing script vs SVG: no extra tools needed on Windows, per-size rendering keeps small sizes crisp; the script is not part of the build (the `.ico` is committed), so CI needs no Python. Icon is loaded from the resource at runtime rather than embedded as bytes, which is the native Win32 way and keeps Explorer and the window in sync.
