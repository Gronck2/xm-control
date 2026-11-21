# 0005 Battery (headset only)

## Goal
Read battery level (0-100) and charging status, keep it updated from notifications and a periodic refresh. No case battery.

## Why
First feature on the new core: simplest read-mostly feature, and it proves the GET, RET and NTFY path end to end on real hardware.

## Scope
- GET `22 00` (BATTERY) or `22 08` (BATTERY_WITH_THRESHOLD), chosen by the support function 0x20 vs 0x28; RET `23`, NTFY `25`.
- Layouts: `23 00 <level> <charging>`, `23 08 <level> <charging> <threshold>`; charging: NOT_CHARGING 0, CHARGING 1, UNKNOWN 2, CHARGED 3.
- State `battery: Option<{level, charging}>`; periodic re-read (e.g. every 60 s) in addition to NTFY; dev CLI `--get battery`.
- Allowlist rows `22 00`, `22 08` (GET only).

## Out of scope
Case/left/right batteries (other `PowerInquiredType` values), power-off-now.

## XM6 status
Battery: **supported**. Variant 0x00 vs 0x08: **unknown until the 0003 capture** (support both; if the capture shows neither function, battery stays None).

## Acceptance
(a) Unit tests: parse both variants; all four charging values; level >100 clamped or rejected (decide in review); NTFY `25` updates state; variant selection from a support list; missing function gives None and no command sent; GET encodes via guard.
(b) Owner: `--get battery` matches the level shown on the phone/Windows within a few percent; plug in the charger and see CHARGING (and later CHARGED at full); during `--watch`, the level change or charger plug appears without polling.

## Open owner decisions
- Recommend the 60 s periodic refresh as a fallback only; remove it if NTFY proves reliable on XM6.
