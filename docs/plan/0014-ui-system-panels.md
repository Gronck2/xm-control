# Plan 0014 UI system panels

## Approach
Same pattern as 0013: view-model sections over `DeviceState` and intents mapped to existing `Command`s; no protocol code. A confirm step for multipoint is part of the view model (pending-confirmation state) so it is testable without the toolkit.

## Steps
1. View-model sections + tests (candidates, hidden rules, confirmation, wearing text).
2. Speak-to-Chat panel.
3. Power/wearing panel.
4. Multipoint toggle with confirmation + device list.
5. Owner checklist; update `docs/arch/overview.md` to the final shape and the README feature list (including "no Clear Bass on XM6").

## Files likely touched
`src/ui/model.rs`, `src/ui/view.rs`, `docs/arch/overview.md`, `README.md`.

## Tradeoffs
A confirmation dialog adds a click but protects against dropping the PC's own link; the list is a plain read-only list (no context menu) to keep unpair unreachable from the UI by construction.
