//! `proc_pidpath`, one call in one `unsafe` block, behind a safe function. Nothing here is public
//! outside the crate.

use std::ffi::OsString;
use std::os::unix::ffi::OsStringExt;
use std::path::PathBuf;

use libc::{PROC_PIDPATHINFO_MAXSIZE, c_int, c_void};

/// The size of the buffer `proc_pidpath` writes into: `PROC_PIDPATHINFO_MAXSIZE` (4 096 bytes).
const BUFFER: usize = 4096;

/// The path of the executable of `pid`, or `None` when the kernel refuses (a gone pid, another
/// user's process) or the answer does not fit the buffer.
pub(crate) fn pidpath(pid: u32) -> Option<PathBuf> {
    let pid = c_int::try_from(pid).ok()?;
    let mut buf = [0u8; BUFFER];
    let size = u32::try_from(PROC_PIDPATHINFO_MAXSIZE).ok()?;
    if usize::try_from(size).ok()? != buf.len() {
        return None;
    }
    // SAFETY: `buf` is a live, writable local of `BUFFER` bytes, which is the size the call is
    // told (`PROC_PIDPATHINFO_MAXSIZE`); the kernel writes at most that many bytes and returns
    // how many it wrote, or a value <= 0 on failure.
    let written = unsafe { libc::proc_pidpath(pid, buf.as_mut_ptr().cast::<c_void>(), size) };
    let written = usize::try_from(written).ok().filter(|n| *n > 0)?;
    let bytes = buf.get(..written)?;
    // The kernel may count a trailing NUL; a path never contains one.
    let end = bytes.iter().position(|b| *b == 0).unwrap_or(bytes.len());
    if end == 0 {
        return None;
    }
    Some(PathBuf::from(OsString::from_vec(bytes[..end].to_vec())))
}
