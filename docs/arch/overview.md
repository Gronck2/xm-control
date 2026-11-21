# Architecture overview

Current shape: `src/main.rs` opens the window when started without arguments, otherwise runs `--connect-test`, `--probe` or the device dev CLI (`--get`, `--set`, `--watch`). There is a hardware-free protocol layer in `src/mdr/`, a Windows Bluetooth transport in `src/bt/`, the device worker in `src/device/` and the window UI in `src/ui/`.

Layers built so far:
- `src/mdr/frame.rs`: MDR frame pack/unpack (escaping, checksum, 2048-byte limit) and a streaming `FrameParser` that drops bad frames and resyncs on `3E`.
- `src/mdr/guard.rs`: the command guard. `check(table, payload, mode)` is the only producer of `AllowedCommand`, and `frame::pack_command` accepts only that type
  (denylist first, then allowlist, unknown rejected; see ADR 0003). Incoming frames are not filtered.

- `src/mdr/session.rs`: `trait Transport` (read with timeout / write / close) and `Session`: stop-and-wait send with ACK wait 1000 ms, up to 10 tries with the same seq,
  ACK for every received T1/T2 frame, unsolicited frames queued (`poll_notifications`), `request` waits 5000 ms for a reply. Blocking; one worker thread is meant to own it.
- `src/bt/`: `enumerate.rs` lists paired devices (Win32 `BluetoothFindFirstRadio/Device`); `winsock.rs` is the `AF_BTH` RFCOMM `Transport` (V2 UUID, port 0 via SDP,
  authenticate + encrypt, connect with timeout); `windows-sys` is the only Win32 dependency (ADR 0002).
- `xm-control --connect-test [--device <mac>]`: connects, prints, closes; sends no MDR command. Without `--device` it picks the single paired `WH-1000XM*`.
- `src/device/handshake.rs`: the V2 init handshake (`00 00`, `02 00`, `04 02`, `04 01`, `04 03`, `06 00`, T2 `06 00` only if table 2 is enabled) into `DeviceInfo`; read-only guard mode; fails cleanly if table 1 is unsupported.
- `src/probe.rs`: `xm-control --probe <out> [--device <mac>] [--keep-mac]`: handshake, then read-only GETs of the MVP features that the support lists advertise, 5 s listening for unsolicited frames,
  and a plain-text dump (device, support lists, feature reads, hex frame log). The MAC and any MAC-shaped text in frames are hidden unless `--keep-mac`. The file is written even if the run fails part-way.
- `src/device/{mod,state,feature}.rs`: the device worker (ADR 0004). `DeviceHandle::connect(mac, wake)` starts one thread that owns the `Session`: handshake, initial sync of the
  supported features, then a loop over `Command`s in and incoming RET/NTFY frames. `Event`s (`Connected`, `Synced`, `StateChanged` + snapshot, `Unhandled`, `Error`, `Disconnected`) go out over
  `mpsc`. `trait Feature` (support-list gate, initial GETs, frame parser, SET encoder) is registered in the static `FEATURES` slice (battery registered; the rest come with tasks 0006-0011). No auto-reconnect.
- Dev CLI: `--get [feature]` (device info and state after the initial sync), `--set <feature> <value>`, `--watch [secs]` (event log); all take `--device <mac>`. Sends nothing but the read-only sync until a feature exists.

Layers, top to bottom: UI (`src/ui`) / dev CLI -> `device` worker (state, events) -> `mdr::session` (ACK, seq, retry) -> `mdr::guard` + `mdr::frame` -> `bt` transport.

- `src/ui/`: the window (task 0012, ADR 0002). `model.rs` is plain Rust: `Model::apply(Event)` and `Model::view() -> ViewState` (status text, button label, enabled flags, model/firmware, masked MAC, battery row hidden when the headset has none); unit-tested.
  `view.rs` is the Win32 layer: one fixed-size, per-monitor-v2 DPI-aware window of native controls (device combo from `bt::enumerate`, Refresh, Connect/Disconnect, status line, a "Device" info group, empty "Sound" and "Controls"
  group boxes reserved for tasks 0013/0014). Connect calls `DeviceHandle::connect` (the connect itself runs in the worker thread); the wake callback does `PostMessageW(WM_APP + 1)` and the window drains the event channel.
  Disconnect drops the handle and waits for the worker's `Disconnected`; closing the window does the same with a 1.5 s cap. The UI sends no SET commands yet; no tray, no auto-reconnect; the MAC is masked until "Show".
- Build: `build.rs` embeds `app.manifest` (comctl32 v6, PerMonitorV2) with no build dependency: MSVC via `/MANIFEST:EMBED`, GNU via `windres` (the object is named `default-manifest.o` and put on a `-B` path so it replaces gcc's own default manifest).
  The exe uses the console subsystem so shells wait for it and `$LASTEXITCODE` is right. With no arguments (GUI) it calls `FreeConsole()` when it is the only process on its console (started from Explorer; a brief console flash is accepted) and leaves a terminal's console alone.
