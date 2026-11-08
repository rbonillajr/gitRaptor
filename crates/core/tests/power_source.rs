//! T007 (DS-TS-GRP-005), PWR1: how the engine translates the macOS power reading.
//!
//! Expects `gitraptor_core::power::reading_from(Option<(bool, bool)>) -> PowerReading`, a pure
//! function: the pair is `(on_battery, low_power)` as read by `gitraptor_macsys::power::reading()`,
//! and `None` is a failed reading.
#![cfg(target_os = "macos")]

use gitraptor_core::power::macos::MacPowerSource;
use gitraptor_core::power::{PowerReading, PowerSource, platform_source, reading_from};

#[test]
fn low_power_wins_over_ac() {
    assert_eq!(
        reading_from(Some((false, true))),
        PowerReading::LowPower,
        "low power mode on AC must read LowPower (D6)"
    );
}

#[test]
fn low_power_wins_over_battery() {
    assert_eq!(
        reading_from(Some((true, true))),
        PowerReading::LowPower,
        "low power mode on battery must read LowPower, not Battery"
    );
}

#[test]
fn battery_without_low_power_reads_battery() {
    assert_eq!(reading_from(Some((true, false))), PowerReading::Battery);
}

#[test]
fn no_battery_and_no_low_power_reads_ac() {
    assert_eq!(
        reading_from(Some((false, false))),
        PowerReading::Ac,
        "a Mac without battery (or on AC) reads Ac"
    );
}

#[test]
fn a_failed_reading_is_unknown() {
    assert_eq!(
        reading_from(None),
        PowerReading::Unknown,
        "an IOKit error must read Unknown"
    );
}

#[test]
fn the_mac_source_reads_a_real_value() {
    assert_ne!(
        MacPowerSource.read(),
        PowerReading::Unknown,
        "MacPowerSource must read this Mac, not Unknown"
    );
}

#[test]
fn platform_source_on_macos_is_no_longer_unknown() {
    assert_ne!(
        platform_source().read(),
        PowerReading::Unknown,
        "platform_source() on macOS must be the MacPowerSource"
    );
}
