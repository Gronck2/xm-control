# Plan 0001 MDR frame codec and command guard

## Approach
New module `src/mdr/` with `frame.rs` (pack, unpack, streaming `FrameParser`) and `guard.rs` (`Table {T1,T2}`, `AllowedCommand`, `check(table, &[u8], Mode)`).
`AllowedCommand` has a private constructor so `frame::pack` can only be fed through the guard. The allowlist is a static table of `(table, cmd, optional sub-byte, kind Get|Set)`;
later tasks append rows next to their feature code (a `const` slice per feature, merged in one function) so the guard stays the single checkpoint.
Denylist is evaluated before the allowlist, so a mistaken allowlist row cannot re-enable a forbidden command (the intersection test catches it too).

## Steps
1. `frame.rs`: constants, `escape`/`unescape`, `checksum`, `pack(AllowedCommand | Ack, seq)`, `unpack`, `FrameParser::push(&[u8]) -> Vec<Frame>`.
2. `guard.rs`: tables, `Mode::{Normal, ReadOnly}`, denylist matcher, allowlist rows for the handshake, `check`.
3. Tests as listed in the task; table-driven denylist vectors copied from protocol.md §5 by hand.
4. Short `docs/arch/overview.md` update (layers).

## Files likely touched
`src/main.rs` (`mod mdr;` only), `src/mdr/mod.rs`, `src/mdr/frame.rs`, `src/mdr/guard.rs`, `docs/arch/overview.md`.

## Tradeoffs
- Allowlist-only (study's recommendation) vs denylist-only: allowlist is safer for unknown commands; the explicit denylist is kept as a second belt and as documented tests.
- Typed `AllowedCommand` vs a runtime check inside `send`: the type makes bypass a compile error.
- Incoming frames are never filtered by the guard (we must be able to read anything the headset sends).
