# Plan 0010 Power and wearing

## Approach
`src/device/power.rs` (timer, auto pause) and `src/device/wearing.rs` (T2 read-only). The timer feature holds `type: 04|05` chosen from the support list, and a candidate list from the capability reply. Uses the shared `OnOff` from `src/mdr/values.rs` (0009).
Wearing is a T2 feature: this is the first feature that exercises T2 framing, so it must pass data type 0x0E through the guard and session.

## Steps
1. Parsers/encoders for `21`, `27`, `28` of both types; elements enum.
2. Auto pause GET/SET/NTFY.
3. Wearing GET/NTFY with the compound gating rule.
4. Registration, CLI, guard rows and the `24 ..` / `F8 0F` rejection tests.
5. Owner checklist.

## Files likely touched
`src/device/{power,wearing}.rs`, `src/device/mod.rs`, `src/mdr/guard.rs`, `src/mdr/values.rs`, `src/main.rs`.

## Tradeoffs
- No "power off now" even though trivial: it is a destructive-feeling action and `24 03` sits next to the forbidden `24 03 FF`. Revisit after the MVP.
- Splitting wearing from power keeps the T2 path isolated, but it ships in one PR to keep the task count down; split it if the PR gets large.
