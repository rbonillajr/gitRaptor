//! `sysctl(KERN_PROCARGS2)`, one call per `unsafe` block, behind a safe function. Nothing here
//! is public outside the crate.

use std::ptr::null_mut;

use libc::{CTL_KERN, KERN_ARGMAX, KERN_PROCARGS2, c_int, c_void};

/// Largest buffer read, in bytes, whatever `kern.argmax` says (macOS ships 1 MiB).
const MAX_BUFFER: usize = 4 << 20;

/// The raw `KERN_PROCARGS2` area of `pid` (argc, executable path, argv, environment), or `None`
/// when the kernel refuses (another user's process, a gone pid, a buffer too small).
pub(crate) fn procargs2(pid: u32) -> Option<Vec<u8>> {
    let pid = c_int::try_from(pid).ok()?;
    let mut argmax: c_int = 0;
    let mut size = std::mem::size_of::<c_int>();
    let mut mib = [CTL_KERN, KERN_ARGMAX];
    // SAFETY: `mib` holds 2 valid names; `argmax` and `size` are live locals of the sizes the
    // call is told; no new value is written (null, 0).
    let rc = unsafe {
        libc::sysctl(
            mib.as_mut_ptr(),
            2,
            (&raw mut argmax).cast::<c_void>(),
            &mut size,
            null_mut(),
            0,
        )
    };
    if rc != 0 || argmax <= 0 {
        return None;
    }
    let cap = usize::try_from(argmax).ok()?.min(MAX_BUFFER);
    let mut buf = vec![0u8; cap];
    let mut size = cap;
    let mut mib = [CTL_KERN, KERN_PROCARGS2, pid];
    // SAFETY: `mib` holds 3 valid names; `buf` is a live, writable allocation of `size` bytes
    // and the kernel writes at most `size` bytes, updating `size`; no new value (null, 0).
    let rc = unsafe {
        libc::sysctl(
            mib.as_mut_ptr(),
            3,
            buf.as_mut_ptr().cast::<c_void>(),
            &mut size,
            null_mut(),
            0,
        )
    };
    if rc != 0 || size > cap {
        return None;
    }
    buf.truncate(size);
    Some(buf)
}
