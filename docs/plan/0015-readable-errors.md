# Plan 0015 Readable error messages

## Approach
Hand-written `impl std::fmt::Display` on the existing error enums (no `thiserror`, no new dependency), then switch the two places that print them with Debug. `CommandError` messages delegate to the inner error's Display for `Guard` and `Frame`. No behaviour change beyond text.

## Branch
Stacked on `task/0014-ui-system-panels` (the UI model with `Event::Error` handling lives there); branch `task/0015-readable-errors`, PR targeted at `task/0014-ui-system-panels`, retargeted to main before 0014 is squash-merged (see Process notes in `docs/plan/README.md`).

## Steps
1. Inspect `GuardError` and `FrameError` variants and write Display for both; refine `SessionError` Display; write Display for `CommandError`.
2. Unit tests next to each enum: a list of one sample per variant, assert `!msg.is_empty()` and `msg != format!("{e:?}")`.
3. `src/ui/model.rs`: `format!("{e}")` for `Event::Error`; add a model test that an Error event sets the notice to the Display text.
4. `src/main.rs` (`println!("{e:?}")` for errors) and `src/probe.rs` (guard refusal line): use Display.
5. Run fmt, clippy, tests.

## Files likely touched
`src/device/mod.rs`, `src/mdr/guard.rs`, `src/mdr/frame.rs`, `src/mdr/session.rs`, `src/ui/model.rs`, `src/main.rs`, `src/probe.rs`. `src/main.rs` is security-shaped: the diff must only change the format specifier, so the pipeline's review looks at it.

## Tradeoffs
Display per enum vs one central `describe(&CommandError)` in the UI: Display serves the CLI too and is idiomatic. `thiserror` would save lines but adds a dependency for a handful of messages. Messages stay English-only (no localization in the MVP).
