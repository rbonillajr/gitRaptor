//! TS-GRP-005 T005: the power mode of a real daemon (in-process) in a temporary profile, with
//! a `FakePowerSource`; never this repo or the real profile (NFR-01). Every wait is on
//! `PowerMode::wait_change`, never a fixed sleep.
#![cfg(any(target_os = "macos", target_os = "linux"))]

mod common;

use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use common::TempProfile;
use gitraptor_core::channel::ChannelConfig;
use gitraptor_core::daemon::{
    Daemon, DaemonConfig, DaemonEnv, LogLimits, PowerConfig, ShutdownHandle, StopCause, StopReport,
    TierConfig, TmCapture,
};
use gitraptor_core::power::{FakePowerSource, PowerMode, PowerReading, PowerSavingSetting};

const WITHIN: Duration = Duration::from_secs(60);

struct Running {
    tp: TempProfile,
    mode: PowerMode,
    handle: ShutdownHandle,
    join: Option<JoinHandle<StopReport>>,
}

impl Drop for Running {
    fn drop(&mut self) {
        if let Some(join) = self.join.take() {
            self.handle.request(StopCause::Signal("TERM"));
            let _ = join.join();
        }
    }
}

impl Running {
    fn settings_path(&self) -> std::path::PathBuf {
        self.tp.dirs().config.join("settings.json")
    }

    /// Waits until the mode saves (`want == true`) or not, within the 60 s of ADR-GRP-015 § 2.
    fn wait_for(&self, want: bool) -> Duration {
        let start = Instant::now();
        let mut seen = self.mode.version();
        while self.mode.is_saving() != want {
            assert!(
                start.elapsed() < WITHIN,
                "the daemon mode must reach saving={want} within 60 s"
            );
            seen = self.mode.wait_change(seen, Duration::from_secs(1));
        }
        start.elapsed()
    }
}

/// A daemon in a fresh profile whose `settings.json` is `settings`, reading `fake` every
/// `poll`.
fn start(settings: Option<&str>, fake: &FakePowerSource, poll: Duration) -> Running {
    let tp = TempProfile::new();
    if let Some(text) = settings {
        let config = tp.dirs().config;
        // 0700 like the daemon's umask: `Profile::open` refuses a 0755 folder.
        std::fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(&config)
            .unwrap();
        std::fs::set_permissions(&config, std::fs::Permissions::from_mode(0o700)).unwrap();
        std::fs::write(config.join("settings.json"), text).unwrap();
    }
    let env = DaemonEnv::from_vars(std::env::vars_os());
    let config = DaemonConfig {
        dirs: tp.dirs(),
        git: env.git_resolve_config(None),
        env,
        heartbeat: Duration::from_secs(3600),
        log: LogLimits::default(),
        stop_deadline: None,
        channel: ChannelConfig::default(),
        protected: None,
        operations: None,
        tm_prior_layer: None,
        tiers: TierConfig::default(),
        discovery: Default::default(),
        power: PowerConfig {
            source: Arc::new(fake.clone()),
            poll,
        },
        tm_capture: TmCapture::default(),
    };
    let daemon = Daemon::start(config).unwrap();
    let mode = daemon.power();
    let handle = daemon.shutdown_handle();
    let join = std::thread::spawn(move || daemon.run());
    Running {
        tp,
        mode,
        handle,
        join: Some(join),
    }
}

#[test]
fn a_fake_source_switches_the_daemon_mode_within_60_s() {
    let fake = FakePowerSource::new(PowerReading::Ac);
    let r = start(None, &fake, Duration::from_millis(50));
    assert_eq!(
        r.mode.setting(),
        PowerSavingSetting::Auto,
        "no key means auto"
    );
    assert!(!r.mode.is_saving(), "auto on AC saves nothing");

    fake.set(PowerReading::Battery);
    let took = r.wait_for(true);
    assert!(took <= WITHIN, "the change took {took:?}");

    fake.set(PowerReading::Ac);
    r.wait_for(false);
}

#[test]
fn on_and_off_override_the_source() {
    // `on` saves on AC.
    let fake = FakePowerSource::new(PowerReading::Ac);
    let on = start(
        Some(r#"{"engine":{"powerSaving":"on"}}"#),
        &fake,
        Duration::from_millis(50),
    );
    assert_eq!(on.mode.setting(), PowerSavingSetting::On);
    on.wait_for(true);

    // `off` does not save on battery.
    let fake = FakePowerSource::new(PowerReading::Battery);
    let off = start(
        Some(r#"{"engine":{"powerSaving":"off"}}"#),
        &fake,
        Duration::from_millis(50),
    );
    assert_eq!(off.mode.setting(), PowerSavingSetting::Off);
    assert!(!off.mode.is_saving(), "off wins over a battery source");

    // The monitor rereads the key when the document changes: `off` -> `auto` on battery saves.
    std::fs::write(off.settings_path(), r#"{"engine":{"powerSaving":"auto"}}"#).unwrap();
    off.wait_for(true);
    assert_eq!(off.mode.setting(), PowerSavingSetting::Auto);
}

#[test]
fn the_mode_is_resolved_before_the_modules_start() {
    // The poll is an hour: only the resolution at start can have set the mode when
    // `Daemon::start` returns, after the modules started. No module ever sees a provisional
    // mode.
    let fake = FakePowerSource::new(PowerReading::Battery);
    let r = start(None, &fake, Duration::from_secs(3600));
    assert!(
        r.mode.is_saving(),
        "auto on battery must already be resolved when the daemon has started"
    );
    let v = r.mode.version();

    let fake = FakePowerSource::new(PowerReading::Ac);
    let r2 = start(
        Some(r#"{"engine":{"powerSaving":"on"}}"#),
        &fake,
        Duration::from_secs(3600),
    );
    assert!(
        r2.mode.is_saving(),
        "the profile's `on` must already be resolved when the daemon has started"
    );
    assert_eq!(
        r.mode.version(),
        v,
        "the initial mode is not a change published later"
    );
}
