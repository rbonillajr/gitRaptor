//! Criterion tests of T003: the monitor with a fake source and a simulated clock (D8).

use std::sync::Arc;
use std::time::{Duration, Instant};

use super::*;
use crate::power::{FakePowerSource, PowerMode, PowerReading};

const POLL: Duration = Duration::from_secs(30);

fn monitor(fake: &FakePowerSource, setting: PowerSavingSetting) -> (PowerMonitor, PowerMode) {
    let (mode, writer) = PowerMode::channel(setting, false);
    let m = PowerMonitor::new(Arc::new(fake.clone()), Box::new(|| None), writer, POLL);
    (m, mode)
}

#[test]
fn a_source_change_is_published_within_60_s() {
    let fake = FakePowerSource::new(PowerReading::Ac);
    let (mut m, mode) = monitor(&fake, PowerSavingSetting::Auto);
    let base = Instant::now();
    m.step(base);
    assert!(!mode.is_saving(), "on AC, auto saves nothing");

    // The source changes at `change`; the clock only moves by the monitor's own ticks.
    let change = base + Duration::from_secs(7);
    fake.set(PowerReading::Battery);
    let mut now = base;
    let mut published = None;
    while now < change + Duration::from_secs(120) {
        now = m.next_tick(now);
        if now < change {
            continue;
        }
        if let Some(c) = m.step(now) {
            published = Some((now, c));
            break;
        }
    }
    let (at, c) = published.expect("the change must be published by a step at a tick");
    assert!(
        at <= change + Duration::from_secs(60),
        "the change at t must be published by t + 60 s, got +{:?}",
        at - change
    );
    assert!(c.active, "auto on battery saves");
    assert_eq!(c.reading, PowerReading::Battery);
    assert!(mode.is_saving(), "the handle follows the published change");
}

#[test]
fn the_same_mode_is_not_published_twice() {
    let fake = FakePowerSource::new(PowerReading::Battery);
    let (mut m, mode) = monitor(&fake, PowerSavingSetting::Auto);
    let base = Instant::now();
    m.step(base);
    assert!(mode.is_saving(), "the first reading settles the mode");
    let v = mode.version();
    for i in 1..=5u32 {
        assert!(
            m.step(base + POLL * i).is_none(),
            "poll {i} with the same mode must publish nothing"
        );
    }
    assert_eq!(
        mode.version(),
        v,
        "the version must not move without a change"
    );

    // Battery -> LowPower: the reading changed but the mode (setting, active) did not.
    fake.set(PowerReading::LowPower);
    assert!(
        m.step(base + POLL * 6).is_none(),
        "a different reading that gives the same mode is not a change"
    );
    assert_eq!(mode.version(), v);
}
