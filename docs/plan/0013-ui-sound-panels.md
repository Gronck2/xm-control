# Plan 0013 UI sound panels

## Approach
Extend the 0012 view model with one section per feature, built from `DeviceState` and rendered by the chosen toolkit. User intents map 1:1 to existing `Command`s (from 0005-0008), so no protocol code is added in this task. To stop echo, the view model compares the incoming value to the displayed one and only emits a command from a real user gesture.

## Steps
1. View-model sections + tests (hidden/disabled rules, echo suppression).
2. Battery, NC/ambient panel (with drag/release semantics).
3. EQ panel (preset dropdown + 10 sliders, labels).
4. DSEE panel if advertised.
5. Owner checklist; screenshots are the owner's to judge.

## Files likely touched
`src/ui/model.rs`, `src/ui/view.rs` (or per-panel files), tests in the same files.

## Tradeoffs
- EQ send-on-release vs live: live feels responsive but floods a stop-and-wait link; the NC slider uses live updates only through the core's rate limiter.
- Hiding vs greying unavailable controls: hide when never advertised, grey when temporarily unavailable (EQ status disabled) so the user sees why.
