# 0010 Auto power off (wearing-based), auto pause on removal, wearing status

## Goal
Read and set the wearing-based auto power-off option, read and set the auto-pause-on-removal toggle, and display the wearing status (both ear cups, from notifications).

## Why
Owner's MVP list; battery-life and daily-use settings.

## Scope
- **Auto power-off, wearing variant only.** T1 function 0x25 AUTO_POWER_OFF_WITH_WEARING_DETECTION is advertised; function 0x24 (the minute-timer variant) is **not**, so there is no timer, no `20 04` / `26 04` / `28 04`, and no minute options.
  - Capability `20 05` -> `21 05 <count> <candidates>`; GET `26 05` -> `27 05 <current> <lastSelect>`; SET `28 05 <current> 00` (lastSelect sent as 0x00); NTFY `29 05 ...` (reference).
  - Elements: WHEN_REMOVED_FROM_EARS 0x10, DISABLE 0x11 (the enum also has 5/30/60/180/15 min = 0x00-0x04; offer only what the capability lists).
  - Fixture decode: `21 05 02 10 11` = candidates {0x10, 0x11}; `27 05 10 00` = currently "power off when removed from ears". So the UI is a two-way choice: "when removed from ears" / "never".
- Auto pause on removal: function 0xF1 (advertised). GET `F6 01` -> `F7 01 <OnOff>`; SET `F8 01 <OnOff>` (inverted polarity, ON = 0x00); NTFY `F9 01 <OnOff>` (reference). Fixture decode: `F7 01 00` = ON.
- Wearing status (T2, data type 0x0E): GET `F2 00` -> `F3 00 <code>`, NTFY `F5 00 <code>`; codes NORMAL 0, ILLEGAL 1, LEFT_NOT_WEAR 2, RIGHT_NOT_WEAR 3, BOTH_NOT_WEAR 4. Fixture: `F3 00 00` = NORMAL. Queried only when T2 exists and one of T2 0xF0 / T1 0xF6 / T1 0xF1 is advertised: on XM6 the rule passes through 0xF1 alone (0xF0 and 0xF6 are absent).
- Dev CLI `--get power`, `--set poweroff <removed|off>`, `--set autopause on|off`, `--get wearing`.
- Allowlist: GET `20 05`, `26 05`, `F6 01`, T2 `F2 00` (all already present for `--probe`); SET `28 05 <10|11> 00` and `F8 01 <00|01>` (validated). NOT allowlisted: any `24 ..` (power off now, factory power off `24 03 FF`, `24 07`), head gesture `F6/F8 0F`, `20 04` / `26 04` / `28 04`.

## Out of scope
Power-off-now button (even `24 03 01`), head gesture (0xFF is advertised, not used), minute timers (not available on XM6), writing wearing status (read-only).

## XM6 status
**Confirmed** by the capture: only the wearing variant exists, candidate list {0x10, 0x11}, auto pause advertised and readable, wearing status answered. Polarity of `F7 01 00` (= on) follows the reference and is proven by the owner's cross-check. SET layouts and NTFY ids are reference-derived.

## Hardware
- Auto power-off and auto pause: **send SET commands** (`28 05`, `F8 01`). Merge only after the owner confirms the behaviour on the headphones (hardware batch 3).
- Wearing status: read-only (no SET).
The PR contains SETs, so it waits for owner confirmation.

## Acceptance
(a) Unit tests, with reply bytes from `tests/fixtures/xm6-probe.txt` (`21 05 02 10 11`, `27 05 10 00`, `F7 01 00`, T2 `F3 00 00`): capability candidates limit what the encoder accepts (`28 05 00 00` rejected on XM6 because 0x00 is not a candidate); no `04` path exists; inverted OnOff for auto pause; wearing codes parse (all five) and an unknown code is kept as unknown; NTFY `F5 00` updates state and emits one event; T2 frame uses data type 0x0E; guard rejects `24 03 01`, `24 03 FF`, `24 07`, `F8 0F ..`, `28 04 ..`, `28 05 01 00`; wearing query gate: passes with only T1 0xF1 (the real XM6 list), skipped when none of the three functions is advertised.
(b) Owner: set "never" then back to "when removed from ears" and read back each time (the original value is "when removed from ears"; the test with "when removed" powers the headset off if you take it off, so run that one last, and say what happened); auto pause: play music, take one ear cup off (or remove the headset) and say whether playback pauses; toggle auto pause off and on and read back; the wearing display changes as the headset is worn/removed (`--watch` shows NTFY for left/right/both). Restore the original values (when removed from ears, auto pause on).

## Open owner decisions
- Recommend hiding the controls the capture does not advertise (minute timers) rather than greying them out; with the capture this is moot for XM6, but the same rule applies to other models later.
