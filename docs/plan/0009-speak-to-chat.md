# Plan 0009 Speak-to-Chat

## Approach
`src/device/stc.rs`: `Stc {enabled, sensitivity, timeout}`. Two GETs at sync, two SET encoders merging from current state. A shared `OnOff` helper (inverted polarity, V2) is introduced here or reused from 0010 if that lands first; put it in `src/mdr/values.rs`.

## Steps
1. `values::OnOff` (ON=0x00, OFF=0x01) with tests, enums for sensitivity and timeout.
2. Parsers for the two RETs and NTFYs, encoders for `F8 0C` and `FC 0C`.
3. Registration, CLI, guard rows + `F8 09` denial test.
4. Owner checklist.

## Files likely touched
`src/device/stc.rs`, `src/mdr/values.rs`, `src/device/mod.rs`, `src/mdr/guard.rs`, `src/main.rs`.

## Tradeoffs
`F8 0C` sits next to the forbidden `F8 09` in the same command id family: the guard matches on the second byte exactly, so a typo cannot reach reset; this is why the denial test is part of the task.
