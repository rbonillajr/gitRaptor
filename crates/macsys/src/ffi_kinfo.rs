//! `sysctl(KERN_PROC_UID)`, one call per `unsafe` block, behind a safe function. Nothing here
//! is public outside the crate.

use std::ptr::null_mut;

use libc::{CTL_KERN, KERN_PROC, KERN_PROC_UID, c_int, c_void};

use crate::process::KINFO_PROC_SIZE;

/// Largest table read: 16 MiB, some 25 000 records, far above any user's process count.
const MAX_BYTES: usize = 16 << 20;
/// Room added to the size the kernel announces, for processes started between the two calls.
const SLACK_RECORDS: usize = 64;
/// Attempts when the table grows past that room between the two calls.
const ATTEMPTS: usize = 3;

/// The raw `struct kinfo_proc` records of the processes whose effective uid is `uid` (the filter
/// of `proc_listpids(PROC_UID_ONLY)` too), or `None` when the kernel refuses, the table is over
/// [`MAX_BYTES`] or it keeps outgrowing the buffer.
pub(crate) fn kinfo_by_uid(uid: u32) -> Option<Vec<u8>> {
    let uid = c_int::try_from(uid).ok()?;
    for _ in 0..ATTEMPTS {
        let mut mib = [CTL_KERN, KERN_PROC, KERN_PROC_UID, uid];
        let mut size = 0usize;
        // SAFETY: `mib` holds 4 valid names; a null buffer asks only for the size, written to the
        // live local `size`; no new value is written (null, 0).
        let rc = unsafe { libc::sysctl(mib.as_mut_ptr(), 4, null_mut(), &mut size, null_mut(), 0) };
        if rc != 0 {
            return None;
        }
        let cap = size.checked_add(SLACK_RECORDS * KINFO_PROC_SIZE)?;
        if cap > MAX_BYTES {
            return None;
        }
        let mut buf = vec![0u8; cap];
        let mut size = cap;
        // SAFETY: `mib` holds 4 valid names; `buf` is a live, writable allocation of `size` bytes
        // and the kernel writes at most `size` bytes, updating `size`; no new value (null, 0).
        let rc = unsafe {
            libc::sysctl(
                mib.as_mut_ptr(),
                4,
                buf.as_mut_ptr().cast::<c_void>(),
                &mut size,
                null_mut(),
                0,
            )
        };
        if rc == 0 && size <= cap {
            buf.truncate(size);
            return Some(buf);
        }
        // ENOMEM: the table grew past the room; ask again.
        if std::io::Error::last_os_error().raw_os_error() != Some(libc::ENOMEM) {
            return None;
        }
    }
    None
}
