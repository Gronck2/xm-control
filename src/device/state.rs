//! The last known state of the headset, one `Option` per feature (`None` = not advertised, not read yet or never answered).
//! Feature tasks add their field here.

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DeviceState {
    pub battery: Option<super::battery::Battery>,
    /// A battery NTFY has been seen, so the fallback re-read stops.
    pub battery_notified: bool,
    pub nc: Option<super::ncasm::NcAsm>,
    pub eq: Option<super::eq::Eq>,
    pub eq_cap: Option<super::eq::Cap>,
    /// Band frequencies in Hz (`5B 00`).
    pub eq_freqs: Option<Vec<u16>>,
    /// An EQ status frame said the EQ is disabled; SET is not sent then.
    pub eq_disabled: bool,
    /// Payload (after the two id bytes) of the last frame the test feature accepted.
    #[cfg(test)]
    pub dummy: Option<Vec<u8>>,
}
