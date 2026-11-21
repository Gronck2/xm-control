# 0010 Auto power off, auto pause on removal, wearing status

## Goal
Read and set the auto power-off timer (including the wearing-based variant), the auto-pause-on-removal toggle, and display the wearing status (both ear cups, from notifications).

## Why
Owner's MVP list; battery-life and daily-use settings.

## Scope
- Auto power-off timer: function 0x24, capability `20 04` -> `21 04 <PodArray candidates>`, GET `26 04`, SET `28 04 <current> <lastSelect>` (lastSelect sent as 0x00). Elements: 5 min 0x00, 30 min 0x01, 60 min 0x02, 180 min 0x03, 15 min 0x04, DISABLE 0x11. Offer only the candidates the capability reply lists.
- Wearing-based power off: function 0x25, same commands with type `05` (`20 05`, `26 05`, `28 05`); extra element POWER_OFF_WHEN_REMOVED_FROM_EARS 0x10.
- Auto pause on removal: function 0xF1, GET `F6 01`, SET `F8 01 <OnOff>` (inverted polarity, ON=0x00).
- Wearing status (T2, data type 0x0E): GET `F2 00` -> `F3 00 <code>`, NTFY `F5 00 <code>`; codes NORMAL 0, ILLEGAL 1, LEFT_NOT_WEAR 2, RIGHT_NOT_WEAR 3, BOTH_NOT_WEAR 4. Queried only when T2 exists and one of T2 0xF0 / T1 0xF6 / T1 0xF1 is advertised.
- Dev CLI `--get power`, `--set poweroff <min|off|removed>`, `--set autopause on|off`, `--get wearing`.
- Allowlist: `20 04`, `26 04`, `28 04`, `20 05`, `26 05`, `28 05`, `F6 01`, `F8 01`, T2 `F2 00`. NOT allowlisted: any `24 ..` (power off now, factory power off `24 03 FF`, `24 07`), head gesture `F6/F8 0F`.

## Out of scope
Power-off-now button (even `24 03 01`), head gesture, writing wearing status (read-only).

## XM6 status
Auto power off: **supported** ("Power Off"). Wearing-based variant (0x25), auto pause (0xF1) and wearing status: **gated / unknown until capture** (wearing status is not listed in the XM6 doc).

## Acceptance
(a) Unit tests: timer element encode/decode for each value; capability candidates limit what the encoder accepts; type `04` vs `05` selection from the support list; inverted OnOff for auto pause; wearing codes parse (all five) and an unknown code is kept as unknown; NTFY `F5 00` updates state and emits one event; T2 frame uses data type 0x0E; guard rejects `24 03 01`, `24 03 FF`, `24 07`, `F8 0F ..`; wearing query skipped when none of the three functions is advertised.
(b) Owner: set 30 min, read back, and report; if wearing-based is advertised, set "when removed" and confirm after taking the headset off that it powers off (describe; remember the test powers the headset off); auto pause on, play music, take one ear cup off (or remove the headset) and say whether playback pauses; the wearing display changes as the headset is worn/removed (`--watch` shows NTFY for left/right/both).

## Open owner decisions
- Recommend hiding the controls the capture does not advertise rather than greying them out.
