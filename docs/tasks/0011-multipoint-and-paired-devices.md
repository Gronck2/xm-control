# 0011 Multipoint toggle and paired/connected device list

## Goal
Show the headset's paired devices with their connected flag (read-only) and a multipoint on/off toggle.

## Why
Owner's MVP list: see what the headset is connected to and control multipoint from the PC.

## Scope (T2, data type 0x0E)
- Device list: type 0x02 (`WITH_BT_CLASS_OF_DEVICE`) if T2 0x32/0x33 advertised, else type 0x00 if 0x30 advertised. GET `36 02` -> `37 02 <MDRArray{mac 17 ASCII, connected u8, CoD Int24BE, pstr name}> <playbackRightDevice u8>`; type 0x00 variant has no CoD.
- Multipoint (source switch control, T2 function 0x31): GET `36 01`, SET `38 01 <0|1>`. Raw u8, **1 = enabled, not the inverted OnOff**.
- Pairing-mode status read `32 02` is optional and display-only.
- Re-read the list on NTFY and after toggling multipoint.
- Dev CLI `--get devices`, `--set multipoint on|off`.
- Allowlist: T2 `36 00|01|02`, `32 02` (GET), `38 01` (SET). **Denied and tested:** `3C ..` entirely (connect/disconnect/switch/unpair, including `3C 00 02` and `3C 02 02`), `34 ..` (pairing mode).

## Out of scope
Unpair (never), pairing mode, connect/disconnect/switch source, renaming, the playing-device marker beyond a read-only flag.

## XM6 status
**Supported** ("Multipoint Control"). Which list type the XM6 uses: **gated**, confirm from the capture. Whether toggling multipoint disconnects a device: **unknown**, owner reports.

## Acceptance
(a) Unit tests: parse list type 0x02 and 0x00 (0 devices, 1, several; MAC string 17 bytes; non-ASCII/invalid MAC length is an error); multipoint encode 1/0 and read back raw; T2 data type used; guard rejects every `3C` and `34` form listed; not advertised gives None.
(b) Owner: list matches the devices actually paired (names, connected flags) incl. this PC; toggle multipoint off and see whether the phone or another source drops (report what happened); toggle back on and check both reconnect; during `--watch`, connecting/disconnecting a device triggers an update.

## Open owner decisions
- Recommend that toggling multipoint asks for confirmation in the UI (it can drop a connection, possibly the PC's own).
- The MAC of the PC itself may appear in the list; recommend marking it by comparing with the connected adapter's address, display only.
