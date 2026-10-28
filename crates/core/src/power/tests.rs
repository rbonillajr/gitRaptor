//! Criterion tests of T003: the resolution table (D6) and the wake of `wait_change` (D7).

use std::time::{Duration, Instant};

use super::*;

#[test]
fn resolve_follows_the_setting_and_the_source() {
    use PowerReading::*;
    use PowerSavingSetting::*;
    // (setting, reading, saves)
    let table = [
        (Off, Ac, false),
        (Off, Battery, false),
        (Off, LowPower, false),
        (Off, Unknown, false),
        (On, Ac, true),
        (On, Battery, true),
        (On, LowPower, true),
        (On, Unknown, true),
        (Auto, Ac, false),
        (Auto, Battery, true),
        (Auto, LowPower, true),
        (Auto, Unknown, false),
    ];
    for (setting, reading, saves) in table {
        assert_eq!(
            resolve(setting, reading),
            saves,
            "resolve({setting:?}, {reading:?}) must follow the D6 table"
        );
    }
}

#[test]
fn wait_change_returns_on_a_change() {
    let (mode, writer) = PowerMode::channel(PowerSavingSetting::Auto, false);
    let seen = mode.version();
    assert!(!mode.is_saving(), "the initial mode is the one given");

    // Nothing published: the wait ends at the timeout with the same version.
    let t = Instant::now();
    assert_eq!(
        mode.wait_change(seen, Duration::from_millis(50)),
        seen,
        "without a change the version does not move"
    );
    assert!(
        t.elapsed() >= Duration::from_millis(40),
        "the wait lasts the timeout"
    );

    // A change wakes the waiter long before its timeout.
    let waiter = {
        let mode = mode.clone();
        std::thread::spawn(move || {
            let t = Instant::now();
            let v = mode.wait_change(seen, Duration::from_secs(30));
            (v, t.elapsed())
        })
    };
    std::thread::sleep(Duration::from_millis(20));
    assert!(
        writer.set(PowerSavingSetting::Auto, true),
        "a new mode is published"
    );
    let (v, waited) = waiter.join().unwrap();
    assert!(v > seen, "wait_change must return the new version");
    assert!(
        waited < Duration::from_secs(10),
        "wait_change must wake on the change"
    );
    assert!(mode.is_saving());
    assert_eq!(mode.version(), v);
    assert!(
        !writer.set(PowerSavingSetting::Auto, true),
        "the same mode is not published again"
    );
    assert_eq!(mode.version(), v, "a repeated mode keeps the version");
}
