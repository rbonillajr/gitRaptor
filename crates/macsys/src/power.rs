//! The power state of this Mac: whether it runs on battery and whether the low power mode is on
//! (ADR-GRP-015 § 2). macOS only.

use crate::ffi_power;

/// One reading of the power state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MacPower {
    /// The providing power source is the battery (a Mac without battery reads `false`).
    pub on_battery: bool,
    /// The low power mode is on (`false` where the system does not offer it).
    pub low_power: bool,
}

/// Reads the power state; `None` when IOKit cannot tell the providing power source.
///
/// A missing low power flag (older systems) reads `false`: it never hides the battery reading.
#[must_use]
pub fn reading() -> Option<MacPower> {
    let on_battery = ffi_power::on_battery()?;
    let low_power = ffi_power::low_power().unwrap_or(false);
    Some(MacPower {
        on_battery,
        low_power,
    })
}
