//! The power monitor of the daemon (DS-TS-GRP-005 T005): the `raptor-power` thread, in
//! `Utility`, that keeps the [`PowerMode`](crate::power::PowerMode) up to date.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc::{RecvTimeoutError, Sender, channel};
use std::thread::JoinHandle;
use std::time::{Duration, Instant, SystemTime};

use super::{Field, Logger};
use crate::power::{
    POWER_POLL, PowerMode, PowerMonitor, PowerReading, PowerSavingSetting, PowerSource,
    UnknownPowerSource, resolve,
};
use crate::profile::ProfileDirs;
use crate::profile::settings::{PROFILE_SETTINGS_FILE, power_saving};
use crate::work_class::{self, WorkClass};

/// Where the daemon reads the power source from, and how often.
#[derive(Clone)]
pub struct PowerConfig {
    pub source: Arc<dyn PowerSource>,
    pub poll: Duration,
}

impl Default for PowerConfig {
    /// A source that never knows, polled every [`POWER_POLL`]: `auto` saves nothing.
    fn default() -> Self {
        Self {
            source: Arc::new(UnknownPowerSource),
            poll: POWER_POLL,
        }
    }
}

impl std::fmt::Debug for PowerConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PowerConfig")
            .field("poll", &self.poll)
            .finish_non_exhaustive()
    }
}

/// The `raptor-power` thread and the mode it keeps. [`PowerThread::stop`], or dropping it,
/// closes the stop channel and the thread ends.
pub(super) struct PowerThread {
    mode: PowerMode,
    running: Option<(Sender<()>, JoinHandle<()>)>,
}

impl PowerThread {
    /// Resolves the initial mode from the profile's `engine.powerSaving` and one reading of the
    /// source, then starts the thread that follows both. The mode is final when this returns:
    /// a consumer never sees a provisional one.
    pub(super) fn start(config: &PowerConfig, dirs: &ProfileDirs, logger: &Logger) -> Self {
        let mut settings = SettingsWatch::new(dirs.clone());
        let setting = settings.read();
        let reading = config.source.read();
        let active = resolve(setting, reading);
        let (mode, writer) = PowerMode::channel(setting, active);
        if reading == PowerReading::Unknown {
            // The diagnostic of ADR-GRP-015 § 2, once per daemon: an unknown source stays so.
            logger.info("power_source_unknown", &[]);
        }
        log_mode(logger, setting, active, reading);

        let poll = config.poll;
        let mut monitor = PowerMonitor::new(
            Arc::clone(&config.source),
            Box::new(move || settings.changed()),
            writer,
            poll,
        );
        let (tx, rx) = channel::<()>();
        let thread_logger = logger.clone();
        let spawned = work_class::spawn_unpooled(WorkClass::Utility, "raptor-power", move || {
            // The first poll comes one period after the resolution at start.
            let mut due = Instant::now() + poll;
            while let Err(RecvTimeoutError::Timeout) =
                rx.recv_timeout(due.saturating_duration_since(Instant::now()))
            {
                let now = Instant::now();
                if let Some(change) = monitor.step(now) {
                    log_mode(
                        &thread_logger,
                        change.setting,
                        change.active,
                        change.reading,
                    );
                }
                due = monitor.next_tick(now);
            }
        });
        let running = match spawned {
            Ok(handle) => Some((tx, handle)),
            Err(_) => {
                // The mode stays the one resolved at start.
                logger.error("power_monitor_start_failed", &[]);
                None
            }
        };
        Self { mode, running }
    }

    pub(super) fn mode(&self) -> PowerMode {
        self.mode.clone()
    }

    /// Ends the thread and waits for it.
    pub(super) fn stop(&mut self) {
        if let Some((tx, handle)) = self.running.take() {
            drop(tx);
            let _ = handle.join();
        }
    }
}

/// The profile settings document, reread only when its modification time (or size) changed.
struct SettingsWatch {
    dirs: ProfileDirs,
    path: PathBuf,
    stamp: Option<(SystemTime, u64)>,
}

impl SettingsWatch {
    fn new(dirs: ProfileDirs) -> Self {
        let path = dirs.config.join(PROFILE_SETTINGS_FILE);
        Self {
            dirs,
            path,
            stamp: None,
        }
    }

    fn stamp(&self) -> Option<(SystemTime, u64)> {
        let meta = std::fs::symlink_metadata(&self.path).ok()?;
        Some((meta.modified().ok()?, meta.len()))
    }

    /// Reads the setting and remembers the stamp of the document read.
    fn read(&mut self) -> PowerSavingSetting {
        self.stamp = self.stamp();
        power_saving(&self.dirs)
    }

    /// The setting, when the document changed since the last read.
    fn changed(&mut self) -> Option<PowerSavingSetting> {
        (self.stamp() != self.stamp).then(|| self.read())
    }
}

fn log_mode(logger: &Logger, setting: PowerSavingSetting, active: bool, reading: PowerReading) {
    logger.info(
        "power_mode",
        &[
            ("setting", Field::Text(setting_name(setting))),
            ("active", Field::Bool(active)),
            ("reading", Field::Text(reading_name(reading))),
        ],
    );
}

fn setting_name(setting: PowerSavingSetting) -> &'static str {
    match setting {
        PowerSavingSetting::Auto => "auto",
        PowerSavingSetting::On => "on",
        PowerSavingSetting::Off => "off",
    }
}

fn reading_name(reading: PowerReading) -> &'static str {
    match reading {
        PowerReading::Ac => "ac",
        PowerReading::Battery => "battery",
        PowerReading::LowPower => "low-power",
        PowerReading::Unknown => "unknown",
    }
}
