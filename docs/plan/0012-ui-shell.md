# Plan 0012 UI shell

## Approach
Keep all logic in a UI-independent `src/ui/model.rs` (view model: `Event` -> `ViewState`, user intents -> `Command`). The chosen toolkit only draws `ViewState` and forwards intents, so swapping stacks later is cheap.
The worker thread wakes the UI through the callback from 0004 (`PostMessage` for Win32, `ctx.request_repaint()` for egui).

## Steps
1. After the ADR: window creation, event loop, wake-up path.
2. View model + tests.
3. Device picker, connect/disconnect, status line, info panel.
4. Layout containers for the 0013/0014 panels (sections with headings), DPI awareness.
5. Owner checklist; update `docs/arch/overview.md`.

## Files likely touched
`Cargo.toml` (+ UI crate if egui), `src/main.rs`, `src/ui/{mod,model,view}.rs`, `docs/arch/overview.md`, `docs/adr/NNNN-ui-stack.md` (by the main thread).

## Tradeoffs: Win32 + Direct2D vs egui/eframe
| | Win32 + Direct2D (no framework) | egui/eframe |
|---|---|---|
| Consistency | Same house style as `C:\test\Accs` and `C:\test\wtf_sync`; code and helper ideas reusable | New style for the owner |
| Controls needed here | Many: sliders (NC level, 10 EQ bands), dropdowns (preset, timers), toggles, a list, text. Native common controls (trackbar, combobox, checkbox, listview) cover them with real Windows look and accessibility, but each needs manual layout and message plumbing. Custom-drawn D2D controls give a consistent look but cost the most code. | All widgets exist out of the box, immediate-mode layout is quick; a 10-slider EQ is a few lines |
| Executable size | Small (a few hundred KB) | Larger (several MB with glow/wgpu), more dependencies, longer CI builds |
| Look/accessibility | Native; screen readers work with standard controls | Custom-drawn; weaker accessibility, not native look |
| Threading/redraw | Message loop + `PostMessage`; straightforward with the worker model | Repaint requests; also straightforward |
| Cost/risk | More code and Win32 `unsafe`; slower to iterate; GNU toolchain local build works (windows-sys) | Faster to build the 14-ish controls; heavy dependency tree to build on the GNU toolchain locally |
Recommendation: **Win32 with native common controls (Direct2D only if custom drawing is wanted, e.g. battery icon)** for consistency, size and native feel, because the control set is standard. Choose egui if development speed matters more than size and look. The owner decides; the protocol/core work is unaffected either way.
