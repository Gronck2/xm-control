# 0004 Device state, notification path and dev CLI

## Goal
UI-independent core: a worker thread that owns the session, a `DeviceState` snapshot, a typed `Command` channel in and a typed `Event` channel out, with unsolicited NTFY frames routed into state updates. Plus a minimal dev CLI (`--get <feature>`, `--set <feature> <value>`) so every later feature can be verified on the real headset before any UI exists.

## Why
Headset buttons and other devices change settings at any time; the UI must show the truth, not its own last write. A single place for this keeps feature tasks small and the UI decision from blocking protocol work.

## Scope
- `DeviceHandle`: `connect(mac)`, `send(Command)`, `events() -> Receiver<Event>`, plus an optional wake callback for UIs (e.g. to post a window message).
- `DeviceState`: `Option<T>` per feature (None = not advertised/unknown), `Event::{Connected(info), Disconnected(reason), StateChanged(feature), Error}`.
- Feature plug-in shape: each feature module registers (a) its GETs for initial sync gated on the support list, (b) a parser for RET and NTFY ids, (c) its SET encoder. This task defines the trait/table and one dummy feature for tests.
- Initial sync after handshake and a safe periodic re-sync only where the reference does it (battery); otherwise rely on NTFY.
- Dev CLI skeleton in `main.rs`: `--get`/`--set` dispatch to feature modules (features register their names in their tasks).
- Disconnect detection and a single reconnect attempt policy (recommend: none in MVP, UI shows "disconnected" + Connect button).

## Out of scope
Concrete features (0005-0011), UI, persistence of settings.

## XM6 status
Generic. NTFY IDs follow the same layout as RET with the NTFY id: **supported** (protocol.md §4).

## Acceptance
(a) Unit tests, no hardware (mock transport):
- Scripted unsolicited NTFY for the dummy feature updates the state and emits exactly one `StateChanged`.
- Command sent from a second thread is serialized with the receive loop (no interleaved frames); ACK for incoming NTFY is sent even while a command waits.
- Features not advertised in the support list stay `None` and their commands return `NotSupported` without touching the transport.
- Transport error produces `Disconnected` and the worker thread exits; handle drop closes the thread.
- Commands rejected by the guard come back as `Error`, nothing sent.
(b) Owner: `--get` on the dummy/handshake data prints model and firmware; pressing a headset button during a `--watch` run (log of events) shows the NTFY arriving (needs a feature from 0005+, so this item is finally checked with 0005).

## Open owner decisions
- Channels (`std::sync::mpsc`) vs a callback: recommend `mpsc` + optional wake callback, works with both UI options.
- ADR candidate: threading/event model (flag for main thread).
