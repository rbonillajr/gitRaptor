//! macOS: a class is the QoS of the thread (`gitraptor_macsys::qos`).

use gitraptor_macsys::qos::{self, QosClass};

use super::{Support, WorkClass};

pub(super) fn support() -> Support {
    Support::Applied
}

pub(super) fn current() -> Option<WorkClass> {
    qos::current().map(|c| match c {
        QosClass::UserInitiated => WorkClass::UserInitiated,
        QosClass::Default => WorkClass::Default,
        QosClass::Utility => WorkClass::Utility,
        QosClass::Background => WorkClass::Background,
    })
}

/// Whether the OS took the class.
pub(super) fn set(class: WorkClass) -> bool {
    let qos = match class {
        WorkClass::UserInitiated => QosClass::UserInitiated,
        WorkClass::Default => QosClass::Default,
        WorkClass::Utility => QosClass::Utility,
        WorkClass::Background => QosClass::Background,
    };
    qos::set_current(qos).is_ok()
}
