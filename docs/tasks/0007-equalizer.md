# 0007 Equalizer: presets and 10-band custom

## Goal
Read and set the EQ preset and the 10-band custom curve (-6..+6), and reflect headset-side changes.

## Why
Core sound control. XM6 has specific quirks that must be modelled, not copied from other models.

## Scope
- Gate on T1 function 0x57 PRESET_EQ_AND_ERRORCODE (advertised; 0x56 TURN_KEY_EQ is also listed but unused; 0x50/0x52/0x53 absent).
- Reads: capability `50 04 01` (01 = ENGLISH) -> `51 04 <band> <step> <MDRArray{presetId, name}>`; band info `5A 00` -> `5B 00 0A` + 10 x `{01, freq u16 BE}`; param `56 00` -> `57 00 <presetId> <count> <steps>`; NTFY `59` (reference).
- Fixture facts: bands are 31, 63, 125, 250, 500, 1000, 2000, 4000, 8000, 16000 Hz (all typed HZ, so 1000 is `03 E8`); `57 00 32 0A 0A 0A 05 05 06 06 06 08 08 09` = preset `0x32` with raw steps `0A 0A 05 05 06 06 06 08 08 09`, i.e. +4 +4 -1 -1 0 0 0 +2 +2 +3 (offset 6).
- SET `58 00 <presetId> <count> <steps...>` (count `0A`); preset only = `58 00 <id> 00`; then re-read `56 00`. A factory preset's own curve is what `56 00` reports, so after a preset-only SET the UI shows the re-read bands.
- Steps raw = value + 6 (13 steps, raw 0..12). Enforce -6..+6 per band. The capability reply (not in the capture) confirms 13 steps; if it reports another step count, the feature disables SET rather than guessing.
- Presets offered: those the headset lists AND that XM6 FW 3.0 accepts: HEAVY 0x30, CLEAR 0x31, HARD 0x32, SOFT 0x33, CUSTOM 0xA0, user settings 0xA1-0xA5. Everything else is hidden/rejected by the encoder (not just the UI). Display names come from the capability reply.
- **Deviation from the owner's wish: XM6 has NO Clear Bass slot.** The param has exactly 10 values (confirmed: count `0A`) = 10 bands; the Clear Bass layout (CB + 5 bands, -10..10) belongs to XM4/XM5-class devices. We do not implement Clear Bass; the UI task must not show it.
- **EQ status is not read:** T1 `52 00` is ACKed but never answered on XM6, so the feature does not send it. EQ is treated as enabled; if an NTFY `53`/`55` ever says disabled, controls are disabled and SET is not sent. (Listening modes, the usual cause of "disabled", are out of the MVP.)
- Dev CLI: `--get eq`, `--set eq preset <name>`, `--set eq bands <10 ints>`.
- Allowlist: GET `50 04 01`, `5A 00`, `56 00`; SET `58 00` with validated arguments (preset id in the accepted set; empty steps or count 10 with each raw step 0..12). The existing probe row `52 00` is not used by this feature.

## Out of scope
Naming/saving user presets, Clear Bass, EQ preset OFF/ROCK/... (rejected by XM6 FW 3.0), listening modes, reading EQ status.

## XM6 status
**Confirmed** by the capture: 10 bands and their frequencies, the param layout (preset + count + 10 raw steps), no Clear Bass slot, `52 00` unanswered. **Reference-only (not captured):** the `50 04 01` capability reply (13 steps, preset list and names), the offset 6 (the observed raw steps 5..10 fit -6..+6), NTFY `59`, which user slots exist.

## Hardware
**Sends SET commands** (`58 00`). The PR merges only after the owner confirms the behaviour on the headphones (hardware batch 2).

## Acceptance
(a) Unit tests, using `tests/fixtures/xm6-probe.txt` for the replies (`5B 00 ...`, `57 00 ...`): step encode/decode at -6, 0, +6 and out-of-range rejection; the fixture param parses to the values above; a 6-value (Clear Bass) param is rejected with a clear error rather than mis-mapped; preset-only SET bytes; encoder refuses forbidden preset ids (e.g. 0x01 ROCK); band info parse (including the escaped `3D 2E 80` frame from the fixture); the feature never sends `52 00`; NTFY `59` updates state; guard accepts valid `58 00` forms and rejects a wrong count, a raw step above 12, a forbidden preset.
(b) Owner: first run `--get eq` and report the capability output (preset names, step count) since it is not in the capture; switch among HEAVY/CLEAR/HARD/SOFT from the CLI and describe the audible change; set a custom curve (e.g. strong bass +6 at 31/63 Hz, flat elsewhere) and confirm it sounds as expected and survives a reconnect; change the preset on the phone app if available and see NTFY in `--watch`; report whether non-listed presets are rejected or ignored by the device. Restore the preset that was active (0x32) afterwards.

## Open owner decisions
- Accept that Clear Bass is not available on XM6 (recommend: yes; document it in the README).
- Whether the UI should offer the 5 user slots: recommend only slots the capability reply reports.
- ADR candidate: none.
