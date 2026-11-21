# 0002 RFCOMM transport and MDR session

## Goal
Connect to a paired XM6 over Bluetooth Classic RFCOMM and exchange MDR commands reliably (seq, ACK, retry), with a transport trait so the session is testable without hardware.

## Why
No feature can work without a reliable link; the protocol has stop-and-wait ACKs with a 0/1 sequence that must be exactly right.

## Scope
- Device discovery: list paired Bluetooth devices (name, MAC, connected flag) via Win32 (`BluetoothFindFirstRadio/Device`), so the user (or `--probe --device <mac>`) can choose.
- Winsock `AF_BTH` RFCOMM socket, V2 UUID `956C7B26-D49A-4BA8-B03F-B17D393CB6E2`, channel resolved by SDP (port 0), authenticate + encrypt options; connect with timeout, clean close.
- `trait Transport { read, write, close }` with the Winsock implementation and an in-memory mock.
- Session: send one command at a time, wait for ACK 1000 ms, up to 10 tries reusing the same seq, flip seq on matching ACK, ignore stale ACK; reply to every received T1/T2 frame with ACK (seq = 1 - received);
  response wait 5000 ms; unsolicited frames delivered to a callback/queue (NTFY path for 0004).
- Only `AllowedCommand` from 0001 is sendable.
- Auto-detect the V1 UUID is not needed; V2 only (XM6).

## Out of scope
Handshake commands (0003), BLE, reconnect-on-loss policy beyond reporting the disconnect, UI.

## XM6 status
V2 UUID for XM5 and newer: **supported** (source comment). Whether Windows resolves the channel without a manual port on the owner's PC: unknown until tried.

## Acceptance
(a) Unit tests with the mock transport:
- Send waits for ACK; on missing ACK retries 10 times with the same seq then returns a timeout error (use a shortened timeout in tests).
- Seq flips only on a matching ACK; stale ACK ignored; two consecutive commands use seq 0 then 1.
- Received T1 frame with seq 0 produces ACK `3E 01 01 00 00 00 00 02 3C`; seq 1 produces `... 00 ... 01 3C` (protocol.md §2).
- Unsolicited frame while waiting for an ACK is both ACKed and queued, not lost.
- Fragmented/coalesced reads; transport close mid-command gives an error, no panic.
- UUID string parses to the expected GUID bytes.
(b) Owner on the real XM6 (a tiny `--connect-test` mode that connects, prints "connected" and closes, no commands sent):
- Headset paired and on: connects; prints connected and disconnects cleanly; the headset shows no odd behaviour (voice prompt, reboot).
- Headset off or out of range: a clear error within the timeout, no hang.
- Headset already connected to the phone (multipoint): still connects (report if not).

## Open owner decisions
- Crate for Win32: recommend `windows-sys` (small, raw, fits the no-framework style) over `windows`. Needs the owner's OK for the dependency.
- Recommend a dedicated worker thread owning the session (blocking I/O + timeouts) over async; no runtime dependency.
