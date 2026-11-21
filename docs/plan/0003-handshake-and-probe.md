# Plan 0003 Handshake and `--probe`

## Approach
`src/device/handshake.rs` drives the session through the fixed step list and returns `DeviceInfo`. `src/probe.rs` wraps it: opens the transport, runs the handshake, then loops over a static list of
read-only probe commands filtered by `DeviceInfo` functions, logging via a `Session` hook that records every frame with a timestamp. Output is plain text, one record per line, easy to diff and to turn into a test fixture.
Because `main.rs` is a security-shaped path, argument parsing stays tiny and the probe constructs the session with `Mode::ReadOnly`.

## Steps
1. Reply parsers (`01 00`, `03 00`, `05 xx`, `07 00`) with unit tests built from the layouts; FunctionType names for the ids used by the MVP (T1 0x20/0x24/0x25/0x28/0x50.. as listed in the tasks), unknown ids stay numeric.
2. Handshake sequence + error handling (stop on Table 1 unsupported).
3. Frame log hook in the session (cheap `Vec<LogEntry>` or a `Write`).
4. Probe command list (GET only), allowlist rows marked `Get`.
5. `--probe` CLI in `main.rs`, file writer, MAC redaction.
6. Mock-based tests; owner runs the probe and sends the file.

## Files likely touched
`src/main.rs`, `src/device/{mod,handshake}.rs`, `src/probe.rs`, `src/mdr/guard.rs` (GET rows), `src/mdr/session.rs` (log hook), `docs/arch/overview.md`.

## Tradeoffs
- Probing every feature GET in one pass (instead of a minimal handshake dump) costs a few more reads but removes a second round-trip with the owner; all reads are GETs on the allowlist.
- Plain text over JSON: owner can read and redact it by eye.
- Issuing probe GETs only when advertised mirrors the reference's behaviour and avoids sending unsupported commands to the headset.
