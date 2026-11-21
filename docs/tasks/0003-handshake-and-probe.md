# 0003 Handshake and `--probe` diagnostic capture

## Goal
Run the V2 init handshake (protocol info, capability, firmware, model, series/color, support functions T1 and T2) and add `--probe <out>`: connect, handshake, then dump parsed info, the raw support-function lists and every raw frame (both directions) to a text file the owner can send us.

## Why
The study has no XM6 capture. The exact support-function lists decide what we may implement (NC type 0x19, battery 0x00/0x08, DSEE 0xE2, wearing status, HBS_1/HBS_2, general-settings slots). This task turns "inferred" into facts early.

## Scope
- Handshake steps 1-7 of protocol.md §3: `00 00`, `02 00`, `04 02`, `04 01`, `04 03`, `06 00`, and T2 `06 00` only if table 2 is enabled in the protocol-info reply. Fail cleanly if Table 1 is unsupported.
- Parsed `DeviceInfo { protocol_version, unique_id, firmware, model_name, series, color, t1_functions: Vec<(type, priority)>, t2_functions }` plus helpers `has_t1(fn)`/`has_t2(fn)`.
- MAC comes from OS enumeration (not MDR); info panel data = model, firmware, MAC.
- `--probe <out> [--device <mac>]`: read-only guard mode (0001), also issues the read-only GETs of every MVP feature **only if** advertised (battery `22 00`/`22 08`, NC `66 19|17|22`, EQ `50 04`/`5A 00`/`56 00`/`52 00`, DSEE `E6 01`/`E2 01`, STC `F6 0C`/`FA 0C`,
  power `20 04`/`26 04`/`20 05`/`26 05`/`F6 01`, wearing T2 `F2 00`, multipoint T2 `36 01`/`36 02`/`32 02`, safe listening T2 `5A 00`/`5A 02`, codec `12 02`, general settings `D1..D4`) so one file answers all questions. These GETs are added to the allowlist as read-only entries.
- Output file: header (tool version, date, model, firmware, MAC redacted by default, flag to keep it), parsed lists with function names where known and raw hex otherwise, then a timestamped hex log `TX/RX <hex>`.
- Listens a few seconds for unsolicited NTFY frames at the end and logs them.

## Out of scope
Any SET, any feature state model, UI.

## XM6 status
Handshake: **supported** (V2). Which functions are advertised: **unknown until this capture**.

## Acceptance
(a) Unit tests, no hardware (mock transport):
- Replies built from protocol.md layouts parse into `DeviceInfo` (protocol info size must be 8; pstr parsing; count-prefixed function lists; table 2 skipped when disabled).
- Malformed/short replies give an error, not a panic.
- Probe against a scripted mock produces the expected file sections; MAC redaction works; probe run in read-only mode cannot emit any SET (test by scanning all TX frames of a full mock run).
- Unknown function ids are kept as raw numbers in the dump.
(b) Owner on the real XM6:
- `xm-control --probe probe.txt` completes; file lists model "WH-1000XM6", a firmware version, and non-empty T1 support list.
- The headset does not change behaviour during the probe (no prompt, no setting change, no reboot) — report anything odd.
- Owner sends `probe.txt` back (we review for private data before committing anything).

## Open owner decisions
- Recommend MAC redacted by default in the file, `--keep-mac` to opt out.
- Recommend committing the reviewed capture as `tests/fixtures/xm6-probe.txt` and using it in tests from then on (needs owner OK, it is device data).
