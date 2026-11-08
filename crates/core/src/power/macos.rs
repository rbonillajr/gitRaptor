//! The power source of macOS: IOKit's providing power source and the low power mode, read
//! through `gitraptor_macsys::power` (DS-TS-GRP-005 T007). XP-45: macOS only on purpose.

use super::{PowerReading, PowerSource, reading_from};

/// Reads this Mac on every call; an IOKit failure reads `Unknown`.
#[derive(Debug, Clone, Copy, Default)]
pub struct MacPowerSource;

impl PowerSource for MacPowerSource {
    fn read(&self) -> PowerReading {
        reading_from(gitraptor_macsys::power::reading().map(|p| (p.on_battery, p.low_power)))
    }
}

#[cfg(test)]
mod tests {
    use super::super::{PowerReading, reading_from};

    #[test]
    fn low_power_wins_over_ac() {
        assert_eq!(reading_from(Some((false, true))), PowerReading::LowPower);
        assert_eq!(reading_from(Some((true, true))), PowerReading::LowPower);
        assert_eq!(reading_from(Some((false, false))), PowerReading::Ac);
        assert_eq!(reading_from(Some((true, false))), PowerReading::Battery);
        assert_eq!(reading_from(None), PowerReading::Unknown);
    }
}
