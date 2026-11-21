# 0001 MDR frame codec and command guard

## Goal
Pure, hardware-free layer: pack/unpack MDR frames and a command guard that is the only way to turn a payload into bytes for the device.

## Why
Everything else sits on this. The safety rule (no DFU, factory reset, unpair) must be enforced at the lowest layer so no later feature or UI code can bypass it.

## Scope
- Frame: `3E | esc(type, seq, len u32 BE, payload, checksum) | 3C`; escape `3C->3D 2C`, `3D->3D 2D`, `3E->3D 2E`; 8-bit sum checksum; max 2048 bytes; data types ACK 0x01, T1 0x0C, T2 0x0E (protocol.md §2).
- Streaming parser: feed bytes, get complete frames, drop bad ones (bad checksum, bad escape, length mismatch), resync on the next `3E`.
- Command guard: payload is accepted only if (1) the denylist does not match and (2) the allowlist matches. Output is an `AllowedCommand` type; the frame packer only accepts that type.
  Data types other than 0x0C/0x0E are rejected for sending.
- Denylist (protocol.md §5): T1 cmd 0x30-0x3F; T1 `F8 09 xx`; T1 `24 03 FF`; T1 `24 07`; T1 `64`, `F4`, `0F`; T2 `3C xx 02` (unpair); T2 `34 ..` (pairing mode); LEA (T1 0x40-0x4D, T2 0x60-0x69), PARTY (T2 0x70-0x7C).
- Allowlist in this task: only the handshake GETs (T1 `00 00`, `02 00`, `04 01|02|03`, `06 00`, T2 `06 00`). Later tasks add entries.
- A read-only mode of the guard (GET commands only) used by `--probe`.

## Out of scope
Transport, seq/ACK/retry (0002), any feature command, power-off-now (`24 03 01`), source switching/connect/disconnect (`3C`).

## XM6 status
Framing and the denylist are protocol-wide: **supported**. No XM6-specific unknowns.

## Acceptance
(a) Unit tests, no hardware:
- The worked frames from protocol.md §2 pack to exactly those bytes (GET_PROTOCOL_INFO, GET_CAPABILITY_INFO, both host ACKs) and unpack back.
- Escaping round trip for payload and checksum bytes equal to 3C/3D/3E; invalid byte after `3D` rejected; checksum error, length mismatch, <7 bytes rejected.
- Parser: split across reads, two frames in one read, garbage before `3E`, resync after a bad frame.
- Guard: every denylist row is rejected (table-driven test, including each of 0x30..=0x3F, `F8 09 00`, `F8 09 01`, `24 03 FF`, `3C 00 02 <mac>`, `3C 02 02 <mac>`, `34 02 01 00`);
  unknown commands are rejected by default; handshake GETs are accepted; read-only mode rejects any SET.
- A test that no allowlist entry intersects the denylist, and that `E8` (listening modes) is not allowlisted.
(b) Owner: nothing to verify on hardware (no device traffic yet).

## Open owner decisions
- Significant design: "typed allowlist enforced in the encoder, denylist first". Recommend an ADR (flagged for the main thread; not created here).
- Recommend: reject at packing time with an error, never silently drop.
