//! T007 (DS-TS-GRP-005), PWR2: the power reading of this Mac, through the safe API.
#![cfg(target_os = "macos")]

use gitraptor_macsys::power;

#[test]
fn reading_returns_a_value_on_this_mac() {
    let reading = power::reading();
    let mac = reading.expect(
        "power::reading() must return Some(MacPower) on this Mac (IOKit power source + low power flag)",
    );
    // Both flags are plain booleans; reading twice must agree on a stable source.
    let again = power::reading().expect("a second reading must also return a value");
    assert_eq!(
        (mac.on_battery, mac.low_power),
        (again.on_battery, again.low_power),
        "two readings in a row must agree"
    );
}
