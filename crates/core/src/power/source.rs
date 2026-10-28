//! Where the power reading comes from.

use std::sync::{Arc, Mutex, PoisonError};

use super::PowerReading;

pub trait PowerSource: Send + Sync {
    fn read(&self) -> PowerReading;
}

/// A source that never knows: `auto` saves nothing (ADR-GRP-015 § 2).
#[derive(Debug, Clone, Copy, Default)]
pub struct UnknownPowerSource;

impl PowerSource for UnknownPowerSource {
    fn read(&self) -> PowerReading {
        PowerReading::Unknown
    }
}

/// A source a test sets by hand. Public so other modules and crates can use it (TS-CKP-001).
#[derive(Debug, Clone)]
pub struct FakePowerSource {
    reading: Arc<Mutex<PowerReading>>,
}

impl FakePowerSource {
    pub fn new(reading: PowerReading) -> Self {
        Self {
            reading: Arc::new(Mutex::new(reading)),
        }
    }

    /// What every clone reads from now on.
    pub fn set(&self, reading: PowerReading) {
        *self.reading.lock().unwrap_or_else(PoisonError::into_inner) = reading;
    }
}

impl PowerSource for FakePowerSource {
    fn read(&self) -> PowerReading {
        *self.reading.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// The source of this OS: `Unknown` everywhere until T007.
pub fn platform_source() -> Arc<dyn PowerSource> {
    Arc::new(UnknownPowerSource)
}
