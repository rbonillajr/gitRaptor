//! The power monitor of the daemon (DS-TS-GRP-005 T005): the `raptor-power` thread, in
//! `Utility`, that keeps the [`PowerMode`](crate::power::PowerMode) up to date.

use std::sync::Arc;
use std::time::Duration;

use crate::power::{POWER_POLL, PowerSource, UnknownPowerSource};

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
