//! The monitor: reads the source every poll and publishes a change of mode (D8). Pure with
//! respect to the clock: the instant comes in as an argument.

use std::sync::Arc;
use std::time::{Duration, Instant};

use super::{PowerChange, PowerModeWriter, PowerSavingSetting, PowerSource, resolve};

/// Reads `engine.powerSaving` again; `None` when it did not change since the last call.
pub type SettingReader = Box<dyn FnMut() -> Option<PowerSavingSetting> + Send>;

pub struct PowerMonitor {
    source: Arc<dyn PowerSource>,
    setting: SettingReader,
    writer: PowerModeWriter,
    poll: Duration,
    last: Option<Instant>,
}

impl PowerMonitor {
    /// A monitor that starts from the setting of `writer` and publishes through it.
    pub fn new(
        source: Arc<dyn PowerSource>,
        setting: SettingReader,
        writer: PowerModeWriter,
        poll: Duration,
    ) -> Self {
        Self {
            source,
            setting,
            writer,
            poll,
            last: None,
        }
    }

    /// One poll at `now`: reads the source, rereads the setting when its reader reports a
    /// change, and publishes only when `(setting, active)` changed. The change it published,
    /// if any.
    pub fn step(&mut self, now: Instant) -> Option<PowerChange> {
        self.last = Some(now);
        let setting = (self.setting)().unwrap_or_else(|| self.writer.setting());
        let reading = self.source.read();
        let active = resolve(setting, reading);
        self.writer.set(setting, active).then_some(PowerChange {
            setting,
            reading,
            active,
        })
    }

    /// When the next poll is due: one poll after the last step, or `now` before the first.
    pub fn next_tick(&self, now: Instant) -> Instant {
        match self.last {
            Some(last) => last + self.poll,
            None => now,
        }
    }
}

#[cfg(test)]
mod tests;
