//! What a feature module (battery, NC/ASM, EQ, ...) provides to the device worker; see ADR 0004.
//! Features are listed in a static slice (`FEATURES` in `mod.rs`), so adding one is a new file plus one line there.

use super::handshake::DeviceInfo;
use super::state::DeviceState;
use crate::mdr::guard::Table;

/// A read-only request for the initial sync. The reply is matched by `reply_id` (and the command's second byte, see `Session::request`);
/// a reply that does not match, or none at all, is not an error (docs/arch/protocol.md section 8.3).
pub struct Get {
    pub table: Table,
    pub payload: Vec<u8>,
    pub reply_id: u8,
}

pub trait Feature: Sync {
    /// The name used by the dev CLI and `Command::{Get, Set}`.
    fn name(&self) -> &'static str;

    /// Support-function ids (table, id) that must ALL be in the headset's lists; otherwise the feature is not used and its state stays `None`.
    fn required(&self) -> &'static [(Table, u8)];

    /// GETs for the initial sync (only asked when the feature is supported).
    fn initial_gets(&self, info: &DeviceInfo) -> Vec<Get>;

    /// Parses an incoming RET or NTFY frame payload; `Changed` makes the worker emit exactly one `StateChanged`. `NotMine` is for ids this feature
    /// does not know, so the worker can report a frame nobody understood.
    fn on_frame(&self, state: &mut DeviceState, table: Table, payload: &[u8]) -> Parsed;

    /// Whether the worker should re-run `initial_gets` now (asked once per refresh interval). Default: never.
    fn wants_refresh(&self, _: &DeviceState) -> bool {
        false
    }

    /// GETs to run right after a successful SET (a re-read of what the headset accepted). Default: none.
    fn after_set(&self) -> Vec<Get> {
        Vec::new()
    }

    /// Turns a user value (CLI text) into the SET payload, starting from the current `state` for fields the user did not name.
    /// It still goes through the guard in `Mode::Normal` before it is sent.
    fn encode_set(&self, state: &DeviceState, value: &str) -> Result<(Table, Vec<u8>), String>;
}

pub fn supported(f: &dyn Feature, info: &DeviceInfo) -> bool {
    f.required().iter().all(|&(t, id)| match t {
        Table::T1 => info.has_t1(id),
        Table::T2 => info.has_t2(id),
    })
}

#[derive(Debug, PartialEq, Eq)]
#[allow(dead_code)] // constructed by the feature tasks
pub enum Parsed {
    NotMine,
    Unchanged,
    Changed,
}
