# Plan 0004 Device state, notifications and dev CLI

## Approach
`src/device/mod.rs`: worker thread loop `select`s between the command receiver and transport reads (short read timeout). Commands are serialized through the session (stop-and-wait); incoming frames
are ACKed by the session first, then dispatched by `(table, command id)` through a registry of feature handlers. Handlers mutate `DeviceState` and push `Event::StateChanged`.
`trait Feature { fn name; fn functions_required() -> &[FnId]; fn initial_gets(&DeviceInfo) -> Vec<Cmd>; fn on_frame(&mut State, &Frame) -> bool; fn encode(set) -> AllowedCommand }`.
Registration is a static slice so adding a feature is one line, and the dev CLI and the UI both consume the same `Command` enum.

## Steps
1. State, Command, Event types; feature trait + registry; dummy feature for tests.
2. Worker thread with mock-transport tests (ordering, NTFY, not-advertised, disconnect).
3. Dev CLI: `--get <name>`, `--set <name> <args>`, `--watch` (print events); connect via `--device <mac>`.
4. Update `docs/arch/overview.md` with the layer diagram.

## Files likely touched
`src/device/{mod,state,feature}.rs`, `src/main.rs`, `src/mdr/session.rs`, `docs/arch/overview.md`.

## Tradeoffs
- One worker + channels vs shared `Arc<Mutex<State>>`: channels give a clean event stream (needed for NTFY to UI) and are UI-framework neutral; a state snapshot is still copied into the event for simple UIs.
- Feature trait adds a little indirection but keeps 0005-0011 independent and parallelizable (separate files, one registry line each).
- Dev CLI is throwaway-grade but kept: it is how the owner tests protocol tasks without a UI.
