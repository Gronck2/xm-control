# Plan 0006 Noise cancelling and ambient

## Approach
`src/device/ncasm.rs`: one `NcAsm` struct (mode, level, voice, auto, sensitivity as Options) and an enum of wire variants selected at connect from the support list. Encode always produces the full payload for the active variant from the whole struct.
Drag rate limiting lives in the device worker (coalesce to latest value, flush on release) so UI code stays dumb.

## Steps
1. Variant selection + parsers/encoders for 0x19, 0x17, 0x22 with table-driven tests (bytes from protocol.md §4.2).
2. State mapping (Off/NC/Ambient) and validation (1..20).
3. Feature registration, NTFY `69`, CLI, guard rows.
4. Coalescing of UNDER_CHANGING updates + test.
5. Owner checklist run; adjust to the capture (type choice, optional fields).

## Files likely touched
`src/device/ncasm.rs`, `src/device/mod.rs`, `src/mdr/guard.rs`, `src/main.rs`.

## Tradeoffs
- Supporting three variants costs little (shared struct) and avoids guessing the XM6 variant; if the capture is clear, delete the unused ones.
- Coalescing in core vs UI: core keeps behaviour identical for any UI stack.
- Not touching 0x70/0x71 avoids sending unknown layouts to the device.
