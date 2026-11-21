//! The last known state of the headset, one `Option` per feature (`None` = not advertised, not read yet or never answered).
//! Feature tasks add their field here.

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DeviceState {
    /// Payload (after the two id bytes) of the last frame the test feature accepted.
    #[cfg(test)]
    pub dummy: Option<Vec<u8>>,
}
