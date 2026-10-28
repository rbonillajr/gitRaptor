//! Where the power reading comes from.

use std::sync::Arc;

use super::PowerReading;

pub trait PowerSource: Send + Sync {
    fn read(&self) -> PowerReading;
}

/// A source that never knows: `auto` saves nothing (ADR-GRP-015 § 2).
#[derive(Debug, Clone, Copy, Default)]
pub struct UnknownPowerSource;

impl PowerSource for UnknownPowerSource {
    fn read(&self) -> PowerReading {
        todo!("T003")
    }
}

/// A source a test sets by hand. Public so other modules and crates can use it (TS-CKP-001).
#[derive(Debug, Clone)]
pub struct FakePowerSource {}

impl FakePowerSource {
    pub fn new(reading: PowerReading) -> Self {
        let _ = reading;
        todo!("T003")
    }

    pub fn set(&self, reading: PowerReading) {
        let _ = reading;
        todo!("T003")
    }
}

impl PowerSource for FakePowerSource {
    fn read(&self) -> PowerReading {
        todo!("T003")
    }
}

/// The source of this OS: `Unknown` everywhere until T007.
pub fn platform_source() -> Arc<dyn PowerSource> {
    todo!("T003")
}
