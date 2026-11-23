//! The window UI (ADR 0002): `model` is plain Rust (events in, display state out, tested); `view` is the Win32 layer that draws it.

pub mod model;
pub mod view;
