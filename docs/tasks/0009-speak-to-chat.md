# 0009 Speak-to-Chat

## Goal
Read and set Speak-to-Chat: on/off, detect sensitivity, and the "mode out" timeout.

## Why
Owner's MVP list; advertised on XM6 (T1 function 0xFC SMART_TALKING_MODE_TYPE2).

## Scope
- Gate on T1 function 0xFC.
- Enable: GET `F6 0C` -> `F7 0C <OnOff> <preview>`; SET `F8 0C <OnOff> <preview>` with **inverted** polarity ON = 0x00, OFF = 0x01; preview is always sent as OFF (0x01); NTFY `F9 0C <OnOff> <preview>` (reference).
- Sensitivity/timeout: GET `FA 0C` -> `FB 0C <sensitivity> <timeout>`; SET `FC 0C <sensitivity> <timeout>`; NTFY `FD 0C ...` (reference). Sensitivity AUTO 0, HIGH 1, LOW 2; timeout FAST 0, MID 1, SLOW 2, NONE 3 (manual off).
- Fixture decode: `F7 0C 01 01` = Speak-to-Chat **off**, preview off; `FB 0C 00 01` = sensitivity AUTO, timeout MID.
- Read-modify-write between the two SETs: changing sensitivity must not change enable and vice versa.
- Dev CLI `--get stc`, `--set stc on|off [--sens auto|high|low] [--timeout fast|mid|slow|none]`.
- Allowlist: GET `F6 0C`, `FA 0C`; SET `F8 0C <00|01> 01` and `FC 0C <00..02> <00..03>` (validated bytes, exact length). `F8 09` stays denied (RESET_SETTINGS / FACTORY_RESET).

## Out of scope
Preview mode, voice assistant (`F*04`), head gesture, general settings.

## XM6 status
**Confirmed** advertised and readable on firmware 3.1.5 (replies above). The polarity decode (`01` = off) follows the reference and is **proven only by the owner's cross-check** (was Speak-to-Chat off at capture time?). SET layouts and NTFY ids are reference-derived.

## Hardware
**Sends SET commands** (`F8 0C`, `FC 0C`). The PR merges only after the owner confirms the behaviour on the headphones (hardware batch 2).

## Acceptance
(a) Unit tests, with reply bytes from `tests/fixtures/xm6-probe.txt` (`F7 0C 01 01`, `FB 0C 00 01`): inverted OnOff encode/decode; sensitivity and timeout values, out-of-range rejected; round trip; preview always 0x01; guard accepts the SET and GET rows and rejects `F8 09 01`, `F8 09 00`, `F8 0F ..`, `F8 08 ..`, `F8 0C` with a wrong length or a value above 1, `FC 0C 03 00`; NTFY updates; not advertised gives None.
(b) Owner: turn on, then talk: headphones pause music / let voice through (describe); turn off: no more triggering; change sensitivity and timeout and report differences; toggle on the headset's own control (if it has one) and see NTFY in `--watch`. Confirm that **on = headset really on**, to catch polarity errors immediately. Restore the previous state afterwards (decoded as off, sensitivity AUTO, timeout MID).

## Open owner decisions
None. (Polarity check is part of the owner verification.)
