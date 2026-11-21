# Plan 0006 Noise cancelling and ambient

## Approach
`src/device/ncasm.rs`: one `NcAsm` struct (mode, level, voice, auto, sensitivity; all always present because XM6 uses the 0x19 layout) with a single parser/encoder pair. Encode always produces the full 9-byte payload from the whole struct.
Drag rate limiting lives in the device worker (coalesce to latest value, flush on release) so UI code stays dumb.
The guard gets its first SET rows here (or in whichever SET PR lands first): a per-row validator hook next to the exact-match table.

## Steps
1. Parser/encoder for `67/68/69 19` with table-driven tests (fixture reply `67 19 01 01 00 00 14 00 00`).
2. State mapping (Off/NC/Ambient) and validation (1..20).
3. Feature registration (gated on 0x6D), NTFY `69`, CLI, guard rows + validator.
4. Coalescing of UNDER_CHANGING updates + test.
5. Owner checklist run (batch 1).

## Files likely touched
`src/device/ncasm.rs`, `src/device/mod.rs`, `src/mdr/guard.rs`, `src/main.rs`.

## Tradeoffs
- The capture proves 0x19, so the 0x17/0x22 variants are deleted from scope: less code, and no way to send a layout the headset does not use.
- Coalescing in core vs UI: core keeps behaviour identical for any UI stack.
- Not touching 0x70/0x71 avoids sending unknown layouts to the device.
- A validator hook in the guard is a little more code than exact rows, but SET payloads carry arguments; exact-match rows cannot express them.
