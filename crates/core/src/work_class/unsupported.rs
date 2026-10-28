//! Linux and Windows (D4): no class is applied and the thread keeps the one it inherited. On
//! Linux an unprivileged thread cannot take a nice back, so lowering it for a stretch could
//! never be undone; nothing is raised to make up for it either.

use super::{Support, WorkClass};

pub(super) fn support() -> Support {
    Support::Unavailable
}

pub(super) fn current() -> Option<WorkClass> {
    None
}

/// Never applied.
pub(super) fn set(class: WorkClass) -> bool {
    let _ = class;
    false
}
