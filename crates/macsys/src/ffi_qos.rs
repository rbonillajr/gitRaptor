//! Thread QoS (`pthread/qos.h`), disk I/O policy (`sys/resource.h`) and the nice of a child
//! before `exec`, one call per `unsafe` block, behind safe functions. Nothing here is public
//! outside the crate and no pointer crosses its boundary. `libc` does not carry the QoS and
//! I/O policy symbols, so they are declared by hand.

use std::os::unix::process::CommandExt;
use std::process::Command;

use libc::{c_int, c_uint};

/// `qos_class_t` values (`sys/qos.h`).
pub(crate) const QOS_CLASS_USER_INTERACTIVE: c_uint = 0x21;
pub(crate) const QOS_CLASS_USER_INITIATED: c_uint = 0x19;
pub(crate) const QOS_CLASS_DEFAULT: c_uint = 0x15;
pub(crate) const QOS_CLASS_UTILITY: c_uint = 0x11;
pub(crate) const QOS_CLASS_BACKGROUND: c_uint = 0x09;

/// `IOPOL_TYPE_DISK`, `IOPOL_SCOPE_*` and `IOPOL_*` (`sys/resource.h`).
const IOPOL_TYPE_DISK: c_int = 0;
const IOPOL_SCOPE_PROCESS: c_int = 0;
const IOPOL_SCOPE_THREAD: c_int = 1;
pub(crate) const IOPOL_IMPORTANT: c_int = 1;
pub(crate) const IOPOL_THROTTLE: c_int = 3;
pub(crate) const IOPOL_UTILITY: c_int = 4;

unsafe extern "C" {
    fn pthread_set_qos_class_self_np(qos_class: c_uint, relative_priority: c_int) -> c_int;
    fn qos_class_self() -> c_uint;
    fn setiopolicy_np(iotype: c_int, scope: c_int, policy: c_int) -> c_int;
    fn getiopolicy_np(iotype: c_int, scope: c_int) -> c_int;
}

/// Sets the QoS class of the calling thread; the error code the call returned on failure.
pub(crate) fn set_thread_qos(class: c_uint) -> Result<(), i32> {
    // SAFETY: takes two integers and only changes the calling thread's own scheduling; an
    // invalid class is rejected with an error code, never undefined behaviour.
    let rc = unsafe { pthread_set_qos_class_self_np(class, 0) };
    if rc == 0 { Ok(()) } else { Err(rc) }
}

/// The QoS class of the calling thread (`QOS_CLASS_UNSPECIFIED`, 0, when it has none).
pub(crate) fn thread_qos() -> c_uint {
    // SAFETY: no arguments; reads the calling thread's own QoS and cannot fail.
    unsafe { qos_class_self() }
}

/// The disk I/O policy of the calling thread; `None` when the call fails.
pub(crate) fn thread_io() -> Option<c_int> {
    // SAFETY: two integers; reads the calling thread's own policy, failing with -1.
    let rc = unsafe { getiopolicy_np(IOPOL_TYPE_DISK, IOPOL_SCOPE_THREAD) };
    (rc >= 0).then_some(rc)
}

/// Makes the child of `cmd` set its own nice and process disk I/O policy right before `exec`.
///
/// The hook runs between `fork` and `exec`, where only async-signal-safe work is sound: it makes
/// exactly two system calls on integers it captured by value, without allocating or taking a
/// lock. A failed call is ignored: the child then runs at the priority it inherited, which never
/// stops the command.
pub(crate) fn lower_child(cmd: &mut Command, nice: c_int, io_policy: c_int) {
    let hook = move || -> std::io::Result<()> {
        // SAFETY: integers only; `who = 0` is the child itself, and an unprivileged process may
        // always raise its own nice. A failure returns -1 and changes nothing.
        let _ = unsafe { libc::setpriority(libc::PRIO_PROCESS, 0, nice) };
        // SAFETY: integers only; the process scope touches the child alone, and a failure
        // returns -1 and changes nothing.
        let _ = unsafe { setiopolicy_np(IOPOL_TYPE_DISK, IOPOL_SCOPE_PROCESS, io_policy) };
        Ok(())
    };
    // SAFETY: `hook` captures two `c_int` by value and only makes the two system calls above:
    // no allocation, no lock and no access to state shared with the parent, so it is sound in
    // the forked child before `exec`.
    unsafe {
        cmd.pre_exec(hook);
    }
}
