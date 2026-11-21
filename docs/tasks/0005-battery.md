# 0005 Battery (headset only)

## Goal
Read battery level (0-100) and charging status, keep it updated from notifications and a periodic refresh. No case battery.

## Why
First feature on the new core: simplest read-mostly feature, and it proves the GET, RET and NTFY path end to end on real hardware.

## Scope
- Gate on T1 function 0x20 BATTERY_LEVEL_INDICATOR (advertised on XM6). GET `22 00`; RET `23 00 <level> <charging>`; NTFY `25 00 <level> <charging>` (reference layout).
- Charging: NOT_CHARGING 0, CHARGING 1, UNKNOWN 2, CHARGED 3. Level above 100 is a parse error (not clamped).
- State `battery: Option<{level, charging}>`; dev CLI `--get battery`.
- Refresh: read at connect, update from NTFY, and re-read every 60 s only while no battery NTFY has been seen (fallback; drop it if NTFY proves reliable).
- Allowlist row `22 00` (GET only; it already exists for `--probe`, this task makes it a feature row).
- **Dropped for XM6:** the threshold variant (`22 08` / `23 08`, function 0x28) is not advertised, so the feature never sends it and has no variant selection. (The `--probe` guard row for `22 08` stays until the probe is cleaned up.)

## Out of scope
Case/left/right batteries (other `PowerInquiredType` values; functions 0x21/0x22 are not advertised), power-off-now.

## XM6 status
**Confirmed** by the capture: `22 00` -> `23 00 61 00` = 97 %, not charging (protocol.md 8.3). NTFY `25` is reference-only.

## Hardware
Read-only: **no SET commands**. Merges on green CI and a clean pipeline; the owner checks below run in hardware batch 1 (0004+0005+0006).

## Acceptance
(a) Unit tests, with the reply bytes taken from `tests/fixtures/xm6-probe.txt` (`23 00 61 00` -> 97 %, NOT_CHARGING): all four charging values; level >100 rejected; NTFY `25` updates state; gating from the real XM6 support list (0x20 present, 0x28 absent); missing function gives None and no command sent; GET encodes via guard; the feature never emits `22 08`.
(b) Owner: `--get battery` matches the level shown on the phone/Windows within a few percent; plug in the charger and see CHARGING (and later CHARGED at full); during `--watch`, the level change or charger plug appears without polling.

## Open owner decisions
- Recommend the 60 s periodic refresh as a fallback only; remove it if NTFY proves reliable on XM6.
