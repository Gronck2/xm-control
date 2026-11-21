# 0007 Equalizer: presets and 10-band custom

## Goal
Read and set the EQ preset and the 10-band custom curve (-6..+6), reflect headset-side changes, and show when EQ is unavailable.

## Why
Core sound control. XM6 has specific quirks that must be modelled, not copied from other models.

## Scope
- Capability `50 04 <lang>` -> `51 04 <band> <step> <MDRArray{presetId, name}>`; band freqs `5A 00` -> `5B 00` (expect 31, 63, 125, 250, 500, 1k, 2k, 4k, 8k, 16k Hz); param `56 00` -> `57 00 <presetId> <PodArray steps>`; NTFY `59`; status `52 00` -> `53 00 <EnableDisable> <errors>`.
- SET `58 00 <presetId> <count> <steps...>`; preset only = `58 00 <id> 00`; then re-read `56 00`.
- Steps raw = value + 6 (13 steps, raw 0..12). Enforce -6..+6 per band.
- Presets offered: those the headset lists AND that XM6 FW 3.0 accepts: HEAVY 0x30, CLEAR 0x31, HARD 0x32, SOFT 0x33, CUSTOM 0xA0, user settings 0xA1-0xA5. Everything else is hidden/rejected by the encoder (not just the UI).
- **Deviation from the owner's wish: XM6 has NO Clear Bass slot.** The XM6 param has 10 values = 10 bands; the Clear Bass layout (CB + 5 bands, -10..10) belongs to XM4/XM5-class devices. We do not implement Clear Bass; the UI task must not show it. The owner's wish is recorded as not possible on this model (unless the capture shows a different layout).
- Status `52 00`: when EQ is disabled (e.g. a listening mode is active) controls are disabled and SET is not sent.
- Dev CLI: `--get eq`, `--set eq preset <name>`, `--set eq bands <10 ints>`.
- Allowlist: `50 04`, `5A 00`, `56 00`, `52 00` (GET); `58 00` (SET).

## Out of scope
Naming/saving user presets, Clear Bass, EQ preset OFF/ROCK/... (rejected by XM6 FW 3.0), listening modes.

## XM6 status
EQ: **supported** with caveats above. Band count/steps/preset list: from the reference code (10/13/9), **confirm in the capture**. Which user slots exist: **unknown**.

## Acceptance
(a) Unit tests: step encode/decode at -6, 0, +6 and out-of-range rejection; 10-value param parse; reject a 6-value (Clear Bass) param with a clear error rather than mis-mapping; preset-only SET bytes; encoder refuses forbidden preset ids (e.g. 0x01 ROCK); band info parse; status disabled blocks SET; NTFY `59` updates state; guard rows.
(b) Owner: switch among HEAVY/CLEAR/HARD/SOFT from the CLI and describe the audible change; set a custom curve (e.g. strong bass +6 at 31/63 Hz, flat elsewhere) and confirm it sounds as expected and survives a reconnect; change the preset on the phone app if available and see NTFY in `--watch`; report whether non-listed presets are rejected or ignored by the device.

## Open owner decisions
- Accept that Clear Bass is not available on XM6 (recommend: yes; document it in the README).
- Whether the UI should offer the 5 user slots: recommend only slots the capture reports.
- ADR candidate: none.
