//! TS-GRP-005 T008 (RES1, guards 8.1 to 8.3): `engine.resources` publishes the power mode and
//! the live pools. Temporary profile root (NFR-01); waits are on channels, never a fixed sleep.
//!
//! Expects `ResourceMonitor::set_power(&self, mode: PowerMode)` (T008).

use std::sync::Arc;
use std::sync::atomic::AtomicU64;

use gitraptor_core::power::{PowerMode, PowerSavingSetting, PowerSavingView};
use gitraptor_core::profile::ProfileDirs;
use gitraptor_core::resources::{ResourceConfig, ResourceMonitor};

fn monitor(tmp: &tempfile::TempDir) -> ResourceMonitor {
    ResourceMonitor::new(
        ResourceConfig::default(),
        ProfileDirs::under_root(tmp.path()),
        Arc::new(AtomicU64::new(0)),
    )
}

#[test]
fn power_saving_follows_the_mode() {
    let tmp = tempfile::tempdir().unwrap();
    let monitor = monitor(&tmp);
    assert!(
        monitor.read().power_saving.is_none(),
        "before the monitor has a power mode, power_saving is null"
    );

    let (mode, writer) = PowerMode::channel(PowerSavingSetting::Auto, false);
    monitor.set_power(mode);
    assert_eq!(
        monitor.read().power_saving,
        Some(PowerSavingView {
            setting: PowerSavingSetting::Auto,
            active: false
        }),
        "power_saving must publish (setting, active) from the mode on every OS"
    );

    writer.set(PowerSavingSetting::Auto, true);
    assert_eq!(
        monitor.read().power_saving,
        Some(PowerSavingView {
            setting: PowerSavingSetting::Auto,
            active: true
        }),
        "power_saving must follow the mode when it changes"
    );

    writer.set(PowerSavingSetting::Off, false);
    assert_eq!(
        monitor.read().power_saving,
        Some(PowerSavingView {
            setting: PowerSavingSetting::Off,
            active: false
        }),
        "power_saving must follow a change of the setting"
    );
}

#[cfg(not(target_os = "macos"))]
#[test]
fn pools_are_none_when_classes_are_unavailable() {
    let tmp = tempfile::tempdir().unwrap();
    let monitor = monitor(&tmp);
    monitor.set_power(PowerMode::fixed(false));
    assert!(
        monitor.read().pools.is_none(),
        "where classes are unavailable pools is null (not an empty list)"
    );
}

#[cfg(target_os = "macos")]
#[test]
fn pools_list_only_running_pools() {
    use std::sync::mpsc::channel;

    use gitraptor_api::resources::{Pool, PoolClass, WorkClass};
    use gitraptor_core::work_class;

    let tmp = tempfile::tempdir().unwrap();
    let monitor = monitor(&tmp);
    monitor.set_power(PowerMode::fixed(false));

    let pools = monitor
        .read()
        .pools
        .expect("on macOS pools is a list (classes are applied), not null");
    assert!(
        !pools.iter().any(|p| p.pool == Pool::Capture),
        "a pool with no live thread must not be listed: {pools:?}"
    );

    let (up_tx, up_rx) = channel::<()>();
    let (stop_tx, stop_rx) = channel::<()>();
    let handle = work_class::spawn(
        Pool::Capture,
        WorkClass::Default,
        "raptor-test-pool",
        move || {
            up_tx.send(()).unwrap();
            let _ = stop_rx.recv();
        },
    )
    .unwrap();
    up_rx.recv().unwrap();
    let pools = monitor.read().pools.expect("pools on macOS");
    assert!(
        pools.contains(&PoolClass {
            pool: Pool::Capture,
            class: WorkClass::Default
        }),
        "a live pool thread must be listed with its class: {pools:?}"
    );
    assert_eq!(
        pools.iter().filter(|p| p.pool == Pool::Capture).count(),
        1,
        "no duplicates"
    );
    assert!(
        !pools.iter().any(|p| p.pool == Pool::Maintenance),
        "only the pools that really run are listed, not the theoretical table: {pools:?}"
    );

    stop_tx.send(()).unwrap();
    handle.join().unwrap();
    let pools = monitor.read().pools.expect("pools on macOS");
    assert!(
        !pools.iter().any(|p| p.pool == Pool::Capture),
        "a pool whose thread ended is no longer listed: {pools:?}"
    );
}
