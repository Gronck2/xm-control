# Plan 0005 Battery

## Approach
`src/device/battery.rs` implementing the `Feature` trait from 0004. Variant chosen once from `DeviceInfo` (0x28 preferred if both listed, matching the reference's order: confirm with the capture). Parsers are plain functions over `&[u8]` payloads for easy tests.

## Steps
1. Types `Charging`, `Battery`; parse RET `23` and NTFY `25` for both variants.
2. Feature registration + initial GET + periodic GET timer in the worker.
3. Guard allowlist rows (GET) + CLI `--get battery`.
4. Tests; owner checklist.

## Files likely touched
`src/device/battery.rs`, `src/device/mod.rs` (registry line), `src/mdr/guard.rs`, `src/main.rs` (CLI name).

## Tradeoffs
Polling plus NTFY: more traffic, but robust if XM6 does not notify every percent. Keep the interval configurable in code only.
