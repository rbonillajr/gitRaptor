//! Crash points of the chaos harness (INF-TMC-001, DS-INF-TMC-001 § 2).
//!
//! Each point is a stable name in a closed list, placed right after a state
//! of the journal is on disk. Only with the `chaos` feature, off by default
//! and turned on by the dev-dependency of the CLI's tests (SEC-06): the first
//! call reads [`CRASH_AT_ENV`] once (a name outside [`POINTS`] panics) and,
//! when it names the point reached, the process kills itself. The death is
//! `SIGKILL`, not a panic: no `Drop` runs, so own Git locks and half-written
//! rows stay as after a real cut, and it is the recovery at the next start
//! (ADR-TMC-003 § 6) that must close them. Without the feature,
//! [`crash_point`] is empty.

#[cfg(feature = "chaos")]
use std::sync::OnceLock;

/// Test hook (`chaos` feature): the name of the point where the daemon dies.
pub const CRASH_AT_ENV: &str = "GITRAPTOR_TEST_TM_CRASH_AT";

/// Every crash point, in the order an undo reaches them.
pub const POINTS: &[&str] = &[
    OPERATION_INTENT,
    PRIOR_PENDING,
    PRIOR_REF,
    OPERATION_PRIOR,
    OPERATION_READY,
    "apply:step-3",
    "apply:step-4",
    "apply:step-5",
    "apply:step-6",
    APPLY_MID_FILES,
    "apply:step-7",
    OPERATION_APPLIED,
];

/// Intent recorded, no prior snapshot yet.
pub const OPERATION_INTENT: &str = "operation:intent";
/// Row of the guaranteed prior `pending`, no ref yet.
pub const PRIOR_PENDING: &str = "prior:pending";
/// Ref of the guaranteed prior created, row still `pending`.
pub const PRIOR_REF: &str = "prior:ref";
/// Prior snapshot recorded in the operation, not `ready` yet.
pub const OPERATION_PRIOR: &str = "operation:prior";
/// Operation `ready`, its step not started.
pub const OPERATION_READY: &str = "operation:ready";
/// Right before the second file exchange of the applier: some files
/// written, others not.
pub const APPLY_MID_FILES: &str = "apply:mid-files";
/// The step ended, the operation not closed yet.
pub const OPERATION_APPLIED: &str = "operation:applied";

/// The point of an applier step (3 to 7), after its journal entry.
pub fn apply_step(step: u32) -> &'static str {
    match step {
        3 => "apply:step-3",
        4 => "apply:step-4",
        5 => "apply:step-5",
        6 => "apply:step-6",
        7 => "apply:step-7",
        _ => "apply:step-other",
    }
}

/// Dies here if the harness asked for this point. Empty unless the crate is
/// built with the `chaos` feature, which only the test builds turn on: a
/// release build, even one with debug assertions, compiles neither the
/// variable read nor the kill (SEC-06).
#[cfg(feature = "chaos")]
pub fn crash_point(name: &str) {
    debug_assert!(POINTS.contains(&name), "unknown crash point {name}");
    static ARMED: OnceLock<Option<String>> = OnceLock::new();
    let armed = ARMED.get_or_init(|| {
        let armed = std::env::var(CRASH_AT_ENV).ok().filter(|v| !v.is_empty());
        if let Some(point) = &armed {
            assert!(
                POINTS.contains(&point.as_str()),
                "{CRASH_AT_ENV} names no crash point: {point}"
            );
        }
        armed
    });
    if armed.as_deref() == Some(name) {
        die();
    }
}

#[cfg(not(feature = "chaos"))]
#[inline(always)]
pub fn crash_point(_name: &str) {}

#[cfg(all(feature = "chaos", unix))]
fn die() -> ! {
    use rustix::process::{Signal, getpid, kill_process};
    // `SIGKILL` to oneself may land after `kill` returns: wait for it here,
    // so nothing else runs (an `abort` would race it and die by `SIGABRT`).
    if kill_process(getpid(), Signal::KILL).is_ok() {
        loop {
            std::thread::park();
        }
    }
    std::process::abort()
}

#[cfg(all(feature = "chaos", not(unix)))]
fn die() -> ! {
    std::process::abort()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_applier_step_has_its_point() {
        for step in 3..=7 {
            assert!(POINTS.contains(&apply_step(step)), "step {step}");
        }
    }

    #[test]
    fn points_are_unique() {
        let mut seen = std::collections::HashSet::new();
        assert!(POINTS.iter().all(|p| seen.insert(p)));
    }
}
