# 0006 Noise cancelling and ambient sound

## Goal
Read and set: mode (Off / NC / Ambient), ambient level 1..20, focus on voice, auto ambient and noise-adaptive sensitivity. Button-driven changes arrive as notifications.

## Why
The headline feature of the headphones.

## Scope
- One wire variant (settled by the capture): function 0x6D, type `0x19`. GET `66 19`; RET `67 19`, NTFY `69 19`, SET `68 19`:
  `<valueChange> <totalEffect> <ncAsmMode> <ambientMode> <ambLevel> <autoAsm> <noiseAdaptiveSensitivity>` (7 bytes after `68 19`).
- Fixture decode: `67 19 01 01 00 00 14 00 00` = CHANGED, total effect ON, mode NC, ambient NORMAL, level 20, auto ambient OFF, sensitivity STANDARD, i.e. NC on; the level is the remembered ambient level and is kept while in NC mode.
- Mapping: Off = totalEffect OFF (0); NC = ON (1) + mode NC (0); Ambient = ON + mode ASM (1); focus on voice = ambientMode VOICE (1); level 1..20; auto ambient = `autoAsm` NcAsmOnOff (OFF 0, ON 1, **not** inverted); sensitivity STANDARD 0 / HIGH 1 / LOW 2. valueChange UNDER_CHANGING 0 / CHANGED 1.
- Slider drag: send `valueChange` UNDER_CHANGING while dragging and CHANGED on release; the core rate-limits drag updates (recommend max ~10/s) and always sends the final CHANGED.
- Always write the full tuple (read-modify-write from current state) so unrelated fields are not reset.
- Dev CLI: `--get nc`, `--set nc <off|nc|ambient> [--level N] [--voice on|off] [--auto on|off] [--sens standard|high|low]`.
- Allowlist: GET `66 19`; SET `68 19` + 7 validated bytes (each field within its enum, level 1..20). The guard's exact-match rows (see the shortcut note in `src/mdr/guard.rs`) need a small per-row validator for SETs; the first SET feature PR to land adds it, later PRs reuse it. `64` stays denied.
- **Dropped for XM6:** the 0x6B/`17` and 0x67/`22` variants and their allowlist rows (functions not advertised); the 0x6B/0x6D conflict does not exist on XM6. The NC/AMB button read (`66 30`, function 0x69) is not captured and not part of this task.

## Out of scope
Function 0x71 ADAPTIVE_CONTROL (advertised, but the reference has no code for it: unknown semantics; ignore its frames), 0x70 AUTO_NCASM (not advertised), NC test mode, ambient capability steps `60 19` (use the fixed 1..20 range), NC/AMB button read and editing.

## XM6 status
**Confirmed:** type `0x19` is the only NC variant advertised (0x6D present; 0x6B, 0x67 absent), reply layout verified by the capture. SET layout, NTFY `69` and the valid level range 1..20 are reference-derived (unverified on XM6 until the batch 1 hardware check).

## Hardware
**Sends SET commands** (`68 19`). The PR merges only after the owner confirms the behaviour on the headphones (hardware batch 1).

## Acceptance
(a) Unit tests, using `tests/fixtures/xm6-probe.txt` for the reply (`67 19 ...`): parse the fixture reply; encode/decode of the full tuple; mode mapping both directions; level bounds 1..20 enforced (0 and 21 rejected); read-modify-write keeps other fields; NTFY `69` updates state; gating from the real XM6 support list; guard accepts valid `68 19 ...` and rejects a wrong length, an out-of-range field, `68 17 ...`, `68 22 ...`; denied `64`/test-mode commands still rejected; drag rate limiter emits final CHANGED.
(b) Owner: set NC / Ambient / Off from the CLI and say what the headphones do (voice prompt, audible change); level 1 vs 20 clearly different; focus-on-voice audibly different; auto ambient and sensitivity toggles take effect (sensitivity is subtle: confirm the setting reads back); press the NC/AMB button on the headset and see the NTFY in `--watch`. Restore the previous mode afterwards (it was NC, level 20 at capture time).

## Open owner decisions
- Expose "auto ambient" and "adaptive sensitivity": recommend yes (the 0x19 layout carries them; they are always present on XM6).
