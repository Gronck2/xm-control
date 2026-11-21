# 0006 Noise cancelling and ambient sound

## Goal
Read and set: mode (Off / NC / Ambient), ambient level 1..20, focus on voice, and the auto/adaptive fields where the headset has them. Button-driven changes arrive as notifications.

## Why
The headline feature of the headphones.

## Scope
- Function gating (protocol.md §4.2): 0x6D with type `0x19`, else 0x6B with `0x17`, else 0x67 with `0x22` (ambient only). Layouts:
  `68 19 <valueChange> <totalEffect> <ncAsmMode> <ambientMode> <ambLevel> <autoAsm> <noiseAdaptiveSensitivity>`, the 0x17 variant without the last two bytes, the 0x22 variant `<valueChange> <totalEffect> <ambientMode> <ambLevel>`.
- GET `66 <type>`, RET `67`, SET `68`, NTFY `69`.
- Mapping: Off = totalEffect OFF; NC = ON + mode NC; Ambient = ON + mode ASM; focus on voice = ambientMode VOICE; level 1..20; auto ambient = `autoAsm` NcAsmOnOff (OFF 0, ON 1 - note: NOT inverted); `noiseAdaptiveSensitivity` STANDARD/HIGH/LOW if present.
- Slider drag: send `valueChange` UNDER_CHANGING while dragging and CHANGED on release; the core rate-limits drag updates (recommend max ~10/s) and always sends the final CHANGED.
- Always write the full tuple (read-modify-write from current state) so unrelated fields are not reset.
- Optional: NC/AMB button function (`66 30`/`68 30`, function 0x69) only as read-only display; no editing in the MVP.
- Dev CLI: `--get nc`, `--set nc <off|nc|ambient> [--level N] [--voice on|off] [--auto on|off]`.
- Allowlist rows `66 17|19|22` (GET), `68 17|19|22` (SET). `64` stays denied.

## Out of scope
Functions `AUTO_NCASM` 0x70 / `ADAPTIVE_CONTROL` 0x71 (the reference has no code for them: unknown semantics), NC test mode, ambient capability steps `60 19` (use the fixed 1..20 range), NC/AMB button editing.

## XM6 status
NC/ambient: **supported** ("new auto ambient level"). Variant `0x19`: **inferred**, confirm from the 0003 capture. Auto ambient and adaptive sensitivity: **gated** (present only in the 0x19 layout). Separate AUTO/ADAPTIVE functions: **unknown**.

## Acceptance
(a) Unit tests: parse/encode each of the three layouts; mode mapping both directions; level bounds 1..20 enforced (0 and 21 rejected); read-modify-write keeps other fields; NTFY `69` updates state; the combined 0x6B+0x6D case picks the type the capture proves (test the decision once known); denied `64`/test-mode commands still rejected; drag rate limiter emits final CHANGED.
(b) Owner: set NC / Ambient / Off from the CLI and say what the headphones do (voice prompt, audible change); level 1 vs 20 clearly different; focus-on-voice audibly different; auto ambient and adaptive toggles only if the capture lists them; press the NC/AMB button on the headset and see the NTFY in `--watch`.

## Open owner decisions
- Whether to expose "auto ambient" and "adaptive sensitivity" if present: recommend yes, behind the gating.
- Whether the capture shows the 0x6B/0x6D conflict: decide after 0003.
