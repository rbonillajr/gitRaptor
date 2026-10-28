//! Quality of service and I/O policy of the current thread, and of a child about to start
//! (ADR-GRP-015 § 1; DS-TS-GRP-005 T001, D9). macOS only.

use std::process::Command;

use crate::ffi_qos;

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

impl QosClass {
    fn raw(self) -> libc::c_uint {
        match self {
            Self::UserInitiated => ffi_qos::QOS_CLASS_USER_INITIATED,
            Self::Default => ffi_qos::QOS_CLASS_DEFAULT,
            Self::Utility => ffi_qos::QOS_CLASS_UTILITY,
            Self::Background => ffi_qos::QOS_CLASS_BACKGROUND,
        }
    }

    /// The process disk I/O policy of a lowered child.
    fn io_policy(self) -> libc::c_int {
        match self {
            Self::UserInitiated | Self::Default => ffi_qos::IOPOL_IMPORTANT,
            Self::Utility => ffi_qos::IOPOL_UTILITY,
            Self::Background => ffi_qos::IOPOL_THROTTLE,
        }
    }

    /// The nice of a lowered child (ADR-GRP-015 § 1); `None` for the classes not lowered.
    fn child_nice(self) -> Option<libc::c_int> {
        match self {
            Self::UserInitiated | Self::Default => None,
            Self::Utility => Some(5),
            Self::Background => Some(10),
        }
    }
}

/// Sets the QoS of the current thread; the disk I/O tier follows the QoS. On error the thread
/// keeps the class it had.
///
/// No explicit thread I/O policy is set: on macOS `setiopolicy_np(IOPOL_SCOPE_THREAD)` opts the
/// thread out of QoS (its QoS reads back unspecified and setting it again fails with `EPERM`),
/// so the two cannot coexist on one thread.
///
/// # Errors
///
/// The code `pthread_set_qos_class_self_np` returned.
pub fn set_current(class: QosClass) -> Result<(), QosError> {
    ffi_qos::set_thread_qos(class.raw()).map_err(QosError)
}

/// The QoS of the current thread; `None` when unspecified. `QOS_CLASS_USER_INTERACTIVE`, which
/// the daemon never sets, reads as the highest class it knows, `UserInitiated`.
pub fn current() -> Option<QosClass> {
    match ffi_qos::thread_qos() {
        ffi_qos::QOS_CLASS_USER_INTERACTIVE | ffi_qos::QOS_CLASS_USER_INITIATED => {
            Some(QosClass::UserInitiated)
        }
        ffi_qos::QOS_CLASS_DEFAULT => Some(QosClass::Default),
        ffi_qos::QOS_CLASS_UTILITY => Some(QosClass::Utility),
        ffi_qos::QOS_CLASS_BACKGROUND => Some(QosClass::Background),
        _ => None,
    }
}

/// The disk I/O policy of the current thread, as the class it maps to (`IOPOL_IMPORTANT` →
/// `Default`, `IOPOL_UTILITY` → `Utility`, `IOPOL_THROTTLE` → `Background`); `None` for any
/// other policy, including the unset `IOPOL_DEFAULT`.
pub fn current_io() -> Option<QosClass> {
    match ffi_qos::thread_io()? {
        ffi_qos::IOPOL_IMPORTANT => Some(QosClass::Default),
        ffi_qos::IOPOL_UTILITY => Some(QosClass::Utility),
        ffi_qos::IOPOL_THROTTLE => Some(QosClass::Background),
        _ => None,
    }
}

/// Lowers a child before `exec`: nice (`Utility` 5, `Background` 10) and the process disk I/O
/// policy. Nothing for `Default` and `UserInitiated`, which keep `posix_spawn`. The thread QoS
/// is not set in the child: it is not safe after `fork`.
pub fn lower_child(cmd: &mut Command, class: QosClass) {
    if let Some(nice) = class.child_nice() {
        ffi_qos::lower_child(cmd, nice, class.io_policy());
    }
}
