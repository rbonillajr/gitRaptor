//! Power saving (ADR-GRP-015 § 2; DS-TS-GRP-005 T003): the mode the engine is in, resolved from
//! `engine.powerSaving` and the power source, and handed to its consumers.

pub mod monitor;
pub mod source;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

pub use gitraptor_api::resources::{PowerSavingSetting, PowerSavingView};
pub use monitor::PowerMonitor;
pub use source::{FakePowerSource, PowerSource, UnknownPowerSource, platform_source};

/// How often the monitor reads the source (D8).
pub const POWER_POLL: Duration = Duration::from_secs(30);

/// What the OS says about the power supply.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PowerReading {
    Ac,
    Battery,
    LowPower,
    Unknown,
}

/// Whether the engine saves power (D6): `off` never, `on` always, `auto` on battery or in low
/// power mode (low power wins even on AC), never on AC or with an unknown source.
pub fn resolve(setting: PowerSavingSetting, reading: PowerReading) -> bool {
    match setting {
        PowerSavingSetting::Off => false,
        PowerSavingSetting::On => true,
        PowerSavingSetting::Auto => {
            matches!(reading, PowerReading::Battery | PowerReading::LowPower)
        }
    }
}

/// A change the monitor published.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PowerChange {
    pub setting: PowerSavingSetting,
    pub reading: PowerReading,
    pub active: bool,
}

#[derive(Debug)]
struct State {
    setting: PowerSavingSetting,
    version: u64,
}

#[derive(Debug)]
struct Shared {
    /// Mirrors `state` for the readers on a hot path, without the lock.
    active: AtomicBool,
    state: Mutex<State>,
    changed: Condvar,
}

impl Shared {
    fn state(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// The mode, as its consumers read it (D7). Cheap to clone; every clone sees the same mode.
#[derive(Debug, Clone)]
pub struct PowerMode {
    shared: Arc<Shared>,
}

/// The only writer of a [`PowerMode`].
#[derive(Debug)]
pub struct PowerModeWriter {
    shared: Arc<Shared>,
}

impl PowerMode {
    /// A mode that never changes (tests).
    pub fn fixed(active: bool) -> Self {
        let setting = if active {
            PowerSavingSetting::On
        } else {
            PowerSavingSetting::Off
        };
        Self::channel(setting, active).0
    }

    /// A mode starting at `(setting, active)` and its only writer.
    pub fn channel(setting: PowerSavingSetting, active: bool) -> (Self, PowerModeWriter) {
        let shared = Arc::new(Shared {
            active: AtomicBool::new(active),
            state: Mutex::new(State {
                setting,
                version: 0,
            }),
            changed: Condvar::new(),
        });
        (
            Self {
                shared: Arc::clone(&shared),
            },
            PowerModeWriter { shared },
        )
    }

    pub fn is_saving(&self) -> bool {
        self.shared.active.load(Ordering::Acquire)
    }

    pub fn setting(&self) -> PowerSavingSetting {
        self.shared.state().setting
    }

    /// Moves by one on every published change.
    pub fn version(&self) -> u64 {
        self.shared.state().version
    }

    /// Returns as soon as the version passes `since` or `timeout` runs out: the version seen.
    pub fn wait_change(&self, since: u64, timeout: Duration) -> u64 {
        let state = self.shared.state();
        let (state, _) = self
            .shared
            .changed
            .wait_timeout_while(state, timeout, |s| s.version <= since)
            .unwrap_or_else(PoisonError::into_inner);
        state.version
    }

    pub fn view(&self) -> PowerSavingView {
        let state = self.shared.state();
        PowerSavingView {
            setting: state.setting,
            active: self.shared.active.load(Ordering::Acquire),
        }
    }
}

impl PowerModeWriter {
    /// Publishes `(setting, active)`; `false` when it is the mode already published.
    pub fn set(&self, setting: PowerSavingSetting, active: bool) -> bool {
        let mut state = self.shared.state();
        if state.setting == setting && self.shared.active.load(Ordering::Acquire) == active {
            return false;
        }
        state.setting = setting;
        self.shared.active.store(active, Ordering::Release);
        state.version += 1;
        drop(state);
        self.shared.changed.notify_all();
        true
    }

    /// The setting last published.
    pub fn setting(&self) -> PowerSavingSetting {
        self.shared.state().setting
    }
}

#[cfg(test)]
mod tests;
