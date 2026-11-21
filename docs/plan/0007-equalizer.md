# Plan 0007 Equalizer

## Approach
`src/device/eq.rs`: `EqInfo {bands: Vec<Hz>, steps, presets: Vec<(id, name)>}` read once at connect, `EqState {preset, bands: Vec<i8>}` (no `enabled` read: XM6 does not answer `52 00`). Encoder works in -6..+6 and adds the offset in one place. Validation of preset ids happens in the encoder against an XM6-accepted set intersected with the advertised list.
A 6-value param (Clear Bass layout) is a parse error, because silently reinterpreting it could write wrong bands to a different model. If the capability read fails or reports a step count other than 13, the feature stays read-only.

## Steps
1. Parsers for `51 04`, `5B 00`, `57 00` with tests on the fixture bytes (and hand-built bytes for `51 04`, which is not in the capture).
2. Encoder for `58 00` (full and preset-only), bounds checks, accepted-preset filter; guard rows with argument validators (reuse the validator from the first SET PR to land, see 0006).
3. Feature registration (gated on 0x57), NTFY `59`, re-read after SET, CLI.
4. Owner checklist (batch 2); adjust the accepted preset list to the capability output.

## Files likely touched
`src/device/eq.rs`, `src/device/mod.rs`, `src/mdr/guard.rs`, `src/main.rs`, `README` (note on Clear Bass).

## Tradeoffs
- Hard-coding the FW 3.0 accepted preset list in the encoder protects the device but may go stale with firmware updates (the capture is FW 3.1.5); keep it a single constant with a comment and revisit with captures.
- Re-reading after SET (as the reference does) costs one round trip but ensures the UI shows what the headset accepted.
- Skipping `52 00` loses the "EQ disabled" signal, but the headset never answers it; guessing from silence would be worse.
