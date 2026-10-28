//! Power saving (ADR-GRP-015 § 2; DS-TS-GRP-005 T003): the mode the engine is in, resolved from
//! `engine.powerSaving` and the power source, and handed to its consumers.

pub mod monitor;
pub mod source;

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

/// Whether the engine saves power (D6).
pub fn resolve(setting: PowerSavingSetting, reading: PowerReading) -> bool {
    let _ = (setting, reading);
    todo!("T003")
}

/// A change the monitor published.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PowerChange {
    pub setting: PowerSavingSetting,
    pub reading: PowerReading,
    pub active: bool,
}

/// The mode, as its consumers read it (D7).
#[derive(Debug, Clone)]
pub struct PowerMode {}

/// The only writer of a [`PowerMode`].
#[derive(Debug)]
pub struct PowerModeWriter {}

impl PowerMode {
    /// A mode that never changes (tests).
    pub fn fixed(active: bool) -> Self {
        let _ = active;
        todo!("T003")
    }

    pub fn channel(setting: PowerSavingSetting, active: bool) -> (Self, PowerModeWriter) {
        let _ = (setting, active);
        todo!("T003")
    }

    pub fn is_saving(&self) -> bool {
        todo!("T003")
    }

    pub fn setting(&self) -> PowerSavingSetting {
        todo!("T003")
    }

    pub fn version(&self) -> u64 {
        todo!("T003")
    }

    /// Returns as soon as the version passes `since` or `timeout` runs out: the version seen.
    pub fn wait_change(&self, since: u64, timeout: Duration) -> u64 {
        let _ = (since, timeout);
        todo!("T003")
    }

    pub fn view(&self) -> PowerSavingView {
        todo!("T003")
    }
}

impl PowerModeWriter {
    /// Publishes `(setting, active)`; `false` when it is the mode already published.
    pub fn set(&self, setting: PowerSavingSetting, active: bool) -> bool {
        let _ = (setting, active);
        todo!("T003")
    }
}

#[cfg(test)]
mod tests;
