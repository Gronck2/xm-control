# 0011 Multipoint toggle and paired/connected device list

## Goal
Show the headset's paired devices with their connected flag (read-only) and a multipoint on/off toggle.

## Why
Owner's MVP list: see what the headset is connected to and control multipoint from the PC.

## Scope (T2, data type 0x0E)
- Device list: the capture settles the variant. T2 0x33 (and 0x31) are advertised; 0x32 and 0x30 are not, so the list type is `0x02` (WITH_BT_CLASS_OF_DEVICE) only. GET `36 02` -> `37 02 <count> <entries> <playbackRightDevice u8>`; entry = 17 ASCII MAC, connected u8, class of device Int24 BE, length-prefixed name; NTFY `39 02 ...` same layout (reference). The type `0x00` variant (no class of device) is **dropped**.
- Fixture decode (`37 02 07 ...`): 7 entries; the first is connected (`01`, class `2A 41 0C`), the other six are not (`00`, class `FF FF FF`); trailing byte `01`. Display only the per-entry connected flag and the name: the meaning of the trailing byte is unproven (the reference compares it with each entry's connected byte), so the "playing device" marker is out.
- MAC validation: exactly 17 bytes of printable ASCII, **not** hex-checked, because the anonymised fixture contains the literal text `XX:XX:XX:XX:XX:XX`. Names are decoded lossily (non-UTF-8 does not fail the parse).
- Multipoint (source switch control, T2 function 0x31): GET `36 01` -> `37 01 <u8>`, SET `38 01 <0|1>`, NTFY `39 01 <value> <result>` (reference). Raw u8, **1 = enabled, not the inverted OnOff** (the reference treats any non-zero as enabled). Fixture: `37 01 01` = enabled.
- Pairing-mode status read `32 02` -> `33 02 <mode> <EnableDisable>` is optional and display-only. Fixture: `33 02 00 00` = normal mode (not pairing).
- Re-read the list on NTFY and after toggling multipoint.
- Dev CLI `--get devices`, `--set multipoint on|off --yes` (the flag stands in for the UI confirmation dialog).
- Allowlist: T2 `36 01`, `36 02`, `32 02` (GET; `36 00` is not used on XM6), `38 01 <00|01>` (SET). **Guard change:** widen the T2 `3C` denial to **every** form (`(Table::T2, 0x3C) => true`, replacing the current "third byte is 02" check), so connect, disconnect, source switch and unpair can never be sent, whatever the argument bytes or length. `34 ..` (pairing mode) stays denied.

## Out of scope
Unpair (never), pairing mode, connect/disconnect/switch source, renaming, the playing-device marker, the type `0x00` list variant.

## XM6 status
**Confirmed** by the capture: list type `0x02` and its layout (7 entries), multipoint read `37 01 01`, pairing-mode status `33 02 00 00`. **Unproven:** multipoint polarity (`01` = on; the SET and NTFY layouts are reference-only), the trailing list byte, and whether toggling multipoint disconnects a device (owner reports).

## Hardware
- Device list, pairing-mode status: read-only.
- Multipoint toggle: **sends a SET command** (`38 01`). The PR merges only after the owner confirms the behaviour on the headphones (hardware batch 3).

## Acceptance
(a) Unit tests, with reply bytes from `tests/fixtures/xm6-probe.txt` (`37 02 ...` with 7 entries, `37 01 01`, `33 02 00 00`): parse the fixture list (7 entries, one connected, names, class values, MAC accepted although not hex); 0, 1 and several entries on hand-built frames; a MAC field of the wrong length or an entry that runs past the frame is an error; multipoint encode 1/0 and read back raw (test that it does not reuse `OnOff`); T2 data type used; gating from the real list (0x33 present, 0x32/0x30 absent, so only list type `0x02`); not advertised gives None.
Guard tests: every `3C` payload is rejected as `Denied`: `3C` alone, `3C 00`, `3C 01 <mac>`, `3C 00 00 <mac>`, `3C 00 01 <mac>`, `3C 00 02 <mac>`, `3C 02 02 <mac>`, `3C 01 01`, and a loop over the second and third byte; `34 02 01 00` and `34 00 00 00` rejected; `38 01 02` and `38 01` rejected; `38 01 00` / `38 01 01` accepted in normal mode only.
(b) Owner: list matches the devices actually paired (names, connected flags) incl. this PC; toggle multipoint off and see whether the phone or another source drops (report what happened, including any alert the headset plays); toggle back on and check both reconnect; during `--watch`, connecting/disconnecting a device triggers an update. Restore multipoint to on (the capture state).

## Open owner decisions
- Recommend that toggling multipoint asks for confirmation in the UI (it can drop a connection, possibly the PC's own).
- The MAC of the PC itself may appear in the list; recommend marking it by comparing with the connected adapter's address, display only.
