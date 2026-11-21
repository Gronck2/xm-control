# Plan 0010 Power and wearing

## Approach
`src/device/power.rs` (wearing-variant auto power-off, auto pause) and `src/device/wearing.rs` (T2 read-only). The power-off feature has one wire type (`05`) and a candidate list from the capability reply; no variant selection (0x24 is not advertised on XM6). Uses the shared `OnOff` from `src/mdr/values.rs` (0009).
Wearing is a T2 feature: this is the first feature that exercises T2 framing, so it must pass data type 0x0E through the guard and session.

## Steps
1. Parsers/encoders for `21 05`, `27 05`, `28 05`; element enum (0x00-0x04, 0x10, 0x11), candidate filter.
2. Auto pause GET/SET/NTFY.
3. Wearing GET/NTFY with the compound gating rule.
4. Registration, CLI, guard rows and the `24 ..` / `F8 0F` / `28 04` rejection tests.
5. Owner checklist (batch 3).

## Files likely touched
`src/device/{power,wearing}.rs`, `src/device/mod.rs`, `src/mdr/guard.rs`, `src/mdr/values.rs`, `src/main.rs`.

## Tradeoffs
- No "power off now" even though trivial: it is a destructive-feeling action and `24 03` sits next to the forbidden `24 03 FF`. Revisit after the MVP.
- Splitting wearing from power keeps the T2 path isolated, but it ships in one PR to keep the task count down; split it if the PR gets large.
- Dropping the 0x24 timer path removes code XM6 never runs; other models are not a goal of the MVP.
