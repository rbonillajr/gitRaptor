//! The monitor: reads the source every poll and publishes a change of mode (D8). Pure with
//! respect to the clock: the instant comes in as an argument.

use std::sync::Arc;
use std::time::{Duration, Instant};

use super::{PowerChange, PowerModeWriter, PowerSavingSetting, PowerSource};

/// Reads `engine.powerSaving` again; `None` when it did not change since the last call.
pub type SettingReader = Box<dyn FnMut() -> Option<PowerSavingSetting> + Send>;

pub struct PowerMonitor {}

impl PowerMonitor {
    pub fn new(
        source: Arc<dyn PowerSource>,
        setting: SettingReader,
        writer: PowerModeWriter,
        poll: Duration,
    ) -> Self {
        let _ = (source, setting, writer, poll);
        todo!("T003")
    }

    /// One poll at `now`: the change it published, if any.
    pub fn step(&mut self, now: Instant) -> Option<PowerChange> {
        let _ = now;
        todo!("T003")
    }

    /// When the next poll is due.
    pub fn next_tick(&self, now: Instant) -> Instant {
        let _ = now;
        todo!("T003")
    }
}

#[cfg(test)]
mod tests;
