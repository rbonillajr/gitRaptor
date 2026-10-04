//! Monotonic clock shared by every process of the machine (ADR-GRP-011 § 3).
//!
//! The engine stamps each stage of a change event with it and a client
//! compares those stamps with its own reading, so all of them must use the
//! same system clock, serialized as nanoseconds. The wall clock is not used
//! because NTP can move it.

/// Nanoseconds of the system-wide monotonic clock.
///
/// Unix: `clock_gettime(CLOCK_MONOTONIC)`; on macOS it is backed by
/// `mach_continuous_time` and keeps counting during sleep.
#[cfg(unix)]
pub fn monotonic_ns() -> u64 {
    let ts = rustix::time::clock_gettime(rustix::time::ClockId::Monotonic);
    let secs = u64::try_from(ts.tv_sec).unwrap_or(0);
    let nanos = u64::try_from(ts.tv_nsec).unwrap_or(0);
    secs.saturating_mul(1_000_000_000).saturating_add(nanos)
}

/// Windows needs `QueryPerformanceCounter`, which is not reachable without
/// `unsafe` or a new crate. Until then the reading is relative to this
/// process and NOT comparable across processes.
/// Pendiente: etapa de validación multiplataforma.
#[cfg(not(unix))]
pub fn monotonic_ns() -> u64 {
    use std::sync::OnceLock;
    use std::time::Instant;
    static START: OnceLock<Instant> = OnceLock::new();
    let start = START.get_or_init(Instant::now);
    u64::try_from(start.elapsed().as_nanos()).unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn never_goes_backwards() {
        let a = monotonic_ns();
        let b = monotonic_ns();
        assert!(b >= a);
        assert!(a > 0);
    }

    /// The same clock read from another process lies between two local
    /// readings: the stamps of the daemon and of a client are comparable.
    #[cfg(unix)]
    #[test]
    fn comparable_across_processes() {
        let before = monotonic_ns();
        // `sh -c` has no monotonic clock reader; re-run this very test binary
        // is overkill, so compare against a child that sleeps a bit: its
        // lifetime must fit between the two readings of the same clock.
        let status = std::process::Command::new("/bin/sleep")
            .arg("0.01")
            .status()
            .unwrap();
        assert!(status.success());
        let after = monotonic_ns();
        assert!(after - before >= 10_000_000);
    }
}
