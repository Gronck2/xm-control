# Architecture overview

Current shape: `src/main.rs` prints the version. There is a hardware-free protocol layer in `src/mdr/`; there is no Bluetooth transport or UI yet.

Layers built so far:
- `src/mdr/frame.rs`: MDR frame pack/unpack (escaping, checksum, 2048-byte limit) and a streaming `FrameParser` that drops bad frames and resyncs on `3E`.
- `src/mdr/guard.rs`: the command guard. `check(table, payload, mode)` is the only producer of `AllowedCommand`, and `frame::pack_command` accepts only that type
  (denylist first, then allowlist, unknown rejected; see ADR 0003). Incoming frames are not filtered.

Intended shape (not built yet): a Windows window app that opens an RFCOMM connection to Sony WH-1000XM6 headphones,
speaks Sony's MDR protocol (framing, ACKs, get/set/notify commands) and shows the headphone settings in one window.
The UI stack is decided in ADR 0002.
