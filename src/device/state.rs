//! The last known state of the headset, one `Option` per feature (`None` = not advertised, not read yet or never answered).
//! Feature tasks add their field here.

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DeviceState {
    pub battery: Option<super::battery::Battery>,
    /// A battery NTFY has been seen, so the fallback re-read stops.
    pub battery_notified: bool,
    /// Payload (after the two id bytes) of the last frame the test feature accepted.
    #[cfg(test)]
    pub dummy: Option<Vec<u8>>,
}
