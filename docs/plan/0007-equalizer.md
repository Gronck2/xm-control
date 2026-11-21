# Plan 0007 Equalizer

## Approach
`src/device/eq.rs`: `EqInfo {bands: Vec<Hz>, steps, presets: Vec<(id, name)>}` read once at connect, `EqState {preset, bands: Vec<i8>, enabled}`. Encoder works in -6..+6 and adds the offset in one place. Validation of preset ids happens in the encoder against an XM6-accepted set intersected with the advertised list.
A 6-value param (Clear Bass layout) is a parse error, because silently reinterpreting it could write wrong bands to a different model.

## Steps
1. Parsers for `51 04`, `5B 00`, `57 00`, `53 00` with tests on hand-built bytes.
2. Encoder for `58 00` (full and preset-only), bounds checks, accepted-preset filter.
3. Feature registration, NTFY `59`, re-read after SET, CLI, guard rows.
4. Owner checklist; adjust the accepted preset list to the capture.

## Files likely touched
`src/device/eq.rs`, `src/device/mod.rs`, `src/mdr/guard.rs`, `src/main.rs`, `README` (note on Clear Bass).

## Tradeoffs
- Hard-coding the FW 3.0 accepted preset list in the encoder protects the device but may go stale with firmware updates; keep it a single constant with a comment and revisit with captures.
- Re-reading after SET (as the reference does) costs one round trip but ensures the UI shows what the headset accepted.
