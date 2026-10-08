//! Byte-range locks (`LockFileEx`, `UnlockFileEx`), the identity of an open file
//! (`GetFileInformationByHandle`) and renames that never replace (`MoveFileExW`), one call per
//! `unsafe` block. Nothing here is public outside the crate.

use std::ffi::OsStr;
use std::fs::File;
use std::io;
use std::os::windows::ffi::OsStrExt;
use std::os::windows::io::AsRawHandle;
use std::path::Path;

use windows_sys::Win32::Foundation::{ERROR_LOCK_VIOLATION, HANDLE};
use windows_sys::Win32::Storage::FileSystem::{
    BY_HANDLE_FILE_INFORMATION, FILE_BASIC_INFO, FILE_DISPOSITION_INFO, FILE_RENAME_INFO,
    FILE_RENAME_INFO_0, FileBasicInfo, FileDispositionInfo, FileRenameInfo,
    GetFileInformationByHandle, GetFileInformationByHandleEx, LOCKFILE_EXCLUSIVE_LOCK,
    LOCKFILE_FAIL_IMMEDIATELY, LockFileEx, MOVEFILE_WRITE_THROUGH, MoveFileExW,
    SetFileInformationByHandle, UnlockFileEx,
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

/// `(volume serial number, file index)` of an open file.
pub(crate) fn file_index(file: &File) -> io::Result<(u32, u64)> {
    let handle = file.as_raw_handle() as HANDLE;
    let mut info = BY_HANDLE_FILE_INFORMATION::default();
    // SAFETY: `handle` is the open handle of `file`, borrowed for the call; `info` is an
    // initialized `BY_HANDLE_FILE_INFORMATION` borrowed only for the call.
    let ok = unsafe { GetFileInformationByHandle(handle, &mut info) } != 0;
    if !ok {
        return Err(io::Error::last_os_error());
    }
    let index = (u64::from(info.nFileIndexHigh) << 32) | u64::from(info.nFileIndexLow);
    Ok((info.dwVolumeSerialNumber, index))
}

/// `ChangeTime` of an open file (100 ns since 1601): it moves on every write, rename or
/// attribute change, and nobody can set it back, unlike the write time.
pub(crate) fn change_time(file: &File) -> io::Result<i64> {
    let handle = file.as_raw_handle() as HANDLE;
    let mut info = FILE_BASIC_INFO::default();
    // SAFETY: `handle` is the open handle of `file`, borrowed for the call; `info` is an
    // initialized `FILE_BASIC_INFO` whose exact size is passed, borrowed only for the call.
    let ok = unsafe {
        GetFileInformationByHandleEx(
            handle,
            FileBasicInfo,
            (&raw mut info).cast(),
            size_of::<FILE_BASIC_INFO>() as u32,
        )
    } != 0;
    if ok {
        Ok(info.ChangeTime)
    } else {
        Err(io::Error::last_os_error())
    }
}

/// Marks the file of an open handle (opened with `DELETE` access) to be deleted when the last
/// handle closes: the entry deleted is exactly the one that was read through this handle.
pub(crate) fn delete_on_close(file: &File) -> io::Result<()> {
    let handle = file.as_raw_handle() as HANDLE;
    let info = FILE_DISPOSITION_INFO { DeleteFile: true };
    // SAFETY: `handle` is the open handle of `file`, borrowed for the call; `info` is an
    // initialized `FILE_DISPOSITION_INFO` whose exact size is passed, borrowed only for the call.
    let ok = unsafe {
        SetFileInformationByHandle(
            handle,
            FileDispositionInfo,
            (&raw const info).cast(),
            size_of::<FILE_DISPOSITION_INFO>() as u32,
        )
    } != 0;
    if ok {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

/// Renames the file of an open handle (opened with `DELETE` access) to `to` (a full path; an
/// absolute drive path gets the `\\?\` prefix), never replacing (`ReplaceIfExists` false): the
/// entry renamed is the one opened, whatever its old name holds now. `RootDirectory` stays null:
/// through `SetFileInformationByHandle` a relative root is refused (`ERROR_INVALID_PARAMETER`)
/// and a bare name is taken relative to the current directory, so only a full path is safe.
pub(crate) fn rename_handle(file: &File, to: &Path) -> io::Result<()> {
    let mut wide = wide(to);
    wide.pop(); // The length is given: no NUL.
    let name_bytes = wide.len() * size_of::<u16>();
    let offset = std::mem::offset_of!(FILE_RENAME_INFO, FileName);
    let size = (offset + name_bytes).max(size_of::<FILE_RENAME_INFO>());
    let len = u32::try_from(name_bytes).map_err(|_| io::Error::other("name too long"))?;
    let header = FILE_RENAME_INFO {
        Anonymous: FILE_RENAME_INFO_0 {
            ReplaceIfExists: false,
        },
        RootDirectory: std::ptr::null_mut(),
        FileNameLength: len,
        FileName: [0],
    };
    // A zeroed buffer aligned for `FILE_RENAME_INFO` (8 bytes), long enough for the name.
    let mut buf = vec![0u64; size.div_ceil(size_of::<u64>())];
    let info = buf.as_mut_ptr().cast::<FILE_RENAME_INFO>();
    // SAFETY: `info` points at the start of `buf`, aligned to 8 and at least
    // `size_of::<FILE_RENAME_INFO>()` bytes long, owned for the whole function.
    unsafe { info.write(header) };
    let name_at = buf
        .as_mut_ptr()
        .cast::<u8>()
        .wrapping_add(offset)
        .cast::<u16>();
    // SAFETY: `buf` holds `offset + name_bytes` bytes or more, so the `wide.len()` code units
    // written from `name_at` stay inside it; `offset` is even, so `name_at` is aligned for
    // `u16`; `wide` and `buf` do not overlap.
    unsafe { std::ptr::copy_nonoverlapping(wide.as_ptr(), name_at, wide.len()) };
    let handle = file.as_raw_handle() as HANDLE;
    let bytes = u32::try_from(buf.len() * size_of::<u64>())
        .map_err(|_| io::Error::other("name too long"))?;
    // SAFETY: `handle` is the open handle of `file`, borrowed for the call; `buf` is an initialized `FILE_RENAME_INFO` followed by its name, `bytes` long,
    // borrowed only for the call.
    let ok =
        unsafe { SetFileInformationByHandle(handle, FileRenameInfo, buf.as_ptr().cast(), bytes) }
            != 0;
    if ok {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

/// UTF-16 with NUL. An absolute drive path gets the `\\?\` prefix, so Windows takes every name
/// as is (no trailing dots or spaces dropped, no device names, no 260-character limit).
fn wide(path: &Path) -> Vec<u16> {
    let raw = OsStr::new(path);
    let bytes = raw.as_encoded_bytes();
    let drive = bytes.len() > 2 && bytes[1] == b':' && (bytes[2] == b'\\' || bytes[2] == b'/');
    let prefix: &[u16] = if drive {
        &[b'\\' as u16, b'\\' as u16, b'?' as u16, b'\\' as u16]
    } else {
        &[]
    };
    prefix
        .iter()
        .copied()
        .chain(raw.encode_wide().map(|c| {
            if drive && c == u16::from(b'/') {
                u16::from(b'\\')
            } else {
                c
            }
        }))
        .chain(Some(0))
        .collect()
}

/// Renames the entry `from` to `to` on the same volume, flushed before returning. Never replaces:
/// fails with `ERROR_ALREADY_EXISTS` or `ERROR_FILE_EXISTS` if `to` exists. A link or junction at
/// `from` is renamed itself, never followed.
pub(crate) fn move_no_replace(from: &Path, to: &Path) -> io::Result<()> {
    let from = wide(from);
    let to = wide(to);
    // SAFETY: both buffers are NUL-terminated UTF-16 strings alive for the call. Without
    // `MOVEFILE_COPY_ALLOWED` the call is a rename on one volume and never copies.
    let ok = unsafe { MoveFileExW(from.as_ptr(), to.as_ptr(), MOVEFILE_WRITE_THROUGH) } != 0;
    if ok {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}
