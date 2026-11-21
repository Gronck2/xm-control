# 0004. Device worker thread with channels in and out

Status: accepted (2026-11-21)

## Context

The headset changes settings on its own (buttons, other devices), so the UI must show what the headset reports, not its own last write.
The MDR session (`src/mdr/session.rs`) is blocking and stop-and-wait, so exactly one thread must use it. The UI stack is Win32 (ADR 0002),
but the dev CLI and tests need the same core without any window.

## Decision

- One worker thread per connection (`src/device/mod.rs`) owns the `Session`. It connects, runs the handshake, does the initial sync, then loops:
  run the next queued command, else wait up to 50 ms for incoming frames. Incoming frames are ACKed by the session and then parsed into `DeviceState`.
- `std::sync::mpsc` channels, no async runtime: `Command` in (`DeviceHandle::send`, callable from any thread), `Event` out
  (`Connected`, `Synced`, `StateChanged` with a state snapshot, `Unhandled`, `Error`, `Disconnected`). An optional wake callback runs after each
  event so a Win32 UI can post a window message and drain the receiver on its own thread.
- The transport is opened inside the worker (a closure), so connecting never blocks the caller and the transport need not be `Send`.
- Features implement `trait Feature` and are listed in one static slice: support-list gate, initial GETs, RET/NTFY parser, SET encoder. Replies and
  notifications take the same path, are matched by command id (not by echo), and a well-formed frame nobody knows is an `Unhandled` event, never an error.
- A GET that is ACKed but never answered ends quietly after the 5 s reply wait (`SessionError::NoReply`) and leaves the feature `None`.
  Only a lost link or a missing ACK ends the session.
- Every SET goes feature encoder, then the guard in `Mode::Normal` (ADR 0003), then the session; a refusal is an `Error` event and nothing is sent.
- No automatic reconnect: after `Disconnected` the worker has exited and the UI shows "disconnected" with a Connect button that makes a new handle.
  Dropping the handle ends the worker.

## Consequences

- A command waits up to 50 ms for the loop to look at the channel; fine for UI use.
- A long initial sync (several silent GETs at 5 s each) delays commands queued meanwhile; they run right after `Synced`.
- The state snapshot in each event is a copy, cheap for this size of state.
- A feature task is one new file, one `DeviceState` field and one line in `FEATURES`, plus guard rows for its commands.
