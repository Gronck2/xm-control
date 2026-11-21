# Plan 0011 Multipoint and paired devices

## Approach
`src/device/peripheral.rs`: `Peripherals {multipoint: Option<bool>, devices: Vec<PairedDevice{mac, connected, class, name}>}`. One list parser (type `0x02` layout, confirmed by the capture). The guard gets T2 rows keyed on `(0x0E, cmd, sub)`; the T2 `3C` denial is widened from "third byte 02" to every `3C` form (`(Table::T2, 0x3C) => true`), and `34` stays a blanket deny; both are checked before the allowlist.

## Steps
1. Parser for `37 02` (and its NTFY `39 02`) and `37 01`/`39 01`; tests with the fixture frames plus hand-built edge cases (bad lengths, truncated entry).
2. Multipoint SET with raw polarity (test that it does not reuse `OnOff`).
3. Guard: widen the `3C` deny, add rows, denial tests (loop over `3C` second/third bytes, truncated forms).
4. Registration, CLI (`--yes` for multipoint), owner checklist (batch 3).

## Files likely touched
`src/device/peripheral.rs`, `src/device/mod.rs`, `src/mdr/guard.rs`, `src/main.rs`.

## Tradeoffs
- The list is read-only by design: unpair lives in the same command (`3C`) as connect/disconnect, so we deny the whole command and add nothing later without a new task and review. The wider deny (instead of only `3C ? 02`) was asked for by the code reviewer: a future code path that builds a `3C` with a wrong byte can no longer slip through.
- Variant selection by support list (not by model) follows the reference; with the capture the `0x00` variant has no consumer on XM6 and is not implemented.
- No hex check on MACs: the committed fixture has placeholder MACs; real ones are 17 printable ASCII bytes either way.
