# 0015 Readable error messages in the UI and the CLI

## Goal
Show worker errors to the user as short plain-English sentences instead of Rust Debug text (`Session(Timeout)`), in the window status line and in the dev CLI.

## Why
`ui/model.rs` renders `Event::Error(e)` with `format!("{e:?}")`, and the CLI prints Debug for the same errors. `CommandError` and `GuardError` have no `Display`; `SessionError` has one, but it is terse ("timed out" for both `Timeout` and `NoReply`) and prints its `FrameError` with Debug. The user cannot tell what happened.

## Blocked by
0014 (the UI code lives on branch `task/0014-ui-system-panels`). Stacked on that branch.

## Scope
- `impl Display` for `CommandError` (`src/device/mod.rs`), `GuardError` (`src/mdr/guard.rs`), `FrameError` (`src/mdr/frame.rs`), and refine the existing `SessionError` Display (`src/mdr/session.rs`) so `Timeout` ("the headset did not answer") and `NoReply` ("the headset acknowledged but did not reply") differ and `Frame` uses `FrameError`'s Display. Also `impl std::error::Error` only where it is free (no new dependency).
- Example messages: `UnknownFeature` "unknown feature: <name>", `NotSupported` "this headset does not support <name>", `BadValue` "invalid value: <text>", `Guard` "the command was refused locally: not allowed".
- UI: `Event::Error(e)` in `src/ui/model.rs` uses `{e}`.
- CLI: use `{e}` where these errors are printed with `{e:?}` (`src/main.rs`, `src/probe.rs` guard-refusal line). Keep Debug for `Event::Unhandled` and other non-error dumps.

## Out of scope
Protocol changes, new commands, any SET, localization, error codes, changing which errors are raised, `Event::Disconnected(String)` (already text).

## XM6 status
Not applicable: no device interaction.

## Acceptance
(a) Unit tests: for every variant of `CommandError`, `GuardError`, `FrameError` and `SessionError` the Display text is non-empty and differs from the Debug text. A UI model test: feeding `Event::Error(..)` makes the status/notice equal the Display text. fmt, clippy, tests green.
(b) Owner: none required (no device commands). Optional: trigger an error in the window (for example a command while disconnected) and confirm the text reads sensibly.

## Open owner decisions
None.
