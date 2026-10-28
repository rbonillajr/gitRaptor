//! Ctrl-C as a flag (US-GRD-006, D8): `raptor guard exec` cancels its window when it sees it.

use std::io;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

/// A flag set when the process receives SIGINT. The handler stays registered for the life of the
/// process: a child started afterwards gets the signal from the process group, and the caller
/// simply ignores the flag from then on.
///
/// # Errors
///
/// The OS refused the handler; on Windows, always `Unsupported` (Ctrl-C ends the process).
#[cfg(unix)]
pub fn interrupt_flag() -> io::Result<Arc<AtomicBool>> {
    let flag = Arc::new(AtomicBool::new(false));
    signal_hook::flag::register(signal_hook::consts::SIGINT, Arc::clone(&flag))?;
    Ok(flag)
}

/// See the Unix version: not available on this platform.
///
/// # Errors
///
/// Always `Unsupported`.
#[cfg(not(unix))]
pub fn interrupt_flag() -> io::Result<Arc<AtomicBool>> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "interrupt flag not supported on this platform",
    ))
}
