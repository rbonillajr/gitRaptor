//! Byte-range locks of an open file (`LockFileEx`, `UnlockFileEx`), one call per `unsafe`
//! block. Nothing here is public outside the crate.

use std::fs::File;
use std::io;
use std::os::windows::io::AsRawHandle;

use windows_sys::Win32::Foundation::{ERROR_LOCK_VIOLATION, HANDLE};
use windows_sys::Win32::Storage::FileSystem::{
    LOCKFILE_EXCLUSIVE_LOCK, LOCKFILE_FAIL_IMMEDIATELY, LockFileEx, UnlockFileEx,
};
use windows_sys::Win32::System::IO::OVERLAPPED;

/// An `OVERLAPPED` that only carries the start of the range.
fn at(offset: u64) -> OVERLAPPED {
    let mut overlapped = OVERLAPPED::default();
    overlapped.Anonymous.Anonymous.Offset = offset as u32;
    overlapped.Anonymous.Anonymous.OffsetHigh = (offset >> 32) as u32;
    overlapped
}

/// Locks the byte at `offset` without waiting. `Ok(false)`: another handle holds a
/// conflicting lock on it.
pub(crate) fn try_lock_byte(file: &File, offset: u64, exclusive: bool) -> io::Result<bool> {
    let handle = file.as_raw_handle() as HANDLE;
    let mut flags = LOCKFILE_FAIL_IMMEDIATELY;
    if exclusive {
        flags |= LOCKFILE_EXCLUSIVE_LOCK;
    }
    let mut overlapped = at(offset);
    // SAFETY: `handle` is the open handle of `file`, borrowed for the call; `overlapped` is an
    // initialized `OVERLAPPED` borrowed only for the call. A handle std opens is synchronous, so
    // with `LOCKFILE_FAIL_IMMEDIATELY` the call never returns pending.
    let ok = unsafe { LockFileEx(handle, flags, 0, 1, 0, &mut overlapped) } != 0;
    if ok {
        return Ok(true);
    }
    let err = io::Error::last_os_error();
    if err.raw_os_error() == Some(ERROR_LOCK_VIOLATION as i32) {
        Ok(false)
    } else {
        Err(err)
    }
}

/// Unlocks the byte at `offset`, locked before through the same handle.
pub(crate) fn unlock_byte(file: &File, offset: u64) -> io::Result<()> {
    let handle = file.as_raw_handle() as HANDLE;
    let mut overlapped = at(offset);
    // SAFETY: as in `try_lock_byte`.
    let ok = unsafe { UnlockFileEx(handle, 0, 1, 0, &mut overlapped) } != 0;
    if ok {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}
