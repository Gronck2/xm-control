# 0009 Speak-to-Chat

## Goal
Read and set Speak-to-Chat: on/off, detect sensitivity, and the "mode out" timeout.

## Why
Owner's MVP list; fully supported on XM6.

## Scope
- Function `SMART_TALKING_MODE_TYPE2` 0xFC (T1 SYSTEM).
- Enable: GET `F6 0C`, SET `F8 0C <OnOff> <preview>` with **inverted** polarity ON = 0x00, OFF = 0x01; preview is always sent as OFF (0x01).
- Sensitivity/timeout: GET `FA 0C`, SET `FC 0C <sensitivity> <timeout>`; sensitivity AUTO 0, HIGH 1, LOW 2; timeout FAST 0, MID 1, SLOW 2, NONE 3 (manual off).
- RET/NTFY ids follow the SYSTEM table (check in `T1`; confirm in the capture); NTFY updates state.
- Read-modify-write between the two SETs: changing sensitivity must not change enable and vice versa.
- Dev CLI `--get stc`, `--set stc on|off [--sens auto|high|low] [--timeout fast|mid|slow|none]`.
- Allowlist: `F6 0C`, `FA 0C` (GET), `F8 0C`, `FC 0C` (SET) only; `F8 09` stays denied (RESET_SETTINGS).

## Out of scope
Preview mode, voice assistant (`F*04`), head gesture, general settings.

## XM6 status
**Supported** (documented). Whether sensitivity and timeout are changeable on XM6: gated on the same function, confirm on hardware.

## Acceptance
(a) Unit tests: inverted OnOff encode/decode; sensitivity and timeout values; round trip; preview always 0x01; guard accepts the four rows and rejects `F8 09 01`, `F8 09 00`; NTFY updates; not advertised gives None.
(b) Owner: turn on, then talk: headphones pause music / let voice through (describe); turn off: no more triggering; change sensitivity and timeout and report differences; toggle on the headset's own control (if it has one) and see NTFY in `--watch`. Confirm that **on = headset really on**, to catch polarity errors immediately.

## Open owner decisions
None. (Polarity check is part of the owner verification.)
