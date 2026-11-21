# Plan 0009 Speak-to-Chat

## Approach
`src/device/stc.rs`: `Stc {enabled, sensitivity, timeout}`. Two GETs at sync, two SET encoders merging from current state. A shared `OnOff` helper (inverted polarity, V2) is introduced here or reused from 0010 if that lands first; put it in `src/mdr/values.rs`.

## Steps
1. `values::OnOff` (ON=0x00, OFF=0x01) with tests, enums for sensitivity and timeout.
2. Parsers for `F7 0C`, `FB 0C` and the NTFYs `F9 0C`, `FD 0C`; encoders for `F8 0C` and `FC 0C` (tests use the fixture bytes).
3. Registration (gated on 0xFC), CLI, guard rows with validators + `F8 09` denial test.
4. Owner checklist (batch 2).

## Files likely touched
`src/device/stc.rs`, `src/mdr/values.rs`, `src/device/mod.rs`, `src/mdr/guard.rs`, `src/main.rs`.

## Tradeoffs
`F8 0C` sits next to the forbidden `F8 09` in the same command id family: the guard matches on the second byte exactly, so a typo cannot reach reset; this is why the denial test is part of the task.
