//! A foreign CPU load for the saturated scenarios of the benches (RES-07 of INF-GRP-002;
//! DS-TS-GRP-005 T013): one busy process per requested core, at normal priority, alive for as
//! long as the [`CpuLoad`] value. The load is a separate process, never a thread of the bench,
//! so the scheduler sees it as another program competing with the engine.

use std::fmt;
use std::process::Child;

/// Why the load could not start.
#[derive(Debug)]
pub enum CpuLoadError {
    /// The OS has no load for this harness (only Unix has one).
    Unsupported,
    /// A busy process could not be started.
    Spawn(std::io::Error),
}

impl fmt::Display for CpuLoadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unsupported => write!(f, "the foreign CPU load is only available on Unix"),
            Self::Spawn(e) => write!(f, "could not start a busy process: {e}"),
        }
    }
}

impl std::error::Error for CpuLoadError {}

/// `n` busy processes; dropping the value kills and reaps them.
pub struct CpuLoad {
    children: Vec<Child>,
}

impl CpuLoad {
    /// Starts `n` busy processes (`/bin/sh -c 'while :; do :; done'`, a fixed argv with an
    /// empty environment) at the priority this process has. A failure kills the ones already
    /// started.
    ///
    /// # Errors
    ///
    /// [`CpuLoadError::Unsupported`] outside Unix; [`CpuLoadError::Spawn`] when a process
    /// cannot start.
    pub fn start(n: usize) -> Result<Self, CpuLoadError> {
        let mut load = Self {
            children: Vec::with_capacity(n),
        };
        for _ in 0..n {
            load.children.push(spawn_busy()?);
        }
        Ok(load)
    }

    /// How many busy processes run.
    #[must_use]
    pub fn len(&self) -> usize {
        self.children.len()
    }

    /// Whether no busy process runs.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.children.is_empty()
    }
}

impl Drop for CpuLoad {
    fn drop(&mut self) {
        for child in &mut self.children {
            let _ = child.kill();
        }
        for child in &mut self.children {
            let _ = child.wait();
        }
    }
}

/// The number of logical cores, at least 1.
#[must_use]
pub fn logical_cores() -> usize {
    std::thread::available_parallelism().map_or(1, std::num::NonZeroUsize::get)
}

#[cfg(unix)]
fn spawn_busy() -> Result<Child, CpuLoadError> {
    use std::process::{Command, Stdio};
    Command::new("/bin/sh")
        .args(["-c", "while :; do :; done"])
        .env_clear()
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(CpuLoadError::Spawn)
}

#[cfg(not(unix))]
fn spawn_busy() -> Result<Child, CpuLoadError> {
    Err(CpuLoadError::Unsupported)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn the_load_starts_and_ends_with_its_value() {
        let load = CpuLoad::start(2).expect("the load starts");
        assert_eq!(load.len(), 2);
        drop(load);
        assert!(CpuLoad::start(0).expect("an empty load").is_empty());
    }

    #[cfg(not(unix))]
    #[test]
    fn the_load_is_unsupported_off_unix() {
        assert!(matches!(CpuLoad::start(1), Err(CpuLoadError::Unsupported)));
    }
}
