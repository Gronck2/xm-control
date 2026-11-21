# Plan 0011 Multipoint and paired devices

## Approach
`src/device/peripheral.rs`: `Peripherals {multipoint: Option<bool>, devices: Vec<PairedDevice{mac, connected, class, name}>}`. The list parser handles both variants by a flag. The guard gets T2 rows keyed on `(0x0E, cmd, sub)` and a blanket deny on T2 `3C` and `34` that is checked before the allowlist.

## Steps
1. Parsers for `37 xx` (both variants) and `37 01`/NTFY; tests with hand-built frames, including bad MAC length.
2. Multipoint SET with raw polarity (test that it does not reuse `OnOff`).
3. Registration, CLI, guard rows and denial tests.
4. Owner checklist.

## Files likely touched
`src/device/peripheral.rs`, `src/device/mod.rs`, `src/mdr/guard.rs`, `src/main.rs`.

## Tradeoffs
- The list is read-only by design: unpair lives in the same command (`3C`) as connect/disconnect, so we deny the whole command and add nothing later without a new task and review.
- Variant selection by support list (not by model) follows the reference.
