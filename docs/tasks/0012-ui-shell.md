# 0012 UI shell: window, connection, info panel

## Goal
An ordinary Windows settings window (no tray) that lets the user pick a paired headset, connect/disconnect, shows connection state and the device info panel (model, firmware, MAC), and hosts the panels added by 0013 and 0014.

## Why
First user-visible app; also where the UI stack is fixed.

## Blocked by
**An ADR for the UI stack (owner decision; the ADR is written by the main thread after the owner decides, not by this task).** Candidates: Win32 + Direct2D without a framework, or egui/eframe. Depends on 0004 (core API). Does not block 0001-0011.

## Scope
- Window with: device picker (paired devices from 0002) + Connect/Disconnect, status line (connecting / connected / disconnected + reason / error), info panel (model name, firmware version read-only, MAC), empty areas for the feature panels.
- Core runs on its worker thread; the UI consumes `Event`s and sends `Command`s only (0004). UI thread never blocks on Bluetooth.
- State changes from the headset (button presses, other apps) update the shown values without user action.
- Window close disconnects cleanly. No tray, no autostart, no settings file in this task.
- Controls not advertised by the headset are hidden.

## Out of scope
The feature panels (0013, 0014), installer, icon polish, dark mode, localization.

## XM6 status
Info panel data (model, firmware): **supported** (handshake). MAC from OS enumeration.

## Acceptance
(a) Unit tests: view-model logic that is UI-stack independent (event -> display state: status text mapping, hidden when feature is None, firmware/model/MAC formatting) lives in plain Rust and is tested; clippy/fmt/test green; app starts and exits in CI smoke form if the stack allows (no device needed).
(b) Owner: launch, pick the XM6, connect: model "WH-1000XM6", firmware and MAC shown correctly; turn the headset off: UI shows disconnected without hanging; reconnect works; window is responsive during connect; resize/DPI scaling look acceptable on the owner's display.

## Open owner decisions
- UI stack (see plan for the trade-off). Recommendation in the plan.
- Single fixed-size window vs resizable: recommend a fixed, compact, scalable-by-DPI window.
