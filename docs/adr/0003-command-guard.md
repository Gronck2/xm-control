# 0003. Command guard: typed AllowedCommand, denylist first

Status: accepted (2026-11-21)

## Context

The app sends commands to headphones we cannot see or hear. Some commands are dangerous: firmware update / DFU, factory reset, unpair,
pairing mode, test modes (docs/arch/protocol.md section 5). A runtime check scattered over feature code or the UI can be forgotten or bypassed.

## Decision

- Frame packing accepts only an `AllowedCommand`, a type whose constructor is private to the guard (`src/mdr/guard.rs`). Bypassing the guard is a compile error.
- The guard checks the denylist first, then the allowlist. A mistaken allowlist row therefore cannot re-enable a forbidden command; a test also checks
  that no allowlist row matches the denylist.
- Unknown commands (not on the allowlist) are rejected. Feature tasks add allowlist rows next to their code.
- Rejection happens at packing time with an error; commands are never silently dropped.
- A read-only mode accepts GET rows only (for `--probe`).
- Incoming frames are not filtered: we must be able to read anything the headset sends.

## Consequences

- Every new command needs an explicit allowlist row and a test; this is deliberate friction.
- The denylist is duplicated knowledge (also in protocol.md); changes must update both.
- Rows currently match the whole payload exactly; a pattern matcher is needed with the first SET rows.
