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
    /// DSEE: true = AUTO, false = OFF.
    pub dsee: Option<bool>,
    /// A DSEE status frame said it is unavailable; SET is not sent then.
    pub dsee_disabled: bool,
    pub stc: super::stc::Stc,
    pub power: super::power::Power,
    /// Auto pause on removal: true = ON.
    pub autopause: Option<bool>,
    pub wearing: Option<super::wearing::Wearing>,
    /// Payload (after the two id bytes) of the last frame the test feature accepted.
    #[cfg(test)]
    pub dummy: Option<Vec<u8>>,
}
