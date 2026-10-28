//! Quality of service and I/O policy of the current thread, and of a child about to start
//! (ADR-GRP-015 § 1; DS-TS-GRP-005 T001, D9). macOS only.

use std::process::Command;

/// The QoS classes the daemon uses, highest first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum QosClass {
    UserInitiated,
    Default,
    Utility,
    Background,
}

/// A failed call, with its `errno`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct QosError(pub i32);

/// Sets the QoS and the disk I/O policy of the current thread. On error the thread keeps the
/// class it had.
pub fn set_current(class: QosClass) -> Result<(), QosError> {
    let _ = class;
    todo!("T001")
}

/// The QoS of the current thread; `None` when unspecified.
pub fn current() -> Option<QosClass> {
    todo!("T001")
}

/// The disk I/O policy of the current thread, as the class it maps to (`IOPOL_IMPORTANT` →
/// `Default`, `IOPOL_UTILITY` → `Utility`, `IOPOL_THROTTLE` → `Background`).
pub fn current_io() -> Option<QosClass> {
    todo!("T001")
}

/// Lowers a child before `exec`: nice and the process disk I/O policy. Nothing for `Default` and
/// `UserInitiated`.
pub fn lower_child(cmd: &mut Command, class: QosClass) {
    let _ = (cmd, class);
    todo!("T001")
}
