# Plan 0005 Battery

## Approach
`src/device/battery.rs` implementing the `Feature` trait from 0004. One wire variant only (`22 00` / `23 00`, function 0x20); the XM6 capture shows 0x28 is not advertised, so there is no variant selection. Parsers are plain functions over `&[u8]` payloads for easy tests.

## Steps
1. Types `Charging`, `Battery`; parse RET `23 00` and NTFY `25 00`.
2. Feature registration + initial GET + periodic GET timer in the worker (timer active only until the first battery NTFY).
3. Guard allowlist row (GET `22 00`) + CLI `--get battery`.
4. Tests from the fixture reply; owner checklist (batch 1).

## Files likely touched
`src/device/battery.rs`, `src/device/mod.rs` (registry line), `src/mdr/guard.rs`, `src/main.rs` (CLI name).

## Tradeoffs
Polling plus NTFY: more traffic, but robust if XM6 does not notify every percent. Keep the interval configurable in code only. Dropping the threshold variant saves code that the XM6 would never run.
